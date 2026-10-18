//! The documented ScriptProcessor CC visibility filters. This is the Lua
//! owner's last-received view, not a second protocol receiver or engine bank.
use std::cell::Cell;

#[derive(Clone, Copy)]
pub(super) enum ControllerFamily { Rpn, Nrpn, DataEntry }

#[derive(Default)]
pub(super) struct ControllerVisibility {
    rpn: Cell<bool>,
    nrpn: Cell<bool>,
    data_entry: Cell<bool>,
}

impl ControllerVisibility {
    pub(super) fn set(&self, family: ControllerFamily, enabled: bool) {
        match family {
            ControllerFamily::Rpn => self.rpn.set(enabled),
            ControllerFamily::Nrpn => self.nrpn.set(enabled),
            ControllerFamily::DataEntry => self.data_entry.set(enabled),
        }
    }

    pub(super) fn receives(&self, controller: u8) -> bool {
        match controller {
            100 | 101 => self.rpn.get(),
            98 | 99 => self.nrpn.get(),
            6 | 38 => self.data_entry.get(),
            _ => true,
        }
    }
}


use mlua::{Lua, RegistryKey, Table, Value};
use sampler_ir::{SourceEventProcessorId, SourceOwnerId};
use std::{cell::RefCell, collections::{BTreeMap, HashSet}, rc::Rc};

/// Last-received *script API* values are local to this actual processor. They
/// are not canonical Core MIDI banks and never project raw wire state twice.
pub(super) struct ProcessorView {
    pub(super) assigned: RefCell<HashSet<String>>,
    pub(super) called: RefCell<HashSet<String>>,
    pub(super) saved: RefCell<BTreeMap<String,String>>,
    pub(super) cc: RefCell<[u8;128]>,
    pub(super) visibility: ControllerVisibility,
}
impl Default for ProcessorView {
    fn default()->Self { Self { assigned:RefCell::new(Default::default()), called:RefCell::new(Default::default()), saved:RefCell::new(Default::default()), cc:RefCell::new([0;128]), visibility:Default::default() } }
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ProcessorAddress {
    pub processor: SourceEventProcessorId,
    pub owner: SourceOwnerId,
    /// Original position among *all* direct EventProcessors children.
    pub ordinal:u32,
}

/// Typed owner-delivery result. Bypassed means the canonical external stage
/// must forward its original Core event, not synthesize a seven-bit copy. This
/// receipt is not the Core ACK and makes no claim about modulation signal state.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ProcessorInputDisposition {CallbackDispatched,Bypassed}

pub(super) struct ProcessorContext {
    pub(super) address:ProcessorAddress,
    pub(super) source_node:u32,
    pub(super) environment:RegistryKey,
    pub(super) view:Rc<ProcessorView>,
    pub(super) program_level:bool,
}

pub(super) const MAX_PROCESSORS:usize=1024;

/// Copy mutable standard-library tables before any authored code runs. Their
/// functions remain the VM's actual builtins: in particular math.random uses
/// the existing VM RNG, with no invented per-processor RNG parity claim.
pub(super) fn fresh_environment(lua:&Lua, pristine:&Table)->mlua::Result<Table> {
    let environment=lua.create_table()?;
    for pair in pristine.clone().pairs::<Value,Value>() {
        let (key,value)=pair?;
        if matches!(&key,Value::String(name) if name.as_bytes().as_ref()==b"_G") {continue;}
        let value=if let Value::Table(table)=value {
            let copy=lua.create_table()?;
            for entry in table.pairs::<Value,Value>() {let (key,value)=entry?;copy.raw_set(key,value)?;}
            if let Some(mt)=table.metatable(){copy.set_metatable(Some(mt))?;}
            Value::Table(copy)
        } else {value};
        environment.raw_set(key,value)?;
    }
    environment.raw_set("_G",environment.clone())?;
    Ok(environment)
}

pub(super) struct ContextGuard<'a> {
    lua:&'a Lua,
    shared:&'a super::Shared,
    previous:Option<usize>,
    view:Rc<ProcessorView>,
    environment:Table,
    restored:Cell<bool>,
}
impl ContextGuard<'_> {
    /// Restoration is part of the actual segment, before Core Complete ACK.
    /// A failed environment restoration cannot authorize a guessed context.
    pub(super) fn restore(&self)->mlua::Result<()> {
        if self.restored.replace(true){return Ok(());}
        if let Err(error)=self.lua.set_globals(self.environment.clone()){
            self.shared.context_failed.set(true);
            if let Some(owner)=self.shared.authority.borrow().as_ref(){owner.cancelled.store(true,Ordering::Release);}
            self.shared.find("ScriptProcessor context restoration failed","");
            return Err(error);
        }
        self.shared.processor.set(self.previous);
        *self.shared.processor_view.borrow_mut()=self.view.clone();Ok(())
    }
}
impl Drop for ContextGuard<'_> {
    fn drop(&mut self) {
        // The error path also restores before returning to any subsequent
        // callback. restore records failure and cancels the actual endpoint.
        let _=self.restore();
    }
}

pub(super) fn enter<'a>(lua:&'a Lua,shared:&'a super::Shared,index:Option<usize>)->mlua::Result<ContextGuard<'a>> {
    if shared.context_failed.get(){return Err(mlua::Error::runtime("ScriptProcessor context is invalid after restoration failure"));}
    let environment=lua.globals();
    let previous=shared.processor.get();
    let view=shared.processor_view.borrow().clone();
    if let Some(index)=index {
        let contexts=shared.processors.borrow();
        let context=contexts.get(index).ok_or_else(||mlua::Error::runtime("stale ScriptProcessor context"))?;
        let next:Table=lua.registry_value(&context.environment)?;
        lua.set_globals(next)?;
        *shared.processor_view.borrow_mut()=context.view.clone();
    }
    shared.processor.set(index);
    Ok(ContextGuard {lua,shared,previous,view,environment,restored:Cell::new(false)})
}


