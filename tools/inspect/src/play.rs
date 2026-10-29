//! Offline performance and between-block engine observations.
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use kontra_engine::{Engine, Prepared, SampleTime, Timed};
use kontra_stream::{Store, StoreConfig};
use serde_json::{Value, json};

use super::{RATE, scenario};

const BLOCK: usize = 128;
const TAIL_SECONDS: u64 = 20;

pub fn run(path: &Path, scenario_name: &str) -> Result<(Value, String)> {
    let events = if scenario_name == "user" {
        scenario::user()
    } else {
        scenario::file(Path::new(scenario_name))?
    };
    let end = events.last().map_or(0, |e| e.frame);
    ensure!(end <= u64::from(RATE) * 3600, "scenario exceeds one hour");
    let last_off = events
        .iter()
        .filter(|e| e.description.get("type").and_then(Value::as_str) == Some("note_off"))
        .map(|e| e.frame)
        .max()
        .context("scenario needs a note-off")?;
    let stop = end.max(last_off + TAIL_SECONDS * u64::from(RATE));
    let ir = kontra_host::instrument::read(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    let samples: Vec<_> = ir.samples.iter().map(|s| s.name.clone()).collect();
    let group_samples: Vec<Vec<usize>> = ir
        .groups
        .iter()
        .map(|g| g.zones.iter().map(|z| z.sample).collect())
        .collect();
    let mut store = Store::new(StoreConfig::DEFAULT).context("creating sample store")?;
    let loaded = kontra_host::instrument::build(ir, RATE, &mut store);
    let diagnostics: Vec<_> = loaded
        .diagnostics
        .iter()
        .map(|d| json!({"impact":format!("{:?}",d.impact), "feature":d.feature, "detail":d.detail}))
        .collect();
    let plan = Arc::new(loaded.plan);
    let zone_samples: Vec<_> = plan
        .zones
        .iter()
        .map(|z| {
            usize::try_from(z.group)
                .ok()
                .and_then(|g| group_samples.get(g))
                .and_then(|g| g.get(usize::from(z.member)))
                .and_then(|id| samples.get(*id))
                .cloned()
        })
        .collect();
    let stream = store.client();
    let mut engine = Engine::new(Arc::clone(&plan), stream.clone(), RATE);
    let mut at = 0u64;
    let mut next = 0usize;
    let mut next_tail = last_off;
    let mut trace = Vec::new();
    let mut peak = 0f32;
    let mut jump = 0f32;
    let mut clipped = 0u64;
    let mut prior = [0f32; 2];
    let mut audio = [[0f32; 2]; BLOCK];
    while at < stop {
        let frames = usize::try_from((stop - at).min(BLOCK as u64))?;
        let until = at + frames as u64;
        let mut timed = Vec::new();
        let mut cc_in_block = false;
        while let Some(e) = events.get(next).filter(|e| e.frame < until) {
            cc_in_block |= e.description.get("type").and_then(Value::as_str) == Some("cc");
            timed.push(Timed {
                at: SampleTime(e.frame),
                body: e.event,
            });
            next += 1;
        }
        stream.settle(Duration::from_secs(5));
        engine.process(&mut audio[..frames], &timed);
        let mut requests = Vec::new();
        engine.drain_requests(|r| requests.push(r));
        for request in requests {
            engine.install(Prepared::new(request, Some(&plan), RATE));
        }
        for frame in &audio[..frames] {
            for (sample, previous) in frame.iter().zip(&mut prior) {
                peak = peak.max(sample.abs());
                jump = jump.max((sample - *previous).abs());
                clipped += u64::from(sample.abs() > 1.0);
                *previous = *sample;
            }
        }
        at = until;
        if cc_in_block || (at >= next_tail && at >= last_off) {
            let voices: Vec<_> = engine
                .inspect_voices()
                .map(|v| {
                    json!({
                        "zone":v.zone, "sample":zone_samples.get(v.zone), "position":v.position,
                        "released":v.released, "gain_before_sample_and_fx":v.gain,
                    })
                })
                .collect();
            let routes: Vec<_> = engine.inspect_eq_routes().iter().map(|r| json!({
                "zone":r.zone, "name":r.name, "insert":r.insert, "band":r.band,
                "routed_source":r.source, "contribution_db":r.contribution_db, "band_gain_db":r.band_gain_db,
            })).collect();
            trace.push(json!({"frame":at, "cc1":engine.controller(1), "cc11":engine.controller(11), "voices":voices, "eq_routes":routes}));
            if at >= next_tail && at >= last_off {
                next_tail += u64::from(RATE);
            }
        }
    }
    let left: Vec<_> = engine
        .inspect_voices()
        .map(|v| {
            json!({"zone":v.zone,
        "sample":zone_samples.get(v.zone), "position":v.position, "released":v.released,
        "gain_before_sample_and_fx":v.gain})
        })
        .collect();
    let summary = format!(
        "{}: scenario {scenario_name}, {} input events\n  peak {peak:.6}, clipped samples {clipped}, max sample jump {jump:.6}\n  {} voices active {:.3} s after last note-off (per-voice gain in JSON; not isolated audio level)\n  EQ route snapshots: {}\n",
        loaded.name,
        events.len(),
        left.len(),
        (stop - last_off) as f64 / f64::from(RATE),
        trace.len()
    );
    Ok((
        json!({"schema_version":1,"command":"play","instrument":path,"engine":"v3",
        "result":{"scenario":scenario_name,"rate":RATE,"events":events.iter().map(|e|json!({"frame":e.frame,"event":e.description})).collect::<Vec<_>>(),
        "trace":trace,"remaining_voices":left,"metrics":{"peak":peak,"clipped_samples":clipped,"max_sample_jump":jump},
        "diagnostics":diagnostics,"unavailable":["script callbacks and commands by event","voice lifecycle events","isolated voice audio levels","non-EQ route values","octave bands","HF burst energy"]}}),
        summary,
    ))
}
