//! Explicit per-activation shared processing ownership. Definition, source,
//! selection family and physical storage identities are deliberately distinct.
//! The first executable schedule has stereo signal edges; no source channel
//! truncation or implicit multichannel projection is introduced here.
use crate::{
    Envelope, EnvelopeState, Error, FamilyId, Frame, Handle, NoteId, PlanId, Prepared, Runtime,
    VoiceId,
};

pub(crate) mod signal;
pub(crate) mod stages;

const MAX_MEMBERS: usize = 16;
/// Actual scope program operand giving an explicitly declared source gate.
#[derive(Clone,Copy,Debug)]
pub struct ProcessingModulationGate {pub source:usize,pub route:usize,pub velocity:crate::VelocityCurve}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessingOwnerId(pub(super) Handle);

#[derive(Clone, Copy, Debug)]
pub struct ProcessingOwnerSnapshot {
    pub id: ProcessingOwnerId,
    pub note: NoteId,
    pub plan: PlanId,
    pub template: usize,
    pub sources: usize,
    pub envelope: f32,
    pub tail_remaining: Option<u32>,
}

/// Immutable exact operand metadata. Current values stay on the generation's
/// existing ControlRamp and SAME owner VoiceModState; no parameter value bank.
pub(crate) struct CompiledProcessingParameter {
    pub definition: sampler_ir::SourceProcessingParameter,
    pub site: crate::ScopeControlSite,
    pub lane: usize,
    pub control: crate::ControlId,
    /// Original control's authored smoothing, joined offaudio to this operand.
    pub ramp_frames:u32,
    pub routes: Box<[usize]>,
}
pub(super) struct PreparedScope {
    /// The canonical prepared owner table is shared with event dispatch.
    /// None is an explicit hand-authored generic scope, never a guessed source ID.
    pub source_owner: Option<crate::prepare::PreparedSourceOwnerIndex>,
    pub chain: Option<usize>,
    pub sum: sampler_ir::ProcessingSumLaw,
    pub envelope: Envelope,
    pub has_amplitude: bool,
    pub bus: Option<usize>,
    pub members: Box<[usize]>,
    pub signal: Option<sampler_ir::ProcessingSignal>,
    pub modulation_gate:Option<ProcessingModulationGate>,
    pub parameters: Box<[CompiledProcessingParameter]>,
    pub parameter_preparation_bytes: usize,
}

/// Admission freezes only the actual onset point; forecasts read current
/// accepted inputs later, at the first physical prelude. Commit is separate.
#[derive(Clone,Copy,PartialEq,Eq)]
pub(super) enum ProcessingModulationPhase {AdmissionOnset,Forecast,Commit}

#[derive(Clone, Copy)]
pub(super) struct ProcessingOwner {
    pub note: NoteId,
    pub plan: PlanId,
    pub template: usize,
    activation: u64,
    pub members: [Option<VoiceId>; MAX_MEMBERS],
    envelope: EnvelopeState,
    tail_remaining: Option<u32>,
    pub(super) failed: bool,
    pub(super) published: bool,
    modulation_started:bool,
    modulation_prepared_at:Option<u64>,
    modulation_prepared_closed:bool,
    modulation_released:bool,
    // Source lifetime derived from the SAME EG phase/control-point endpoint.
    // This is not an envelope/value copy or an independent evaluator clock.
    modulation_source_end:Option<u64>,
}

#[cfg(test)]
impl ProcessingOwner {
    /// Read exact owner timing in sibling-module admission tests without
    /// exposing mutable envelope state to production code.
    pub(super) fn envelope_remaining_for_test(&self) -> usize {
        self.envelope.remaining()
    }
}

