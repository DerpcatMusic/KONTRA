//! v1-UVI adapter to the existing shared scanner. Decrypted content stays in memory.
use super::{
    access,
    host::UiKind,
    library::Library,
    script::{Input, InputKind},
    ufs::Ufs,
    ui_assets::UiAssets,
    worker::{Request, Stamp, StartConfig, Worker},
};
use crate::scan_metrics as metrics;
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};
pub fn one(id: &str, out: &Path) -> Value {
    let mut r = json!({"loads":"no","ui":"error","plays_note":"no","controls_bound":"0/0","load_ms":0.,"first_audio_ms":null,"ui_first_frame_ms":null,"cache_state":"cold","stage":"bank access","reason":"UVI initialization failed"});
    metrics::checkpoint(out, &r);
    let t = Instant::now();
    let run = (|| -> anyhow::Result<()> {
        anyhow::ensure!(
            std::env::var("KONTRA_UVI_STATIC_PCM_CACHE").ok().as_deref() != Some("1"),
            "scanner requires PCM cache disabled"
        );
        let (path, member) = id
            .split_once("::")
            .ok_or_else(|| anyhow::anyhow!("UVI bank/member required"))?;
        let reader = access::ReaderNamespaces::open(&access::reader_path(
            std::env::var_os("KONTRA_UVI_READER")
                .as_deref()
                .map(Path::new),
        )?)?;
        let bank = Ufs::open(Path::new(path))?;
        let directory = bank.decode_directory(&reader.metadata)?;
        // Recovery is pure; unlike ensure_content_state it never writes an access record.
        let state = if directory.files.iter().any(|m| m.mode == 2) {
            Some(access::recover_content_state(
                Path::new(path),
                &bank,
                &directory,
            )?)
        } else {
            None
        };
        let library = Library::open(
            Path::new(path),
            &reader.metadata,
            state.as_ref().map(|s| s.key),
        )?;
        let program = library.program(member, &reader.program)?;
        r["program_read"] = json!(true);
        r["zones"] = json!(program.program.sample_zones.len());
        let rejected = super::playback::preflight(&program.program);
        r["preflight_rejections"] = json!(rejected.len());
        let mut rejection_kinds = std::collections::BTreeMap::<String, usize>::new();
        for rejection in rejected {
            *rejection_kinds.entry(rejection.kind).or_default() += 1;
        }
        r["preflight_kinds"] = json!(rejection_kinds);
        let mut pick = (0..=127u8)
            .map(|k| {
                let n = program
                    .program
                    .sample_zones
                    .iter()
                    .filter(|z| {
                        !z.bypassed
                            && !z.purged
                            && (z.low_key..=z.high_key).contains(&k)
                            && (z.low_velocity..=z.high_velocity).contains(&64)
                    })
                    .count();
                (n, std::cmp::Reverse(k.abs_diff(60)), k)
            })
            .filter(|(n, ..)| *n > 0)
            .max_by_key(|(_, distance, key)| (*distance, *key))
            .map(|(.., k)| (k, 64))
            .or_else(|| {
                program
                    .program
                    .sample_zones
                    .iter()
                    .find(|z| !z.bypassed && !z.purged)
                    .map(|z| {
                        (
                            z.low_key,
                            ((u16::from(z.low_velocity) + u16::from(z.high_velocity)) / 2).max(1)
                                as u8,
                        )
                    })
            })
            .unwrap_or((60, 64));
        let config = StartConfig {
            bank: Path::new(path).into(),
            expected_bank_uuid: Some(bank.header.uuid),
            member: member.into(),
            metadata_namespace: reader.metadata,
            program_namespace: reader.program,
            content_key: state.as_ref().map(|s| s.key),
            content_bank: state.and_then(|s| s.bank),
            sample_rate: 48000,
        };
        let mut assets = UiAssets::open(&config)?;
        r["stage"] = json!("worker initialization");
        metrics::checkpoint(out, &r);
        // Section J starts at the production import, after the metadata/UI-asset prepass.
        let onset_start = Instant::now();
        let mut worker = Worker::start(config, 1, 1)?;
        if worker.wait_ready(Duration::from_secs(60)).is_err() {
            r["initialization_errors"] = json!(worker.stats().errors);
            worker.stop();
            return Err(anyhow::anyhow!("initialization failed"));
        }
        r["loads"] = json!("yes");
        r["load_ms"] = json!(t.elapsed().as_secs_f64() * 1000.);
        let mut views = Vec::new();
        let mut snapshots = Vec::new();
        let mut pending_paints = Vec::new();
        let (mut visible, mut bound, mut ui_error, mut blank) = (0, 0, false, false);
        r["stage"] = json!("Original UI");
        metrics::checkpoint(out, &r);
        for processor in worker.ui_processors().into_iter().take(64) {
            let request = worker.request_ui_snapshot(processor)?;
            let started = Instant::now();
            loop {
                if let Some(reply) = worker.poll_ui_snapshot() {
                    if reply.request != request {
                        continue;
                    }
                    match reply.snapshot {
                        Err(_) => {
                            ui_error = true;
                            views.push(json!({"snapshot_error":true}));
                        }
                        Ok(snapshot) => {
                            let interactive = snapshot
                                .widgets
                                .iter()
                                .filter(|w| {
                                    w.effective_visible
                                        && matches!(
                                            w.kind,
                                            UiKind::XY
                                                | UiKind::Menu
                                                | UiKind::Table
                                                | UiKind::Slider
                                                | UiKind::Knob
                                                | UiKind::NumBox
                                                | UiKind::Button
                                                | UiKind::OnOffButton
                                        )
                                })
                                .count();
                            visible += interactive;
                            bound += interactive;
                            let shown = snapshot
                                .widgets
                                .iter()
                                .filter(|w| w.effective_visible)
                                .count();
                            blank |= shown == 0;
                            views.push(json!({"widgets":snapshot.widgets.len(),"visible":shown,"interactive":interactive,"bound":interactive,"render":null}));
                            pending_paints.push((views.len() - 1, reply.stamp));
                            snapshots.push(snapshot);
                        }
                    }
                    break;
                }
                if started.elapsed() > Duration::from_secs(2) {
                    ui_error = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let colours = crate::plugin::uvi_ui::KeyColours::merge(&snapshots);
        let native_valid_keys: Vec<u8> = colours
            .colours
            .iter()
            .filter(|(_, c)| c.eq_ignore_ascii_case("#00FFFFFF"))
            .map(|(&k, _)| k)
            .collect();
        let native_preferred = native_valid_keys
            .iter()
            .copied()
            .filter(|k| {
                program.program.sample_zones.iter().any(|z| {
                    !z.bypassed
                        && !z.purged
                        && (z.low_key..=z.high_key).contains(k)
                        && (z.low_velocity..=z.high_velocity).contains(&64)
                })
            })
            .min_by_key(|k| k.abs_diff(60))
            .map(|k| (k, 64));
        r["native_valid_keys"] = json!(native_valid_keys);
        r["native_key_conflicts"] = json!(colours.conflicts);
        r["native_preferred_note"] = json!(native_preferred);
        let planned = metrics::note(0);
        let pick_source = if planned.is_some() {
            "shared-note-plan"
        } else if native_preferred.is_some() {
            "native-valid-keys"
        } else {
            "active-zone-nearest60-fallback"
        };
        if let Some(note) = planned.or(native_preferred) {
            pick = note;
        }
        r["pick_source"] = json!(pick_source);
        r["pick"] = json!(pick);
        r["programs"] = json!([{"source":"uvi","program":0,"pick":pick,"pick_source":pick_source,
            "native_valid_keys":native_valid_keys,"native_preferred_note":native_preferred,
            "first_audio_ms":null,"ui_first_frame_ms":r["ui_first_frame_ms"],"cache_state":"cold",
            "load_path":"v1 UVI production worker; full PCM"}]);
        r["controls_bound"] = json!(format!("{bound}/{visible}"));
        r["stage"] = json!("play and Original paint");
        metrics::checkpoint(out, &r);
        let mut port = worker
            .take_audio_port()
            .ok_or_else(|| anyhow::anyhow!("audio port unavailable"))?;
        let mut peak = 0f32;
        let mut nonfinite = 0;
        let mut first_audio_ms = None;
        // Retain pre-audition native declarations, then paint off the audio observation thread.
        let (audio_result, paint_result) = std::thread::scope(|scope| {
            let painting = scope.spawn(|| {
                let mut first_frame = None;
                let mut error = false;
                for (snapshot, &(view, stamp)) in snapshots.iter().zip(&pending_paints) {
                    let paint =
                        crate::ui::scan_uvi::paint(snapshot, &mut assets, stamp, onset_start);
                    if first_frame.is_none()
                        && let Ok(frame) = &paint
                    {
                        first_frame = frame["ui_first_frame_ms"].as_f64();
                    }
                    error |= paint.is_err();
                    views[view]["render"] =
                        paint.unwrap_or_else(|e| metrics::error("Original paint", e));
                }
                (error, first_frame)
            });
            let audio_result = (|| -> anyhow::Result<()> {
                for block in 0..96 {
                    let stamp = Stamp {
                        epoch: 1,
                        generation: 1,
                        frame: block * 256,
                    };
                    let input = if block == 0 {
                        vec![
                            Input {
                                frame: 0,
                                kind: InputKind::Controller {
                                    channel: 0,
                                    controller: 1,
                                    value: 100,
                                },
                            },
                            Input {
                                frame: 0,
                                kind: InputKind::Controller {
                                    channel: 0,
                                    controller: 11,
                                    value: 127,
                                },
                            },
                            Input {
                                frame: 0,
                                kind: InputKind::NoteOn {
                                    channel: 0,
                                    note: pick.0,
                                    velocity: pick.1,
                                },
                            },
                        ]
                    } else if block == 72 {
                        vec![Input {
                            frame: stamp.frame,
                            kind: InputKind::NoteOff {
                                channel: 0,
                                note: pick.0,
                            },
                        }]
                    } else {
                        Vec::new()
                    };
                    port.realtime()
                        .try_submit(Request::new(stamp, &input)?)
                        .map_err(|_| anyhow::anyhow!("packet rejected"))?;
                    let start = Instant::now();
                    let packet = loop {
                        if let Some(packet) = port.realtime().try_receive_available(stamp)? {
                            break packet;
                        }
                        if start.elapsed() > Duration::from_secs(2) {
                            return Err(anyhow::anyhow!("audio packet timeout"));
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    };
                    // Observe received PCM before pacing sleep; never derive onset from load_ms.
                    observe_first_audio(&mut first_audio_ms, onset_start, &packet.audio);
                    if let Some(wait) =
                        Duration::from_secs_f64(256.0 / 48000.0).checked_sub(start.elapsed())
                    {
                        std::thread::sleep(wait);
                    }
                    for x in packet.audio.into_iter().flatten() {
                        if x.is_finite() {
                            peak = peak.max(x.abs());
                        } else {
                            nonfinite += 1;
                        }
                    }
                }
                Ok(())
            })();
            (audio_result, painting.join())
        });
        match paint_result {
            Ok((error, first_frame)) => {
                ui_error |= error;
                r["ui_first_frame_ms"] = json!(first_frame);
            }
            Err(_) => {
                ui_error = true;
                r["paint_worker_failed"] = json!(true);
            }
        }
        r["programs"][0]["ui_first_frame_ms"] = r["ui_first_frame_ms"].clone();
        let diag = assets.diagnostics();
        r["ui"] = json!(if ui_error {
            "error"
        } else if blank {
            "blank"
        } else if diag.failed > 0 || diag.limited > 0 {
            "missing-images"
        } else if views.is_empty() {
            "no-ui"
        } else {
            "original-ok"
        });
        r["controls_bound"] = json!(format!("{bound}/{visible}"));
        r["views"] = json!(views);
        r["programs"][0]["views"] = r["views"].clone();
        r["asset_failures"] = json!(diag.failed);
        r["font_failures"] = json!(diag.font_failed);
        r["asset_limit"] = json!(diag.limited);
        r["ui_resident_bytes"] = json!(assets.resident_bytes());
        drop(port);
        let stats = worker.stats();
        r["sample_resident_bytes"] = json!(stats.resource_resident_pcm_bytes);
        r["programs"][0]["sample_resident_bytes"] = r["sample_resident_bytes"].clone();
        r["render_ns"] = json!(stats.render_ns);
        r["render_cpu_ns"] = json!(stats.render_cpu_ns);
        r["render_deadline_misses"] = json!(stats.render_deadline_misses);
        r["runtime_errors"] = json!(stats.errors);
        worker.stop();
        r["peak"] = json!(peak);
        r["nonfinite"] = json!(nonfinite);
        r["pick"] = json!(pick);
        let audible = peak > 1e-5 && nonfinite == 0;
        r["first_audio_ms"] = json!(first_audio_ms);
        r["programs"][0]["first_audio_ms"] = r["first_audio_ms"].clone();
        audio_result?;
        r["plays_note"] = json!(if audible { "yes" } else { "silent" });
        r["reason"] =
            json!("v1 UVI production worker; Original panel; 0.5s audition; no native comparison");
        r["stage"] = json!("complete");
        Ok(())
    })();
    if let Err(e) = run {
        r["failure"] = metrics::error(r["stage"].as_str().unwrap_or("probe"), e);
    }
    r
}

fn observe_first_audio(first: &mut Option<f64>, load_start: Instant, audio: &[[f32; 2]]) {
    if first.is_none() && audio.iter().flatten().any(|x| x.is_finite() && *x != 0.) {
        *first = Some(load_start.elapsed().as_secs_f64() * 1000.);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn onset_requires_actual_finite_output_and_keeps_first_observation() {
        let start = std::time::Instant::now();
        let mut first = None;
        super::observe_first_audio(&mut first, start, &[[0., -0.], [f32::NAN, f32::INFINITY]]);
        assert_eq!(first, None);
        // A finite signal below the separate audible threshold still has an onset.
        super::observe_first_audio(&mut first, start, &[[0., 0.], [0., 1e-8]]);
        assert!(first.is_some_and(|ms| ms.is_finite() && ms >= 0.));
        let observed = first;
        super::observe_first_audio(&mut first, start, &[[0.2, 0.]]);
        assert_eq!(first, observed);
    }
}
