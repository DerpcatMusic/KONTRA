//! Plugin sound seam, not an exported CLAP/VST3 benchmark. Audio never leaves RAM.
use kontakto::sound::{
    BlockInfo, Core, CoreLoader, LoadRequest,
    event::Event,
    v2::{V2Core, V2Loader},
};
use serde_json::{Value, json};
use std::path::Path;

struct Player(V2Core);
fn cpu_checks() {
    use sampler_core::{Input, Instruction, Limits, Prepared, Program, Protocol, Runtime};
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(vec![Program::new(vec![Instruction::End]).unwrap()], Some(0))
        .unwrap();
    let limits = Limits::for_plan(&plan, 8, 8);
    let mut rt = Runtime::new(plan, limits).unwrap();
    let note = rt
        .trigger(
            Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(1),
            },
            60,
            1.,
        )
        .unwrap();
    rt.release(note).unwrap();
    rt.render(&mut [[0.; 2]; 64]).unwrap();
    let mut ended = 0;
    rt.flush_ended(|_| {
        ended += 1;
        true
    });
    assert_eq!(ended, 0, "unflushed callback pins its note");
    assert_eq!(rt.note_count(), 1);
    rt.flush_behaviors(|_, _, _| true);
    rt.flush_ended(|_| {
        ended += 1;
        true
    });
    assert_eq!(ended, 1);
    assert_eq!(rt.note_count(), 0);
    println!("lifecycle: end notifications 0 before callback flush, 1 after");
    for notes in [64, 16384] {
        let plan = Prepared::new(48000, vec![], vec![], 0).unwrap();
        let limits = Limits::for_plan(&plan, notes, 8);
        let mut rt = Runtime::new(plan, limits).unwrap();
        let mut times = Vec::with_capacity(1000);
        for _ in 0..1000 {
            let start = std::time::Instant::now();
            rt.flush_ended(|_| true);
            times.push(start.elapsed().as_nanos() as u64);
        }
        println!("idle_flush notes={notes}: {}", quantiles(times));
    }
}
impl Player {
    fn load(path: &Path) -> (Self, Value) {
        let loaded = V2Loader
            .prepare(
                &LoadRequest {
                    path: path.into(),
                    sample_rate: 48000.,
                    ..Default::default()
                },
                &mut |_| {},
                &|| false,
            )
            .unwrap();
        let stream = loaded.stream.as_ref().map(|s| json!({"head_frames":s.report.head_frames,"head_bytes":s.report.head_bytes,"pool_bytes":s.report.pool_bytes,"full_bytes":s.report.full_bytes,"latency_p95_us":s.report.latency_p95.as_micros()}));
        let info = json!({"engine":"v2", "stream":stream, "missing_features":loaded.report.missing.len(), "script_callbacks":loaded.report.decoded.script_callbacks});
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, loaded.part);
        (Self(core), info)
    }
    fn event(&mut self, s: u8, a: u8, b: u8) {
        self.0.event(0, Event::midi1(s, a, b));
    }
    fn render(&mut self, frames: usize) -> f32 {
        self.0.begin_block(&BlockInfo {
            frames,
            ..Default::default()
        });
        let mut peak = 0.0f32;
        for n in (0..frames).step_by(128) {
            let len = (frames - n).min(128);
            let r = self.0.render(len);
            peak = peak.max(
                r.buses[0]
                    .iter()
                    .flat_map(|x| &x[..len])
                    .fold(0.0f32, |a, b| a.max(b.abs())),
            );
        }
        self.0.end_block(frames, &mut |_| true);
        peak
    }
    fn voices(&self) -> usize {
        self.0.voices().active
    }
    fn problems(&self) -> Value {
        serde_json::to_value(self.0.problems(0)).unwrap()
    }
}
include!("../tools/cpu-audit-common.rs");
