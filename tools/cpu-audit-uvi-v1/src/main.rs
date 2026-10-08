//! Native v1 UVI Player processing, including Lua and DSP; PCM stays in RAM.
use kontakto::uvi::{
    crypto,
    library::{BankResources, Library},
    player::Player as NativePlayer,
    script::{Input, InputKind},
    ufs,
};
use serde_json::{Value, json};
use std::{path::Path, rc::Rc};

// Reuse the pinned reader's validation/recovery without changing product exports.
#[allow(dead_code)]
#[path = "../../../src/uvi/access.rs"]
mod access;

struct Player {
    inner: NativePlayer<'static>,
    inputs: Vec<Input>,
    logs: usize,
}

fn cpu_checks() {
    assert!(event_kind(0x90, 60, 100).is_some());
    assert!(event_kind(0xb0, 64, 127).is_some());
    assert!(event_kind(0x70, 60, 100).is_none());
    assert!(event_kind(0x90, 128, 100).is_none());
    assert!(
        Path::new("bank.ufs::Presets/P.uvip")
            .to_str()
            .unwrap()
            .split_once("::")
            .is_some()
    );
}

fn event_kind(status: u8, a: u8, b: u8) -> Option<InputKind> {
    if a > 127 || b > 127 {
        return None;
    }
    Some(match status {
        0x90 => InputKind::NoteOn {
            channel: 0,
            note: a,
            velocity: b,
        },
        0x80 => InputKind::NoteOff {
            channel: 0,
            note: a,
        },
        0xb0 => InputKind::Controller {
            channel: 0,
            controller: a,
            value: b,
        },
        _ => return None,
    })
}

impl Player {
    fn load(path: &Path) -> (Self, Value) {
        match Self::try_load(path) {
            Ok(player) => player,
            Err(error) => {
                println!(
                    "{}",
                    json!({"engine":"v1-uvi", "loads":"no", "error_sha256": format!("{:x}", sha2::Sha256::digest(error.to_string().as_bytes()))})
                );
                std::process::exit(2);
            }
        }
    }

    fn try_load(path: &Path) -> anyhow::Result<(Self, Value)> {
        use anyhow::Context;
        let (bank, member) = path
            .to_str()
            .context("UTF-8 bank/member required")?
            .split_once("::")
            .context("bank.ufs::member required")?;
        let reader = access::ReaderNamespaces::open(&access::reader_path(None)?)?;
        let ufs = ufs::Ufs::open(Path::new(bank))?;
        let directory = ufs.decode_directory(&reader.metadata)?;
        let state = if directory.files.iter().any(|m| m.mode == 2) {
            Some(access::recover_content_state(
                Path::new(bank),
                &ufs,
                &directory,
            )?)
        } else {
            None
        };
        let library = Rc::new(Library::open(
            Path::new(bank),
            &reader.metadata,
            state.as_ref().map(|s| s.key),
        )?);
        let loaded = library.program(member, &reader.program)?;
        anyhow::ensure!(
            kontakto::uvi::playback::preflight(&loaded.program).is_empty(),
            "native preflight rejected"
        );
        let samples = library.samples(&loaded)?;
        let count = samples.len();
        let resources = BankResources::new(library.clone(), &loaded.path, samples)?;
        let modules = library.modules()?;
        // ponytail: one CLI load retains its graph until process exit; use an owning
        // wrapper if this adapter ever loads more than one preset per process.
        let program = Box::leak(Box::new(loaded.program));
        let inner = NativePlayer::new(program, modules, resources, 48000)?;
        let info = json!({"engine":"v1-uvi", "loads":"yes", "samples":count,
            "measurement_seam":"native-player-lua-and-dsp; excludes worker transport/UI",
            "product_source_sha":"4bffbb18", "static_pcm_cache":false});
        Ok((
            Self {
                inner,
                inputs: Vec::with_capacity(32),
                logs: 0,
            },
            info,
        ))
    }

    fn event(&mut self, status: u8, a: u8, b: u8) {
        self.inputs.push(Input {
            frame: self.inner.current_frame(),
            kind: event_kind(status, a, b).expect("validated MIDI"),
        });
    }

    fn render(&mut self, frames: usize) -> f32 {
        let mut peak = 0.0f32;
        for begin in (0..frames).step_by(128) {
            let result = match self.inner.render(&self.inputs, (frames - begin).min(128)) {
                Ok(result) => result,
                Err(error) => {
                    println!(
                        "{}",
                        json!({"engine":"v1-uvi", "render":"failed", "error_sha256":format!("{:x}", sha2::Sha256::digest(error.to_string().as_bytes()))})
                    );
                    std::process::exit(3);
                }
            };
            self.inputs.clear();
            self.logs += result.logs.len() + result.dropped_logs;
            for sample in result.audio.iter().flatten() {
                assert!(sample.is_finite(), "nonfinite PCM");
                peak = peak.max(sample.abs());
            }
        }
        peak
    }

    fn voices(&self) -> usize {
        self.inner.active_voices()
    }
    fn problems(&self) -> Value {
        json!({"script_log_records":self.logs, "diagnostic_kinds":self.inner.diagnostics().len()})
    }
}
use sha2::Digest;
include!("../../cpu-audit-common.rs");
