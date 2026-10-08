//! Single-instrument production loader/worker probe; only aggregate metadata is saved.
use kontakto::sound::{
    BlockInfo, Core, CoreLoader, LoadRequest,
    event::{Event, HostNote, HostPattern},
    v2::{V2Core, V2Loader},
};
use std::{
    io::Write,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::panic::set_hook(Box::new(|_| {}));
    let args: Vec<_> = std::env::args().collect();
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&args[2])?;
    for id in [&args[1]] {
        let (bank, member) = id.split_once("::").unwrap();
        let request = LoadRequest {
            path: std::path::Path::new(bank).join(member),
            sample_rate: 48000.0,
            ..Default::default()
        };
        let t = Instant::now();
        let record = match V2Loader.prepare(&request, &mut |_| {}, &|| false) {
            Err(_) => {
                serde_json::json!({"id":id,"loaded":false,"load_ms":t.elapsed().as_secs_f64()*1000.0})
            }
            Ok(loaded) => {
                let load_ms = t.elapsed().as_secs_f64() * 1000.0;
                let controls = loaded.controls.len();
                let init_errors = loaded
                    .report
                    .missing
                    .iter()
                    .filter(|m| m.feature == "lua error")
                    .count();
                let budget_findings = loaded
                    .report
                    .missing
                    .iter()
                    .filter(|m| m.feature == "lua error" && m.value.contains("time budget exceeded"))
                    .count();
                let resident = loaded.stream.as_ref().map(|s| s.resident_bytes());
                let mut mids: Vec<_> = loaded
                    .instrument
                    .as_ref()
                    .map(|i| {
                        i.zones
                            .iter()
                            .map(|z| ((u16::from(z.keys.low) + u16::from(z.keys.high)) / 2) as u8)
                            .collect()
                    })
                    .unwrap_or_else(Vec::new);
                mids.sort_unstable();
                let key = mids.get(mids.len() / 2).copied().unwrap_or(60);
                let mut core = V2Core::with_parts(1, 48000.0);
                core.install(0, loaded.part);
                for (cc, value) in [(1, 100), (2, 100), (11, 127)] {
                    core.play(0, Event::midi1(0xb0, cc, value));
                }
                let note = HostNote {
                    port: 0,
                    channel: 0,
                    key,
                    id: 1,
                    clap: true,
                };
                core.play(
                    0,
                    Event::NoteOn {
                        note,
                        velocity: 100.0 / 127.0,
                        tune: 0.0,
                    },
                );
                let (mut peak, mut render_ms, mut max_ms, mut misses) = (0f32, 0f64, 0f64, 0usize);
                let (mut first_signal, mut nonfinite) = (None, 0usize);
                for block in 0..384 {
                    if block == 288 {
                        core.play(
                            0,
                            Event::NoteOff(HostPattern {
                                port: 0,
                                channel: 0,
                                key: i32::from(key),
                                id: 1,
                                clap: true,
                            }),
                        );
                    }
                    let start = Instant::now();
                    core.begin_block(&BlockInfo {
                        frames: 64,
                        ..Default::default()
                    });
                    let render = core.render(64);
                    for bus in render.buses {
                        for channel in bus {
                            for x in &channel[..64] {
                                if !x.is_finite() {
                                    nonfinite += 1;
                                } else {
                                    peak = peak.max(x.abs());
                                    if x.abs() > 1e-4 && first_signal.is_none() {
                                        first_signal = Some(block * 64);
                                    }
                                }
                            }
                        }
                    }
                    core.end_block(64, &mut |_| true);
                    let ms = start.elapsed().as_secs_f64() * 1000.0;
                    render_ms += ms;
                    max_ms = max_ms.max(ms);
                    misses += usize::from(ms > 64.0 / 48.0);
                    if let Some(wait) =
                        Duration::from_secs_f64(64.0 / 48000.0).checked_sub(start.elapsed())
                    {
                        std::thread::sleep(wait);
                    }
                }
                serde_json::json!({"id":id,"loaded":true,"load_ms":load_ms,"controls":controls,"init_errors":init_errors,"budget_findings":budget_findings,"stream_resident_bytes":resident,"key":key,"peak":peak,"first_signal_frame":first_signal,"nonfinite":nonfinite,"render_ms":render_ms,"block_max_ms":max_ms,"deadline_misses":misses,"problems":core.problems(0)})
            }
        };
        writeln!(out, "{record}")?;
        out.flush()?;
    }
    Ok(())
}