impl Prepared {
    /// Binds programs to real ProcessingScope definitions. This does not install
    /// a global/note-less modulator or activate an importer native profile.
    pub fn with_processing_modulation(self,programs:Vec<crate::ModProgram>,scopes:Vec<Option<usize>>,max_state_bytes:usize)->Result<Self,Error>{
        let gates=vec![None;scopes.len()];self.with_processing_modulation_gates(programs,scopes,gates,max_state_bytes)
    }
    pub fn with_processing_modulation_gates(mut self,programs:Vec<crate::ModProgram>,scopes:Vec<Option<usize>>,gates:Vec<Option<ProcessingModulationGate>>,max_state_bytes:usize)->Result<Self,Error>{
        if gates.len()!=scopes.len(){return Err(Error::InvalidInput);}
        if scopes.len()!=self.processing_scopes.len()||programs.len()>scopes.len()|| (0..programs.len()).any(|program|!scopes.contains(&Some(program))){return Err(Error::InvalidInput);}
        // Parameter rows need the actual owner projector even when their only
        // work is an ordered fixed-source fold. Never publish a missing map
        // and hope the audio path supplies an empty program later.
        if self.processing_scopes.iter().zip(&scopes).any(|(scope,program)|!scope.parameters.is_empty()&&program.is_none()){
            return Err(Error::InvalidInput);
        }
        for (scope,binding) in scopes.iter().enumerate(){
            let Some(program)=binding else{continue;};
            let program=programs.get(*program).ok_or(Error::InvalidInput)?;
            for route in &program.routes {
                let field=match route.target.compiled(){
                    Some(crate::voice_mod::CompiledTarget::Pan)=>crate::SourceOwnerControl::Pan,
                    Some(crate::voice_mod::CompiledTarget::Attenuate|crate::voice_mod::CompiledTarget::Decibels)=>crate::SourceOwnerControl::Gain,
                    Some(crate::voice_mod::CompiledTarget::SourcePhase(_)|crate::voice_mod::CompiledTarget::SourceRate(_))=>continue,
                    Some(crate::voice_mod::CompiledTarget::ProcessingParameter{scope:actual,binding})=>{
                        if actual as usize!=scope||self.processing_scopes[scope].parameters.get(binding as usize).is_none(){return Err(Error::InvalidInput);}continue;
                    },
                    _=>return Err(Error::InvalidInput),
                };
                if !self.has_processing_output_role(scope,field){return Err(Error::InvalidInput);}
            }
        }
        for (scope,gate)in gates.iter().enumerate(){
            let Some(gate)=gate else{continue;};let index=scopes[scope].ok_or(Error::InvalidInput)?;
            let program=&programs[index];let route=program.routes.get(gate.route).ok_or(Error::InvalidInput)?;
            if !matches!(program.sources.get(gate.source),Some(crate::ModSource::Envelope(_)))
                ||route.source!=gate.source||route.target!=crate::ModTarget::Attenuate||route.depth!=1.
                ||route.invert||route.shape.is_some()||route.scale.is_some()||route.more_scales.iter().any(Option::is_some)
                ||route.lag!=0||route.live_gain!=1.||route.unipolar
                ||program.controls.iter().any(|(row,_)|*row==gate.route)
                ||matches!(gate.velocity,crate::VelocityCurve::Power(p)if !p.is_finite()||p<=0.){return Err(Error::InvalidInput);}
        }
        let parameter_bytes=self.processing_parameter_preparation_bytes(&programs,&scopes)?;
        let mut preparation_bytes=parameter_bytes.checked_add(crate::voice_mod::VoiceModulation::scoped_preparation_bytes(&programs,programs.capacity(),&scopes,scopes.capacity())?).ok_or(Error::Capacity)?
            .checked_add(std::alloc::Layout::array::<Option<ProcessingModulationGate>>(gates.capacity()).map_err(|_|Error::Capacity)?.size()).ok_or(Error::Capacity)?;
        if preparation_bytes>max_state_bytes{return Err(Error::Capacity);}
        for(scope,program)in scopes.iter().enumerate(){
            let rows=&mut self.processing_scopes[scope].parameters;
            let routes=programs.get(program.unwrap_or(usize::MAX)).map_or(&[][..],|p|&p.routes);
            let mut counts=Vec::new();counts.try_reserve_exact(rows.len()).map_err(|_|Error::Capacity)?;counts.resize(rows.len(),0usize);
            let count_spare=std::alloc::Layout::array::<usize>(counts.capacity()-counts.len()).map_err(|_|Error::Capacity)?.size();
            preparation_bytes=preparation_bytes.checked_add(count_spare).ok_or(Error::Capacity)?;
            if preparation_bytes>max_state_bytes{return Err(Error::Capacity);}
            for route in routes {if let Some(crate::voice_mod::CompiledTarget::ProcessingParameter{scope:actual,binding})=route.target.compiled(){
                if actual as usize!=scope{return Err(Error::InvalidInput);}
                let count=counts.get_mut(binding as usize).ok_or(Error::InvalidInput)?;*count=count.checked_add(1).ok_or(Error::Capacity)?;
            }}
            let mut indices=Vec::new();indices.try_reserve_exact(rows.len()).map_err(|_|Error::Capacity)?;
            let builder_spare=std::alloc::Layout::array::<Vec<usize>>(indices.capacity()-rows.len()).map_err(|_|Error::Capacity)?.size();
            preparation_bytes=preparation_bytes.checked_add(builder_spare).ok_or(Error::Capacity)?;
            if preparation_bytes>max_state_bytes{return Err(Error::Capacity);}
            for count in counts {let mut row=Vec::new();row.try_reserve_exact(count).map_err(|_|Error::Capacity)?;
                // Reserve/box may coexist. Count observed spare capacity too;
                // allocator metadata/RSS is not a logical payload certificate.
                let spare=row.capacity().checked_sub(count).ok_or(Error::Capacity)?.checked_mul(2).ok_or(Error::Capacity)?;
                let bytes=std::alloc::Layout::array::<usize>(spare).map_err(|_|Error::Capacity)?.size();
                preparation_bytes=preparation_bytes.checked_add(bytes).ok_or(Error::Capacity)?;
                if preparation_bytes>max_state_bytes{return Err(Error::Capacity);}indices.push(row);
            }
            for(index,route)in routes.iter().enumerate(){if let Some(crate::voice_mod::CompiledTarget::ProcessingParameter{binding,..})=route.target.compiled(){indices[binding as usize].push(index);}}
            for(row,routes)in rows.iter_mut().zip(indices){row.routes=routes.into_boxed_slice();}
        }
        // This builder owns an unpublished candidate. Retire its previous
        // compiled program before constructing replacement prepared arrays;
        // errors still consume the candidate, never mutate a Runtime.
        self.processing_modulation=Default::default();
        let ranges=vec![0;scopes.len()];
        let modulation=crate::voice_mod::VoiceModulation::new_resolved(programs,scopes,ranges,Vec::new(),|_|None,|id|self.real_control(id))?;
        self.validate_constant_property_program(&modulation)?;
        for(scope,definition)in self.processing_scopes.iter().enumerate(){
            if !definition.parameters.is_empty()&&modulation.program(scope).is_none(){return Err(Error::InvalidInput);}
        }
        let minimum=crate::voice_mod::VoiceModState::checked_storage_bytes(modulation.shape(),1,self.processing_scopes.len())?;
        if minimum>max_state_bytes.checked_sub(preparation_bytes).ok_or(Error::Capacity)?{return Err(Error::Capacity);}
        self.voice_modulation.validate_provider_bindings(&modulation,&self.region_processing)?;
        for (scope,gate)in self.processing_scopes.iter_mut().zip(gates){scope.modulation_gate=gate;}
        self.processing_modulation=modulation;self.processing_modulation_limit=max_state_bytes.checked_sub(preparation_bytes).ok_or(Error::Capacity)?;Ok(self)
    }

