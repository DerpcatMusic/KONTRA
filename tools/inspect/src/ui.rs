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

use super::RATE;

const BLOCK: usize = 128;
const BLOCKS: usize = 30;

fn targets(widget: &Widget) -> Option<Vec<f64>> {
    let values = match &widget.kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } | Kind::ValueEdit { range, .. } => {
            vec![range.min, range.max]
        }
        Kind::Switch | Kind::Button { .. } => vec![1.0, 0.0],
        Kind::Menu { items } => items
            .iter()
            .filter(|i| i.visible)
            .map(|i| f64::from(i.value))
            .collect(),
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

fn render(
    engine: &mut Engine,
    plan: &Plan,
    stream: &kontra_stream::StreamClient,
    events: &[Timed<Event>],
    mut keep: impl FnMut(&Engine),
) -> (f64, bool) {
    let mut samples = [[0.0f32; 2]; BLOCK];
    let mut energy = 0.0f64;
    let mut settled = true;
    for block in 0..BLOCKS {
        settled &= stream.settle(Duration::from_secs(5));
        engine.process(&mut samples, if block == 0 { events } else { &[] });
        // Each block's trace is taken, then cleared: the buffer holds one
        // block, so a long sweep drops nothing.
        keep(engine);
        engine.clear_trace();
        for sample in samples.iter().flatten() {
            energy += f64::from(*sample).powi(2);
        }
        let mut requests = Vec::new();
        engine.drain_requests(|r| requests.push(r));
        for request in requests {
            engine.install(Prepared::new(request, Some(plan), RATE));
        }
    }
    ((energy / (BLOCK * BLOCKS * 2) as f64).sqrt(), settled)
}

pub fn run(path: &Path) -> Result<(Json, String)> {
    let instrument = kontra_host::instrument::read(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
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
    engine.set_offline_wait(true);
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
    let (_, mut settled) = render(&mut engine, &plan, &stream, &events, |_| {});
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
            if (seen.len() - 1) % stride != 0 {
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
                    let (rms, ready) = render(
                        engine,
                        &plan,
                        &stream,
                        &[Timed { at, body: off }, Timed { at, body: note }],
                        |engine| note_fields(engine, &plan, &zone_samples, &mut acts, &mut names),
                    );
                    (rms, acts, ready)
                };
                let (_, first, ready_first) = fresh(&mut engine);
                let (prior_rms, base, ready) = fresh(&mut engine);
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
                let (rms, ready) = render(&mut engine, &plan, &stream, &[], |engine| {
                    events.extend(
                        (engine.trace_events().iter())
                            .map(|e| super::trace_output::event(e, &plan, &zone_samples, RATE)),
                    );
                    route_events.extend(
                        (engine.trace_route_values())
                            .map(|v| super::trace_output::route(&v, &plan, &zone_samples)),
                    );
                });
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
                let (_, next, ready_next) = fresh(&mut engine);
                let (_, again, ready_again) = fresh(&mut engine);
                settled &= ready_next && ready_again;
                let note_changed = note_diff(&first, &base, &next, &again);
                moves.push(json!({"value":value,"errors":errors,"note_actions_changed":note_changed,"script_cells_changed":cell_diff(&before,&after),"panel_changed":panel_changed,"panel_size":[before_panel.len(),after_panel.len()], "group_sounds_changed":group_changes,"group_levels_changed":group_levels,"bus_levels_changed":bus_levels,"shared_effects_changed":effects,"modulators_changed":mod_changes,"stage_events":events,"route_events":route_events,
                    "output":{"before_rms":prior_rms,"after_rms":rms}, "faults_before":before_faults,"faults_after":after_faults,
                    "unhandled_before":before_unhandled,"unhandled_after":after_unhandled,"missing_frames":engine.stats().missing_frames.saturating_sub(before_missing),"settled":ready,
                    "trace_dropped":{"control":dropped_control,"render":dropped_render}}));
            }
            controls.push(json!({"slot":slot,"widget":widget_index,"name":widget.name,"kind":format!("{:?}",widget.kind),"hidden":widget.hidden,"enabled":widget.enabled,"moves":moves}));
        }
    }
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
        "{}: {} UI models, {} controls, {failed} moves with errors\n  missing sample frames {}; offline wait timeouts {}; dropped trace events {}; truncated moves {truncated:?}; complete {complete}\n",
        loaded.name,
        models.len(),
        controls.len(),
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
        json!({"schema_version":1,"command":"ui","instrument":path,"engine":"v3","result":{"models":models,"model_errors":model_errors,"controls":controls,"render_complete":complete,"sampled":{"per_slot":sample,"time_capped":capped},"trace_truncated":truncated,"diagnostics":format!("{:?}",loaded.diagnostics),"unavailable":["foreign VM internal script variables", "complete engine parameter state"],"measurement":"sequential 80ms stereo RMS under held C4; time evolution can change output without a control effect"}}),
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
