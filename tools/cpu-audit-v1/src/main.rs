use kontakto::{
    engine::{Bank, Engine, MEMORY_LIMIT, Streaming, effects, load_scripts},
    import,
};
use serde_json::{Value, json};
use std::path::Path;
struct Player(Engine);
fn cpu_checks() {}
impl Player {
    fn load(path: &Path) -> (Self, Value) {
        let i = import::read(path).unwrap();
        let (script, errors) = load_scripts(&i, i.script_state.clone(), 48000.);
        let controllers = script
            .as_deref()
            .map_or(&[][..], |s| &s.init_controllers[..]);
        let bank = Bank::load_counting(
            &i,
            MEMORY_LIMIT,
            Streaming::Auto,
            controllers,
            &Default::default(),
        )
        .unwrap();
        let info = json!({"engine":"v1","preload_frames":bank.preload,"head_bytes":bank.bytes,"samples":bank.sample_count(),"script_errors":errors.len()});
        let fx = effects(&i, script.as_deref(), 48000.);
        let mut e = Engine::default();
        e.set_bank(Some(Box::new(bank)));
        e.set_fx(fx);
        e.set_script(script);
        (Self(e), info)
    }
    fn event(&mut self, s: u8, a: u8, b: u8) {
        match s {
            0x90 => self.0.note_on(0, a, b),
            0x80 => self.0.note_off(0, a),
            0xb0 => self.0.cc(0, a, b),
            _ => panic!("event"),
        }
    }
    fn begin(&mut self, frames: usize) {
        self.0.begin_audio_block(frames, 1, false);
    }
    fn render(&mut self, frames: usize) -> f32 {
        let mut peak = 0.0f32;
        for n in (0..frames).step_by(128) {
            let len = (frames - n).min(128);
            let (mut l, mut r) = ([0.; 128], [0.; 128]);
            self.0.render(&mut l[..len], &mut r[..len]);
            peak = peak.max(
                l[..len]
                    .iter()
                    .chain(&r[..len])
                    .fold(0.0f32, |a, b| a.max(b.abs())),
            );
        }
        peak
    }
    fn voices(&self) -> usize {
        self.0.active_voices()
    }
    fn problems(&self) -> Value {
        json!({"underruns":self.0.underruns(),"dropped_commands":self.0.dropped_commands()})
    }
}
include!("../../cpu-audit-common.rs");
