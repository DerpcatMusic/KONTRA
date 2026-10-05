//! Authored integration probe, not a production KSP adapter or vendor golden.
//! Exercises the existing VM through KspEngine against the independent v2 kernel.
use kontakto::ksp::{self, EnginePar, EventId, Fade, KspEngine, NoteLength, NoteSpec, VoicePar};
use sampler_core::{Input, Limits, NoteId, Pcm, Protocol, Runtime, VoiceId};

// Same thread-local allocation/free check used by the legacy plugin unit tests.
#[path = "../crates/sampler-core/tests/support/mod.rs"]
mod support;
use support::without_heap;

struct Binding {
    note: NoteId,
    voice: VoiceId,
    whole_sample: bool,
}

struct Bridge {
    core: Runtime,
    root: Option<NoteId>,
    bindings: Vec<Binding>,
}

impl Bridge {
    fn render(&mut self, output: &mut [[f32; 2]]) {
        self.core.render(output).unwrap();
        for b in &self.bindings {
            if b.whole_sample && !self.core.voice_active(b.voice) {
                // The VM's sample-length note completes independently of key-up.
                if self.core.note(b.note).is_ok() {
                    self.core.release(b.note).unwrap();
                }
            }
        }
    }
}

impl KspEngine for Bridge {
    fn play_note(&mut self, at: u32, n: &NoteSpec<'_>) -> Option<EventId> {
        // This probe deliberately accepts only the fixture's unity-rate PCM surface.
        // Unexpected services fail the test instead of silently claiming support.
        assert_eq!(
            (n.sample_offset_us, n.volume_mdb, n.tune_mc, n.pan),
            (0, 0, 0, 0)
        );
        assert!(n.groups.contains(0));
        if self.bindings.len() == self.bindings.capacity() {
            return None;
        }
        let note = self
            .core
            .child(
                self.root.unwrap(),
                n.note,
                f64::from(n.velocity) / 127.0,
                n.owner.is_some(),
                sampler_core::Inheritance::Snapshot,
            )
            .ok()?;
        let at = self.core.now().checked_add(u64::from(at))?;
        let voice = match self.core.start(note, 0, at, 1.0) {
            Ok(v) => v,
            Err(_) => {
                self.core.release(note).unwrap();
                return None;
            }
        };
        self.bindings.push(Binding {
            note,
            voice,
            whole_sample: matches!(n.length, NoteLength::Sample),
        });
        Some(EventId(self.bindings.len() as u32))
    }
    fn note_off(&mut self, at: u32, id: EventId, _: &NoteSpec<'_>) {
        let note = self.bindings[id.0 as usize - 1].note;
        self.core
            .release_at(note, self.core.now() + u64::from(at))
            .unwrap();
    }
    fn voice_active(&self, id: EventId) -> bool {
        id.0.checked_sub(1)
            .and_then(|i| self.bindings.get(i as usize))
            .is_some_and(|b| self.core.voice_active(b.voice))
    }
    fn fade(&mut self, _: u32, _: EventId, _: Fade) {
        panic!("fade outside this fixture's capability");
    }
    fn set_par(&mut self, _: u32, _: EventId, _: VoicePar, _: i32) {
        panic!("parameter outside this fixture's capability");
    }
    fn controller(&mut self, _: u32, _: u8, _: i32) {
        panic!("controller outside this fixture's capability");
    }
    fn group_count(&self) -> usize {
        1
    }
    fn group_name(&self, _: usize) -> &str {
        "fixture"
    }
    fn sample_rate(&self) -> f64 {
        f64::from(self.core.sample_rate())
    }
    fn set_engine_par(&mut self, _: u32, _: EnginePar, _: i32) -> bool {
        false
    }
    fn engine_par(&self, _: EnginePar) -> Option<i32> {
        None
    }
}