    pub(crate) fn processing_parameter_lane(&self,scope:usize,site:crate::ScopeControlSite,role:sampler_ir::ProcessorParameter)->Result<usize,Error>{
        let definition=self.processing_scopes.get(scope).ok_or(Error::InvalidInput)?;
        match site {
            crate::ScopeControlSite::SignalNode(index)=>{
                let signal=self.processing_signals.get(scope).and_then(Option::as_ref).ok_or(Error::InvalidInput)?;
                let Some(signal::Node::Stage(stage))=signal.nodes.get(index)else{return Err(Error::InvalidInput);};
                stage.processor_parameter_lane(role).ok_or(Error::InvalidInput)
            },
            crate::ScopeControlSite::ChainPre(_)|crate::ScopeControlSite::ChainPost(_)=>{
                self.voice_chains.view(definition.chain.ok_or(Error::InvalidInput)?).processor_parameter_lane(site,role,&self.filters).ok_or(Error::InvalidInput)
            },
        }
    }
    pub(crate) fn validate_processing_parameters(&self)->Result<(),Error>{
        for(scope,definition)in self.processing_scopes.iter().enumerate(){
            for(index,row)in definition.parameters.iter().enumerate(){
                let source_owner=definition.source_owner.and_then(|owner|self.source_owners().get(owner)).ok_or(Error::InvalidInput)?;
                if source_owner.id!=row.definition.owner||row.definition.compiled.is_none_or(|site|site.scope!=scope){return Err(Error::InvalidInput);}
                let lane=self.processing_parameter_lane(scope,row.site,row.definition.parameter)?;
                let operand=self.dsp_bindings.get(lane).ok_or(Error::InvalidInput)?;
                let control=row.control;
                if lane!=row.lane||operand.control!=control||operand.ramp_frames!=row.ramp_frames||definition.parameters[..index].iter().any(|old|old.lane==lane){return Err(Error::InvalidInput);}
                let domain=self.controls[self.control_index(control)?].domain;
                if !matches!(domain,crate::ControlDomain::Real{min,max}if min>0.&&max<=20000.)||(!row.definition.fixed_factors.is_empty()&&operand.ramp_frames!=0){return Err(Error::InvalidInput);}
            }
        }Ok(())
    }
    pub(crate) fn processing_parameter_preparation_bytes(&self,programs:&[crate::ModProgram],scopes:&[Option<usize>])->Result<usize,Error>{
        fn add<T>(total:&mut usize,count:usize)->Result<(),Error>{*total=total.checked_add(std::alloc::Layout::array::<T>(count).map_err(|_|Error::Capacity)?.size()).ok_or(Error::Capacity)?;Ok(())}
        let mut bytes=0usize;
        for(scope,definition)in self.processing_scopes.iter().enumerate(){
            bytes=bytes.checked_add(definition.parameter_preparation_bytes).ok_or(Error::Capacity)?;
            // Count table, per-binding Vec builders and boxed-conversion overlap.
            add::<usize>(&mut bytes,definition.parameters.len())?;
            add::<Vec<usize>>(&mut bytes,definition.parameters.len().checked_mul(2).ok_or(Error::Capacity)?)?;
            for row in &definition.parameters{add::<usize>(&mut bytes,row.routes.len())?;}
            if let Some(program)=scopes.get(scope).copied().flatten(){
                let program=programs.get(program).ok_or(Error::InvalidInput)?;
                add::<usize>(&mut bytes,program.routes.len().checked_mul(2).ok_or(Error::Capacity)?)?;
            }
        }Ok(bytes)
    }