pub(super) struct EventShape {
    pub(super) id:SourceEventProcessorId,
    pub(super) ordinal:u32,
    pub(super) kind:String,
    pub(super) bypass:bool,
}
pub(super) struct OwnerShape {
    pub(super) id:SourceOwnerId,
    pub(super) parent:Option<SourceOwnerId>,
    pub(super) kind:sampler_ir::SourceOwnerKind,
    pub(super) events:Vec<EventShape>,
}

// Actual per-owner Core/Lua authority transport. Creating/binding these endpoints
// is offaudio; only the existing ScriptThread Lua owner may rendezvous. The host
// audio port never executes these methods or borrows a Runtime.
use std::sync::{Arc,atomic::{AtomicBool,Ordering}};
use std::time::{Duration,Instant};
use sampler_core::{ExternalInputId,ExternalOutput,ExternalOutputResult,ExternalTurn,PlanId,RuntimeId};
const AUTHORITY_QUEUE:usize=256;
const AUTHORITY_OPERATIONS:u32=65_536;

#[derive(Debug)]
pub enum AuthorityOperation {
    /// Pure real Running validation, including owner-only changes and no-ops.
    Validate,
    QueryConstant{node:u32},
    Post(ExternalOutput),
    Command(super::Command),
    Complete{next_frame:Option<u64>},
    Cancel,
}
#[derive(Debug)]
pub struct AuthorityRequest {pub turn:ExternalTurn,pub ordinal:u32,pub operation:AuthorityOperation}
#[derive(Clone,Copy,Debug)]
pub enum AuthorityValue {Validated,Posted(ExternalOutputResult),Applied,Completed,Constant(sampler_core::SourceConstantSnapshot)}
#[derive(Debug)]
pub struct AuthorityReply {
    pub input:ExternalInputId,pub ordinal:u32,
    pub result:Result<AuthorityValue,sampler_core::Error>,
    /// The SAME owned command returns to its real Lua owner after borrowing it
    /// for application. In particular an impulse upload never loses its last
    /// reference on the Core execution path, on success or refusal.
    pub retired_command:Option<super::Command>,
}

/// One actual Core actor's endpoint. A reservation is checked BEFORE applying
/// an operation, then its exact result is returned once; queueing is not ACK.
pub struct CoreAuthorityPort {
    runtime:RuntimeId,plan:PlanId,requests:rtrb::Consumer<AuthorityRequest>,
    replies:rtrb::Producer<AuthorityReply>,cancelled:Arc<AtomicBool>,
}
impl CoreAuthorityPort {
    pub fn runtime(&self)->RuntimeId{self.runtime}
    pub fn plan(&self)->PlanId{self.plan}
    pub fn cancelled(&self)->bool{self.cancelled.load(Ordering::Acquire)}
    pub fn reply_slots(&self)->usize{self.replies.slots()}
    pub fn pop(&mut self)->Option<AuthorityRequest>{self.requests.pop().ok()}
    pub fn reply(&mut self,reply:AuthorityReply)->Result<(),AuthorityReply>{self.replies.push(reply).map_err(|rtrb::PushError::Full(reply)|reply)}
    pub fn cancel(&self){self.cancelled.store(true,Ordering::Release);}
}
impl Drop for CoreAuthorityPort {fn drop(&mut self){self.cancelled.store(true,Ordering::Release);}}

pub(crate) struct OwnerAuthorityPort {
    pub(super) runtime:RuntimeId,pub(super) plan:PlanId,
    pub(super) proof:Option<Vec<OwnerShape>>,
    requests:rtrb::Producer<AuthorityRequest>,replies:rtrb::Consumer<AuthorityReply>,
    cancelled:Arc<AtomicBool>,timeout:Duration,owner:Option<std::thread::ThreadId>,
    current:Option<ExternalTurn>,ordinal:u32,operations:u32,
}
impl OwnerAuthorityPort {
    pub(super) fn bind_current_thread(&mut self){self.owner=Some(std::thread::current().id());}
    pub(super) fn begin(&mut self,turn:ExternalTurn)->Result<(),sampler_core::Error>{
        if self.cancelled.load(Ordering::Acquire){return Err(sampler_core::Error::Canceled);}
        if self.owner!=Some(std::thread::current().id())||turn.plan!=self.plan{return Err(sampler_core::Error::InvalidInput);}
        if self.current.is_some(){return Err(sampler_core::Error::Busy);}
        self.current=Some(turn);self.ordinal=0;self.operations=0;Ok(())
    }
    pub(super) fn current(&self)->Option<ExternalTurn>{self.current}
    pub(super) fn cancelled(&self)->bool{self.cancelled.load(Ordering::Acquire)}
    /// The Core owner already canceled the actual continuations at the reset
    /// barrier. This runs only on the Lua owner after its ordered reset marker;
    /// it grants no new token and never re-enables an aborted endpoint.
    pub(super) fn reset_barrier(&mut self,runtime:RuntimeId,plan:PlanId)->Result<(),sampler_core::Error>{
        use sampler_core::Error;
        if self.owner!=Some(std::thread::current().id())||self.runtime!=runtime||self.plan!=plan{return Err(Error::StaleHandle);}
        if self.cancelled.load(Ordering::Acquire){return Err(Error::Canceled);}
        if self.current.is_some(){return Err(Error::Busy);}
        // No call may remain in flight when the sole Core actor reached H.
        if self.replies.slots()!=0{return Err(Error::Busy);}
        self.ordinal=0;self.operations=0;Ok(())
    }
    pub(super) fn call(&mut self,operation:AuthorityOperation)->Result<AuthorityValue,sampler_core::Error>{
        use sampler_core::Error;
        if self.owner!=Some(std::thread::current().id()){return Err(Error::InvalidInput);}
        if self.cancelled.load(Ordering::Acquire){return Err(Error::Canceled);}
        let turn=self.current.ok_or(Error::InvalidInput)?;
        let terminal=matches!(operation,AuthorityOperation::Complete{..}|AuthorityOperation::Cancel);
        if !terminal&&self.operations==AUTHORITY_OPERATIONS{return Err(Error::Capacity);}
        let ordinal=self.ordinal;self.ordinal=self.ordinal.checked_add(1).ok_or(Error::Capacity)?;
        if !terminal{self.operations+=1;}
        let deadline=Instant::now().checked_add(self.timeout).ok_or(Error::InvalidInput)?;
        self.requests.push(AuthorityRequest{turn,ordinal,operation}).map_err(|_|Error::Capacity)?;
        loop {
            if self.cancelled.load(Ordering::Acquire){return Err(Error::Canceled);}
            if let Ok(reply)=self.replies.pop(){
                if reply.input!=turn.input||reply.ordinal!=ordinal{self.cancelled.store(true,Ordering::Release);return Err(Error::InvalidInput);}
                if terminal&&(reply.result.is_ok()||matches!(&reply.result,Err(Error::Canceled|Error::StaleHandle))){
                    // A real terminal API refusal for this exact input/ordinal
                    // establishes that this segment no longer owns a live turn.
                    // A stale nonterminal destination never does so.
                    self.current=None;
                }
                return reply.result;
            }
            if Instant::now()>=deadline{self.cancelled.store(true,Ordering::Release);return Err(Error::Busy);}
            // Explicitly the existing non-RT Lua owner, never the host or Core.
            std::thread::sleep(Duration::from_micros(50));
        }
    }
}

