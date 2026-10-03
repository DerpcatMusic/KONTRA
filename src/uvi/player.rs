//! Persistent native Program playback on an allocating worker thread.
use super::{
    library::BankResources,
    playback::Renderer,
    program::Program,
    script::{self, Session},
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

/// The VM can represent any positive finite tempo; the currently admitted DSP
/// processors share a bounded tempo range. Reject incompatible audio input
/// before advancing either clock. This check also serves realtime ingress.
pub(crate) fn input_is_valid(input: &script::Input) -> bool {
    script::input_is_valid(input)
        && !matches!(input.kind, script::InputKind::Transport { tempo, .. } if !(1. ..=1000.).contains(&tempo))
}

/// Audio and diagnostics from one exclusive frame interval.
#[derive(Debug)]
pub struct Rendered {
    pub audio: Vec<[f32; 2]>,
    pub commands: usize,
    pub host_commands: usize,
    /// Private script diagnostics; do not publish without reviewing their contents.
    pub logs: Vec<String>,
    pub dropped_logs: usize,
}

/// Owns the VM and resource cache alongside the native graph renderer.
/// Disk reads, Lua, preparation and rendering allocate. Keep this object on
/// one worker thread, including destruction; never use it in an audio callback.
pub struct Player<'a> {
    session: Session,
    renderer: Renderer<'a>,
    resources: BankResources,
    resource_revision: u64,
    frame: u64,
    failed: bool,
}

impl<'a> Player<'a> {
    pub fn new(
        program: &'a Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: BankResources,
        sample_rate: u32,
    ) -> Result<Self> {
        let unsupported = super::playback::preflight(program);
        ensure!(
            unsupported.is_empty(),
            "Native UVI graph preflight failed: {}",
            serde_json::to_string(&unsupported)?
        );
        let session = Session::new_program_chain(
            program,
            modules,
            Some(resources.capability()),
            sample_rate,
        )?;
        // Initialization may have resolved additional sample/impulse aliases.
        let renderer = Renderer::new(program, resources.samples(), sample_rate)?;
        let resource_revision = resources.revision();
        Ok(Self {
            session,
            renderer,
            resources,
            resource_revision,
            frame: 0,
            failed: false,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.session.sample_rate()
    }
    pub fn current_frame(&self) -> u64 {
        self.frame
    }
    pub fn requires_planned_segments(&self) -> bool {
        self.renderer.requires_planned_segments()
    }
    pub fn diagnostics(&self) -> Vec<&'static str> {
        self.renderer.diagnostics()
    }

    /// Inputs use absolute frames in [current_frame, current_frame + frames).
    /// Callbacks at the end boundary remain pending for the following block.
    pub fn render(&mut self, inputs: &[script::Input], frames: usize) -> Result<Rendered> {
        ensure!(
            !self.failed,
            "UVI player must be replaced after an execution or render failure"
        );
        ensure!(
            frames > 0 && frames <= self.sample_rate() as usize * 60,
            "Invalid UVI player block length"
        );
        let end = self
            .frame
            .checked_add(frames as u64)
            .context("UVI player timeline overflow")?;
        ensure!(
            inputs.first().is_none_or(|input| input.frame >= self.frame)
                && inputs.last().is_none_or(|input| input.frame < end),
            "UVI input is outside the player block"
        );
        ensure!(
            !self.requires_planned_segments()
                || (self.frame.is_multiple_of(256) && frames.is_multiple_of(256)),
            "This UVI player requires complete 256-frame native control segments"
        );
        script::validate_sequence(inputs, end - 1)?;
        ensure!(
            inputs.iter().all(input_is_valid),
            "UVI audio transport tempo must be between 1 and 1000 BPM"
        );
        let result = (|| {
            let processed = self.session.process(inputs, end - 1)?;
            if self.resources.revision() != self.resource_revision {
                self.renderer
                    .install_prepared_samples(self.resources.samples())?;
                self.resource_revision = self.resources.revision();
            }
            let audio =
                self.renderer
                    .render(&processed.commands, &processed.host_commands, frames)?;
            self.frame = end;
            Ok(Rendered {
                audio,
                commands: processed.commands.len(),
                host_commands: processed.host_commands.len(),
                logs: processed.logs,
                dropped_logs: processed.dropped_logs,
            })
        })();
        // Script/processor failures may have changed state before they surfaced.
        // Never continue two clocks after only one of them advanced successfully.
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}