    pub fn processing_scope_count(&self) -> usize {
        self.processing_scopes.len()
    }
    /// Actual source definition identity joined by the ordinary compiler.
    /// Liveness remains fenced by this Prepared generation's real PlanId.
    pub fn processing_scope_source_owner(&self, scope: usize) -> Option<sampler_ir::SourceOwnerId> {
        let owner = self.processing_scopes.get(scope)?.source_owner?;
        Some(self.source_owners().get(owner)?.id)
    }
}

impl Runtime {
    /// Fixed owner capacity is separate from later physical-source growth.
    /// Tails occupy owners after source slots become reusable.
    pub fn processing_owner_capacity(&self) -> usize {
        self.processing_owners.slots.len()
    }
    pub fn processing_owner_count(&self) -> usize {
        self.processing_owners.count()
    }
    pub fn visit_processing_owners(&self, mut visit: impl FnMut(ProcessingOwnerSnapshot)) {
        for (index, slot) in self.processing_owners.slots.iter().enumerate() {
            if let Some(owner) = slot.value.as_ref().filter(|o| o.published) {
                visit(ProcessingOwnerSnapshot {
                    id: ProcessingOwnerId(self.processing_owners.id(index)),
                    note: owner.note,
                    plan: owner.plan,
                    template: owner.template,
                    sources: owner.members.iter().flatten().count(),
                    envelope: owner.envelope.current(),
                    tail_remaining: owner.tail_remaining,
                });
            }
        }
    }

    pub(super) fn begin_processing_activation(&mut self) -> Option<u64> {
        self.processing_activation = self.processing_activation.checked_add(1)?;
        Some(self.processing_activation)
    }

    pub(super) fn processing_member_admission(
        &mut self,
        note: NoteId,
        activation: Option<u64>,
        region: usize,
    ) -> Result<Option<(ProcessingOwnerId, usize, bool)>, Error> {
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let plan = n.plan;
        let prepared = &self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared;
        let Some((template, ordinal)) = prepared.region_processing.get(region).copied().flatten()
        else {
            return Ok(None);
        };
        let activation = activation.ok_or(Error::ArithmeticOverflow)?;
        let scope = &prepared.processing_scopes[template];
        let gate = (scope.has_amplitude||scope.modulation_gate.is_some()) && !prepared.region_has_amplitude(region);
        let existing = self.processing_owners.slots.iter().position(|slot| {
            slot.value.as_ref().is_some_and(|owner| {
                owner.note == note && owner.template == template && owner.activation == activation
            })
        });
        if let Some(index) = existing {
            let owner = self.processing_owners.slots[index]
                .value
                .as_ref()
                .ok_or(Error::StaleHandle)?;
            if owner.failed || owner.members[ordinal].is_some() {
                return Err(Error::Capacity);
            }
            return Ok(Some((
                ProcessingOwnerId(self.processing_owners.id(index)),
                ordinal,
                gate,
            )));
        }
        if self.voices.available() == 0 {
            return Err(Error::Capacity);
        }
        let next = n.processing_owners.checked_add(1).ok_or(Error::Capacity)?;
        let id = ProcessingOwnerId(self.processing_owners.insert(ProcessingOwner {
            note,
            plan,
            template,
            activation,
            members: [None; MAX_MEMBERS],
            envelope: EnvelopeState::new(scope.envelope),
            tail_remaining: None,
            failed: false,
            published: false,modulation_started:false,modulation_prepared_at:None,modulation_prepared_closed:false,modulation_released:false,modulation_source_end:None,
        })?);
        self.notes
            .get_mut(note.0)
            .ok_or(Error::StaleHandle)?
            .processing_owners = next;
        let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        generation
            .dsp
            .owner_cells
            .claim(id.0.index)
            .fill(crate::dsp::ProcessorState::default());
        generation.dsp.owner_delays.claim(id.0.index).fill([0.; 2]);
        generation.dsp.owner_signal_history.claim(id.0.index).fill(0.);
        Ok(Some((id, ordinal, gate)))
    }

