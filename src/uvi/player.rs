//! Persistent native Program playback on an allocating worker thread.
use super::{
    library::BankResources,
    playback::Renderer,
    program::Program,
    script::{self, Session},
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub use super::script::HostedInput;

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
    pub host_completions: Vec<script::HostCompletion>,
    pub audio: Vec<[f32; 2]>,
    pub commands: usize,
    pub host_commands: usize,
    /// Private script diagnostics; do not publish without reviewing their contents.
    pub logs: Vec<String>,
    pub dropped_logs: usize,
    /// Bit i reports a nonfatal admission rejection of ui_inputs[i].
    pub rejected_ui: u64,
}

/// Counts describe native dispatch, including processing silence. Retained
/// voice membership includes releasing and silent voices; neither is an
/// audibility or reference-engine fidelity measurement.
#[derive(serde::Serialize)]
pub struct RuntimeNodeEvidence {
    pub node: super::program::NodeId,
    pub kind: String,
    pub processed_blocks: Option<u64>,
    pub currently_bypassed: Option<bool>,
    pub retained_voice_instances: Option<usize>,
    pub evidence_source: &'static str,
}

#[derive(serde::Serialize)]
pub struct RuntimeEvidence {
    pub frame: u64,
    pub retained_voice_instances: usize,
    pub nodes: Vec<RuntimeNodeEvidence>,
    pub processing_semantics: &'static str,
    pub audibility_verified: bool,
    pub falcon_numerical_fidelity_verified: bool,
}

/// Owns the VM and resource cache alongside the native graph renderer.
/// Disk reads, Lua, preparation and rendering allocate. Keep this object on
/// one worker thread, including destruction; never use it in an audio callback.
pub struct Player<'a> {
    program: &'a Program,
    session: Session,
    renderer: Renderer<'a>,
    resources: BankResources,
    resource_revision: u64,
    frame: u64,
    failed: bool,
    hosted: bool,
}