#[test]
fn v2_ksp_suppression_children_wait_and_release_share_the_native_kernel_without_heap() {
    let pcm = [Pcm {
        rate: 48000,
        frames: Box::from([[0.25; 2]; 96]),
    }];
    let core = fixture_runtime(
        48000,
        &pcm,
        Limits {
            notes: 8,
            channels: 4,
            families: 8,
            expressions: 8,
            voices: 8,
            commands: 8,
        },
    )
    .unwrap();
    let mut bridge = Bridge {
        core,
        root: None,
        bindings: Vec::with_capacity(8),
    };
    let script = r#"on init
declare polyphonic $original
declare $resumed
make_persistent($resumed)
end on
on note
ignore_event($EVENT_ID)
$original := $EVENT_NOTE
play_note($EVENT_NOTE,100,0,-1)
play_note($EVENT_NOTE+7,100,0,0)
wait(1000)
$resumed := $original
end on"#;
    let (mut script, errors) = ksp::Runtime::with_scripts(&[script], &mut bridge, 2, Vec::new());
    assert!(errors.iter().all(Option::is_none), "{errors:?}");
    let original = Input {
        protocol: Protocol::Clap,
        port: 3,
        group: 0,
        channel: 0,
        key: 48,
        external_id: Some(7),
    };
    let mut output = [[0.0; 2]; 120];
    without_heap(|| {
        let root = bridge.core.note_on(original, 60, 100.0 / 127.0).unwrap();
        bridge.root = Some(root);
        bridge.core.pin(root).unwrap(); // The wait continuation is an independent owner.
        script.note_on(&mut bridge, 0, 60, 100);
        script.process(&mut bridge, 24);
        bridge.render(&mut output[..24]);
        assert_eq!(
            bridge.bindings.len(),
            2,
            "ignored root must not become a third voice"
        );
        script.note_off(&mut bridge, 0, 60);
        bridge.core.release(root).unwrap();
        script.process(&mut bridge, 48);
        bridge.render(&mut output[24..72]);
        bridge.core.unpin(root).unwrap();
        bridge
            .core
            .flush_ended(|_| panic!("detached whole sample is still sounding"));
        script.process(&mut bridge, 48);
        bridge.render(&mut output[72..]);
        bridge.core.flush_ended(|_| false);
        assert_eq!(
            bridge.core.note_count(),
            1,
            "pending host end retains the root"
        );
        let mut ends = 0;
        bridge.core.flush_ended(|input| {
            assert_eq!(input, original);
            ends += 1;
            true
        });
        assert_eq!(ends, 1);
        assert_eq!(
            (bridge.core.note_count(), bridge.core.voice_count()),
            (0, 0)
        );
    });
    assert!(output[..24].iter().all(|f| *f == [0.5; 2]));
    assert!(output[24..96].iter().all(|f| *f == [0.25; 2]));
    assert!(output[96..].iter().all(|f| *f == [0.0; 2]));
    assert_eq!(script.persistence()[0]["$resumed"], ksp::Value::Int(60));
    assert!(
        script.diagnostics().is_empty(),
        "{:?}",
        script.diagnostics()
    );
}

#[test]
fn v2_native_saturation_reset_and_terminal_retry_do_not_allocate_or_free() {
    let pcm = [Pcm {
        rate: 48000,
        frames: Box::from([[0.5; 2]; 16]),
    }];
    let mut core = fixture_runtime(
        48000,
        &pcm,
        Limits {
            notes: 4,
            channels: 4,
            families: 4,
            expressions: 4,
            voices: 2,
            commands: 2,
        },
    )
    .unwrap();
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    };
    without_heap(|| {
        for _ in 0..100 {
            let a = core.note_on(input, 60, 1.0).unwrap();
            let b = core
                .child(a, 64, 1.0, true, sampler_core::Inheritance::Snapshot)
                .unwrap();
            core.start(a, 0, core.now() + 1, 1.0).unwrap();
            core.start(b, 0, core.now() + 2, 1.0).unwrap();
            assert_eq!(
                core.release_at(a, core.now() + 3),
                Err(sampler_core::Error::Capacity)
            );
            core.render(&mut [[0.0; 2]; 8]).unwrap();
            core.panic();
            core.flush_ended(|_| false);
            assert_eq!(core.note_count(), 1);
            core.flush_ended(|_| true);
            assert_eq!(
                (
                    core.note_count(),
                    core.voice_count(),
                    core.pending_commands()
                ),
                (0, 0, 0)
            );
        }
    });
}

fn fixture_runtime(rate: u32, pcm: &[Pcm], limits: Limits) -> Result<Runtime, sampler_core::Error> {
    Runtime::new(
        sampler_core::Prepared::new(rate, pcm.to_vec(), Vec::new(), 0)?,
        limits,
    )
}
