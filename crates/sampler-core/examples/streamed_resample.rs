//! Synthetic high-pitch source workload; original library cells remain the CPU gate.
use sampler_core::*;
use serde_json::json;
use std::time::Instant;
#[path = "../tests/support/mod.rs"]
mod support;

fn main() {
    assert!(!cfg!(debug_assertions), "use --profile corpus");
    for voices in [8, 64] {
        for step in [1.5f64, 2.5, 4., 8., 16.] {
            for streamed in [false, true] {
                let started = Instant::now();
                let data = vec![[0.125, -0.25]; PAGE_FRAMES * 8];
                let pcm = if streamed {
                    Pcm::streamed(48000, data.len()).unwrap()
                } else {
                    Pcm::new(48000, data.clone().into_boxed_slice()).unwrap()
                };
                let (mut cache, mut worker) = StreamCache::new(voices * 3).unwrap();
                if streamed {
                    for page in 0..8 {
                        cache.request(&pcm, page, 0).unwrap();
                        let mut job = worker.next_job().unwrap();
                        let range = job.range();
                        job.frames_mut().copy_from_slice(&data[range]);
                        worker.complete(job, Ok(())).unwrap();
                        assert!(matches!(cache.poll(), Some(PageUpdate::Loaded(_))));
                    }
                }
                let region = Region {
                    sample: 0,
                    key_low: 60,
                    key_high: 60,
                    root_key: None,
                    velocity_low: 0.,
                    velocity_high: 1.,
                    gain: 1.,
                    envelope: Envelope::default(),
                    playback: Playback {
                        start: 4096,
                        end: Some(24576),
                        transpose_semitones: 12. * step.log2(),
                        loop_range: Some(Loop {
                            start: 4096,
                            end: 24576,
                            shape: LoopShape::Wrap,
                            mode: LoopMode::Continuous,
                            passes: None,
                        }),
                        ..Playback::default()
                    },
                };
                let plan = Prepared::new(48000, vec![pcm], vec![region], 1).unwrap();
                let limits = Limits::for_plan(&plan, voices, voices);
                let mut rt = Runtime::new(plan, limits).unwrap();
                if streamed {
                    rt = rt.with_stream_cache(cache);
                }
                let mut audio = [[0.; 2]; 64];
                support::without_heap(|| {
                    for id in 0..voices {
                        rt.trigger(
                            Input {
                                protocol: Protocol::Native,
                                port: 0,
                                group: 0,
                                channel: 0,
                                key: 60,
                                external_id: Some(id as i32),
                            },
                            60,
                            1.,
                        )
                        .unwrap();
                    }
                    for _ in 0..32 {
                        rt.render(&mut audio).unwrap();
                    }
                });
                let setup_ms = started.elapsed().as_secs_f64() * 1000.;
                let mut times = [0u128; 256];
                support::without_heap(|| {
                    for elapsed in &mut times {
                        let started = Instant::now();
                        rt.render(&mut audio).unwrap();
                        *elapsed = started.elapsed().as_nanos();
                        assert_eq!(rt.voice_count(), voices);
                        for frame in audio {
                            assert!((frame[0] - voices as f32 * 0.125).abs() < 1e-3);
                            assert!((frame[1] + voices as f32 * 0.25).abs() < 1e-3);
                        }
                    }
                });
                times.sort_unstable();
                assert_eq!(rt.stream_underruns(), 0);
                println!(
                    "{}",
                    json!({"voices":voices,"step":step,"streamed":streamed,"block":64,"setup_ms":setup_ms,"p50_us":times[128] as f64/1000.,"p99_us":times[252] as f64/1000.,"underruns":0,"audio_heap_calls":0,"dc_pcm_ok":true})
                );
            }
        }
    }
}
