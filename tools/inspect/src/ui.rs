//! Exercise script controls through the host's UI model, under a held note.
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

fn cells(engine: &Engine, plan: &Plan) -> Vec<(u8, Vec<Option<i64>>, Vec<Option<String>>)> {
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

fn cell_diff(
    before: &[(u8, Vec<Option<i64>>, Vec<Option<String>>)],
    after: &[(u8, Vec<Option<i64>>, Vec<Option<String>>)],
) -> Vec<Json> {
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

fn sound(engine: &Engine) -> Vec<String> {
    engine
        .group_sounds()
        .iter()
        .map(|x| format!("{x:?}"))
        .collect()
}

fn render(
    engine: &mut Engine,
    plan: &Plan,
    stream: &kontra_stream::StreamClient,
    events: &[Timed<Event>],
) -> (f64, bool) {
    let mut samples = [[0.0f32; 2]; BLOCK];
    let mut energy = 0.0f64;
    let mut settled = true;
    for block in 0..BLOCKS {
        settled &= stream.settle(Duration::from_secs(5));
        engine.process(&mut samples, if block == 0 { events } else { &[] });
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
    let (_, mut settled) = render(&mut engine, &plan, &stream, &events);
    engine.clear_trace();
    let mut controls = Vec::new();
    let mut models = Vec::new();
    for slot in 0..loaded.faces.len().max(loaded.pages.len()) {
        let Some(model) = tree(&loaded, &engine, slot)? else {
            continue;
        };
        models.push(json!({"slot":slot, "widgets":model.widgets.iter().map(|w| format!("{w:?}")).collect::<Vec<_>>(), "assets":format!("{:?}",model.assets), "unsupported":format!("{:?}",model.unsupported)}));
        let lua = loaded.pages.get(slot).is_some_and(Option::is_some);
        let slot_id = u8::try_from(slot).context("too many script slots")?;
        let mut index = 0;
        while let Some(widget) =
            tree(&loaded, &engine, slot)?.and_then(|t| t.widgets.get(index).cloned())
        {
            let widget_index = index;
            index += 1;
            let Some(values) = targets(&widget) else {
                continue;
            };
            let mut moves = Vec::new();
            if values.is_empty() {
                moves.push(json!({"error":"control has no selectable values"}));
            }
            for value in values {
                let Some(id) = widget.source_id else {
                    moves.push(json!({"value":value,"error":"control has no source id"}));
                    continue;
                };
                let at = engine.now();
                let off = Event::NoteOff(HostPattern {
                    port: 0,
                    channel: 0,
                    key: 60,
                    id: -1,
                    clap: true,
                });
                let (prior_rms, ready) = render(
                    &mut engine,
                    &plan,
                    &stream,
                    &[Timed { at, body: off }, Timed { at, body: note }],
                );
                settled &= ready;
                engine.clear_trace();
                let before = cells(&engine, &plan);
                let before_sound = sound(&engine);
                let before_mods: Vec<_> = engine.mods().map(|m| format!("{m:?}")).collect();
                let before_faults = format!("{:?}", engine.script_faults());
                let before_unhandled = format!("{:?}", engine.unhandled_actions());
                let before_missing = engine.stats().missing_frames;
                engine.clear_trace();
                if lua {
                    if let Ok(id) = u32::try_from(id) {
                        engine.set_widget(slot_id, id, value);
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
                engine.clear_trace();
                let (rms, ready) = render(&mut engine, &plan, &stream, &[]);
                settled &= ready;
                events.extend(
                    engine
                        .trace_events()
                        .iter()
                        .map(|e| super::trace_output::event(e, &plan, &zone_samples, RATE)),
                );
                engine.clear_trace();
                let after = cells(&engine, &plan);
                let after_sound = sound(&engine);
                let after_mods: Vec<_> = engine.mods().map(|m| format!("{m:?}")).collect();
                let group_changes: Vec<_> = before_sound
                    .iter()
                    .zip(&after_sound)
                    .enumerate()
                    .filter_map(|(i, (a, b))| {
                        (a != b).then(|| json!({"group":i,"before":a,"after":b}))
                    })
                    .collect();
                let mod_changes: Vec<_> = before_mods
                    .iter()
                    .zip(&after_mods)
                    .enumerate()
                    .filter_map(|(i, (a, b))| {
                        (a != b).then(|| json!({"index":i,"before":a,"after":b}))
                    })
                    .collect();
                moves.push(json!({"value":value,"script_cells_changed":cell_diff(&before,&after), "group_sounds_changed":group_changes,"modulators_changed":mod_changes,"stage_events":events,
                    "output":{"before_rms":prior_rms,"after_rms":rms}, "faults_before":before_faults,"faults_after":format!("{:?}",engine.script_faults()),
                    "unhandled_before":before_unhandled,"unhandled_after":format!("{:?}",engine.unhandled_actions()),"missing_frames":engine.stats().missing_frames.saturating_sub(before_missing),"settled":ready}));
            }
            controls.push(json!({"slot":slot,"widget":widget_index,"name":widget.name,"kind":format!("{:?}",widget.kind),"hidden":widget.hidden,"enabled":widget.enabled,"moves":moves}));
        }
    }
    let stats = engine.stats();
    let stream_stats = stream.stats();
    let complete = settled
        && engine.offline_wait_timeouts() == 0
        && stats.missing_frames == 0
        && stream_stats.read_errors == 0
        && stream_stats.requests_refused == 0
        && engine.trace_dropped() == 0;
    let summary = format!(
        "{}: {} UI models, {} controls exercised\n  missing sample frames {}; offline wait timeouts {}; dropped trace events {}; complete {complete}\n",
        loaded.name,
        models.len(),
        controls.len(),
        stats.missing_frames,
        engine.offline_wait_timeouts(),
        engine.trace_dropped()
    );
    Ok((
        json!({"schema_version":1,"command":"ui","instrument":path,"engine":"v3","result":{"models":models,"controls":controls,"render_complete":complete,"diagnostics":format!("{:?}",loaded.diagnostics),"unavailable":["foreign VM internal script variables", "complete engine parameter state"],"measurement":"sequential 80ms stereo RMS under held C4; time evolution can change output without a control effect"}}),
        summary,
    ))
}
