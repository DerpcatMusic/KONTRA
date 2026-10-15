//! Plugin sound seam, not an exported CLAP/VST3 benchmark. Audio never leaves RAM.
use kontakto::sound::{
    BlockInfo, Core, CoreLoader, LoadRequest,
    event::{Event, HostNote, HostPattern},
    v2::{V2Core, V2Loader},
};
use serde_json::{Value, json};
use std::path::Path;

struct Player(V2Core, f64, u64);
impl Drop for Player {
    fn drop(&mut self) {
        if std::env::var_os("KONTRA_SIGNAL_TRACE").as_deref() == Some(std::ffi::OsStr::new("1")) {
            // Finish the observation artifact after the measured player retires.
            drop(std::mem::replace(
                &mut self.0,
                V2Core::with_parts(1, 48000.),
            ));
            assert!(sampler_core::trace_report::flush(
                std::time::Duration::from_secs(30)
            ));
        }
    }
}
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
        (Self(core, 0., 0), info)
    }
    fn event(&mut self, s: u8, a: u8, b: u8) {
        // Audit-schedule sentinels encode an exact note ID, not a MIDI packet.
        // Production receives the same typed host events from CLAP/VST3.
        if matches!(s, 0xf0 | 0xf1) {
            let id = i32::from(a) + (i32::from(b) << 8);
            let key = 60 + (id % 12) as u8;
            let event = if s == 0xf0 {
                Event::NoteOn {
                    note: HostNote { port: 0, channel: 0, key, id, clap: true },
                    velocity: 0.5 + f64::from(id) / 600.,
                    tune: 0.,
                }
            } else {
                Event::NoteOff(HostPattern { port: 0, channel: 0, key: i32::from(key), id, clap: true })
            };
            self.0.event(0, event);
            return;
        }
        self.0.event(0, Event::midi1(s, a, b));
    }
    fn begin(&mut self, frames: usize) {
        self.0.begin_block(&BlockInfo { frames, ..Default::default() });
    }
    fn render(&mut self, frames: usize) -> f32 {
        let mut peak = 0.0f32;
        for n in (0..frames).step_by(128) {
            let len = (frames - n).min(128);
            let r = self.0.render(len);
            for sample in r.buses[0].iter().flat_map(|x| &x[..len]) {
                peak = peak.max(sample.abs());
                self.1 += f64::from(*sample).powi(2);
                self.2 += u64::from(!sample.is_finite());
            }
        }
        self.0.end_block(frames, &mut |_| true);
        peak
    }
    fn voices(&self) -> usize {
        self.0.voices().active
    }
    fn problems(&self) -> Value {
        let mut problems = serde_json::to_value(self.0.problems(0)).unwrap();
        if let Some(stats) = self.0.runtime_stats(0) {
            problems["v1_control_starts"] = stats.v1_control_starts.into();
            problems["v1_control_blocks"] = stats.v1_control_blocks.into();
        }
        problems["witness_pcm_energy"] = self.1.into();
        problems["witness_nonfinite_samples"] = self.2.into();
        problems
    }
}
include!("../tools/cpu-audit-common.rs");
