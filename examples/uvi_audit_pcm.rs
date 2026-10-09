//! Offline A/B witness; writes a WAV or a numeric PCM hash without retaining audio.
use kontakto::sound::{
    BlockInfo, Core, CoreLoader, LoadRequest,
    event::Event,
    v2::{V2Core, V2Loader},
};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        5,
        "PATH BLOCK piano|strings|fx|gate OUTPUT.wav|--hash"
    );
    let block: usize = args[2].parse().unwrap();
    assert!([32, 64, 256].contains(&block));
    let loaded = V2Loader
        .prepare(
            &LoadRequest {
                path: args[1].clone().into(),
                sample_rate: 48000.,
                program: std::env::var("PROBE_PROGRAM")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                ..Default::default()
            },
            &mut |_| {},
            &|| false,
        )
        .unwrap_or_else(|_| panic!("load failed; authored diagnostics omitted"));
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, loaded.part);
    let keys: Vec<u8> = if args[3] == "gate" {
        vec![
            std::env::var("PROBE_NOTE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
        ]
    } else if args[3] == "piano" {
        vec![48, 52, 55, 60, 64, 67, 72, 76]
    } else {
        (48..60).collect()
    };
    let mut events = vec![(0, 0xb0, 1, 110), (0, 0xb0, 11, 127), (0, 0xb0, 64, 127)];
    if let Ok(switch) = std::env::var("PROBE_KEYSWITCH") {
        let key: u8 = switch.parse().unwrap();
        core.begin_block(&BlockInfo {
            frames: 128,
            offline: true,
            ..Default::default()
        });
        core.event(0, Event::midi1(0x90, key, 64));
        core.render(128);
        core.event(0, Event::midi1(0x80, key, 0));
        core.end_block(128, &mut |_| true);
    }
    let velocity = if args[3] == "gate" { 64 } else { 100 };
    for key in keys {
        events.push((0, 0x90, key, velocity));
        events.push((48000, 0x80, key, 0));
    }
    events.push((144000, 0xb0, 64, 0));
    events.sort_by_key(|e| e.0);
    let mut samples = Vec::with_capacity(192000 * 2);
    let mut next = 0;
    for begin in (0..192000).step_by(block) {
        while next < events.len() && events[next].0 < begin + block {
            let (_, status, a, b) = events[next];
            core.event(0, Event::midi1(status, a, b));
            next += 1;
        }
        core.begin_block(&BlockInfo {
            frames: block,
            offline: true,
            ..Default::default()
        });
        for offset in (0..block).step_by(128) {
            let len = (block - offset).min(128);
            let rendered = core.render(len);
            for i in 0..len {
                samples.extend([rendered.buses[0][0][i], rendered.buses[0][1][i]]);
            }
        }
        core.end_block(block, &mut |_| true);
    }
    assert_eq!(samples.len(), 192000 * 2);
    assert!(samples.iter().all(|x| x.is_finite()));
    if args[4] == "--hash" {
        let mut hash = blake3::Hasher::new();
        for sample in &samples {
            hash.update(&sample.to_le_bytes());
        }
        println!(
            "PCM_HASH {}",
            serde_json::json!({
                "hash": hash.finalize().to_hex().to_string(), "samples": samples.len(),
                "nonzero": samples.iter().any(|x| x.abs() > 1e-7), "problems": core.problems(0),
            })
        );
        return;
    }
    panic!("audit witness only supports --hash; PCM is never retained");
}