    pub(super) fn attach_processing_member(
        &mut self,
        owner: ProcessingOwnerId,
        ordinal: usize,
        voice: VoiceId,
        gate: bool,
    ) {
        if let Some(state) = self.processing_owners.get_mut(owner.0) {
            state.members[ordinal] = Some(voice);
        }
        if let Some(source) = self.voices.get_mut(voice.0) {
            source.processing = Some((owner, ordinal));
            source.processing_gate = gate;
        }
    }
    pub(super) fn fail_processing_owner(&mut self, owner: ProcessingOwnerId) {
        if let Some(state) = self.processing_owners.get_mut(owner.0) {
            state.failed = true;
        }
    }
    pub(super) fn finish_processing_activation(&mut self, activation: Option<u64>) {
        let Some(activation) = activation else {
            return;
        };
        for index in 0..self.processing_owners.slots.len() {
            let Some(owner) = self.processing_owners.slots[index].value else {
                continue;
            };
            if owner.activation != activation || owner.published {
                continue;
            }
            let id = ProcessingOwnerId(self.processing_owners.id(index));
            if owner.failed || owner.members.iter().all(Option::is_none) {
                if let Some(log) = self.selection_log.as_mut().and_then(|log| log.last_mut()) {
                    if let Some(plan) = self.plans.get(owner.plan.0) {
                        for selected in &mut log.candidates {
                            if plan.prepared.processing_scopes[owner.template]
                                .members
                                .contains(&selected.region)
                            {
                                selected.started = None;
                            }
                        }
                    }
                }
                self.end_processing_owner(id);
            } else if let Some(owner) = self.processing_owners.get_mut(id.0) {
                owner.published = true;
            }
        }
    }

    fn owner_has_family(&self, owner: &ProcessingOwner, family: FamilyId) -> bool {
        owner
            .members
            .iter()
            .flatten()
            .any(|id| self.voices.get(id.0).is_some_and(|v| v.family == family))
    }
    pub(super) fn release_processing_note(&mut self, note: NoteId) {
        for (index,slot) in self.processing_owners.slots.iter_mut().enumerate() {
            if let Some(owner) = slot.value.as_mut()
                && owner.note == note
            {
                let has = self
                    .plans
                    .get(owner.plan.0)
                    .is_some_and(|p| p.prepared.processing_scopes[owner.template].has_amplitude);
                if has {owner.envelope.release();}
                if let Some(generation)=self.plans.get_mut(owner.plan.0){
                    if !owner.modulation_released{
                        generation.dsp.owner_modulation.release(&generation.prepared.processing_modulation,index);
                        if owner.modulation_started && let Some(gate)=generation.prepared.processing_scopes[owner.template].modulation_gate {
                            let end=generation.dsp.owner_modulation.scope_gate_end(&generation.prepared.processing_modulation,index,gate.source,true);
                            owner.modulation_source_end=match(owner.modulation_source_end,end){(Some(old),Some(new))=>Some(old.min(new)),(old,None)=>old,(None,end)=>end};
                        }
                        owner.modulation_released=true;
                    }
                }
            }
        }
    }
    pub(super) fn stop_processing_owners(&mut self) {
        for index in 0..self.processing_owners.slots.len() {
            if self.processing_owners.slots[index].value.is_some() {
                self.end_processing_owner(ProcessingOwnerId(self.processing_owners.id(index)));
            }
        }
    }
    pub(super) fn release_processing_family(&mut self, family: FamilyId) {
        for index in 0..self.processing_owners.slots.len() {
            let Some(owner) = self.processing_owners.slots[index].value else {
                continue;
            };
            if !self.owner_has_family(&owner, family) {
                continue;
            }
            let has = self
                .plans
                .get(owner.plan.0)
                .is_some_and(|p| p.prepared.processing_scopes[owner.template].has_amplitude);
            if has && let Some(owner) = self.processing_owners.slots[index].value.as_mut(){owner.envelope.release();}
            // This is an explicit Core family-gate event (never source EOF).
            // Preserve the same shared-family authority as the existing scope
            // amplifier; an arbitrary physical member ending does not call it.
            if let Some(state)=self.processing_owners.slots[index].value.as_mut()
                &&!state.modulation_released
                &&let Some(generation)=self.plans.get_mut(state.plan.0){
                generation.dsp.owner_modulation.release(&generation.prepared.processing_modulation,index);
                if state.modulation_started&&let Some(gate)=generation.prepared.processing_scopes[state.template].modulation_gate{
                    let end=generation.dsp.owner_modulation.scope_gate_end(&generation.prepared.processing_modulation,index,gate.source,true);
                    state.modulation_source_end=match(state.modulation_source_end,end){(Some(old),Some(new))=>Some(old.min(new)),(old,None)=>old,(None,end)=>end};
                }state.modulation_released=true;
            }

        }
    }
    pub(super) fn choke_processing_family(&mut self, family: FamilyId, frames: u32) {
        for index in 0..self.processing_owners.slots.len() {
            let Some(owner) = self.processing_owners.slots[index].value else {
                continue;
            };
            if !self.owner_has_family(&owner, family) {
                continue;
            }
            if frames == 0 {
                self.end_processing_owner(ProcessingOwnerId(self.processing_owners.id(index)));
            } else if let Some(owner) = self.processing_owners.slots[index].value.as_mut() {
                owner.envelope.choke(frames);
            }
        }
    }
    pub(crate) fn mark_processing_onset_failed(&mut self, index: usize) {
        let Some((owner, _)) = self.voices.slots.get(index).and_then(|s| s.value.as_ref()).and_then(|v| v.processing) else { return; };
        if let Some(owner) = self.processing_owners.get_mut(owner.0) { owner.failed = true; }
    }
    fn end_processing_owner(&mut self, id: ProcessingOwnerId) {
        let Some(owner) = self.processing_owners.get(id.0).copied() else {
            return;
        };
        for source in owner.members.into_iter().flatten() {
            if self.voices.get(source.0).is_some() {
                self.end_voice(source);
            }
        }
        if let Some(generation)=self.plans.get_mut(owner.plan.0){generation.dsp.owner_modulation.stop(id.0.index);}
        self.processing_owners.remove(id.0);
        if let Some(note) = self.notes.get_mut(owner.note.0) {
            note.processing_owners -= 1;
        }
    }