/// Pure metadata proof is copied transiently from ONE actual plan table for the
/// VM's existing shape comparison, then discarded after owner binding. It is not
/// an executable second hierarchy or a mutable control/bypass-value cache.
pub(crate) fn authority_pair(runtime:&sampler_core::Runtime,plan:PlanId,timeout:Duration)->Result<(CoreAuthorityPort,OwnerAuthorityPort),sampler_core::Error>{
    use sampler_core::Error;
    if timeout.is_zero()||timeout>Duration::from_secs(5){return Err(Error::InvalidInput);}
    let table=runtime.source_owners(plan)?;
    let mut proof=Vec::new();let mut processors=0usize;
    for owner in table.rows().iter().filter(|owner|owner.event_processors.iter().any(|row|row.kind.as_ref()=="ScriptProcessor")) {
        processors=processors.checked_add(owner.event_processors.iter().filter(|row|row.kind.as_ref()=="ScriptProcessor").count()).ok_or(Error::Capacity)?;
        if processors>MAX_PROCESSORS{return Err(Error::Capacity);}
        for processor in owner.event_processors.iter().filter(|row|row.kind.as_ref()=="ScriptProcessor"){
            use sampler_core::{SourceEventFamily as Family,SourceTaskFamily as Task};
            let mut executable=false;
            for family in [Family::Note,Family::Release,Family::Controller,Family::PolyPressure,Family::ProgramChange,
                Family::Task(Task::Init),Family::Task(Task::Transport),Family::Task(Task::Spawned),Family::Task(Task::Run)]{
                executable|=runtime.executable_source_binding(plan,processor.id,family)?.is_some();
            }
            if !executable{return Err(Error::InvalidInput);}
        }
        let parent=owner.parent.map(|parent|table.get(parent).map(|row|row.id).ok_or(Error::InvalidInput)).transpose()?;
        let events=owner.event_processors.iter().map(|row|EventShape{id:row.id,ordinal:row.ordinal,kind:row.kind.to_string(),bypass:row.bypass.saved}).collect();
        proof.push(OwnerShape{id:owner.id,parent,kind:owner.kind,events});
    }
    std::alloc::Layout::array::<AuthorityRequest>(AUTHORITY_QUEUE).map_err(|_|Error::Capacity)?;
    std::alloc::Layout::array::<AuthorityReply>(AUTHORITY_QUEUE).map_err(|_|Error::Capacity)?;
    let (requests,core_requests)=rtrb::RingBuffer::new(AUTHORITY_QUEUE);
    let (core_replies,replies)=rtrb::RingBuffer::new(AUTHORITY_QUEUE);
    let cancelled=Arc::new(AtomicBool::new(false));
    Ok((CoreAuthorityPort{runtime:runtime.id(),plan,requests:core_requests,replies:core_replies,cancelled:cancelled.clone()},
        OwnerAuthorityPort{runtime:runtime.id(),plan,proof:Some(proof),requests,replies,cancelled,timeout,owner:None,current:None,ordinal:0,operations:0}))
}

impl Drop for OwnerAuthorityPort {fn drop(&mut self){self.cancelled.store(true,Ordering::Release);}}


