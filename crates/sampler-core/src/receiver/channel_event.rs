//! Typed channel-program events use the existing plan-owned Continuation. There
//! is no program/receiver mirror in an adapter and no physical NoteId admission.
use crate::{BehaviorId,ChannelAddress,ChannelScope,Error,PerformanceId,PlanId,Prepared,Runtime};
use crate::behavior::PlanContext;
use crate::controller_event::CapturedControllerTarget;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ProgramBankIntent {
    /// MIDI2 Bank Valid=0; also an explicit authored preserve operation.
    KeepSelected,
    /// MIDI1 Program uses the accepted pending Bank Select CC0/32 state.
    UsePendingBankSelect,
    Select([u8;2]),
}
impl ProgramBankIntent {pub(crate) fn valid(self)->bool{!matches!(self,Self::Select(bank) if bank.into_iter().any(|value|value>=128))}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ProgramSelection {pub program:u8,pub bank:[u8;2]}
/// Protocol state is real, but it must not be advertised as a factory catalog.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ProgramPlaybackCapability {NoPresetCatalog}
#[derive(Clone,Copy,Debug)]
pub(crate) enum ProgramProvenance {Wire(super::Version),Script}
#[derive(Clone,Copy,Debug)]
pub(crate) struct ProgramEvent {
    pub original_frame:u64,
    pub performance:usize,
    /// Logical execution target; physical provenance never changes on repost.
    pub origin:ChannelAddress,
    pub source:ChannelAddress,
    pub channels:u16,
    pub epochs:[u64;16],
    pub secondary:Option<CapturedControllerTarget>,
    pub receiver:Option<super::ReceiverId>,
    pub host_epochs:Option<[u64;16]>,
    pub provenance:ProgramProvenance,
    pub program:u8,
    pub bank:ProgramBankIntent,
    pub stage:usize,
    pub pending:bool,
    pub reserved:usize,
}
impl ProgramEvent {
    pub fn scope(self)->ChannelScope{ChannelScope{protocol:self.origin.protocol,port:self.origin.port,group:self.origin.group,channels:self.channels}}
    pub(crate) fn controller_capture(self)->Option<super::components::CapturedReceiver>{
        self.receiver.map(|receiver|super::components::CapturedReceiver{
            receiver,kind:super::components::ReceiverActionKind::Controller,provenance:super::components::ReceiverProvenance::Script,
            source:self.source,host_epochs:self.host_epochs,policy_epochs:self.epochs,
        })
    }
}
impl Prepared {
    pub fn with_program_change_programs(self,programs:Vec<usize>)->Result<Self,Error>{
        let mut stages=self.stages.to_vec();stages.resize(stages.len().max(programs.len()),crate::Stage::default());
        for stage in &mut stages{stage.program_change=None;}
        for (stage,program) in stages.iter_mut().zip(programs){stage.program_change=Some(program);}
        self.with_stages(stages)
    }
}
impl Runtime {
    pub fn program_selection(&self,performance:PerformanceId,address:ChannelAddress)->Result<Option<ProgramSelection>,Error>{
        if address.channel>=16||address.group>=16{return Err(Error::InvalidInput);}
        let performance=self.performance_index(performance)?;Ok(self.performance_state.channel_current(performance,address).program)
    }
    pub fn program_playback_capability(&self,plan:PlanId)->Result<ProgramPlaybackCapability,Error>{
        self.plans.get(plan.0).ok_or(Error::StaleHandle)?;Ok(ProgramPlaybackCapability::NoPresetCatalog)
    }
    /// A decoded wire Program event. Optional bank follows that version's law.
    pub fn receive_program_change(&mut self,performance:PerformanceId,origin:ChannelAddress,version:super::Version,program:u8,bank:Option<[u8;2]>)->Result<(Option<BehaviorId>,usize),Error>{
        if (version==super::Version::Midi1&&(bank.is_some()||origin.protocol!=crate::Protocol::Midi1))||(version==super::Version::Midi2&&origin.protocol!=crate::Protocol::Midi2){return Err(Error::InvalidInput);}
        self.dispatch_program_change_targets(performance,origin,origin,1u16.checked_shl(u32::from(origin.channel)).unwrap_or(0),program,
            if version==super::Version::Midi1{ProgramBankIntent::UsePendingBankSelect}else{bank.map_or(ProgramBankIntent::KeepSelected,ProgramBankIntent::Select)},None,None,ProgramProvenance::Wire(version))
    }
    pub fn dispatch_program_change(&mut self,performance:PerformanceId,origin:ChannelAddress,channels:u16,program:u8,bank:ProgramBankIntent)->Result<(Option<BehaviorId>,usize),Error>{
        self.dispatch_program_change_targets(performance,origin,origin,channels,program,bank,None,None,ProgramProvenance::Script)
    }
    pub(crate) fn dispatch_program_change_targets(&mut self,performance:PerformanceId,origin:ChannelAddress,source:ChannelAddress,channels:u16,program:u8,bank:ProgramBankIntent,secondary:Option<ChannelScope>,receiver:Option<super::ReceiverId>,provenance:ProgramProvenance)->Result<(Option<BehaviorId>,usize),Error>{
        if origin.channel>=16||origin.group>=16||source.channel>=16||source.group>=16||channels==0||program>=128||!bank.valid(){return Err(Error::InvalidInput);}
        let performance=self.performance_index(performance)?;
        if let Some(id)=receiver{let state=self.receiver(id)?;if state.performance!=self.performance(performance)?||state.port!=origin.port||state.group!=origin.group{return Err(Error::InvalidInput);}}
        self.apply_due();
        let receiver=match receiver{Some(id)=>Some(id),None=>self.capture_registered_program(performance,origin,channels)?};
        let scope=ChannelScope{protocol:origin.protocol,port:origin.port,group:origin.group,channels};
        let event=ProgramEvent{original_frame:self.now,performance,origin,source,channels,epochs:self.performance_state.epochs(performance,scope),
            secondary:secondary.map(|scope|CapturedControllerTarget{scope,epochs:self.performance_state.epochs(performance,scope)}),
            receiver,host_epochs:receiver.map(|id|self.receiver_host_epochs(id,performance)).transpose()?.flatten(),
            provenance,program,bank,stage:0,pending:true,reserved:0};
        self.validate_program_event(self.active_plan,event)?;
        let callback=self.admit_program_event(self.active_plan,event)?;
        Ok((callback,if callback.is_none(){self.program_target_count(event)}else{0}))
    }
    fn program_targets(&self,event:ProgramEvent)->(ChannelScope,Option<ChannelScope>){
        (self.performance_state.surviving_scope(event.performance,event.scope(),event.epochs),event.secondary.map(|target|self.performance_state.surviving_scope(event.performance,target.scope,target.epochs)).filter(|scope|scope.channels!=0))
    }
    fn program_target_count(&self,event:ProgramEvent)->usize{
        let (primary,secondary)=self.program_targets(event);
        primary.channels.count_ones() as usize+secondary.map_or(0,|secondary|(0..16u8).filter(|&channel|secondary.channels&(1<<channel)!=0&&!primary.contains(ChannelAddress{protocol:secondary.protocol,port:secondary.port,group:secondary.group,channel})).count())
    }
    pub(super) fn validate_program_event(&self,plan:PlanId,event:ProgramEvent)->Result<(),Error>{
        if event.program>=128||!event.bank.valid()||event.origin.channel>=16||event.origin.group>=16||event.source.channel>=16||event.source.group>=16{return Err(Error::InvalidInput);}
        self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        if let Some(id)=event.receiver{let state=self.receiver(id)?;if state.performance!=self.performance(event.performance)?||state.port!=event.origin.port||state.group!=event.origin.group{return Err(Error::InvalidInput);}}
        let (scope,secondary)=self.program_targets(event);
        if let Some(id)=event.receiver{
            let state=self.receiver(id)?;
            for target in [Some(scope),secondary].into_iter().flatten(){
                if target.port!=state.port||target.group!=state.group{return Err(Error::InvalidInput);}
                let domain=super::state::OwnedReceiver::domain(target.protocol)?;
                if (0..16u8).any(|channel|target.channels&(1<<channel)!=0&&(!state.accepts_wire_channel(domain,channel)||(state.zone_domain(domain)&&channel!=state.manager_channel()))){return Err(Error::InvalidInput);}
            }
        }
        if scope.channels==0&&secondary.is_none(){return Ok(());}
        // Same union admission and actual row owner as controllers; no bank write
        // or Stage/Continuation publication occurs until the complete union fits.
        let primary=if scope.channels!=0{scope}else{secondary.ok_or(Error::InvalidInput)?};
        self.performance_state.validate_scopes(event.performance,primary,secondary.filter(|scope|*scope!=primary))
    }
    pub(super) fn admit_program_event(&mut self,plan:PlanId,mut event:ProgramEvent)->Result<Option<BehaviorId>,Error>{
        if self.external_execution_aborted{return Err(Error::Canceled);}
        let (scope,secondary)=self.program_targets(event);event.channels=scope.channels;
        event.secondary=event.secondary.filter(|_|secondary.is_some());
        if event.channels==0&&secondary.is_none(){return Ok(None);}
        self.validate_program_event(plan,event)?;
        let stages=&self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.stages;
        let next=stages.iter().enumerate().skip(event.stage).find_map(|(index,stage)|stage.program_change.map(|program|(index,program)));
        if let Some((stage,program))=next{
            let count=stages[event.stage..].iter().filter(|stage|stage.program_change.is_some()).count();
            if !self.behavior_room(count){return Err(Error::Capacity);}
            event.stage=stage;event.reserved=count-1;
            self.validate_plan_context(plan,program,PlanContext::Program(event))?;
            self.behaviors.reserve(event.reserved);
            match self.admit_plan_context(plan,program,PlanContext::Program(event)){
                Ok(id)=>{self.resume_behavior(id);Ok(Some(id))},
                Err(error)=>{self.behaviors.unreserve(event.reserved);Err(error)}
            }
        }else{self.publish_program_event(plan,event)?;Ok(None)}
    }
    pub(super) fn forward_program_event(&mut self,id:BehaviorId)->Result<(),Error>{
        let callback=*self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let PlanContext::Program(event)=callback.context else{return Err(Error::InvalidInput);};
        if !event.pending{return Ok(());}
        let plan=self.behavior_plan(callback.owner)?;
        let next=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.stages.iter().enumerate().skip(event.stage+1).find_map(|(index,stage)|stage.program_change.map(|program|(index,program)));
        if let Some((stage,program))=next{
            let remaining=event.reserved.checked_sub(1).ok_or(Error::Capacity)?;
            self.behaviors.unreserve(1);
            let next=self.admit_plan_context(plan,program,PlanContext::Program(ProgramEvent{stage,reserved:remaining,..event}));
            let next=match next{Ok(next)=>next,Err(error)=>{self.behaviors.reserve(1);return Err(error);}};
            self.behaviors.get_mut(id.0).ok_or(Error::StaleHandle)?.context=PlanContext::Program(ProgramEvent{pending:false,reserved:0,..event});
            self.resume_behavior(next);
        }else{
            self.publish_program_event(plan,event)?;
            self.behaviors.get_mut(id.0).ok_or(Error::StaleHandle)?.context=PlanContext::Program(ProgramEvent{pending:false,..event});
        }Ok(())
    }
    fn publish_program_event(&mut self,plan:PlanId,event:ProgramEvent)->Result<(),Error>{
        self.validate_program_event(plan,event)?;let (primary,secondary)=self.program_targets(event);
        for scope in [Some(primary).filter(|scope|scope.channels!=0),secondary].into_iter().flatten(){
            self.performance_state.publish_program(event.performance,scope,event.program,event.bank);
        }Ok(())
    }
}