    /// The actual owner row is provisionally prepared before physical cursor
    /// admission. Cold retries reseed only this unpublished row; shared restart
    /// domains publish only once every member has its pinned onset admitted.
    pub(super) fn prepare_processing_modulation(&mut self,id:ProcessingOwnerId,at:u64,phase:ProcessingModulationPhase)->Result<(),Error>{
        let owner=self.processing_owners.get_mut(id.0).ok_or(Error::StaleHandle)?;
        let note=self.notes.get(owner.note.0).ok_or(Error::StaleHandle)?;
        let expression=self.expressions.get(note.expression.0).ok_or(Error::StaleHandle)?;
        let generation=self.plans.get_mut(owner.plan.0).ok_or(Error::StaleHandle)?;
        if generation.prepared.processing_modulation.is_empty(){return Ok(());}
        let performance=self.selections[owner.note.0.index].performance;
        let inputs=crate::voice_mod::Inputs::new(note,expression.value,
            &self.performance_state.channel_current(performance,note.address).controllers,
            self.release_times[owner.note.0.index].counter(note.key_down(),at),
            &self.note_params[owner.note.0.index].mods,generation.controls.base_reals());
        let clock=crate::voice_mod::Clock{rate:f64::from(self.rate),tempo:self.tempo,now:at};
        let closed=owner.modulation_released||!note.gate();
        if !owner.modulation_started&&owner.modulation_prepared_at!=Some(at){
            generation.dsp.owner_modulation.start_scoped(&generation.prepared.processing_modulation,id.0.index,owner.template,&inputs,clock,owner.activation,closed);
            if let Some(gate)=generation.prepared.processing_scopes[owner.template].modulation_gate{
                owner.modulation_source_end=generation.dsp.owner_modulation.scope_gate_end(&generation.prepared.processing_modulation,id.0.index,gate.source,false);
            }
            // Physical consumers have no independent shared source. Only their
            // own local state is deferred until this actual cold attempt.
            for voice in owner.members.into_iter().flatten(){
                generation.modulation.defer_provider_restart(&generation.prepared.voice_modulation,voice.0.index);
            }
            owner.modulation_prepared_at=Some(at);owner.modulation_prepared_closed=closed;
        }else if !owner.modulation_started&&owner.modulation_prepared_closed!=closed{
            // Release has already reached this real row through the existing
            // note/family gate path. Keep its admitted phase/onset; no reset.
            // An onset-only closed birth ends at its real initial point. An
            // explicitly forecast row retains the ordinary future-point law.
            if let Some(gate)=generation.prepared.processing_scopes[owner.template].modulation_gate{
                let forecast=generation.dsp.owner_modulation.scoped_has_forecast(id.0.index,at)?;
                let end=generation.dsp.owner_modulation.scope_gate_end(&generation.prepared.processing_modulation,id.0.index,gate.source,forecast);
                owner.modulation_source_end=match(owner.modulation_source_end,end){(Some(old),Some(new))=>Some(old.min(new)),(old,None)=>old,(None,end)=>end};
            }
            owner.modulation_prepared_closed=closed;
        }
        // SampleStart observes times[F,F], with no implicit forecast during
        // trigger. Accepted same-frame CC/rate/depth edits must reach the first
        // future point, exactly as in the ordinary local source path.
        if phase==ProcessingModulationPhase::AdmissionOnset{return Ok(());}
        let mut ramp=generation.dsp.owner_modulation.advance(&generation.prepared.processing_modulation,id.0.index,&inputs,clock);
        if phase==ProcessingModulationPhase::Commit&&!owner.modulation_started{
            generation.dsp.owner_modulation.commit_scoped_start(id.0.index);
            owner.modulation_released=closed;owner.modulation_started=true;
        }
        if phase==ProcessingModulationPhase::Commit{
            if let Some(gate)=generation.prepared.processing_scopes[owner.template].modulation_gate{
                let factor=match gate.velocity{crate::VelocityCurve::Constant=>1.,crate::VelocityCurve::Linear=>inputs.velocity,crate::VelocityCurve::Power(p)=>inputs.velocity.powf(p)};
                ramp.from.scalar_gain*=factor;ramp.to.scalar_gain*=factor;
            }
            let mut parameters=generation.dsp.modulated_parameters.claim(0);
            parameters.copy_from_slice(&generation.dsp.parameters);
            generation.prepared.project_processing_outputs(owner.template,ramp,&mut parameters);
            generation.dsp.owner_modulation.project_processing_parameters(&generation.prepared.processing_modulation,
                id.0.index,owner.template,&generation.prepared.processing_scopes[owner.template].parameters,ramp.begin,ramp.end,self.rate,&mut parameters)?;
        }
        Ok(())
    }
    pub(super) fn render_processing_owners(&mut self, output: &mut [Frame], at: u64) {
        // Existing render_segment already splits at CELL and events, so this
        // task runs <=BLOCK and parameter ramps use the identical absolute clock.
        for index in 0..self.processing_owners.slots.len() {
            let Some(before) = self.processing_owners.slots[index].value else {
                continue;
            };
            if !before.published {
                continue;
            }
            let id = ProcessingOwnerId(self.processing_owners.id(index));
            let explicit = self.plans.get(before.plan.0).is_some_and(|generation|
                generation.prepared.processing_scopes[before.template].signal.is_some());
            // Shared history belongs to the activation, including ordinary
            // stereo imported chains. No member may advance it alone merely
            // because this owner does not use a wide signal stage.
            if before.failed { self.end_processing_owner(id); continue; }
            if !self.prepare_processing_cold_start(&before,at,output.len()) {continue;}
            if before.members.iter().flatten().any(|voice|
                self.voices.get(voice.0).is_some_and(|v| !v.started || v.cursor.holding_onset() || v.cursor.pending_start_runway() > 1)) {
                // Processing ownership is independent of take/Family startup
                // cohorts. Shared envelope and DSP state rest until all members.
                continue;
            }
            // Start/evaluate only after every actual member has an admitted
            // onset. Scheduled or cold unpublished source work cannot advance
            // this owner or publish a shared restart domain.
            if self.prepare_processing_modulation(id,at,ProcessingModulationPhase::Commit).is_err(){self.end_processing_owner(id);continue;}
            let Some(law) = self
                .plans
                .get(before.plan.0)
                .map(|p| p.prepared.processing_scopes[before.template].sum)
            else {
                continue;
            };
            let mut sum = [[0.; 2]; crate::dsp::BLOCK];
            let deadline=self.processing_owners.get(id.0).and_then(|owner|owner.modulation_source_end);
            let source_remaining=before.envelope.remaining().min(deadline.map_or(usize::MAX,|end|usize::try_from(end.saturating_sub(at)).unwrap_or(usize::MAX)));
            let source_len=if before.tail_remaining.is_some(){0}else{output.len().min(source_remaining)};
            let mut produced = 0usize;
            if explicit {
                match self.render_signal_sources(&before, source_len, at) {
                    Ok(count) => produced = count,
                    Err(_) => {
                        self.nonfinite_frames = self.nonfinite_frames.saturating_add(1);
                        self.end_processing_owner(id);
                        continue;
                    }
                }
            } else {
            // Member order, not physical slot order. No note-to-note state sharing.
            for source in before.members.into_iter().flatten() {
                if self.voices.get(source.0).is_none() {
                    continue;
                }
                let mut part = [[0.; 2]; crate::dsp::BLOCK];
                if source_len != 0 {
                    produced = produced.max(
                        self.render_source_voice::<false>(
                            source.0.index,
                            &mut part[..source_len],
                            at,
                            true,
                        )
                        .unwrap_or(0),
                    );
                }
                for (to, from) in sum.iter_mut().zip(part).take(output.len()) {
                    match law {
                        sampler_ir::ProcessingSumLaw::OrderedF32 => {
                            to[0] += from[0];
                            to[1] += from[1];
                        }
                    }
                }
            }
            }
            // Stop source reads exactly at the owning amplitude's termination;
            // shared post-amplitude state may still render its zero-input tail.
            if source_remaining!=usize::MAX && source_len==source_remaining {
                for source in before.members.into_iter().flatten() {
                    if self.voices.get(source.0).is_some() {
                        self.end_voice(source);
                    }
                }
            }
            let Some(mut owner) = self.processing_owners.get(id.0).copied() else {
                continue;
            };
            let Some(note) = self.notes.get(owner.note.0) else {
                continue;
            };
            let Some(expression) = self.expressions.get(note.expression.0) else {
                continue;
            };
            let native_note=match note.pitch{crate::NotePitch::Key(key)=>f64::from(key),crate::NotePitch::Absolute(pitch)=>pitch};
            let generation = match self.plans.get_mut(owner.plan.0) {
                Some(p) => p,
                None => continue,
            };
            let scope = &generation.prepared.processing_scopes[owner.template];
            let chain = scope
                .chain
                .map(|c| generation.prepared.voice_chains.view(c));
            let pan_home=chain.is_some_and(|chain|chain.expression_pan)
                || generation.prepared.processing_signals.get(owner.template).and_then(Option::as_ref).is_some_and(|signal|signal.expression_pan);
            let expression_gains=if pan_home {[expression.rendered.gain as f32;2]}else{expression.rendered.gains};
            let stage_expression=crate::Expression{pan:if pan_home{expression.rendered.pan}else{expression.value.pan},..expression.value};
            let mut tail_advance = 0usize;
            let len = if let Some(tail) = owner.tail_remaining {
                tail_advance = output.len().min(tail as usize);
                tail_advance
            } else if owner.members.iter().all(Option::is_none) {
                let signal_tail = generation.prepared.processing_signals.get(owner.template)
                    .and_then(Option::as_ref).map_or(0, |signal| signal.tail_frames);
                // The sum is admitted during preparation; no wrapping lifetime.
                let tail = chain.map_or(0, |c| c.owner_tail_frames()) + signal_tail;
                let prefix = produced.min(output.len());
                let len = output.len().min(prefix.saturating_add(tail as usize));
                tail_advance = len - prefix;
                owner.tail_remaining = Some(tail);
                len
            } else {
                output.len()
            };
            let wide=explicit && generation.prepared.processing_signals.get(owner.template)
                .and_then(Option::as_ref).is_some_and(|signal|signal.output.get()!=2);
            if wide {
                if signal::render_wide_stages(generation,&mut owner,index,len,at,&stage_expression,expression.rendered.gain as f32).is_err(){
                    generation.dsp.owner_signal_history.claim(index).fill(0.);
                    self.nonfinite_frames=self.nonfinite_frames.saturating_add(1);
                    self.end_processing_owner(id);
                    continue;
                }
            } else {
            if explicit && signal::render_stages(generation, &owner, index, len, at, &mut sum[..len], &stage_expression,native_note).is_err() {
                generation.dsp.owner_signal_history.claim(index).fill(0.);
                self.nonfinite_frames = self.nonfinite_frames.saturating_add(1);
                self.end_processing_owner(id);
                continue;
            }
            let mut cells = generation.dsp.owner_cells.claim(index);
            let mut delay = generation.dsp.owner_delays.claim(index);
            let filters = &mut generation.dsp.filters.as_mut_slice()[0];
            filters.clear_source_modulation();
            let mut fault = false;
            if let Some(chain) = chain {
                for &bus in &chain.tap_buses {
                    generation.dsp.feeds[bus].samples = [[0.; crate::dsp::BLOCK]; 2];
                }
                let projected=generation.dsp.modulated_parameters.claim(0);
                let context = crate::dsp::RenderContext {
                    controls: None,
                    amplifier: None,
                    expression: expression_gains,
                    delay: &mut delay[..chain.delay_frames],
                    parameters: if generation.dsp.owner_modulation.program(index).is_some(){&projected}else{&generation.dsp.parameters},
                    filters: crate::dsp::svf::FilterContext {
                        tempo: self.tempo,
                        bank: filters,
                        expression: Some((note.expression, stage_expression)),
                        spaces: &mut [],
                    },
                    at,
                    feeds: &mut generation.dsp.feeds,
                    trace: None,
                    stretch: None,
                };
                fault = chain.render_owner(
                    &mut owner.envelope,
                    &mut sum[..len],
                    &mut cells[..chain.stages()],
                    context,
                );
                if !fault {
                    for &bus in &chain.tap_buses {
                        let feed = &generation.dsp.feeds[bus].samples;
                        let target = generation.dsp.buses.input(bus, len);
                        for (i, frame) in target.iter_mut().enumerate() {
                            for c in 0..2 {
                                frame[c] +=
                                    (feed[c][i] * f64::from(expression_gains[c])) as f32;
                            }
                        }
                        generation.dsp.buses.fed(bus, len);
                    }
                }
            } else {
                for frame in &mut sum[..len] {
                    let level = owner
                        .envelope
                        .constant_level()
                        .unwrap_or_else(|| owner.envelope.next());
                    for c in 0..2 {
                        frame[c] *= level * expression_gains[c];
                        fault |= !frame[c].is_finite();
                    }
                }
            }
            if fault {
                sum.fill([0.; 2]);
                self.nonfinite_frames = self.nonfinite_frames.saturating_add(1);
            }
            let target = match scope.bus {
                Some(bus) => generation.dsp.buses.input(bus, len),
                None => &mut output[..len],
            };
            for (frame, from) in target.iter_mut().zip(sum) {
                for c in 0..2 {
                    frame[c] += from[c];
                }
            }
            if let Some(bus) = scope.bus {
                generation.dsp.buses.fed(bus, len);
            }
            drop((cells, delay));
            }
            if let Some(tail) = owner.tail_remaining {
                owner.tail_remaining = Some(tail.saturating_sub(tail_advance as u32));
            }
            let done = owner.tail_remaining == Some(0);
            if let Some(state) = self.processing_owners.get_mut(id.0) {
                *state = owner;
            }
            if done {
                self.end_processing_owner(id);
            }
        }
    }
}