impl<'a> Player<'a> {
    pub fn new(
        program: &'a Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: BankResources,
        sample_rate: u32,
    ) -> Result<Self> {
        Self::new_inner(
            program,
            modules,
            resources,
            sample_rate,
            None,
            None,
            None,
            None,
        )
    }
    pub fn new_hosted(
        program: &'a Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: BankResources,
        sample_rate: u32,
        epoch: u64,
        generation: u64,
    ) -> Result<Self> {
        Self::new_inner(
            program,
            modules,
            resources,
            sample_rate,
            Some((epoch, generation)),
            None,
            None,
            None,
        )
    }
    /// Prepare a fresh replacement; malformed state cannot mutate a live player.
    pub fn new_with_state(
        program: &'a Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: BankResources,
        sample_rate: u32,
        activation: Option<(u64, u64)>,
        saved: Option<&super::state::SavedState>,
    ) -> Result<Self> {
        Self::new_inner(
            program,
            modules,
            resources,
            sample_rate,
            activation,
            saved,
            None,
            None,
        )
    }
    /// Initialization-only timing observer. Never retained by the player or
    /// called while rendering audio.
    pub(crate) fn new_with_state_traced(
        program: &'a Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: BankResources,
        sample_rate: u32,
        activation: Option<(u64, u64)>,
        saved: Option<&super::state::SavedState>,
        stage: &mut dyn FnMut(&'static str),
        initialized_ui: &mut dyn FnMut(&Session),
    ) -> Result<Self> {
        Self::new_inner(
            program,
            modules,
            resources,
            sample_rate,
            activation,
            saved,
            Some(stage),
            Some(initialized_ui),
        )
    }
    fn new_inner(
        program: &'a Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: BankResources,
        sample_rate: u32,
        activation: Option<(u64, u64)>,
        saved: Option<&super::state::SavedState>,
        mut stage: Option<&mut dyn FnMut(&'static str)>,
        mut initialized_ui: Option<&mut dyn FnMut(&Session)>,
    ) -> Result<Self> {
        let mut stage = |name| {
            if let Some(stage) = stage.as_deref_mut() {
                stage(name);
            }
        };
        let hosted = activation.is_some();
        stage("uvi_player_preflight");
        let unsupported = super::playback::preflight(program);
        ensure!(
            unsupported.is_empty(),
            "Native UVI graph preflight failed: {}",
            serde_json::to_string(&unsupported)?
        );
        let capability = Some(resources.capability());
        if let Some(saved) = saved {
            stage("uvi_restore_validation_and_audio");
            saved.validate(program)?;
            saved.prepare_audio(capability.as_ref().unwrap())?;
        }
        // A fresh instrument may load aliases during authored initialization.
        // Only restoration has a complete captured override set to prevalidate.
        let prepared = if let Some(saved) = saved {
            stage("uvi_restore_renderer_init");
            let mut renderer = Renderer::new(program, resources.samples(), sample_rate)?;
            saved.prepare_renderer(&mut renderer)?;
            Some(renderer)
        } else {
            None
        };
        stage("uvi_lua_init");
        let mut session = if let Some((epoch, generation)) = activation {
            Session::new_hosted_program_chain_with_state(
                program,
                modules,
                capability,
                sample_rate,
                epoch,
                generation,
                saved,
            )?
        } else {
            Session::new_program_chain_with_state(program, modules, capability, sample_rate, saved)?
        };
        // Fresh initialization has completed authored constructors and onInit.
        // Restores remain private until their prevalidated renderer prefix and
        // authored onLoad/changed/onInit commands have applied successfully.
        if saved.is_none()
            && let Some(publish) = initialized_ui.as_mut()
        {
            publish(&session);
        }
        let mut renderer = match prepared {
            Some(renderer) => renderer,
            None => {
                stage("uvi_renderer_init");
                Renderer::new(program, resources.samples(), sample_rate)?
            }
        };
        if saved.is_some() {
            stage("uvi_restore_apply");
            let processed = session.drain()?;
            // The preload is the prefix; authored onLoad/changed/onInit commands
            // follow it and therefore retain final native initialization authority.
            renderer.install_prepared_samples(resources.samples())?;
            renderer.apply_boundary(
                &processed.commands,
                hosted.then_some(processed.command_roots.as_slice()),
                &processed.host_commands,
            )?;
        }
        if saved.is_some()
            && let Some(publish) = initialized_ui.as_mut()
        {
            publish(&session);
        }
        stage("uvi_player_finalize");
        let resource_revision = resources.revision();
        Ok(Self {
            program,
            session,
            renderer,
            resources,
            resource_revision,
            frame: 0,
            failed: false,
            hosted,
        })
    }
    pub fn acknowledge_host_completions(&mut self, roots: &[script::HostRoot]) -> Result<()> {
        ensure!(!self.failed, "UVI player must be replaced after a failure");
        ensure!(self.hosted, "Player has no hosted activation");
        self.session.acknowledge_host_completions(roots)
    }
    pub fn sample_rate(&self) -> u32 {
        self.session.sample_rate()
    }
    pub fn active_voices(&self) -> usize {
        self.renderer.active_voices()
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

    /// Explicit owning-worker inspection only. Unknown/unsupported evidence is
    /// represented by None, never inferred from successful graph admission.
    /// This traverses the graph only when a controller requests a snapshot.
    pub fn runtime_evidence(&self) -> RuntimeEvidence {
        let mut nodes: Vec<_> = self
            .program
            .nodes
            .iter()
            .enumerate()
            .map(|(node, source)| RuntimeNodeEvidence {
                node,
                kind: source.kind.clone(),
                processed_blocks: None,
                currently_bypassed: None,
                retained_voice_instances: None,
                evidence_source: "not_instrumented",
            })
            .collect();
        for evidence in self.renderer.runtime_evidence() {
            if let Some(node) = nodes.get_mut(evidence.node) {
                node.processed_blocks = Some(evidence.processed_blocks);
                node.currently_bypassed = evidence.currently_bypassed;
                node.retained_voice_instances = Some(evidence.retained_voice_instances);
                node.evidence_source = "renderer_native_dispatch";
            }
        }
        RuntimeEvidence {
            frame: self.frame,
            retained_voice_instances: self.active_voices(),
            nodes,
            processing_semantics: "native 256-frame intervals containing successful processing past known bypass gates; silence and retained/releasing voices are included; uninstrumented nodes are unknown",
            audibility_verified: false,
            falcon_numerical_fidelity_verified: false,
        }
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

    pub fn saved_state(&mut self) -> Result<super::state::SavedState> {
        ensure!(!self.failed, "UVI player must be replaced after a failure");
        let result = (|| {
            let saved = self.session.saved_state(self.frame)?;
            let processed = self.session.drain()?;
            if self.resources.revision() != self.resource_revision {
                self.renderer
                    .install_prepared_samples(self.resources.samples())?;
                self.resource_revision = self.resources.revision();
            }
            self.renderer.apply_boundary(
                &processed.commands,
                self.hosted.then_some(processed.command_roots.as_slice()),
                &processed.host_commands,
            )?;
            Ok(saved)
        })();
        // onSave can mutate Lua before an error. Never continue a partially
        // failed native callback; the old persisted payload remains untouched.
        if result.is_err() {
            self.failed = true;
        }
        result
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
        self.render_with_controls(inputs, ui_inputs, &[], frames)
    }
    /// Keep note/control wire order in `hosted` using Event for non-notes.
    /// Separate legacy inputs keep the existing hosted-first same-frame ties.
    pub fn render_hosted(
        &mut self,
        inputs: &[script::Input],
        hosted: &[HostedInput],
        frames: usize,
    ) -> Result<Rendered> {
        ensure!(self.hosted, "Player has no hosted activation");
        self.render_with_controls(inputs, &[], hosted, frames)
    }
    pub fn render_hosted_with_ui(
        &mut self,
        inputs: &[script::Input],
        ui_inputs: &[UiInput],
        hosted: &[HostedInput],
        frames: usize,
    ) -> Result<Rendered> {
        ensure!(self.hosted, "Player has no hosted activation");
        self.render_with_controls(inputs, ui_inputs, hosted, frames)
    }
    fn render_with_controls(
        &mut self,
        inputs: &[script::Input],
        ui_inputs: &[UiInput],
        hosted: &[HostedInput],
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
        ensure!(
            hosted.len() <= script::HOST_ROOT_CAPACITY
                && hosted
                    .windows(2)
                    .all(|pair| pair[0].frame() <= pair[1].frame())
                && hosted
                    .iter()
                    .all(|event| event.frame() >= self.frame && event.frame() < end),
            "Invalid hosted input sequence"
        );
        ensure!(
            hosted.iter().all(|event| match event {
                HostedInput::Event(input) => input_is_valid(input),
                _ => true,
            }),
            "Invalid hosted non-note input or audio transport tempo"
        );
        // Token admission uses the backend ledger before any UI, callback or
        // clock mutation. Execution failures remain fatal once processing starts.
        if !hosted.is_empty() {
            self.session.validate_hosted_inputs(hosted)?;
        }
        let result = (|| {
            let mut rejected_ui = 0u64;
            let processed = if ui_inputs.is_empty() && hosted.is_empty() {
                self.session.process(inputs, end - 1)?
            } else {
                let (mut midi, mut ui, mut hosted_index) = (0, 0, 0);
                while midi < inputs.len() || ui < ui_inputs.len() || hosted_index < hosted.len() {
                    if ui < ui_inputs.len()
                        && (midi == inputs.len() || ui_inputs[ui].frame <= inputs[midi].frame)
                        && (hosted_index == hosted.len()
                            || ui_inputs[ui].frame <= hosted[hosted_index].frame())
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
                    } else if hosted_index < hosted.len()
                        && (midi == inputs.len()
                            || hosted[hosted_index].frame() <= inputs[midi].frame)
                    {
                        match hosted[hosted_index] {
                            HostedInput::On { root, input } => {
                                self.session.host_note_on(root, input)?
                            }
                            HostedInput::Off { root, frame } => {
                                self.session.host_note_off(root, frame)?
                            }
                            HostedInput::Choke { root, frame } => {
                                self.session.host_note_choke(root, frame)?
                            }
                            HostedInput::Event(input) => self.session.input(input)?,
                        }
                        hosted_index += 1;
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
            let (audio, host_completions) = if self.hosted {
                let audio = self.renderer.render_with_roots(
                    &processed.commands,
                    &processed.command_roots,
                    &processed.host_commands,
                    frames,
                )?;
                let host_completions = self
                    .session
                    .complete_host_roots(end, &self.renderer.sounding_roots())?;
                (audio, host_completions)
            } else {
                (
                    self.renderer
                        .render(&processed.commands, &processed.host_commands, frames)?,
                    Vec::new(),
                )
            };
            self.frame = end;
            Ok(Rendered {
                audio,
                host_completions,
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