/// A Lua compatibility projection retained only for the lifetime of one real
/// captured continuation. Identity plus unchanged numeric API fields permits an
/// exact Core Original post; a reconstructed/edited table is an authored value.
pub(super) struct ControllerProjection {
    pub(super) table:Table,
    pub(super) fields:Vec<(&'static str,f64)>,
    pub(super) handler:&'static str,
    pub(super) poly_key:Option<u8>,
    pub(super) program_bank:Option<sampler_core::ProgramBankIntent>,
}
impl ControllerProjection {
    pub(super) fn new(lua:&Lua,source:sampler_core::ChannelAddress,number:u8,value:u32,poly_key:Option<u8>)->mlua::Result<Self>{
        Self::new_with_channel(lua,source,source.channel,number,value,poly_key)
    }
    pub(super) fn new_with_channel(lua:&Lua,source:sampler_core::ChannelAddress,channel:u8,number:u8,value:u32,poly_key:Option<u8>)->mlua::Result<Self>{
        if channel>=16{return Err(mlua::Error::runtime("invalid logical script channel"));}
        let (kind,handler)=match (number,poly_key){
            (0..=127,None)=>(3,"onController"),(128,None)=>(4,"onPitchBend"),
            (129,None)=>(5,"onAfterTouch"),(129,Some(_))=>(6,"onPolyAfterTouch"),
            _=>return Err(mlua::Error::runtime("unsupported captured controller kind")),
        };
        let mut fields=vec![("type",f64::from(kind)),("channel",f64::from(channel)+1.)];
        // The native script API names only four MIDI inputs. A larger physical
        // port is preserved by Original, never truncated into a made-up input.
        if source.port<4{fields.push(("input",f64::from(source.port)+1.));}
        if number<128{fields.push(("controller",f64::from(number)));}
        if let Some(key)=poly_key{fields.push(("note",f64::from(key)));}
        if number==128{
            let delta=f64::from(value)-2147483648.;
            let bend=delta/if delta<0.{2147483648.}else{2147483647.};
            fields.push(("bend",bend));fields.push(("value",bend));
        }else{fields.push(("value",f64::from(value>>25)));}
        let table=lua.create_table()?;
        for &(name,value) in &fields{table.raw_set(name,value)?;}
        Ok(Self{table,fields,handler,poly_key,program_bank:None})
    }
    pub(super) fn program(lua:&Lua,source:sampler_core::ChannelAddress,channel:u8,program:u8,bank:sampler_core::ProgramBankIntent)->mlua::Result<Self>{
        if channel>=16||program>=128{return Err(mlua::Error::runtime("invalid captured program/channel"));}
        let mut fields=vec![("type",7.),("channel",f64::from(channel)+1.),("value",f64::from(program)),("program",f64::from(program))];
        if source.port<4{fields.push(("input",f64::from(source.port)+1.));}
        let table=lua.create_table()?;for &(name,value) in &fields{table.raw_set(name,value)?;}
        Ok(Self{table,fields,handler:"onProgramChange",poly_key:None,program_bank:Some(bank)})
    }
    fn bank_fields_absent(&self,table:&Table)->mlua::Result<bool>{
        // Captured Program has at most these five public scalar fields.
        // Reject unknown/copied bank payloads within six raw iterations, even
        // when the authored table is large; no native schema is invented.
        for (index,pair) in table.clone().pairs::<Value,Value>().enumerate(){
            if index>=5{return Ok(false);}
            let (name,_)=pair?;
            let Value::String(name)=name else{return Ok(false);};
            if !matches!(name.to_str()?.as_ref(),"type"|"channel"|"value"|"program"|"input"){return Ok(false);}
        }Ok(true)
    }
    fn original(&self,table:&Table)->mlua::Result<bool>{
        if self.program_bank.is_some()&&!self.bank_fields_absent(table)?{return Ok(false);}
        if table.to_pointer()!=self.table.to_pointer(){return Ok(false);}
        for &(name,value) in &self.fields{
            if super::number(&table.raw_get::<Value>(name)?)!=Some(value){return Ok(false);}
        }
        // These routing/dispatch fields were not present in this controller
        // projection; adding one is an authored change even on the same table.
        for name in ["layer","oscIndex","slice","dim1","dim2"]{
            if !table.raw_get::<Value>(name)?.is_nil(){return Ok(false);}
        }
        if !self.fields.iter().any(|(name,_)|*name=="input")&&!table.raw_get::<Value>("input")?.is_nil(){return Ok(false);}
        Ok(true)
    }
    pub(super) fn output(&self,table:&Table)->mlua::Result<ExternalOutput>{
        if self.program_bank.is_some()&&!self.bank_fields_absent(table)?{return Err(mlua::Error::runtime("unknown Lua Program fields/bank serialization"));}
        if self.original(table)?{return Ok(ExternalOutput::Original);}
        if !self.fields.iter().any(|(name,_)|*name=="input"){return Err(mlua::Error::runtime("authored event cannot represent this physical input in the legacy four-input API"));}
        let expected_input=self.fields.iter().find_map(|(name,value)|(*name=="input").then_some(*value));
        if super::number(&table.raw_get::<Value>("input")?)!=expected_input{return Err(mlua::Error::runtime("authored logical input needs an admitted owner route"));}
        let expected_channel=self.fields.iter().find_map(|(name,value)|(*name=="channel").then_some(*value)).ok_or_else(||mlua::Error::runtime("missing captured logical channel"))?;
        let channel=super::number(&table.raw_get::<Value>("channel")?).ok_or_else(||mlua::Error::runtime("postEvent channel must be numeric"))?;
        if !channel.is_finite()||channel.fract()!=0.||!(1. ..=16.).contains(&channel){return Err(mlua::Error::runtime("postEvent channel outside 1..16"));}
        let rerouted=channel!=expected_channel;
        let channels=1u16<<((channel as u8)-1);
        for name in ["layer","oscIndex","slice","dim1","dim2"]{if !table.raw_get::<Value>(name)?.is_nil(){return Err(mlua::Error::runtime("authored controller target needs typed canonical route"));}}
        let kind=super::number(&table.raw_get::<Value>("type")?).ok_or_else(||mlua::Error::runtime("postEvent type must be numeric"))?;
        let integer=|name:&str,max:u8|->mlua::Result<u8>{
            let n=super::number(&table.raw_get::<Value>(name)?).ok_or_else(||mlua::Error::runtime("postEvent value must be numeric"))?;
            if !n.is_finite()||n.fract()!=0.||n<0.||n>f64::from(max){return Err(mlua::Error::runtime("postEvent integer outside native API range"));}
            Ok(n as u8)
        };
        if kind==7.{
            // Explicit product preservation rule: only edits of the SAME
            // captured table retain its opaque typed bank intent. This is not
            // a native Lua bank serialization schema or pending-bank default.
            let bank=self.program_bank.ok_or_else(||mlua::Error::runtime("new Lua Program bank serialization is unknown"))?;
            if table.to_pointer()!=self.table.to_pointer()||rerouted||!self.bank_fields_absent(table)?{return Err(mlua::Error::runtime("unknown authored Program bank/route serialization"));}
            let before=self.fields.iter().find_map(|(name,value)|(*name=="program").then_some(*value)).ok_or_else(||mlua::Error::runtime("missing captured program"))?;
            let program=integer("program",127)?;let value=integer("value",127)?;
            let chosen=match (f64::from(program)!=before,f64::from(value)!=before){
                (true,true)if program!=value=>return Err(mlua::Error::runtime("conflicting Program value aliases")),
                (true,_)=>program,(_,true)=>value,_=>program,
            };
            return Ok(ExternalOutput::ProgramChange{program:chosen,bank,target:sampler_core::ExternalTarget::Captured});
        }
        if kind==6.{
            let key=integer("note",127)?;let value=sampler_core::ReceiverValue::Bits7(integer("value",127)?).full_scale();
            return Ok(if !rerouted&&self.poly_key==Some(key){ExternalOutput::PolyPressure{value}}else{ExternalOutput::RoutedPolyPressure{key,value,channels}});
        }
        if kind==3.{
            let number=integer("controller",127)?;let value=integer("value",127)?;
            // This is the actual authored integer. Never reconstruct its
            // precision from Core's normalized bank payload or equal values.
            return Ok(if rerouted{ExternalOutput::RoutedController7{number,value,channels}}else{ExternalOutput::Controller7{number,value}});
        }
        let (number,value)=if kind==4.{
            let before=self.fields.iter().find_map(|(name,value)|(*name=="bend").then_some(*value));
            let bend=super::number(&table.raw_get::<Value>("bend")?);
            let value=super::number(&table.raw_get::<Value>("value")?);
            let n=if bend!=before{bend}else{value.or(bend)}.ok_or_else(||mlua::Error::runtime("postEvent bend missing"))?;
            if !n.is_finite()||!(-1. ..=1.).contains(&n){return Err(mlua::Error::runtime("postEvent bend outside -1..1"));}
            let full=(2147483648.+n*if n<0.{2147483648.}else{2147483647.}).round();
            (128,full as u32)
        }else if kind==5.{(129,sampler_core::ReceiverValue::Bits7(integer("value",127)?).full_scale())}
        else{return Err(mlua::Error::runtime("authored poly-pressure/program/note requires typed canonical action"));};
        Ok(if rerouted{ExternalOutput::RoutedController{number,value,channels}}else{ExternalOutput::Controller{number,value}})
    }
}

/// Actual nonmusical VM provenance, retained on the genuine coroutine before
/// independent creator normalization. The Table is the SAME canonical widget,
/// never a fabricated Core identity or a source/controller capture.
#[derive(Clone)]
pub(super) enum PreparedLuaOrigin {None,RestoredWidget(mlua::Table),UnclassifiedEvent}
impl PreparedLuaOrigin {
    pub(super) fn core_origin(&self)->Result<sampler_core::PreparedGlobalTaskOrigin,String>{match self{
        Self::None=>Ok(sampler_core::PreparedGlobalTaskOrigin::None),
        Self::RestoredWidget(_)=>Ok(sampler_core::PreparedGlobalTaskOrigin::RestoredWidgetEvent),
        Self::UnclassifiedEvent=>Err("prepared musical/unclassified Event task needs its genuine captured creator".into()),
    }}
}

/// Preparation-only address of one retained real Lua coroutine. This row is
/// not event authority: execution is joined to the actual returned BehaviorId.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PreparedRootTask {
    pub row:usize,
    pub processor:SourceEventProcessorId,
    pub family:sampler_core::SourceTaskFamily,
    pub origin:sampler_core::PreparedGlobalTaskOrigin,
}
#[derive(Clone,Copy,Debug)]
pub(crate) struct PreparedRootBinding {pub row:usize,pub behavior:sampler_core::BehaviorId}
pub(super) struct PreparedLuaRoot {
    pub(super) origin:PreparedLuaOrigin,
    pub(super) descriptor:PreparedRootTask,
    pub(super) behavior:Option<sampler_core::BehaviorId>,
    pub(super) thread:mlua::Thread,
    pub(super) args:mlua::MultiValue,
    /// Only an already executed run prefix has a residual rendered wait.
    pub(super) delay:Option<u64>,
    pub(super) remaining_work:u64,
    pub(super) exhausted:bool,
}

