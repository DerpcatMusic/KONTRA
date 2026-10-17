//! Plan-owned processor tasks. Creator provenance is independent of any retained
//! original event object; no task is made a Note owner to borrow its lifetime.
use crate::{BehaviorId, Error, PerformanceId, PlanId, Prepared, Runtime};
use crate::behavior::{PlanContext, NoteControllerContext};
use crate::controller_event::{CapturedControllerTarget,ControllerEvent};
use super::external::{ExternalInputId,Phase};
use sampler_ir::SourceEventProcessorId;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum SourceTaskFamily { Init, Transport, Spawned, Run }
#[derive(Clone,Copy,Debug,PartialEq)]
pub struct SourceTaskHostContext {
    /// Actual Runtime-local execution F; the rack maps its installation origin
    /// before admission. This is not an alias for the absolute host rack frame.
    pub frame:u64,pub tempo:f64,pub playing:bool
}
#[derive(Clone,Copy,Debug,PartialEq)]
pub enum SourceTaskReason { Init(SourceTaskHostContext),Transport(SourceTaskHostContext),Spawned,Run }
impl SourceTaskReason {
    pub fn family(self)->SourceTaskFamily {match self{Self::Init(_)=>SourceTaskFamily::Init,Self::Transport(_)=>SourceTaskFamily::Transport,Self::Spawned=>SourceTaskFamily::Spawned,Self::Run=>SourceTaskFamily::Run}}
    fn validate(self,now:u64)->Result<(),Error>{match self{
        Self::Init(host)|Self::Transport(host)=>{
            if host.frame!=now{return Err(Error::PastEvent);}
            if !host.tempo.is_finite()||host.tempo<=0.{return Err(Error::InvalidInput);}
            Ok(())
        },_=>Ok(())}}
}
/// A task has no triggering physical note. Event means its invocation genuinely
/// came from an event callback; it is not a lease or an expression identity.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum SourceTaskCreator {None,Event,NonMusicalEvent(NonMusicalTaskOrigin)}
/// Precise nonmusical callback provenance; never a physical Note/Receiver lease.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum NonMusicalTaskOrigin {RestoredWidget}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum PreparedGlobalTaskOrigin {None,RestoredWidgetEvent}
impl PreparedGlobalTaskOrigin {
    fn creator(self)->SourceTaskCreator{match self{
        Self::None=>SourceTaskCreator::None,
        Self::RestoredWidgetEvent=>SourceTaskCreator::NonMusicalEvent(NonMusicalTaskOrigin::RestoredWidget),
    }}
}
/// One actual retained coroutine's compiled entry request. Equal processor and
/// family rows are valid independent coroutines, not duplicate VM task IDs.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PreparedGlobalSourceTask {
    pub processor:SourceEventProcessorId,pub family:SourceTaskFamily,pub origin:PreparedGlobalTaskOrigin,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PreparedSourceTaskBinding {pub processor:SourceEventProcessorId,pub family:SourceTaskFamily,pub program:usize}
