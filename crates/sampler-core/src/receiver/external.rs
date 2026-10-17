//! External segments park the actual native continuation, never a ticket arena
//! or a detached mutable receiver copy.
use crate::{BehaviorId,BehaviorOwner,Error,Input,NoteId,NotePitch,PlanId,Runtime};
use crate::behavior::{Continuation,PlanContext};
use crate::controller_event::ControllerEvent;
use sampler_ir::SourceEventProcessorId;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ExternalInputId { callback:BehaviorId, sequence:u64 }
impl ExternalInputId { pub fn behavior(self)->BehaviorId {self.callback} }
#[derive(Clone,Copy,Debug)]
pub enum ExternalEvent {
    Controller { source:crate::ChannelAddress, number:u8, value:u32, poly_key:Option<u8> },
    Note { note:NoteId, source:Input, pitch:NotePitch, velocity:f64, release_velocity:Option<f64>, released:bool },
    PhysicalRelease(super::PhysicalReleaseEvent),
    Plan,
}
#[derive(Clone,Copy,Debug)]
pub struct ExternalTurn {
    pub input:ExternalInputId,
    pub plan:PlanId,
    pub processor:SourceEventProcessorId,
    pub owner:crate::PreparedSourceOwnerIndex,
    pub original_frame:u64,
    pub resume_frame:u64,
    /// Actual checked segment-admission order, independent of original capture time.
    pub order:u64,
    pub event:ExternalEvent,
    pub receiver:Option<crate::ReceiverId>,
}
#[derive(Clone,Copy,Debug)]
pub enum ExternalOutput {
    /// Transparent event: retains original wire precision/provenance.
    Original,
    /// Explicit normalized full-resolution authored replacement. A Lua adapter
    /// performs its declared seven-bit conversion before this API.
    Controller { number:u8,value:u32 },
    /// Manual gate distinct from the actual physical input gate.
    Note { pitch:NotePitch,velocity:f64 },
    /// Exact previously returned child, never a key-only recipient search.
    Release { note:NoteId,velocity:Option<f64> },
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ExternalOutputResult { Forwarded,Note(NoteId) }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Phase { Pending,Running,Sleeping }
#[derive(Clone,Copy,Debug)]
pub(crate) struct ExternalState {
    processor:SourceEventProcessorId,
    owner:crate::PreparedSourceOwnerIndex,
    original_frame:u64,
    resume_frame:u64,
    order:u64,
    sequence:u64,
    await_pc:usize,
    phase:Phase,
    event:ExternalEvent,
    controller:Option<ControllerEvent>,
    controller_path_used:bool,
}
impl ExternalState {
    pub(crate) fn blocks_dispatch(self)->bool {self.phase!=Phase::Sleeping}
}
impl Runtime {
    pub(crate) fn await_external_input(&mut self,id:BehaviorId,processor:SourceEventProcessorId)->Result<(),Error>{
        let callback=*self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        if callback.outcome.is_some(){return Err(Error::StaleHandle);}
        let plan=self.behavior_plan(callback.owner)?;
        let prepared=&self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared;
        let processor_row=prepared.source_owners.processor(processor).ok_or(Error::InvalidInput)?;
        let owner=processor_row.owner;
        // Lookup is actual source identity; the compiled stage must establish
        // executable kind/bypass and hierarchical branch admission beforehand.
        let sequence=callback.external_sequence.checked_add(1).ok_or(Error::Capacity)?;
        let (original_frame,event,controller)=match callback.external {
            Some(state) if state.phase==Phase::Sleeping && state.processor==processor => {
                if state.resume_frame!=self.now{return Err(Error::PastEvent);}
                (state.original_frame,state.event,state.controller)
            }
            Some(_)=>return Err(Error::Busy),
            None=>{
                let event=match callback.context {
                    PlanContext::Controller(event)=>ExternalEvent::Controller{
                        source:event.receiver.map_or(event.origin,|capture|capture.source),
                        number:event.number,value:event.value,poly_key:event.poly_key},
                    crate::behavior::PlanContext::NoteControllers(context) if context.physical_event.is_some()=>
                        ExternalEvent::PhysicalRelease(context.physical_event.ok_or(Error::InvalidInput)?),
                    _=>match callback.owner {
                        BehaviorOwner::Note(note)=>{
                            let n=self.notes.get(note.0).ok_or(Error::StaleHandle)?;
                            let source=n.input_source.or(n.input).ok_or(Error::InvalidInput)?;
                            ExternalEvent::Note{note,source,pitch:n.pitch,velocity:n.velocity,
                                release_velocity:self.release_times[note.0.index].velocity,
                                released:matches!(callback.note_stage,Some(crate::behavior::NoteStage::Release(_)))}
                        }
                        BehaviorOwner::Plan(_)=>ExternalEvent::Plan,
                    },
                };
                let controller=match callback.context{PlanContext::Controller(event)=>Some(event),_=>None};
                let original_frame=match callback.context {
                    PlanContext::Controller(event)=>event.original_frame,
                    PlanContext::NoteControllers(context) if context.physical_event.is_some()=>
                        context.physical_event.ok_or(Error::InvalidInput)?.original_frame,
                    _=>match callback.owner {
                        BehaviorOwner::Note(note)=>{
                            let time=&self.release_times[note.0.index];
                            if matches!(callback.note_stage,Some(crate::behavior::NoteStage::Release(_))){time.key_at}else{time.admitted_at}
                        },
                        BehaviorOwner::Plan(_)=>self.now,
                    },
                };
                (original_frame,event,controller)
            }
        };
        let order=self.next_external_turn_order.checked_add(1).ok_or(Error::Capacity)?;
        self.next_external_turn_order=order;
        let c=self.behaviors.get_mut(id.0).ok_or(Error::StaleHandle)?;
        c.external_sequence=sequence;
        c.external=Some(ExternalState{processor,owner,original_frame,resume_frame:self.now,order,sequence,
            await_pc:c.pc.checked_sub(1).ok_or(Error::InvalidInput)?,phase:Phase::Pending,event,controller,controller_path_used:false});
        c.waiting=true;Ok(())
    }
    fn external_turn(&self,id:BehaviorId,c:&Continuation,state:ExternalState)->Result<ExternalTurn,Error>{
        Ok(ExternalTurn{input:ExternalInputId{callback:id,sequence:state.sequence},
            plan:self.behavior_plan(c.owner)?,processor:state.processor,owner:state.owner,
            original_frame:state.original_frame,resume_frame:state.resume_frame,order:state.order,event:state.event,receiver:c.context.receiver()})
    }
    /// Actual parked owners remain visible while the external owner runs. Actual
    /// segment admission order, not old capture timestamps, selects the next turn.
    pub fn pending_external_turn(&self)->Result<Option<ExternalTurn>,Error>{
        if self.external_execution_aborted{return Err(Error::Canceled);}
        if
!self.has_external_programs(){return Ok(None);}
        let mut first:Option<(u64,ExternalTurn)>=None;
        for (index,slot) in self.behaviors.slots.iter().enumerate(){
            let Some(c)=slot.value.as_ref() else{continue;};
            let Some(state)=c.external else{continue;};
            if c.outcome.is_some()||state.phase==Phase::Sleeping{continue;}
            if state.resume_frame<self.now{return Err(Error::PastEvent);}
            if state.resume_frame!=self.now{continue;}
            let turn=self.external_turn(BehaviorId(self.behaviors.id(index)),c,state)?;
            if first.as_ref().is_none_or(|old|state.order<old.0){first=Some((state.order,turn));}
        }
        Ok(first.map(|row|row.1))
    }
    fn validate_external_segment(&self,id:ExternalInputId,phase:Phase)->Result<(Continuation,ExternalState),Error>{
        if self.external_execution_aborted{return Err(Error::Canceled);}
        let c=*self.behaviors.get(id.callback.0).ok_or(Error::StaleHandle)?;
        let state=c.external.ok_or(Error::StaleHandle)?;
        if c.outcome.is_some()||state.sequence!=id.sequence||state.phase!=phase{return Err(Error::StaleHandle);}
        if state.resume_frame!=self.now{return Err(Error::PastEvent);}
        let plan=self.behavior_plan(c.owner)?;
        if self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners.processor(state.processor)
            .is_none_or(|processor|processor.owner!=state.owner){return Err(Error::StaleHandle);}
        Ok((c,state))
    }
    pub fn begin_external_turn(&mut self,id:ExternalInputId)->Result<ExternalTurn,Error>{
        let (c,state)=self.validate_external_segment(id,Phase::Pending)?;
        let turn=self.external_turn(id.callback,&c,state)?;
        self.behaviors.get_mut(id.callback.0).ok_or(Error::StaleHandle)?.external.as_mut().ok_or(Error::StaleHandle)?.phase=Phase::Running;
        Ok(turn)
    }
    /// Each successful intentional post is a musical side effect. A later post
    /// refusing does not roll it back, as with existing synchronous callbacks.
    pub fn post_external_output(&mut self,id:ExternalInputId,output:ExternalOutput)->Result<ExternalOutputResult,Error>{
        let (callback,state)=self.validate_external_segment(id,Phase::Running)?;
        let result=match output {
            ExternalOutput::Original=>match state.event {
                ExternalEvent::Controller{..}=>{
                    let mut event=state.controller.ok_or(Error::InvalidInput)?;
                    let still_pending=matches!(callback.context,PlanContext::Controller(current) if current.pending);
                    if !state.controller_path_used && still_pending {
                        self.post_external_owned_controller(id.callback,event)?;
                    }else{
                        event.stage=event.stage.checked_add(1).ok_or(Error::Capacity)?;
                        event.pending=true;event.reserved=0;
                        self.admit_controller(self.behavior_plan(callback.owner)?,event)?;
                    }
                    ExternalOutputResult::Forwarded
                }
                ExternalEvent::Note{pitch,velocity,released:false,..}=>ExternalOutputResult::Note(
                    self.play_behavior(id.callback,pitch,velocity,crate::Inheritance::Expression,crate::Duration::UntilSilent,0)?),
                // An original release must carry its actual forwarded child ID;
                // no physical-gate or same-key guess may release unrelated notes.
                ExternalEvent::Note{released:true,..}|ExternalEvent::PhysicalRelease(_)|ExternalEvent::
Plan=>return Err(Error::InvalidInput),
            },
            ExternalOutput::Controller{number,value}=>{
                let still_pending=matches!(callback.context,PlanContext::Controller(current) if current.pending);
                if number>=130{return Err(Error::InvalidInput);}
                if !state.controller_path_used && still_pending {
                    let mut event=state.controller.ok_or(Error::InvalidInput)?;
                    event.number=number;event.value=value;event.poly_key=None;event.poly_recipients=None;
                    if let Some(capture)=event.receiver.as_mut(){
                        capture.provenance=super::components::ReceiverProvenance::Script;
                        capture.kind=super::components::ReceiverActionKind::Controller;
                    }
                    self.post_external_owned_controller(id.callback,event)?;
                }else{self.write_behavior_controller(id.callback,number,value)?;}
                ExternalOutputResult::Forwarded
            }
            ExternalOutput::Note{pitch,velocity}=>ExternalOutputResult::Note(
                self.play_behavior(id.callback,pitch,velocity,crate::Inheritance::Expression,crate::Duration::UntilSilent,0)?),
            ExternalOutput::Release{note,velocity}=>{
                match callback.owner {
                    BehaviorOwner::Note(parent)=>{
                        if self.notes.get(note.0).ok_or(Error::StaleHandle)?.parent!=Some(parent){return Err(Error::InvalidInput);}
                    }
                    BehaviorOwner::Plan(plan)=>{
                        // UVI releaseVoice receives the actual returned NoteId;
                        // scope it to this retained plan, never a key search.
                        if self.note_plan(note)?!=plan{return Err(Error::InvalidInput);}
                    }}
                crate::release::validate_velocity(velocity)?;
                self.key_up_with_cause(note,velocity,crate::ReleaseCause::Script)?;
                ExternalOutputResult::Forwarded
            }
        };
        if matches!(output,ExternalOutput::Original|ExternalOutput::Controller{..}) && state.controller.is_some(){
            self.behaviors.get_mut(id.callback.0).ok_or(Error::StaleHandle)?.external.as_mut().ok_or(Error::StaleHandle)?.controller_path_used=true;
        }
        Ok(result)
    }
    fn post_external_owned_controller(&mut self,id:BehaviorId,mut event:ControllerEvent)->Result<(),Error>{
        let before=*self.controller_event_mut(id)?;
        event.reserved=before.reserved;event.pending=true;
        *self.controller_event_mut(id)?=event;
        if let Err(error)=self.forward_controller(id){
            *self.controller_event_mut(id)?=before;return Err(error);
        }
        Ok(())
    }
    /// ACK current F. None consumes unposted original and finishes; Some(F')
    /// keeps this SAME callback/NoteId lease until its actual scheduled resume.
    pub fn complete_external_segment(&mut self,id:ExternalInputId,next_resume:Option<u64>)->Result<(),Error>{
        let (_,state)=self.validate_external_segment(id,Phase::Running)?;
        if let Some(at)=next_resume {
            if at<=self.now{return Err(Error::PastEvent);}
            if self.available_commands()==0{return Err(Error::Capacity);}
            state.sequence.checked_add(1).ok_or(Error::Capacity)?;
        }
        if next_resume.is_some() { self.reserved_commands+=1; }
        self.flush_zone_parameters(id.callback);
        self.flush_deferred(id.callback);
        if next_resume.is_some() { self.reserved_commands-=1; }
        if let Ok(event)=self.controller_event_mut(id.callback){event.pending=false;}
        self.release_controller_reserve(id.callback);
        let c=self.behaviors.get_mut(id.callback.0).ok_or(Error::StaleHandle)?;
        if c.outcome.is_some(){return Ok(());}
        match next_resume {
            Some(at)=>{
                let pending=c.external.as_mut().ok_or(Error::StaleHandle)?;
                pending.phase=Phase::Sleeping;pending.resume_frame=at;
                c.pc=state.await_pc;c.waiting=true;self.queue(at,crate::Action::Resume(id.callback));
            }
            None=>{c.external=None;c.waiting=false;self.resume_behavior(id.callback);}
        }
        Ok(())
    }
    pub fn cancel_external_input(&mut self,id:ExternalInputId)->Result<(),Error>{
        // Cancellation is valid before delivery, during a segment, or while its
        // next resume sleeps. An old sequence can never cancel a newer turn.
        let c=self.behaviors.get(id.callback.0).ok_or(Error::StaleHandle)?;
        if c.outcome.is_some()||c.external.is_none_or(|state|state.sequence!=id.sequence){return Err(Error::StaleHandle);}
        self.cancel_behavior(id.callback)
    }
    pub(crate) fn has_external_programs(&self)->bool{
        self.plans.slots.iter().any(|slot|slot.value.as_ref().is_some_and(|generation|generation.prepared.external_programs))
    }
    pub(crate) fn external_clock_blocked(&self)->bool {
        if !self.has_external_programs(){return false;}
        if self.pending_physical_event_frame().is_some_and(|at|at<=self.now){return true;}
        self.behaviors.slots.iter().any(|slot|slot.value.as_ref().is_some_and(|c|
            c.outcome.is_none()&&c.external.is_some_and(|state|state.phase!=Phase::Sleeping&&state.resume_frame<=self.now)))
    }
    /// Pure maximum prefix before the next actual event/segment boundary. A
    /// returned zero asks the sole execution owner to service the current frame;
    /// Busy means an actual external turn must ACK before PCM may advance.
    pub fn external_render_frames(&self,requested:usize)->Result<usize,Error>{
        if self.external_execution_aborted{return Err(Error::Canceled);}
        if
!self.has_external_programs(){return Ok(requested);}
        let end=self.now.checked_add(u64::try_from(requested).map_err(|_|Error::ClockOverflow)?).ok_or(Error::ClockOverflow)?;
        if self.external_clock_blocked(){return Err(Error::Busy);}
        let due=self.next_scheduled_frame().into_iter().chain(self.next_external_frame()).min();
        let boundary=due.map_or(end,|at|at.min(end));
        Ok(usize::try_from(boundary.saturating_sub(self.now)).map_err(|_|Error::ClockOverflow)?)
    }
    pub(crate) fn validate_external_render_prefix(&self,frames:usize)->Result<(),Error>{
        if self.external_render_frames(frames)?<frames{return Err(Error::Busy);}
        Ok(())
    }
    /// Immutable next external segment boundary, including a currently delivered
    /// turn. A worker combines this with next_scheduled_frame for block slicing.
    pub fn next_external_frame(&self)->Option<u64>{
        if !self.has_external_programs(){return None;}
        self.behaviors.slots.iter().filter_map(|slot|{
            let c=slot.value.as_ref()?;if c.outcome.is_some(){return None;}
            c.external.map(|state|state.resume_frame)
        }).chain(self.pending_physical_event_frame()).
min()
    }
    /// Drain due native work at F before rack polling. Every Part must reach this
    /// barrier before ANY Part renders the following sample interval.
    pub fn service_external_boundary(&mut self)->Result<Option<ExternalTurn>,Error>{
        if self.external_execution_aborted{return Err(Error::Canceled);}
        if !self.external_clock_blocked(){self.apply_due();}
        self.pending_external_turn()
    }
}

/// Diagnostic counts only: actual generational tokens/physical receipts retain
/// their ordinary APIs and identities through cancellation and host ACK.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ExternalAbortReport{pub callbacks_cancelled:usize,pub external_turns_cancelled:usize}
impl Runtime {
    pub fn external_execution_aborted(&self)->bool{self.external_execution_aborted}
    /// Terminal execution fence at the actual Core frontier. No due dispatch,
    /// timeline advance, heap work, state extraction, rewind or retry occurs.
    /// Physical receipts/debts remain drainable; they are NOT invented key-ups.
    pub fn abort_external_execution(&mut self)->ExternalAbortReport{
        if self.external_execution_aborted{return ExternalAbortReport{callbacks_cancelled:0,external_turns_cancelled:0};}
        let mut report=ExternalAbortReport{callbacks_cancelled:0,external_turns_cancelled:0};
        for slot in &self.behaviors.slots{if let Some(callback)=&slot.value&&callback.outcome.is_none(){
            report.callbacks_cancelled+=1;report.external_turns_cancelled+=usize::from(callback.external.is_some());
        }}
        self.external_execution_aborted=true;self.panic();
        // Their real notes were closed by panic; no delayed authored output may
        // survive the fault and replay when a canceled callback is acknowledged.
        self.deferred.clear();report
    }
}