pub(super) struct ExternalWaiting {
    pub(super) origin:PreparedLuaOrigin,
    pub(super) behavior:sampler_core::BehaviorId,
    pub(super) processor:SourceEventProcessorId,
    pub(super) thread:mlua::Thread,
    pub(super) projection:Option<Rc<ControllerProjection>>,
    pub(super) creator:super::PlayCreator,
    pub(super) wall:Duration,
    pub(super) remaining_work:u64,
    pub(super) exhausted:bool,
}


#[cfg(test)]
mod projection_tests{
    use super::*;
    fn source()->sampler_core::ChannelAddress{sampler_core::ChannelAddress{protocol:sampler_core::Protocol::Midi2,port:1,group:3,channel:7}}
    #[test]
    fn unchanged_identity_keeps_original_but_authored_copy_has_explicit_legacy_precision(){
        let lua=Lua::new();let view=ControllerProjection::new(&lua,source(),74,0x9123_4567,None).unwrap();
        assert!(matches!(view.output(&view.table).unwrap(),ExternalOutput::Original));
        let copy=lua.create_table().unwrap();for &(name,value) in &view.fields{copy.raw_set(name,value).unwrap();}
        assert!(matches!(view.output(&copy).unwrap(),ExternalOutput::Controller7{number:74,value:72}));
        view.table.raw_set("value",96).unwrap();
        assert!(matches!(view.output(&view.table).unwrap(),ExternalOutput::Controller7{number:74,value:96}));
    }
    #[test]
    fn pitch_compatibility_projection_and_authored_endpoints_are_exact(){
        let lua=Lua::new();
        for (raw,bend) in [(0,-1.),(0x8000_0000,0.),(u32::MAX,1.)]{
            let view=ControllerProjection::new(&lua,source(),128,raw,None).unwrap();
            assert_eq!(view.table.raw_get::<f64>("bend").unwrap(),bend);
            assert!(matches!(view.output(&view.table).unwrap(),ExternalOutput::Original));
        }
        for (bend,raw) in [(-1.,0),(0.,0x8000_0000),(1.,u32::MAX)]{
            let view=ControllerProjection::new(&lua,source(),128,0x9000_0000,None).unwrap();view.table.raw_set("bend",bend).unwrap();
            assert!(matches!(view.output(&view.table).unwrap(),ExternalOutput::Controller{number:128,value}if value==raw));
        }
    }
    #[test]
    fn changed_poly_pressure_or_logical_routes_cannot_silently_become_channel_pressure_or_source_rewrites(){
        let lua=Lua::new();let poly=ControllerProjection::new(&lua,source(),129,0x9123_4567,Some(60)).unwrap();
        assert!(matches!(poly.output(&poly.table).unwrap(),ExternalOutput::Original));
        poly.table.raw_set("value",64).unwrap();assert!(matches!(poly.output(&poly.table).unwrap(),ExternalOutput::PolyPressure{value}if value==sampler_core::ReceiverValue::Bits7(64).full_scale()));
        for (field,value) in [("channel",0.),("input",0.),("type",7.)]{
            let view=ControllerProjection::new(&lua,source(),1,0x8123_4567,None).unwrap();view.table.raw_set(field,value).unwrap();assert!(view.output(&view.table).is_err());
        }
    }
}


