//! Exercise script controls through the host's UI model, under a held note.
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use kontra_engine::{Engine, Event, HostNote, HostPattern, Plan, Prepared, Text, Timed};
use kontra_host::ui::lua::Page as _;
use kontra_stream::{Store, StoreConfig};
use kontra_ui_model::{Interface, Kind, Value, Widget};
use serde_json::{Value as Json, json};

/// Trace events a move keeps of each kind in the report
/// (`KONTRA_INSPECT_KEEP_EVENTS`, default 64).
fn kept_events() -> usize {
    std::env::var("KONTRA_INSPECT_KEEP_EVENTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64)
}

/// This process's resident memory, so a sweep shows whether moves leak.
fn rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

use super::RATE;

const BLOCK: usize = 128;
const BLOCKS: usize = 30;
/// The slowest block `render` processed, ns (the audio thread's worst turn).
/// What the Lua callbacks spent in that block (wall), and the GC within it.
static WORST_BLOCK_LUA_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static WORST_BLOCK_GC_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static WORST_BLOCK_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The most entries of one menu a sweep moves to.
const MENU_MOVES: usize = 16;

fn targets(widget: &Widget) -> Option<Vec<f64>> {
    let values = match &widget.kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } | Kind::ValueEdit { range, .. } => {
            vec![range.min, range.max]
        }
        Kind::Switch | Kind::Button { .. } => vec![1.0, 0.0],
        // A long menu (a controller list, a preset list) is moved to 16
        // entries spread over it, first and last included.
        Kind::Menu { items } => {
            let shown: Vec<f64> = (items.iter().filter(|i| i.visible))
                .map(|i| f64::from(i.value))
                .collect();
            let n = shown.len();
            if n <= MENU_MOVES {
                shown
            } else {
                (0..MENU_MOVES)
                    .filter_map(|k| shown.get(k * (n - 1) / (MENU_MOVES - 1)).copied())
                    .collect()
            }
        }
        _ => return None,
    };
    Some(values)
}

fn tree(
    loaded: &kontra_host::instrument::Loaded,
    engine: &Engine,
    slot: usize,
) -> Result<Option<Interface>> {
    let slot_id = u8::try_from(slot).context("too many script slots")?;
    if let Some(Some(face)) = loaded.faces.get(slot) {
        return kontra_host::ui::interface(face, slot_id, engine, &|_| None)
            .map(Some)
            .map_err(|e| anyhow::anyhow!("building UI model for slot {slot}: {e}"));
    }
    let current = engine
        .vm(slot_id)
        .and_then(kontra_engine::Vm::own_page)
        .and_then(|p| p.downcast::<kontra_lua::Ui>().ok());
    let Some(page) = current
        .as_deref()
        .or_else(|| loaded.pages.get(slot).and_then(Option::as_ref))
    else {
        return Ok(None);
    };
    let mut tree = kontra_host::ui::lua::tree(page, &|_| None);
    let mut text = Text::EMPTY;
    for (index, _) in (0u32..).zip(0..tree.widgets.len()) {
        if let Some(look) = engine.widget(slot_id, index) {
            let words = engine.widget_text(slot_id, index, &mut text);
            kontra_host::ui::lua::apply(
                &mut tree,
                index as usize,
                look,
                words.then(|| text.as_str()),
            );
        }
    }
    Ok(Some(tree))
}

type Cells = (u8, Vec<Option<i64>>, Vec<Option<String>>);

fn cells(engine: &Engine, plan: &Plan) -> Vec<Cells> {
    plan.scripts
        .iter()
        .enumerate()
        .filter_map(|(s, script)| {
            let slot = u8::try_from(s).ok()?;
            let numbers = (0..script.program.cells)
                .map(|i| engine.script_cell_in(slot, i))
                .collect();
            let texts = (0..script.program.text_cells)
                .map(|i| {
                    engine
                        .script_text_in(slot, i)
                        .map(|t| t.as_str().to_owned())
                })
                .collect();
            Some((slot, numbers, texts))
        })
        .collect()
}

fn cell_diff(before: &[Cells], after: &[Cells]) -> Vec<Json> {
    let mut changes = Vec::new();
    for ((slot, a, ta), (_, b, tb)) in before.iter().zip(after) {
        for (index, (from, to)) in a.iter().zip(b).enumerate() {
            if from != to {
                changes.push(json!({"slot":slot,"cell":index,"before":from,"after":to}));
            }
        }
        for (index, (from, to)) in ta.iter().zip(tb).enumerate() {
            if from != to {
                changes.push(json!({"slot":slot,"text_cell":index,"before":from,"after":to}));
            }
        }
    }
    changes
}

/// What a note did, as named fields: each voice event's sample, pitch and
/// gain, and each script action's arguments outside `init` (whose resumed
/// loops run on their own clock), keyed by kind and occurrence.
type Note = std::collections::BTreeMap<String, Json>;

