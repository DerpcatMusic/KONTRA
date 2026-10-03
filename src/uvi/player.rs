//! Persistent native Program playback on an allocating worker thread.
use super::{
    library::BankResources,
    playback::Renderer,
    program::Program,
    script::{self, Session},
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub const MAX_UI_EDITS: usize = 64;

/// Owned, fixed-size widget edit on the same absolute clock as MIDI inputs.
#[derive(Clone, Copy)]
pub struct UiInput {
    pub frame: u64,
    pub edit: super::host::UiEdit,
}

impl std::fmt::Debug for UiInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiInput")
            .field("frame", &self.frame)
            .field("processor", &self.edit.processor)
            .field("widget", &self.edit.widget)
            .finish_non_exhaustive()
    }
}

pub(crate) fn ui_input_is_valid(input: &UiInput) -> bool {
    use super::host::UiEditValue;
    let finite = |value: f64| value.is_finite() && (value as f32).is_finite();
    input.edit.widget > 0
        && match input.edit.value {
            UiEditValue::Number(value) => finite(value),
            UiEditValue::TableCell { index, value } => index > 0 && finite(value),
            UiEditValue::Boolean(_) | UiEditValue::Push => true,
        }
}

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
    /// Bit i reports a nonfatal admission rejection of ui_inputs[i].
    pub rejected_ui: u64,
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

    /// Read initialized controls on this player's owning thread without running
    /// callbacks, reading resources or advancing either playback clock.
    pub fn ui_snapshot(
        &self,
        processor: super::program::NodeId,
    ) -> Result<super::host::UiSnapshot> {
        ensure!(!self.failed, "UVI player must be replaced after a failure");
        self.session.ui_snapshot(processor)
    }

    /// Inputs use absolute frames in [current_frame, current_frame + frames).
    /// Callbacks at the end boundary remain pending for the following block.
    pub fn render(&mut self, inputs: &[script::Input], frames: usize) -> Result<Rendered> {
        self.render_with_ui(inputs, &[], frames)
    }

    /// MIDI and UI streams must each be ordered. UI edits run before MIDI at
    /// equal frames, preserving order within each stream. Both clocks advance
    /// once through the original block; there is no independent idle UI clock.
    pub fn render_with_ui(
        &mut self,
        inputs: &[script::Input],
        ui_inputs: &[UiInput],
        frames: usize,
    ) -> Result<Rendered> {
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
        ensure!(
            ui_inputs.len() <= MAX_UI_EDITS
                && ui_inputs.iter().all(|input| {
                    input.frame >= self.frame && input.frame < end && ui_input_is_valid(input)
                })
                && ui_inputs
                    .windows(2)
                    .all(|pair| pair[0].frame <= pair[1].frame),
            "Invalid UVI player UI input sequence"
        );
        let result = (|| {
            let mut rejected_ui = 0u64;
            let processed = if ui_inputs.is_empty() {
                self.session.process(inputs, end - 1)?
            } else {
                let (mut midi, mut ui) = (0, 0);
                while midi < inputs.len() || ui < ui_inputs.len() {
                    if ui < ui_inputs.len()
                        && (midi == inputs.len() || ui_inputs[ui].frame <= inputs[midi].frame)
                    {
                        let input = &ui_inputs[ui];
                        match self.session.edit_ui(&input.edit, input.frame) {
                            Ok(()) => {}
                            Err(script::UiEditError::Rejected(_)) => rejected_ui |= 1u64 << ui,
                            Err(error @ script::UiEditError::Execution(_)) => {
                                return Err(error.into());
                            }
                        }
                        ui += 1;
                    } else {
                        self.session.input(inputs[midi])?;
                        midi += 1;
                    }
                }
                self.session.advance(end - 1)?;
                self.session.drain()?
            };
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
                rejected_ui,
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