#[cfg(test)]
mod channel_event24_projection_tests{
    use super::*;
    fn source()->sampler_core::ChannelAddress{sampler_core::ChannelAddress{protocol:sampler_core::Protocol::Midi2,port:1,group:3,channel:7}}
    #[test]
    fn actual_logical_channel_is_visible_without_rewriting_physical_origin(){
        let lua=Lua::new();let physical=source();
        let view=ControllerProjection::new_with_channel(&lua,physical,2,74,0x9123_4567,None).unwrap();
        assert_eq!(view.table.raw_get::<u8>("channel").unwrap(),3);
        assert_eq!(physical.channel,7);
        assert!(matches!(view.output(&view.table).unwrap(),ExternalOutput::Original));
        view.table.raw_set("channel",12).unwrap();
        assert!(matches!(view.output(&view.table).unwrap(),ExternalOutput::RoutedController7{number:74,value:72,channels}if channels==1<<11));
    }
    #[test]
    fn poly_value_only_preserves_captured_selector_but_key_or_channel_edits_are_explicit(){
        let lua=Lua::new();let view=ControllerProjection::new_with_channel(&lua,source(),2,129,0x9123_4567,Some(60)).unwrap();
        view.table.raw_set("value",96).unwrap();
        assert!(matches!(view.output(&view.table).unwrap(),ExternalOutput::PolyPressure{value}if value==sampler_core::ReceiverValue::Bits7(96).full_scale()));
        view.table.raw_set("note",61).unwrap();
        assert!(matches!(view.output(&view.table).unwrap(),ExternalOutput::RoutedPolyPressure{key:61,channels,..}if channels==1<<2));
        view.table.raw_set("channel",16).unwrap();
        assert!(matches!(view.output(&view.table).unwrap(),ExternalOutput::RoutedPolyPressure{key:61,channels,..}if channels==1<<15));
    }
    #[test]
    fn unchanged_program_is_original_and_same_identity_program_edit_preserves_typed_opaque_bank(){
        let lua=Lua::new();let view=ControllerProjection::program(&lua,source(),2,93,sampler_core::ProgramBankIntent::Select([11,107])).unwrap();
        assert_eq!(view.table.raw_get::<u8>("program").unwrap(),93);
        assert!(matches!(view.output(&view.table).unwrap(),ExternalOutput::Original));
        view.table.raw_set("program",94).unwrap();
        assert!(matches!(view.output(&view.table).unwrap(),ExternalOutput::ProgramChange{program:94,bank:sampler_core::ProgramBankIntent::Select([11,107]),target:sampler_core::ExternalTarget::Captured}));
        let copy=lua.create_table().unwrap();for pair in view.table.clone().pairs::<Value,Value>(){let(k,v)=pair.unwrap();copy.raw_set(k,v).unwrap();}
        assert!(view.output(&copy).is_err(),"copied Lua table has no native bank serialization authority");
    }
    #[test]
    fn program_unknown_fields_alias_conflicts_and_reroutes_refuse_without_bank_defaults(){
        let lua=Lua::new();
        for field in ["bank","bankMSB","bankLSB","bankValid","customBank"]{
            let view=ControllerProjection::program(&lua,source(),2,93,sampler_core::ProgramBankIntent::KeepSelected).unwrap();
            view.table.raw_set(field,0).unwrap();assert!(view.output(&view.table).is_err());
        }
        let view=ControllerProjection::program(&lua,source(),2,93,sampler_core::ProgramBankIntent::KeepSelected).unwrap();
        view.table.raw_set("program",94).unwrap();view.table.raw_set("value",95).unwrap();assert!(view.output(&view.table).is_err());
        let view=ControllerProjection::program(&lua,source(),2,93,sampler_core::ProgramBankIntent::KeepSelected).unwrap();
        view.table.raw_set("channel",4).unwrap();assert!(view.output(&view.table).is_err());
    }
}