#[derive(Clone,Copy,Debug)]
pub(crate) enum TaskDomain {
    Global{performance:usize},
    Captured{performance:usize,origin:crate::ChannelAddress,source:crate::ChannelAddress,channels:u16,epochs:[u64;16],secondary:Option<CapturedControllerTarget>,receiver:Option<super::components::CapturedReceiver>,stage:usize},
}
impl TaskDomain {
    pub(crate) fn performance(self)->usize{match self{Self::Global{performance}|Self::Captured{performance,..}=>performance}}
    pub(crate) fn source(self)->Option<crate::ChannelAddress>{match self{Self::Captured{source,..}=>Some(source),_=>None}}
    pub(crate) fn receiver(self)->Option<crate::ReceiverId>{match self{Self::Captured{receiver,..}=>receiver.map(|capture|capture.receiver),_=>None}}
    pub(crate) fn stage(self)->Option<usize>{match self{Self::Captured{stage,..}=>Some(stage),_=>None}}
    pub(crate) fn event(self,number:u8,value:u32,now:u64)->Option<ControllerEvent>{match self{
        Self::Global{..}=>None,
        Self::Captured{performance,origin,source,channels,epochs,secondary,mut receiver,stage}=>{
            if receiver.is_none()&&source!=origin{return None;}
            if let Some(capture)=receiver.as_mut(){capture.kind=super::components::ReceiverActionKind::Controller;capture.provenance=super::components::ReceiverProvenance::Script;}
            Some(ControllerEvent{original_frame:now,performance,origin,channels,epochs,secondary,receiver,number,value,poly_key:None,poly_recipients:None,stage:stage+1,pending:true,reserved:0})
        }
    }}
}
#[derive(Clone,Copy,Debug)]
pub(crate) struct RunParent {pub input:ExternalInputId,pub resume_sequence:u64,pub priority:u32}
#[derive(Clone,Copy,Debug)]
pub(crate) struct TaskContext {
    pub processor:SourceEventProcessorId,pub reason:SourceTaskReason,pub creator:SourceTaskCreator,
    pub domain:TaskDomain,pub original_frame:u64,pub run_parent:Option<RunParent>,
}
#[derive(Clone,Copy,Debug)]
pub struct SourceTaskEvent {pub reason:SourceTaskReason,pub creator:SourceTaskCreator,pub source:Option<crate::ChannelAddress>}
/// Mutable once-only state belongs to a real installed generation, not the VM
/// address, a snapshot of another generation, or the immutable source table.
pub(crate) struct TaskState {initialized:Box<[bool]>}
impl TaskState {
    pub(crate) fn new(prepared:&Prepared)->Result<Self,Error>{
        std::alloc::Layout::array::<bool>(prepared.source_tasks.len()).map_err(|_|Error::Capacity)?;
        let mut values=Vec::new();values.try_reserve_exact(prepared.source_tasks.len()).map_err(|_|Error::Capacity)?;
        std::alloc::Layout::array::<bool>(values.capacity()).map_err(|_|Error::Capacity)?;
        values.resize(prepared.source_tasks.len(),false);Ok(Self{initialized:values.into_boxed_slice()})
    }
}
impl Prepared {
    /// Offaudio explicit executable entries. A source metadata row is not a task
    /// binding and cannot activate Init or transport by itself.
    pub fn with_source_tasks(mut self,bindings:Vec<PreparedSourceTaskBinding>)->Result<Self,Error>{
        std::alloc::Layout::array::<PreparedSourceTaskBinding>(bindings.capacity()).map_err(|_|Error::Capacity)?;
        for (index,binding) in bindings.iter().enumerate(){
            let row=self.source_owners.processor(binding.processor).ok_or(Error::InvalidInput)?;
            if row.kind.as_ref()!="ScriptProcessor"||bindings[..index].iter().any(|old|old.processor==binding.processor&&old.family==binding.family){return Err(Error::InvalidInput);}
            let program=self.programs.get(binding.program).ok_or(Error::InvalidInput)?;
            if program.requires_note||program.requires_controller||program.requires_performance||program.wait_lifetime!=crate::WaitLifetime::Callback{return Err(Error::InvalidInput);}
            if !matches!(program.code.get(program.entry),Some(crate::Instruction::AwaitExternal{processor})if *processor==binding.processor)
                ||!matches!(program.code.get(program.entry+1),Some(crate::Instruction::End)){return Err(Error::InvalidInput);}
        }
        self.external_programs|=!bindings.is_empty();self.source_tasks=bindings.into_boxed_slice();Ok(self)
    }
    /// Append genuine task entry programs without replacing any existing
    /// program, stage, source hierarchy or callback binding. The caller supplies
    /// an explicit new-entry ceiling (not a heap/RSS byte certificate).
    pub fn with_source_task_entries(mut self,entries:Vec<(SourceEventProcessorId,SourceTaskFamily)>,max_new_entries:usize)->Result<Self,Error>{
        self.append_source_task_entries(&entries,max_new_entries)?;Ok(self)
    }
    /// Transactional offaudio counterpart: an invalid roster, exhausted limit
    /// or failed checked construction leaves this Prepared completely intact.
    pub fn append_source_task_entries(&mut self,entries:&[(SourceEventProcessorId,SourceTaskFamily)],max_new_entries:usize)->Result<(),Error>{
        if entries.is_empty(){return Ok(());}
        if entries.len()>max_new_entries{return Err(Error::Capacity);}
        self.validate_source_tasks()?;
        let old_programs=self.programs.len();
        let program_count=old_programs.checked_add(entries.len()).ok_or(Error::Capacity)?;
        // StartProgram's public target domain is u32; appended entry indexes
        // must remain representable, rather than saturating a factory index.
        u32::try_from(program_count.checked_sub(1).ok_or(Error::Capacity)?).map_err(|_|Error::Capacity)?;
        let binding_count=self.source_tasks.len().checked_add(entries.len()).ok_or(Error::Capacity)?;
        let instruction_count=entries.len().checked_mul(2).ok_or(Error::Capacity)?;
        let layout=|size:Result<std::alloc::Layout,std::alloc::LayoutError>|size.map(|layout|layout.size()).map_err(|_|Error::Capacity);
        let program_bytes=layout(std::alloc::Layout::array::<crate::Program>(program_count))?;
        let binding_bytes=layout(std::alloc::Layout::array::<PreparedSourceTaskBinding>(binding_count))?;
        let key_bytes=layout(std::alloc::Layout::array::<(SourceEventProcessorId,u8)>(binding_count))?;
        let code_bytes=layout(std::alloc::Layout::array::<crate::Instruction>(instruction_count))?;
        // Checked roster/payload arithmetic includes the retained old roster
        // and the simultaneously constructed replacement. Old bytecode/text
        // Arcs are shared, not cloned into another mutable program/cache.
        let old_bytes=layout(std::alloc::Layout::array::<crate::Program>(old_programs))?
            .checked_add(layout(std::alloc::Layout::array::<PreparedSourceTaskBinding>(self.source_tasks.len()))?).ok_or(Error::Capacity)?;
        old_bytes.checked_add(program_bytes).and_then(|bytes|bytes.checked_add(binding_bytes))
            .and_then(|bytes|bytes.checked_add(key_bytes)).and_then(|bytes|bytes.checked_add(code_bytes)).ok_or(Error::Capacity)?;
        let family_key=|family:SourceTaskFamily|->u8{match family{SourceTaskFamily::Init=>0,SourceTaskFamily::Transport=>1,SourceTaskFamily::Spawned=>2,SourceTaskFamily::Run=>3}};
        let mut keys=Vec::new();keys.try_reserve_exact(binding_count).map_err(|_|Error::Capacity)?;
        for binding in &self.source_tasks{keys.push((binding.processor,family_key(binding.family)));}
        for &(processor,family)in entries{
            let row=self.source_owners.processor(processor).ok_or(Error::InvalidInput)?;
            if row.kind.as_ref()!="ScriptProcessor"{return Err(Error::InvalidInput);}
            keys.push((processor,family_key(family)));
        }
        layout(std::alloc::Layout::array::<(SourceEventProcessorId,u8)>(keys.capacity()))?;
        keys.sort_unstable();
        if keys.windows(2).any(|pair|pair[0]==pair[1]){return Err(Error::InvalidInput);}
        let mut programs=Vec::new();programs.try_reserve_exact(program_count).map_err(|_|Error::Capacity)?;
        let mut bindings=Vec::new();bindings.try_reserve_exact(binding_count).map_err(|_|Error::Capacity)?;
        layout(std::alloc::Layout::array::<crate::Program>(programs.capacity()))?;
        layout(std::alloc::Layout::array::<PreparedSourceTaskBinding>(bindings.capacity()))?;
        programs.extend(self.programs.iter().cloned());bindings.extend_from_slice(&self.source_tasks);
        for &(processor,family)in entries{
            let index=programs.len();
            let mut code=Vec::new();code.try_reserve_exact(2).map_err(|_|Error::Capacity)?;
            layout(std::alloc::Layout::array::<crate::Instruction>(code.capacity()))?;
            code.extend([crate::Instruction::AwaitExternal{processor},crate::Instruction::End]);
            let program=crate::Program::new(code)?.with_wait_lifetime(crate::WaitLifetime::Callback);
            if program.requires_note||program.requires_controller||program.requires_performance||program.locals!=0{return Err(Error::InvalidInput);}
            programs.push(program);bindings.push(PreparedSourceTaskBinding{processor,family,program:index});
        }
        // Complete every fallible construction before replacing a live field.
        // No with_programs call: that would clear unrelated execution bindings.
        let programs=programs.into_boxed_slice();let bindings=bindings.into_boxed_slice();
        self.programs=programs;self.source_tasks=bindings;self.external_programs=true;
        Ok(())
    }
    pub fn source_tasks(&self)->&[PreparedSourceTaskBinding]{&self.source_tasks}
    pub(crate) fn validate_source_tasks(&self)->Result<(),Error>{
        for (index,binding) in self.source_tasks.iter().enumerate(){
            let row=self.source_owners.processor(binding.processor).ok_or(Error::InvalidInput)?;
            if row.kind.as_ref()!="ScriptProcessor"||self.source_tasks[..index].iter().any(|old|old.processor==binding.processor&&old.family==binding.family){return Err(Error::InvalidInput);}
            let program=self.programs.get(binding.program).ok_or(Error::InvalidInput)?;
            if program.requires_note||program.requires_controller||program.requires_performance||program.wait_lifetime!=crate::WaitLifetime::Callback{return Err(Error::InvalidInput);}
            if !matches!(program.code.get(program.entry),Some(crate::Instruction::AwaitExternal{processor})if *processor==binding.processor)
                ||!matches!(program.code.get(program.entry+1),Some(crate::Instruction::End)){return Err(Error::InvalidInput);}
        }Ok(())
    }
}
impl Runtime {
    fn task_domain(&self,context:PlanContext)->Result<TaskDomain,Error>{
        Ok(match context{
            PlanContext::Task(task)=>task.domain,
            PlanContext::Bare=>return Err(Error::InvalidInput),
            PlanContext::Controller(event)=>TaskDomain::Captured{performance:event.performance,origin:event.origin,source:event.receiver.map_or(event.origin,|capture|capture.source),channels:event.channels,epochs:event.epochs,secondary:event.secondary,receiver:event.receiver,stage:event.stage},
            PlanContext::Program(event)=>TaskDomain::Captured{performance:event.performance,origin:event.origin,source:event.source,channels:event.channels,epochs:event.epochs,secondary:event.secondary,receiver:event.controller_capture(),stage:event.stage},
            PlanContext::Control(event)=>match event.controller_epochs{
                None=>TaskDomain::Global{performance:event.performance},
                Some(epochs)=>TaskDomain::Captured{performance:event.performance,origin:event.origin,source:event.receiver.map_or(event.origin,|capture|capture.source),channels:event.channels,epochs,secondary:event.controller_secondary,receiver:event.receiver,stage:event.stage}},
            PlanContext::NoteControllers(NoteControllerContext{performance,origin,source,epoch,stage,receiver,policy_epochs,host_epochs,..})=>{
                let mut epochs=[0;16];epochs[usize::from(origin.channel)]=epoch;
                TaskDomain::Captured{performance,origin,source,channels:1<<origin.channel,epochs,secondary:None,stage,receiver:receiver.map(|receiver|super::components::CapturedReceiver{receiver,kind:super::components::ReceiverActionKind::Controller,provenance:super::components::ReceiverProvenance::Script,source,policy_epochs,host_epochs})}
            }
        })
    }
    fn validate_task_cost(&self,plan:PlanId,count:usize,bindings:usize)->Result<usize,Error>{
        let lookup=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners.processor_lookup_work_bound()?;
        // Parent validation plus actual AwaitExternal parking each resolve the
        // real table: few executable task bindings do not make unused rows free.
        let per_task=self.behaviors.slots.len().checked_mul(2)
            .and_then(|n|n.checked_add(self.behavior_stride))
            .and_then(|n|n.checked_add(2))
            .and_then(|n|n.checked_add(lookup.checked_mul(2)?)).ok_or(Error::Capacity)?;
        let cost=per_task.checked_mul(count).and_then(|n|n.checked_add(bindings.checked_mul(4)?)).ok_or(Error::Capacity)?;
        if cost>self.behavior_fuel||cost>self.block_fuel_left{return Err(Error::Capacity);}Ok(cost)
    }
    // Execute the actual validated first AwaitExternal entry directly under
    // the task admission work charge. Queuing it behind exhausted ordinary
    // Ready work could park a run parent without ever exposing its child turn.
    // This is the same continuation/PC and same AwaitExternal implementation,
    // not a synthetic token or an unexecuted metadata binding.
    fn park_source_task(&mut self,id:BehaviorId)->Result<(),Error>{
        let callback=*self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let PlanContext::Task(task)=callback.context else{return Err(Error::InvalidInput);};
        let plan=self.behavior_plan(callback.owner)?;
        let program=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.programs.get(callback.program).ok_or(Error::InvalidInput)?;
        if callback.pc!=program.entry||!matches!(program.code.get(callback.pc),Some(crate::Instruction::AwaitExternal{processor})if *processor==task.processor){return Err(Error::InvalidInput);}
        let next=callback.pc.checked_add(1).ok_or(Error::Capacity)?;
        self.behaviors.get_mut(id.0).ok_or(Error::StaleHandle)?.pc=next;
        self.await_external_input(id,task.processor)
    }
    /// Admit all bound entries of one genuine first-F/transport event before
    /// running any prefix. Binding-list order is explicit offaudio processor order.
    pub fn start_source_tasks(&mut self,plan:PlanId,reason:SourceTaskReason,performance:PerformanceId)->Result<usize,Error>{
        if !matches!(reason,SourceTaskReason::Init(_)|SourceTaskReason::Transport(_)){return Err(Error::InvalidInput);}
        reason.validate(self.now)?;let performance=self.performance_index(performance)?;
        if self.external_execution_aborted{return Err(Error::Canceled);}
        self.apply_due();
        let generation=self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        self.validate_task_cost(plan,0,generation.prepared.source_tasks.len())?;
        let count=generation.prepared.source_tasks.iter().enumerate().filter(|(index,binding)|binding.family==reason.family()&&(!matches!(reason,SourceTaskReason::Init(_))||!generation.source_tasks.initialized[*index])).count();
        if count==0{return Ok(0);}
        generation.callbacks.checked_add(count).ok_or(Error::Capacity)?;
        self.next_external_turn_order.checked_add(self.reserved_external_turn_orders).and_then(|order|order.checked_add(count as u64)).ok_or(Error::Capacity)?;
        let cost=self.validate_task_cost(plan,count,generation.prepared.source_tasks.len())?;
        // Every program was validated offaudio. Check callback/locals and all
        // aggregate arena/Ready/ID room before the first task owner is inserted.
        if count>self.source_task_scratch.capacity()||!self.behavior_room(count){return Err(Error::Capacity);}
        let bindings=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_tasks.len();
        for index in 0..bindings{
            let generation=self.plans.get(plan.0).ok_or(Error::StaleHandle)?;let binding=generation.prepared.source_tasks[index];
            if binding.family!=reason.family()||(matches!(reason,SourceTaskReason::Init(_))&&generation.source_tasks.initialized[index]){continue;}
            self.validate_plan_context(plan,binding.program,PlanContext::Task(TaskContext{processor:binding.processor,reason,creator:SourceTaskCreator::None,domain:TaskDomain::Global{performance},original_frame:self.now,run_parent:None}))?;
        }
        self.source_task_scratch.clear();
        for index in 0..bindings{
            let generation=self.plans.get(plan.0).ok_or(Error::StaleHandle)?;let binding=generation.prepared.source_tasks[index];
            if binding.family!=reason.family()||(matches!(reason,SourceTaskReason::Init(_))&&generation.source_tasks.initialized[index]){continue;}
            let context=TaskContext{processor:binding.processor,reason,creator:SourceTaskCreator::None,domain:TaskDomain::Global{performance},original_frame:self.now,run_parent:None};
            match self.admit_plan_context(plan,binding.program,PlanContext::Task(context)){
                Ok(id)=>self.source_task_scratch.push(id),
                Err(error)=>{
                    for at in 0..self.source_task_scratch.len(){let id=self.source_task_scratch[at];self.behaviors.remove(id.0);self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?.callbacks-=1;}
                    self.source_task_scratch.clear();return Err(error);
                }
            }
        }
        if matches!(reason,SourceTaskReason::Init(_)){
            let generation=self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
            for (index,binding) in generation.prepared.source_tasks.iter().enumerate(){if binding.family==reason.family(){generation.source_tasks.initialized[index]=true;}}
        }
        self.block_fuel_left-=cost;
        for index in 0..self.source_task_scratch.len(){self.park_source_task(self.source_task_scratch[index])?;}
        self.source_task_scratch.clear();Ok(count)
    }
    /// Compatibility entry for a genuine main/restore residual with no creator.
    pub fn start_prepared_source_task(
        &mut self,plan:PlanId,processor:SourceEventProcessorId,
        family:SourceTaskFamily,performance:PerformanceId,
    )->Result<BehaviorId,Error>{
        self.start_prepared_global_source_task(plan,processor,family,performance,PreparedGlobalTaskOrigin::None)
    }
    /// One prepared global residual. A restored Widget child is manual/nonmusical;
    /// it never receives a fabricated physical key, expression or capture epoch.
    pub fn start_prepared_global_source_task(
        &mut self,plan:PlanId,processor:SourceEventProcessorId,family:SourceTaskFamily,
        performance:PerformanceId,origin:PreparedGlobalTaskOrigin,
    )->Result<BehaviorId,Error>{
        let mut output=[None];
        self.start_prepared_global_source_tasks(plan,performance,
            &[PreparedGlobalSourceTask{processor,family,origin}],&mut output)?;
        output[0].ok_or(Error::InvalidInput)
    }
    /// Admit a complete actual prepared-root batch before any root can run.
    /// Caller storage is prepared offaudio. On admission refusal it is unchanged;
    /// retained VM rows must not move until this actual BehaviorId batch succeeds.
    pub fn start_prepared_global_source_tasks(
        &mut self,plan:PlanId,performance:PerformanceId,
        requests:&[PreparedGlobalSourceTask],outputs:&mut[Option<BehaviorId>],
    )->Result<usize,Error>{
        let count=requests.len();
        if outputs.len()<count||outputs[..count].iter().any(Option::is_some){return Err(Error::InvalidInput);}
        for request in requests{Self::prepared_root_reason(request.family)?;}
        let performance=self.performance_index(performance)?;
        if self.external_execution_aborted{return Err(Error::Canceled);}
        self.apply_due();
        let generation=self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        if count==0{return Ok(0);}
        let bindings=generation.prepared.source_tasks.len();
        // Every row performs real binding lookup in both validation/admission.
        // Charge that product, not a single scan hidden behind batch length.
        let scans=bindings.checked_mul(count).ok_or(Error::Capacity)?;
        let cost=self.validate_task_cost(plan,count,scans)?;
        generation.callbacks.checked_add(count).ok_or(Error::Capacity)?;
        let count64=u64::try_from(count).map_err(|_|Error::Capacity)?;
        self.next_external_turn_order.checked_add(self.reserved_external_turn_orders)
            .and_then(|order|order.checked_add(count64)).ok_or(Error::Capacity)?;
        if count>self.source_task_scratch.capacity(){return Err(Error::Capacity);}
        // Schema of ALL real entries is checked before behavior_room can reclaim
        // a finished slot and before the first new Plan callback is inserted.
        for request in requests{
            let binding=generation.prepared.source_tasks.iter()
                .find(|binding|binding.processor==request.processor&&binding.family==request.family)
                .ok_or(Error::InvalidInput)?;
            let program=generation.prepared.programs.get(binding.program).ok_or(Error::InvalidInput)?;
            if program.requires_note||program.requires_controller||program.requires_performance
                ||program.wait_lifetime!=crate::WaitLifetime::Callback
                ||!matches!(program.code.get(program.entry),Some(crate::Instruction::AwaitExternal{processor})if *processor==request.processor)
                ||!matches!(program.code.get(program.entry+1),Some(crate::Instruction::End)){
                return Err(Error::InvalidInput);
            }
        }
        if !self.behavior_room(count){return Err(Error::Capacity);}
        self.source_task_scratch.clear();
        for request in requests{
            let binding=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_tasks.iter()
                .find(|binding|binding.processor==request.processor&&binding.family==request.family)
                .copied().ok_or(Error::InvalidInput)?;
            let context=TaskContext{processor:request.processor,reason:Self::prepared_root_reason(request.family)?,
                creator:request.origin.creator(),domain:TaskDomain::Global{performance},
                original_frame:self.now,run_parent:None};
            match self.admit_plan_context(plan,binding.program,PlanContext::Task(context)){
                Ok(id)=>self.source_task_scratch.push(id),
                Err(error)=>{
                    // No authored prefix or external entry has run yet.
                    for at in 0..self.source_task_scratch.len(){
                        self.behaviors.remove(self.source_task_scratch[at].0);
                        self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?.callbacks-=1;
                    }
                    self.source_task_scratch.clear();return Err(error);
                }
            }
        }
        self.block_fuel_left-=cost;
        // Actual first AwaitExternal has a validated shape and aggregate order
        // credit. It parks the SAME continuations without executing a Lua prefix.
        for index in 0..count{self.park_source_task(self.source_task_scratch[index])?;}
        for (slot,id)in outputs[..count].iter_mut().zip(&self.source_task_scratch){*slot=Some(*id);}
        self.source_task_scratch.clear();Ok(count)
    }
    fn prepared_root_reason(family:SourceTaskFamily)->Result<SourceTaskReason,Error>{match family{
        SourceTaskFamily::Spawned=>Ok(SourceTaskReason::Spawned),
        SourceTaskFamily::Run=>Ok(SourceTaskReason::Run),
        _=>Err(Error::InvalidInput),
    }}
    /// Spawn/run is a real independent Plan continuation. It never copies a
    /// pending forwarding reservation or gives a task its caller's note pin.
    pub fn spawn_source_task(&mut self,parent:ExternalInputId,immediate:bool)->Result<BehaviorId,Error>{
        let callback=self.behaviors.get(parent.behavior().0).copied().ok_or(Error::StaleHandle)?;
        let plan=self.behavior_plan(callback.owner)?;
        let bindings=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_tasks.len();
        // Price the actual potentially long processor lookup before doing it.
        // The complete Running/sequence/source check below is still authority.
        let cost=self.validate_task_cost(plan,1,bindings)?;
        let (callback,state)=self.validate_external_segment(parent,Phase::Running)?;
        let reason=if immediate{SourceTaskReason::Run}else{SourceTaskReason::Spawned};
        let binding=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_tasks.iter().find(|binding|binding.processor==state.processor&&binding.family==reason.family()).copied().ok_or(Error::InvalidInput)?;
        let creator=match callback.context{PlanContext::Task(task)=>task.creator,PlanContext::Bare=>SourceTaskCreator::None,_=>SourceTaskCreator::Event};
        let domain=self.task_domain(callback.context)?;
        let run_parent=if immediate{Some(RunParent{input:parent,resume_sequence:state.sequence.checked_add(1).ok_or(Error::Capacity)?,priority:state.priority.checked_add(1).ok_or(Error::Capacity)?})}else{None};
        self.next_external_turn_order.checked_add(self.reserved_external_turn_orders).and_then(|order|order.checked_add(if immediate{2}else{1})).ok_or(Error::Capacity)?;
        let child=self.admit_plan_context(plan,binding.program,PlanContext::Task(TaskContext{processor:binding.processor,reason,creator,domain,original_frame:self.now,run_parent}))?;
        self.block_fuel_left-=cost;
        if run_parent.is_some(){
            self.reserved_external_turn_orders+=1;
            self.behaviors.get_mut(parent.behavior().0).ok_or(Error::StaleHandle)?.external.as_mut().ok_or(Error::StaleHandle)?.phase=Phase::WaitingRunChild;
        }
        self.park_source_task(child)?;
        if let Some(link)=run_parent&&let Some(state)=self.behaviors.get_mut(child.0).and_then(|callback|callback.external.as_mut()){state.priority=link.priority;}
        Ok(child)
    }
    /// Called at a task's first yield/end/cancel; no recursive execution. It
    /// releases one actual run edge and makes the parent eligible at the SAME F.
    pub(crate) fn release_task_run_parent(&mut self,child:BehaviorId)->Result<(),Error>{
        let callback=self.behaviors.get(child.0).ok_or(Error::StaleHandle)?;
        let PlanContext::Task(task)=callback.context else{return Ok(());};
        let Some(link)=task.run_parent else{return Ok(());};
        if let PlanContext::Task(task)=&mut self.behaviors.get_mut(child.0).ok_or(Error::StaleHandle)?.context{task.run_parent=None;}
        self.reserved_external_turn_orders=self.reserved_external_turn_orders.checked_sub(1).ok_or(Error::InvalidInput)?;
        let Some(parent)=self.behaviors.get_mut(link.input.behavior().0) else{return Ok(());};
        if parent.outcome.is_some(){return Ok(());}
        let Some(state)=parent.external.as_mut()else{return Ok(());};
        if state.sequence!=link.input.sequence()||state.phase!=Phase::WaitingRunChild{return Ok(());}
        let order=self.next_external_turn_order.checked_add(1).ok_or(Error::Capacity)?;
        self.next_external_turn_order=order;
        state.phase=Phase::Pending;state.sequence=link.resume_sequence;state.order=order;state.priority=link.priority;state.resume_frame=self.now;
        parent.external_sequence=link.resume_sequence;Ok(())
    }
}