/// Adds `engine`'s trace (one block) to `note`, `names` keeping each
/// callback's name by id across blocks.
fn note_fields(
    engine: &Engine,
    plan: &Plan,
    zones: &[Option<String>],
    note: &mut Note,
    names: &mut std::collections::HashMap<i64, String>,
) {
    let key = |note: &Note, kind: String| {
        let n = note
            .keys()
            .filter(|k| k.starts_with(&format!("{kind}#")))
            .count();
        format!("{kind}#{n}")
    };
    for e in engine.trace_events().iter() {
        let e = super::trace_output::event(e, plan, zones, RATE);
        match e["type"].as_str() {
            Some("callback") => {
                if let (Some(id), Some(name)) =
                    (e["callback_id"].as_i64(), e["callback_name"].as_str())
                {
                    names.insert(id, name.to_owned());
                }
            }
            Some("script_action") => {
                let callback = e["callback_id"].as_i64().and_then(|id| names.get(&id));
                if callback.is_some_and(|c| c == "init") {
                    continue;
                }
                let k = key(note, format!("{}", e["action"]));
                note.insert(k, e["args"].clone());
            }
            Some("voice") => {
                let k = key(note, format!("voice {}", e["event"]));
                let fields = json!({"sample":e["sample"],"pitch":e["pitch_midi_relative_to_root"],"gain":e["gain"]});
                note.insert(k, fields);
            }
            _ => {}
        }
    }
}

/// The fields notes `next` and `again` (after a move) agree on that differ
/// from what notes `first` and `base` (before it) agreed on: what moves on
/// its own (round robins, humanizing, note ids, a velocity that drifts
/// note by note) is left out.
fn note_diff(first: &Note, base: &Note, next: &Note, again: &Note) -> Json {
    let keys: std::collections::BTreeSet<&String> = base.keys().chain(next.keys()).collect();
    let changed: Vec<_> = keys
        .into_iter()
        .filter(|k| {
            first.get(*k) == base.get(*k)
                && next.get(*k) == again.get(*k)
                && base.get(*k) != next.get(*k)
        })
        .map(|k| json!({"field":k,"before":base.get(k),"after":next.get(k)}))
        .collect();
    json!(changed)
}

/// Each group's sound, and whether a script purged it (it plays silent).
fn sound(engine: &Engine) -> Vec<String> {
    let purged: HashSet<usize> = engine.purged().collect();
    (engine.group_sounds().iter().enumerate())
        .map(|(g, x)| format!("{x:?} purged {}", purged.contains(&g)))
        .collect()
}

/// This thread's CPU time, ns: a block the machine preempted is not a spike of ours.
fn thread_cpu_ns() -> u64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `t` is a valid timespec the call fills in.
    let ok = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) } == 0;
    if ok {
        (t.tv_sec as u64) * 1_000_000_000 + t.tv_nsec as u64
    } else {
        0
    }
}

/// Calls per script action, by name: (calls, calls left unanswered).
type Counts = std::collections::BTreeMap<String, (u32, u32)>;

/// Adds what happened between `before` and `after` to `to`.
fn add(to: &mut Counts, before: &Counts, after: &Counts) {
    for (name, &(c, u)) in after {
        let (c0, u0) = before.get(name).copied().unwrap_or_default();
        let e = to.entry(name.clone()).or_default();
        e.0 += c.saturating_sub(c0);
        e.1 += u.saturating_sub(u0);
    }
}

/// Each script action called so far, by name: (calls, calls left unanswered).
fn calls(engine: &Engine) -> Counts {
    let mut out = std::collections::BTreeMap::new();
    for (a, n) in engine.action_calls() {
        out.entry(format!("{a:?}")).or_insert((0, 0)).0 = n;
    }
    for (a, n) in engine.unhandled_actions() {
        out.entry(format!("{a:?}")).or_insert((0, 0)).1 = n;
    }
    out
}