// Property-only standalone authority. These Copy requests have no musical or
// callback capability. Product actor service never accepts this endpoint.
#[derive(Clone,Copy,Debug)]
pub enum SourcePropertyOperation {Query{node:u32},Set{node:u32,field:sampler_core::SourceConstantField,value:sampler_core::ControlValue}}
#[derive(Clone,Copy)]
struct SourcePropertyRequest {sequence:u64,operation:SourcePropertyOperation}
#[derive(Clone,Copy)]
struct SourcePropertyReply {sequence:u64,result:Result<sampler_core::SourceConstantSnapshot,sampler_core::Error>}
pub struct CoreSourcePropertyPort {
    runtime:RuntimeId,plan:PlanId,requests:rtrb::Consumer<SourcePropertyRequest>,replies:rtrb::Producer<SourcePropertyReply>,cancelled:Arc<AtomicBool>,
}
pub struct OwnerSourcePropertyPort {
    pub(super) definitions:Vec<sampler_core::SourceConstantProperty>,
    requests:rtrb::Producer<SourcePropertyRequest>,replies:rtrb::Consumer<SourcePropertyReply>,cancelled:Arc<AtomicBool>,sequence:u64,timeout:Duration,
}
impl Drop for CoreSourcePropertyPort{fn drop(&mut self){self.cancelled.store(true,Ordering::Release);}}
impl Drop for OwnerSourcePropertyPort{fn drop(&mut self){self.cancelled.store(true,Ordering::Release);}}
impl CoreSourcePropertyPort {
    /// Bounded single-writer service at actual Runtime F. No queue admission ACK.
    pub fn service(&mut self,runtime:&mut sampler_core::Runtime)->Result<usize,sampler_core::Error>{
        use sampler_core::Error;
        if runtime.id()!=self.runtime{return Err(Error::StaleHandle);}
        if self.cancelled.load(Ordering::Acquire){return Err(Error::Canceled);}
        let mut count=0;
        while count<64&&self.replies.slots()>0{
            let Ok(request)=self.requests.pop()else{break;};
            let result=if self.cancelled.load(Ordering::Acquire){Err(Error::Canceled)}else{match request.operation{
                SourcePropertyOperation::Query{node}=>runtime.source_constant_property(self.plan,node),
                SourcePropertyOperation::Set{node,field,value}=>runtime.set_source_constant_property(self.plan,node,field,value),
            }};
            // The sole producer reserved a slot before pop; no allocation/drop.
            if self.replies.push(SourcePropertyReply{sequence:request.sequence,result}).is_err(){self.cancelled.store(true,Ordering::Release);return Err(Error::Capacity);}
            count+=1;
        }Ok(count)
    }
}
impl OwnerSourcePropertyPort {
    pub(super) fn call(&mut self,operation:SourcePropertyOperation)->Result<sampler_core::SourceConstantSnapshot,sampler_core::Error>{
        use sampler_core::Error;
        if self.cancelled.load(Ordering::Acquire){return Err(Error::Canceled);}
        let node=match operation{SourcePropertyOperation::Query{node}|SourcePropertyOperation::Set{node,..}=>node};
        if !self.definitions.iter().any(|row|row.node==node){return Err(Error::InvalidInput);}
        let sequence=self.sequence;self.sequence=self.sequence.checked_add(1).ok_or(Error::Capacity)?;
        let deadline=Instant::now().checked_add(self.timeout).ok_or(Error::InvalidInput)?;
        self.requests.push(SourcePropertyRequest{sequence,operation}).map_err(|_|Error::Capacity)?;
        loop{
            if let Ok(reply)=self.replies.pop(){
                if reply.sequence!=sequence{self.cancelled.store(true,Ordering::Release);return Err(Error::InvalidInput);}return reply.result;
            }
            if self.cancelled.load(Ordering::Acquire){return Err(Error::Canceled);}
            if Instant::now()>=deadline{self.cancelled.store(true,Ordering::Release);return Err(Error::Busy);}
            // Only independently serviced non-RT ScriptThread owner may wait.
            std::thread::sleep(Duration::from_micros(50));
        }
    }
}
pub(crate) fn source_property_pair(runtime:&sampler_core::Runtime,plan:PlanId,timeout:Duration)->Result<(CoreSourcePropertyPort,OwnerSourcePropertyPort),sampler_core::Error>{
    use sampler_core::Error;
    if timeout.is_zero()||timeout>Duration::from_secs(5){return Err(Error::InvalidInput);}
    let count=runtime.source_constant_properties(plan)?.count();
    if count==0||count>4096{return Err(Error::InvalidInput);}
    let mut definitions=Vec::new();definitions.try_reserve_exact(count).map_err(|_|Error::Capacity)?;
    definitions.extend(runtime.source_constant_properties(plan)?);
    // Typed payload arrays are checked offaudio. Ring headers/cancellation
    // allocation are fixed per endpoint, documented separately; no note multiplier.
    let _proof=std::alloc::Layout::array::<sampler_core::SourceConstantProperty>(definitions.capacity()).map_err(|_|Error::Capacity)?;
    let _requests=std::alloc::Layout::array::<SourcePropertyRequest>(AUTHORITY_QUEUE).map_err(|_|Error::Capacity)?;
    let _replies=std::alloc::Layout::array::<SourcePropertyReply>(AUTHORITY_QUEUE).map_err(|_|Error::Capacity)?;
    let (send,requests)=rtrb::RingBuffer::new(AUTHORITY_QUEUE);let(replies,receive)=rtrb::RingBuffer::new(AUTHORITY_QUEUE);
    let cancelled=Arc::new(AtomicBool::new(false));
    Ok((CoreSourcePropertyPort{runtime:runtime.id(),plan,requests,replies,cancelled:cancelled.clone()},OwnerSourcePropertyPort{definitions,requests:send,replies:receive,cancelled,sequence:0,timeout}))
}