/// What a phase added to the running counts of [`calls`].
fn phase(before: &Counts, after: &Counts) -> Json {
    (after.iter())
        .filter_map(|(name, &(c, u))| {
            let (c0, u0) = before.get(name).copied().unwrap_or_default();
            (c > c0 || u > u0).then(|| (name.clone(), json!([c - c0, u.saturating_sub(u0)])))
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

fn render(
    engine: &mut Engine,
    plan: &Plan,
    stream: &kontra_stream::StreamClient,
    events: &[Timed<Event>],
    keep: impl FnMut(&Engine),
) -> (f64, bool, Vec<[f32; 2]>) {
    render_for(engine, plan, stream, events, keep, BLOCKS)
}

/// [`render`] for `blocks` blocks.
fn render_for(
    engine: &mut Engine,
    plan: &Plan,
    stream: &kontra_stream::StreamClient,
    events: &[Timed<Event>],
    mut keep: impl FnMut(&Engine),
    blocks: usize,
) -> (f64, bool, Vec<[f32; 2]>) {
    let mut samples = [[0.0f32; 2]; BLOCK];
    let mut frames = Vec::with_capacity(BLOCK * blocks);
    let mut energy = 0.0f64;
    let mut settled = true;
    for block in 0..blocks {
        settled &= stream.settle(Duration::from_secs(5));
        let _ = kontra_lua::probe::take();
        let began = thread_cpu_ns();
        engine.process(&mut samples, if block == 0 { events } else { &[] });
        let spent = thread_cpu_ns().saturating_sub(began);
        let lua = kontra_lua::probe::take();
        if spent > WORST_BLOCK_NS.fetch_max(spent, std::sync::atomic::Ordering::Relaxed) {
            // Wall time (the probe reads the monotonic clock), unlike the CPU-time block above.
            WORST_BLOCK_LUA_NS.store(lua.run_ns, std::sync::atomic::Ordering::Relaxed);
            WORST_BLOCK_GC_NS.store(lua.gc_ns, std::sync::atomic::Ordering::Relaxed);
        }
        // Each block's trace is taken, then cleared: the buffer holds one
        // block, so a long sweep drops nothing.
        keep(engine);
        engine.clear_trace();
        for sample in samples.iter().flatten() {
            energy += f64::from(*sample).powi(2);
        }
        frames.extend_from_slice(&samples);
        let mut requests = Vec::new();
        engine.drain_requests(|r| requests.push(r));
        for request in requests {
            engine.install(Prepared::new(request, Some(plan), RATE));
        }
        // `set_zone_par` edits, applied together off audio as the host does.
        let mut edits = Vec::new();
        engine.drain_zone_edits(|e| {
            edits.push(e);
            true
        });
        if !edits.is_empty() {
            let mut next = Plan::clone(engine.plan());
            let done: Vec<_> = (edits.iter()).map(|e| (*e, e.apply(&mut next))).collect();
            let next = kontra_prepare::tables(&next, stream).unwrap_or(next);
            engine.swap_plan(&mut Arc::new(next), &done);
        }
    }
    (
        (energy / (BLOCK * blocks * 2) as f64).sqrt(),
        settled,
        frames,
    )
}

/// Blocks a move gets before the next note: a script that waits before
/// it acts (a fade out, `wait(150)`, then the room's new `Time`) has acted.
const SETTLE_BLOCKS: usize = 240;

/// Blocks a fresh note renders: 320 ms, long enough for a room's early
/// reflections and a 2048-point spectrum.
const FRESH_BLOCKS: usize = 120;

/// A render's sound without its phase: each channel's octave-band levels
/// (dBFS, Hann-windowed 2048-point FFT) and the channels' correlation. A
/// humanized note (random detune, attack pitch) keeps its print.
fn print(frames: &[[f32; 2]]) -> (Vec<f64>, f64) {
    use rustfft::{FftPlanner, num_complex::Complex};
    const N: usize = 2048;
    let window: Vec<f64> = (0..N)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (N - 1) as f64).cos())
        .collect();
    let fft = FftPlanner::<f64>::new().plan_fft_forward(N);
    let centers = [
        62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
    ];
    let mut sums = vec![0.0f64; centers.len() * 2];
    let mut buf = vec![Complex::new(0.0, 0.0); N];
    for chunk in frames.chunks_exact(N / 2).collect::<Vec<_>>().windows(2) {
        for c in 0..2 {
            for (i, (b, w)) in buf.iter_mut().zip(&window).enumerate() {
                let f = if i < N / 2 {
                    chunk[0][i]
                } else {
                    chunk[1][i - N / 2]
                };
                *b = Complex::new(f64::from(f[c]) * w, 0.0);
            }
            fft.process(&mut buf);
            for (i, bin) in buf.iter().enumerate().take(N / 2).skip(1) {
                let hz = i as f64 * f64::from(RATE) / N as f64;
                if let Some(k) = centers
                    .iter()
                    .position(|&m| hz >= m / 2f64.sqrt() && hz < m * 2f64.sqrt())
                {
                    sums[c * centers.len() + k] += bin.norm_sqr();
                }
            }
        }
    }
    let db = sums
        .iter()
        .map(|&p| 10.0 * (p.max(1e-20)).log10() - 60.0)
        .collect();
    let (mut lr, mut ll, mut rr) = (0.0f64, 0.0f64, 0.0f64);
    for f in frames {
        let (l, r) = (f64::from(f[0]), f64::from(f[1]));
        lr += l * r;
        ll += l * l;
        rr += r * r;
    }
    (db, lr / (ll * rr).sqrt().max(1e-20))
}

/// How far print `b` is from `a`: the RMS of their band differences (dB,
/// bands either holds above 70 dB under its loudest) plus ten times the
/// change in correlation.
fn print_distance(a: &(Vec<f64>, f64), b: &(Vec<f64>, f64)) -> f64 {
    let top = a.0.iter().chain(&b.0).copied().fold(f64::MIN, f64::max);
    let diffs: Vec<f64> = (a.0.iter().zip(&b.0))
        .filter(|(x, y)| x.max(**y) > top - 70.0)
        .map(|(x, y)| (x - y).powi(2))
        .collect();
    let bands = (diffs.iter().sum::<f64>() / diffs.len().max(1) as f64).sqrt();
    bands + 10.0 * (a.1 - b.1).abs()
}

/// How far render `b` is from render `a`, dB relative to `a`'s energy:
/// level, spectrum, stereo image and timing all show (−inf when equal).
fn difference_db(a: &[[f32; 2]], b: &[[f32; 2]]) -> f64 {
    let (mut diff, mut base) = (0.0f64, 0.0f64);
    for (x, y) in a.iter().zip(b) {
        for c in 0..2 {
            base += f64::from(x[c]).powi(2);
            diff += f64::from(x[c] - y[c]).powi(2);
        }
    }
    if diff == 0.0 {
        return f64::NEG_INFINITY;
    }
    10.0 * (diff / base.max(1e-30)).log10()
}

/// Left and right RMS of a render.
fn sides(frames: &[[f32; 2]]) -> [f64; 2] {
    let n = frames.len().max(1) as f64;
    [0, 1].map(|c| (frames.iter().map(|f| f64::from(f[c]).powi(2)).sum::<f64>() / n).sqrt())
}

pub fn run(path: &Path) -> Result<(Json, String)> {
    let instrument = kontra_host::instrument::read(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    // KONTRA_INSPECT_DUMP=DIR: each script's source and the modules it loads.
    if let Some(dir) = std::env::var_os("KONTRA_INSPECT_DUMP").map(std::path::PathBuf::from) {
        for script in &instrument.scripts {
            let dir = dir.join(format!("{}-{}", script.slot, script.name.replace('/', "_")));
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join("source"), &script.source)?;
            for (name, bytes) in &script.data {
                let file = dir.join(name.trim_start_matches('/'));
                if let Some(parent) = file.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(file, bytes)?;
            }
        }
    }
    let samples: Vec<_> = instrument.samples.iter().map(|s| s.name.clone()).collect();
    let members: Vec<Vec<usize>> = instrument
        .groups
        .iter()
        .map(|g| g.zones.iter().map(|z| z.sample).collect())
        .collect();
    let mut store = Store::new(StoreConfig::DEFAULT).context("creating sample store")?;
    let loaded = kontra_host::instrument::build(instrument, RATE, &mut store);
    let plan = Arc::new(loaded.plan.clone());
    let zone_samples: Vec<_> = plan
        .zones
        .iter()
        .map(|z| {
            usize::try_from(z.group)
                .ok()
                .and_then(|g| members.get(g))
                .and_then(|g| g.get(usize::from(z.member)))
                .and_then(|i| samples.get(*i))
                .cloned()
        })
        .collect();
    let stream = store.client();
    let mut engine = Engine::new_traced(Arc::clone(&plan), stream.clone(), RATE, 65536);
    engine.set_render_mode(kontra_engine::RenderMode::Offline);
    engine.clear_trace();
    let at = engine.now();
    let note = Event::NoteOn {
        note: HostNote {
            port: 0,
            channel: 0,
            key: 60,
            id: -1,
            clap: true,
        },
        velocity: 100.0 / 127.0,
        tune: 0.0,
    };
    let events = [
        Timed {
            at,
            body: Event::midi1(0xb0, 1, 127),
        },
        Timed {
            at,
            body: Event::midi1(0xb0, 11, 127),
        },
        Timed { at, body: note },
    ];
    // The script's init runs in the first blocks, before any event.
    let (_, mut settled, _) = render(&mut engine, &plan, &stream, &[], |_| {});
    let at_load = calls(&engine);
    let events: Vec<_> = (events.iter())
        .map(|e| Timed {
            at: engine.now(),
            body: e.body,
        })
        .collect();
    let (_, ready, _) = render(&mut engine, &plan, &stream, &events, |_| {});
    settled &= ready;
    let at_note = calls(&engine);
    // What the probe notes around each move called: not the moves' own.
    let mut probes = Counts::new();
    engine.clear_trace();
    let mut controls = Vec::new();
    let list_only = std::env::var_os("KONTRA_INSPECT_UI_LIST").is_some();
    // KONTRA_INSPECT_UI_SAMPLE=N moves about N controls a script slot
    // shows, every k-th in tree order (so every page has some);
    // KONTRA_INSPECT_UI_SECONDS=S stops moving after S seconds. Both cut
    // the sweep short: `sampled` says so.
    let env = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<usize>().ok());
    let sample = env("KONTRA_INSPECT_UI_SAMPLE").filter(|&n| n > 0);
    let deadline = env("KONTRA_INSPECT_UI_SECONDS")
        .map(|s| std::time::Instant::now() + Duration::from_secs(s as u64));
    let mut capped = false;
    // KONTRA_INSPECT_UI_ONLY=a,b moves only the controls whose names hold
    // one of the words.
    let words = std::env::var("KONTRA_INSPECT_UI_ONLY").unwrap_or_default();
    let only = |name: &str| words.is_empty() || words.split(',').any(|w| name.contains(w));
    // KONTRA_INSPECT_UI_LEAN=1 keeps each move's verdicts, not its trace.
    let lean = std::env::var_os("KONTRA_INSPECT_UI_LEAN").is_some();
    // A move keeps at most the trace's capacity per window; the moves that
    // passed it are named with how many events they dropped (the renders
    // between moves are not kept, so their drops lose nothing). Only a
    // visible control's truncation leaves the sweep incomplete: a hidden
    // developer reset (Audio Imperia's) runs 12k-300k events in one call.
    let mut truncated = Vec::new();
    let mut models = Vec::new();
    let mut model_errors = Vec::new();
    for slot in 0..loaded.faces.len().max(loaded.pages.len()) {
        let model = match tree(&loaded, &engine, slot) {
            Ok(Some(model)) => model,
            Ok(None) => continue,
            Err(error) => {
                model_errors.push(json!({"slot":slot,"error":error.to_string()}));
                continue;
            }
        };
        models.push(json!({"slot":slot, "widgets":model.widgets.iter().map(|w| format!("{w:?}")).collect::<Vec<_>>(), "assets":format!("{:?}",model.assets), "unsupported":format!("{:?}",model.unsupported)}));
        let lua = loaded.pages.get(slot).is_some_and(Option::is_some);
        let slot_id = u8::try_from(slot).context("too many script slots")?;
        let mut seen = HashSet::new();
        let stride = sample.map_or(1, |n| {
            let all = tree(&loaded, &engine, slot).ok().flatten();
            let movable = all.map_or(0, |t| {
                t.widgets.iter().filter(|w| targets(w).is_some()).count()
            });
            movable.div_ceil(n).max(1)
        });
        loop {
            if deadline.is_some_and(|d| std::time::Instant::now() > d) {
                capped = true;
                break;
            }
            let current = match tree(&loaded, &engine, slot) {
                Ok(Some(tree)) => tree,
                Ok(None) => break,
                Err(error) => {
                    model_errors.push(json!({"slot":slot,"error":error.to_string()}));
                    break;
                }
            };
            let next = current
                .widgets
                .iter()
                .enumerate()
                .find_map(|(index, widget)| {
                    let key = (
                        widget.source_id,
                        widget.name.clone(),
                        widget.source_id.is_none().then_some(index),
                    );
                    (!seen.contains(&key))
                        .then(|| targets(widget).map(|values| (index, widget.clone(), key, values)))
                        .flatten()
                });
            let Some((widget_index, widget, key, values)) = next else {
                break;
            };
            if seen.len() >= 10_000 {
                model_errors.push(
                    json!({"slot":slot,"error":"UI continued creating controls beyond 10000"}),
                );
                break;
            }
            seen.insert(key);
            if (seen.len() - 1) % stride != 0 || !only(&widget.name) {
                continue;
            }
            // KONTRA_INSPECT_UI_LIST=1: each control and the values it would
            // visit, without moving it (tools/oracle/controls.py reads this).
            if list_only {
                controls.push(json!({"slot":slot,"widget":widget_index,"name":widget.name,"kind":format!("{:?}",widget.kind).split(|c: char| !c.is_alphanumeric()).next(),"hidden":widget.hidden,"enabled":widget.enabled,"targets":values}));
                continue;
            }
            let mut moves = Vec::new();
            if values.is_empty() {
                moves.push(json!({"error":"control has no selectable values"}));
            }
            for value in values {
                let Some(id) = widget.source_id else {
                    moves.push(json!({"value":value,"error":"control has no source id"}));
                    continue;
                };
                let off = Event::NoteOff(HostPattern {
                    port: 0,
                    channel: 0,
                    key: 60,
                    id: -1,
                    clap: true,
                });
                // A fresh note's script actions, twice: what differs between
                // the two (note ids, humanizing) is the note's own noise.
                let fresh = |engine: &mut Engine| {
                    let at = engine.now();
                    let mut acts = Note::new();
                    let mut names = std::collections::HashMap::new();
                    let (rms, ready, wave) = render_for(
                        engine,
                        &plan,
                        &stream,
                        &[Timed { at, body: off }, Timed { at, body: note }],
                        |engine| note_fields(engine, &plan, &zone_samples, &mut acts, &mut names),
                        FRESH_BLOCKS,
                    );
                    // Then its release: a room's length, a reverb's decay
                    // and a release time live in the tail.
                    let at = engine.now();
                    let (_, done, tail) = render_for(
                        engine,
                        &plan,
                        &stream,
                        &[Timed { at, body: off }],
                        |_| {},
                        FRESH_BLOCKS,
                    );
                    (rms, acts, ready && done, (wave, tail))
                };
                let probing = calls(&engine);
                let (_, first, ready_first, wave_first) = fresh(&mut engine);
                let (prior_rms, base, ready, wave_base) = fresh(&mut engine);
                add(&mut probes, &probing, &calls(&engine));
                settled &= ready_first && ready;
                engine.clear_trace();
                let panel = |engine: &Engine| -> Vec<String> {
                    (tree(&loaded, engine, slot).ok().flatten())
                        .map(|t| t.widgets.iter().map(|w| format!("{w:?}")).collect())
                        .unwrap_or_default()
                };
                let before_panel = panel(&engine);
                let before = cells(&engine, &plan);
                let before_sound = sound(&engine);
                let (before_groups, before_buses) = engine.inspect_levels();
                let before_effects: Vec<_> = engine
                    .inspect_shared_effects()
                    .iter()
                    .map(|v| format!("{v:?}"))
                    .collect();
                let before_mods: Vec<_> = engine.mods().map(|m| format!("{m:?}")).collect();
                let before_faults = format!("{:?}", engine.script_faults());
                let before_unhandled = format!("{:?}", engine.unhandled_actions());
                let before_missing = engine.stats().missing_frames;
                engine.clear_trace();
                let dropped_before = engine.trace_dropped();
                WORST_BLOCK_NS.store(0, std::sync::atomic::Ordering::Relaxed);
                WORST_BLOCK_LUA_NS.store(0, std::sync::atomic::Ordering::Relaxed);
                WORST_BLOCK_GC_NS.store(0, std::sync::atomic::Ordering::Relaxed);
                let moved_at = thread_cpu_ns();
                if lua {
                    if let Ok(id) = u32::try_from(id) {
                        engine.set_widget(slot_id, id, value, None);
                    } else {
                        moves.push(json!({"value":value,"error":"negative widget id"}));
                        continue;
                    }
                } else {
                    let v = if matches!(widget.value, Some(Value::Real(_))) {
                        value.to_bits() as i64
                    } else {
                        value.round() as i64
                    };
                    engine.set_control(slot_id, id, v);
                }
                // The move runs on the audio thread: what it costs one block.
                let move_us = thread_cpu_ns().saturating_sub(moved_at) as f64 / 1e3;
                let mut events: Vec<_> = engine
                    .trace_events()
                    .iter()
                    .map(|e| super::trace_output::event(e, &plan, &zone_samples, RATE))
                    .collect();
                let mut route_events: Vec<_> = engine
                    .trace_route_values()
                    .map(|v| super::trace_output::route(&v, &plan, &zone_samples))
                    .collect();
                engine.clear_trace();
                let dropped_control = engine.trace_dropped() - dropped_before;
                let (rms, ready, _) = render_for(
                    &mut engine,
                    &plan,
                    &stream,
                    &[],
                    |engine| {
                        events
                            .extend((engine.trace_events().iter()).map(|e| {
                                super::trace_output::event(e, &plan, &zone_samples, RATE)
                            }));
                        route_events.extend(
                            (engine.trace_route_values())
                                .map(|v| super::trace_output::route(&v, &plan, &zone_samples)),
                        );
                    },
                    SETTLE_BLOCKS,
                );
                settled &= ready;
                let dropped_render = engine.trace_dropped() - dropped_before - dropped_control;
                if dropped_control + dropped_render > 0 {
                    truncated.push((
                        widget.name.clone(),
                        widget.hidden,
                        dropped_control + dropped_render,
                    ));
                }
                let after = cells(&engine, &plan);
                let after_panel = panel(&engine);
                // The value the control holds after the move: a script may
                // clamp or reset it, or the move may not land.
                let landed = (tree(&loaded, &engine, slot).ok().flatten())
                    .and_then(|t| t.widgets.into_iter().find(|w| w.source_id == Some(id)))
                    .and_then(|w| w.value)
                    .map(|v| format!("{v:?}"));
                // Other widgets whose state the move changed (shown, value,
                // text): a panel switch or a linked control.
                let panel_changed: Vec<_> = (before_panel.iter().zip(&after_panel).enumerate())
                    .filter(|(i, (a, b))| *i != widget_index && a != b)
                    .map(|(i, _)| i)
                    .collect();
                let after_sound = sound(&engine);
                let (after_groups, after_buses) = engine.inspect_levels();
                let after_effects: Vec<_> = engine
                    .inspect_shared_effects()
                    .iter()
                    .map(|v| format!("{v:?}"))
                    .collect();
                let group_levels: Vec<_> = before_groups.iter().zip(&after_groups).enumerate().filter(|(_, (a,b))| a != b).map(|(i,(a,b))| json!({"group":i,"before":format!("{a:?}"),"after":format!("{b:?}")})).collect();
                let bus_levels: Vec<_> = before_buses
                    .iter()
                    .zip(&after_buses)
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .map(|(i, (a, b))| json!({"bus":i,"before":a,"after":b}))
                    .collect();
                let effects = (before_effects != after_effects)
                    .then(|| json!({"before":before_effects,"after":after_effects}));
                let after_mods: Vec<_> = engine.mods().map(|m| format!("{m:?}")).collect();
                let group_changes: Vec<_> = before_sound
                    .iter()
                    .zip(&after_sound)
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .map(|(i, (a, b))| json!({"group":i,"before":a,"after":b}))
                    .collect();
                let mod_changes: Vec<_> = before_mods
                    .iter()
                    .zip(&after_mods)
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .map(|(i, (a, b))| json!({"index":i,"before":a,"after":b}))
                    .collect();
                let after_faults = format!("{:?}", engine.script_faults());
                let after_unhandled = format!("{:?}", engine.unhandled_actions());
                let mut errors = Vec::new();
                if before_faults != after_faults {
                    errors.push("script fault count changed");
                }
                if before_unhandled != after_unhandled {
                    errors.push("unhandled action count changed");
                }
                if !ready {
                    errors.push("stream settle timed out");
                }
                if engine.stats().missing_frames > before_missing {
                    errors.push("sample frames missing");
                }
                // And one after the move: a difference the noise does not
                // explain is the move reaching the next note.
                let probing = calls(&engine);
                let (_, next, ready_next, wave_next) = fresh(&mut engine);
                let (_, again, ready_again, wave_again) = fresh(&mut engine);
                add(&mut probes, &probing, &calls(&engine));
                // A fresh note after the move against one before it, with
                // what two renders on either side differ by as the floor
                // (humanizing, round robins): the move is heard when the
                // change stands clear of both.
                let prints =
                    [&wave_first, &wave_base, &wave_next, &wave_again].map(|(head, tail)| {
                        let (mut bands, correlation) = print(head);
                        bands.extend(print(tail).0);
                        (bands, correlation)
                    });
                let floor = print_distance(&prints[0], &prints[1])
                    .max(print_distance(&prints[2], &prints[3]));
                let change = print_distance(&prints[1], &prints[2])
                    .min(print_distance(&prints[0], &prints[3]));
                let finite = |x: f64| if x.is_finite() { json!(x) } else { json!(null) };
                let sound = json!({"change_db":finite(change),"floor_db":finite(floor),
                    "waveform_change_db":finite(difference_db(&wave_base.0, &wave_next.0)),
                    "heard": change > 2.0 * floor + 0.5,
                    "correlation":[prints[1].1, prints[2].1],
                    "lr_rms_before":sides(&wave_base.0),"lr_rms_after":sides(&wave_next.0)});
                settled &= ready_next && ready_again;
                let note_changed = note_diff(&first, &base, &next, &again);
                // A move's first events only: a VWinds move runs ~150 script
                // callbacks, and keeping all of a 1200-move sweep's held 20 GB.
                let stage_total = events.len();
                let route_total = route_events.len();
                events.truncate(kept_events());
                route_events.truncate(kept_events());
                moves.push(json!({"value":value,"landed":landed,"move_us":move_us,"worst_block_lua_wall_us":WORST_BLOCK_LUA_NS.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e3,"worst_block_gc_wall_us":WORST_BLOCK_GC_NS.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e3,"worst_block_us":WORST_BLOCK_NS.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e3,"errors":errors,"note_actions_changed":note_changed,"script_cells_changed":cell_diff(&before,&after),"panel_changed":panel_changed,"panel_size":[before_panel.len(),after_panel.len()], "group_sounds_changed":group_changes,"group_levels_changed":group_levels,"bus_levels_changed":bus_levels,"shared_effects_changed":effects,"modulators_changed":mod_changes,"stage_events":if lean { json!([]) } else { json!(events) },"stage_events_total":stage_total,"route_events":if lean { json!([]) } else { json!(route_events) },"route_events_total":route_total,"rss_kb":rss_kb(),
                    "output":{"before_rms":prior_rms,"after_rms":rms},"sound":sound, "faults_before":before_faults,"faults_after":after_faults,
                    "unhandled_before":before_unhandled,"unhandled_after":after_unhandled,"missing_frames":engine.stats().missing_frames.saturating_sub(before_missing),"settled":ready,
                    "trace_dropped":{"control":dropped_control,"render":dropped_render}}));
            }
            // Back where the preset had it: the next control moves from the
            // preset's state, not from wherever this one was left (a muted
            // room would hide every room control after it).
            if let (Some(id), false) = (widget.source_id, moves.is_empty()) {
                let start = widget.number();
                if lua {
                    if let Ok(id) = u32::try_from(id) {
                        engine.set_widget(slot_id, id, start, None);
                    }
                } else {
                    let v = if matches!(widget.value, Some(Value::Real(_))) {
                        start.to_bits() as i64
                    } else {
                        start.round() as i64
                    };
                    engine.set_control(slot_id, id, v);
                }
                let (_, ready, _) =
                    render_for(&mut engine, &plan, &stream, &[], |_| {}, SETTLE_BLOCKS);
                settled &= ready;
            }
            controls.push(json!({"slot":slot,"widget":widget_index,"name":widget.name,"kind":format!("{:?}",widget.kind),"hidden":widget.hidden,"enabled":widget.enabled,"moves":moves}));
        }
    }
    let at_end = calls(&engine);
    // The sweep's own calls: from the note phase's end, less the probes'.
    let mut ui_from = at_note.clone();
    add(&mut ui_from, &Counts::new(), &probes);
    let phases = json!({
        "load": phase(&Default::default(), &at_load),
        "note": phase(&at_load, &at_note),
        "ui": phase(&ui_from, &at_end),
        "probe_notes": phase(&Counts::new(), &probes),
        // Named by the scripts that resolve the id, as `load` does.
        "unanswered_pars": (engine.unanswered_pars().iter())
            .map(|(p, n)| {
                let i = usize::try_from(p[0] - kontra_ksp::SYMBOL_BASE).ok();
                let mut names: Vec<&str> = (loaded.symbols.iter())
                    .filter_map(|s| s.get(i?).map(|n| &**n))
                    .filter(|n| n.starts_with("$ENGINE_PAR"))
                    .collect();
                names.dedup();
                json!([p, n, names])
            })
            .collect::<Vec<_>>(),
        "faults": format!("{:?}", engine.script_faults()),
    });
    let stats = engine.stats();
    let stream_stats = stream.stats();
    let complete = model_errors.is_empty()
        && settled
        && engine.offline_wait_timeouts() == 0
        && stats.missing_frames == 0
        && stream_stats.read_errors == 0
        && stream_stats.requests_refused == 0
        && truncated.iter().all(|(_, hidden, _)| *hidden);
    let failed = controls
        .iter()
        .flat_map(|c| {
            c.get("moves")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
        })
        .filter(|m| {
            m.get("error").is_some()
                || m.get("errors")
                    .and_then(Json::as_array)
                    .is_some_and(|e| !e.is_empty())
        })
        .count();
    let refused: Vec<_> = (engine.zone_edits_refused().iter().enumerate())
        .filter(|(_, n)| **n > 0)
        .map(|(slot, n)| format!("script {slot}: {n}"))
        .collect();
    let summary = format!(
        "{}: {} UI models, {} controls, {failed} moves with errors; worst block {:.0} us of {:.0} us\n  missing sample frames {}; offline wait timeouts {}; dropped trace events {}; truncated moves {truncated:?}; complete {complete}\n",
        loaded.name,
        models.len(),
        controls.len(),
        WORST_BLOCK_NS.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e3,
        BLOCK as f64 / 48_000.0 * 1e6,
        stats.missing_frames,
        engine.offline_wait_timeouts(),
        engine.trace_dropped()
    );
    // `set_zone_par` calls refused because the queue was full, by script.
    let summary = if refused.is_empty() {
        summary
    } else {
        format!("{summary}  zone edits refused: {}\n", refused.join(", "))
    };
    Ok((
        json!({"schema_version":1,"command":"ui","instrument":path,"engine":"v3","result":{"models":models,"model_errors":model_errors,"controls":controls,"render_complete":complete,"calls":phases,"sampled":{"per_slot":sample,"time_capped":capped},"trace_truncated":truncated,"diagnostics":format!("{:?}",loaded.diagnostics),"unavailable":["foreign VM internal script variables", "complete engine parameter state"],"measurement":"sequential 80ms stereo RMS under held C4; time evolution can change output without a control effect"}}),
        summary,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontra_ui_model::{Display, MenuItem, PageRef, Range, Rect};

    #[test]
    fn targets_use_the_model_range_and_visible_menu_values() {
        let knob = Widget::new(
            "gain",
            PageRef(0),
            Rect::default(),
            Kind::Knob {
                range: Range {
                    min: -2.0,
                    max: 3.0,
                    default: 0.0,
                    step: None,
                },
                display: Display::default(),
            },
        );
        assert_eq!(targets(&knob), Some(vec![-2.0, 3.0]));
        let menu = Widget::new(
            "menu",
            PageRef(0),
            Rect::default(),
            Kind::Menu {
                items: vec![
                    MenuItem {
                        text: "a".into(),
                        value: 42,
                        visible: true,
                    },
                    MenuItem {
                        text: "b".into(),
                        value: 99,
                        visible: false,
                    },
                ],
            },
        );
        assert_eq!(targets(&menu), Some(vec![42.0]));
    }

    #[test]
    fn cell_changes_include_old_and_new_values() {
        let before = vec![(2, vec![Some(1), Some(3)], vec![Some("old".into())])];
        let after = vec![(2, vec![Some(1), Some(4)], vec![Some("new".into())])];
        let changes = cell_diff(&before, &after);
        assert_eq!(
            changes,
            vec![
                json!({"slot":2,"cell":1,"before":3,"after":4}),
                json!({"slot":2,"text_cell":0,"before":"old","after":"new"})
            ]
        );
    }
}