#[cfg(test)]
mod source_property_port42g_tests {
    use super::*;
    fn plan()->sampler_core::Prepared{
        let mut ir=sampler_ir::Instrument{source:sampler_ir::SourceFormat::Uvi,..Default::default()};
        ir.source_indices.owners.push(sampler_ir::SourceOwner{id:sampler_ir::SourceOwnerId::UviNode(1),parent:None,kind:sampler_ir::SourceOwnerKind::Instrument,children:vec![],groups:vec![],zones:vec![],predicates:vec![],bypass:None,mute:None,event_processors:vec![]});
        ir.controls.push(sampler_ir::Control{key:"value".into(),label:"Value".into(),value:sampler_ir::ControlValue::Continuous{min:f64::MIN,max:f64::MAX,default:0.25,unit:sampler_ir::ControlUnit::None},automation:sampler_ir::Automation::None});
        ir.controls.push(sampler_ir::Control{key:"style".into(),label:"Style".into(),value:sampler_ir::ControlValue::Toggle{default:false},automation:sampler_ir::Automation::None});
        ir.source_indices.constant_properties.push(sampler_ir::SourceConstantProperty{node:42,owner:sampler_ir::SourceOwnerId::UviNode(1),value:sampler_ir::ControlRef(0),style:sampler_ir::ControlRef(1),bipolar:false,law:sampler_ir::SourceConstantPropertyLaw::DeclaredCoreTargetRead});
        sampler_core::lower::lower(&ir,48000,vec![],|_,plan|Ok(plan)).unwrap()
    }
    fn runtime()->sampler_core::Runtime{let plan=plan();let limits=sampler_core::Limits::for_plan(&plan,4,4);sampler_core::Runtime::new(plan,limits).unwrap()}
    #[test]
    fn property_request_ack_uses_actual_runtime_plan_and_rejects_invalid_target_before_retention(){
        use sampler_core::{SourceConstantField as Field,ControlValue,Error};
        let mut rt=runtime();let plan=rt.active_plan();let(mut core,mut owner)=source_property_pair(&rt,plan,Duration::from_secs(2)).unwrap();let mut other=runtime();
        let actor=std::thread::spawn(move||{
            assert_eq!(owner.call(SourcePropertyOperation::Set{node:42,field:Field::Value,value:ControlValue::Real(f64::NAN)}),Err(Error::InvalidInput));
            let accepted=owner.call(SourcePropertyOperation::Set{node:42,field:Field::Value,value:ControlValue::Real(0.75)}).unwrap();assert_eq!((accepted.value,accepted.revision,accepted.frame),(0.75,1,0));
            let query=owner.call(SourcePropertyOperation::Query{node:42}).unwrap();assert_eq!(query.value,0.75);
        });
        assert_eq!(core.service(&mut other),Err(Error::StaleHandle));assert_eq!(other.source_constant_property(other.active_plan(),42).unwrap().value,0.25);
        let deadline=Instant::now()+Duration::from_secs(2);while !actor.is_finished(){match core.service(&mut rt){Ok(_)=>{},Err(sampler_core::Error::Canceled)=>break,Err(error)=>panic!("{error:?}")};assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_micros(50));}actor.join().unwrap();assert_eq!(rt.source_constant_property(plan,42).unwrap().value,0.75);assert_eq!(rt.now(),0);
    }
    #[test]
    fn accepted_but_undelivered_property_result_is_not_rolled_back_or_retried_after_fence(){
        use sampler_core::{SourceConstantField as Field,ControlValue,Error};
        let mut rt=runtime();let plan=rt.active_plan();let(mut core,mut owner)=source_property_pair(&rt,plan,Duration::from_millis(100)).unwrap();
        let actor=std::thread::spawn(move||{
            assert_eq!(owner.call(SourcePropertyOperation::Set{node:42,field:Field::Value,value:ControlValue::Real(0.75)}),Err(Error::Busy));
            assert_eq!(owner.call(SourcePropertyOperation::Query{node:42}),Err(Error::Canceled));
        });
        let deadline=Instant::now()+Duration::from_secs(2);let request=loop{if let Ok(request)=core.requests.pop(){break request;}assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_micros(50));};
        // A real request from the actual endpoint, accepted by real Core. Hold
        // its actual return to model delivery loss; do not fabricate rollback.
        let SourcePropertyOperation::Set{node,field,value}=request.operation else{panic!("real property set");};let accepted=rt.set_source_constant_property(plan,node,field,value).unwrap();assert_eq!(accepted.value,0.75);
        actor.join().unwrap();assert!(core.cancelled.load(Ordering::Acquire));assert_eq!(rt.source_constant_property(plan,42).unwrap().value,0.75);assert_eq!(core.service(&mut rt),Err(Error::Canceled));
        // The bounded result is retained here until explicit endpoint teardown.
        let result=SourcePropertyReply{sequence:request.sequence,result:Ok(accepted)};assert_eq!(result.result.unwrap().revision,1);
    }
    #[test]
    fn source_property_endpoint_is_not_a_plan_lease_and_old_generation_never_retargets(){
        use sampler_core::{Runtime,Limits,Error};
        let prepared=plan();let limits=Limits::for_plan(&prepared,4,4);let(mut rt,mut updates)=Runtime::with_plan_updates(prepared,limits,3,2).unwrap();let old=rt.active_plan();let(mut core,mut owner)=source_property_pair(&rt,old,Duration::from_secs(2)).unwrap();
        let old_request=rt.plan_request(old).unwrap();
        let request=updates.submit(Box::new(plan())).unwrap();assert_eq!(rt.poll_plan_update(),Ok(Some(request)));let new=rt.active_plan();assert_ne!(old,new);
        // poll_plan_update already collects inactive, unreferenced generations
        // after publication. The still-live property endpoints must not lease
        // the old one; prove its actual control-side ownership transfer.
        assert_eq!(rt.collect_retired_plans(),0);
        assert_eq!(rt.plan_count(),1);assert_eq!(rt.plan_request(old),Err(Error::StaleHandle));
        let retired=updates.retired().expect("old generation transfers while both property endpoints remain alive");
        assert_eq!(retired.request,old_request);assert!(updates.retired().is_none());
        drop(retired);
        let actor=std::thread::spawn(move||assert_eq!(owner.call(SourcePropertyOperation::Query{node:42}),Err(Error::StaleHandle)));
        let deadline=Instant::now()+Duration::from_secs(2);while !actor.is_finished(){match core.service(&mut rt){Ok(_)=>{},Err(sampler_core::Error::Canceled)=>break,Err(error)=>panic!("{error:?}")};assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_micros(50));}actor.join().unwrap();assert_eq!(rt.source_constant_property(new,42).unwrap().value,0.25);
    }
}
