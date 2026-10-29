//! Actual retained event values. VM-object lifetime is independent of physical
//! key ownership, callback lifetime and queued-post receipt lifetime.
use crate::{Arena,Error,ExpressionId,Handle,PlanId,Runtime};
use super::external::{CapturedChannelAction,ExternalEvent,ExternalInputId,Phase};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct EventValueId(pub(crate) Handle);
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct EventPostId(pub(crate) Handle);
#[derive(Clone,Copy,Debug)]
pub struct EventValueSnapshot {
    pub plan:PlanId,pub processor:sampler_ir::SourceEventProcessorId,
    pub owner:crate::PreparedSourceOwnerIndex,pub logical_channel:Option<u8>,pub original_frame:u64,
    pub original:ExternalEvent,pub event_origin:Option<crate::NoteId>,
    /// A genuine retained expression generation, never a reconstructed NoteId.
    /// This is identity metadata; it does not establish native saved-table law.
    pub expression:Option<ExpressionId>,
}
#[derive(Clone,Copy)]
pub(crate) struct EventValue {
    pub snapshot:EventValueSnapshot,pub receiver:Option<super::ReceiverId>,
    captured:CapturedChannelAction,vm_owned:bool,revoked:bool,posts:usize,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum EventPostOutcome {Pending,Committed,Refused(Error),Cancelled}
#[derive(Clone,Copy)]
pub(crate) struct EventPost {value:EventValueId,outcome:EventPostOutcome}
impl EventPost{pub(crate) fn is_pending(&self)->bool{self.outcome==EventPostOutcome::Pending}}

fn arena_bytes<T>(count:usize)->Result<usize,Error>{
    let slots=std::alloc::Layout::array::<crate::Slot<T>>(count).map_err(|_|Error::Capacity)?.size();
    let free=std::alloc::Layout::array::<u64>(count.div_ceil(64)).map_err(|_|Error::Capacity)?.size();
    slots.checked_add(free).ok_or(Error::Capacity)
}
impl Runtime {
    /// Explicit offaudio minimum reserve. Never shrink/recreate generation
    /// history in the same Runtime, even after all values/posts were reclaimed.
    pub fn with_event_value_capacity(mut self,values:usize,posts:usize)->Result<Self,Error>{
        if values==0||posts==0{return Err(Error::InvalidInput);}
        if self.notes.count()!=0||self.behaviors.count()!=0||self.event_values.count()!=0||self.event_posts.count()!=0{return Err(Error::Busy);}
        let values=values.max(self.event_values.slots.len());let posts=posts.max(self.event_posts.slots.len());
        let old=arena_bytes::<EventValue>(self.event_values.slots.len())?.checked_add(arena_bytes::<EventPost>(self.event_posts.slots.len())?).ok_or(Error::Capacity)?;
        let new=arena_bytes::<EventValue>(values)?.checked_add(arena_bytes::<EventPost>(posts)?).ok_or(Error::Capacity)?;
        old.checked_add(new.checked_mul(2).ok_or(Error::Capacity)?).ok_or(Error::Capacity)?;
        if values>self.event_values.slots.len(){let (mut slots,mut free)=Arena::try_blank(values)?;self.event_values.grow(&mut slots,&mut free);}
        if posts>self.event_posts.slots.len(){let (mut slots,mut free)=Arena::try_blank(posts)?;self.event_posts.grow(&mut slots,&mut free);}
        Ok(self)
    }
    /// The actual currently running capture is the only creation authority.
    /// Numeric event/Note IDs or source tuple equality cannot create a value.
    pub fn capture_external_value(&mut self,input:ExternalInputId)->Result<EventValueId,Error>{
        let (callback,state)=self.validate_external_segment(input,Phase::Running)?;
        let turn=self.external_turn(input.behavior(),&callback,state)?;
        if self.event_values.available()==0{return Err(Error::Capacity);}
        let expression=match turn.event{
            ExternalEvent::Note{note,..}=>Some(self.expression_id(note)?),_=>None};
        let captures=expression.map(|id|self.expressions.get(id.0).ok_or(Error::StaleHandle)?.captures.checked_add(1).ok_or(Error::Capacity)).transpose()?;
        let generation=self.plans.get(turn.plan.0).ok_or(Error::StaleHandle)?;
        let plan_refs=generation.event_values.checked_add(1).ok_or(Error::Capacity)?;
        let receiver_refs=turn.receiver.map(|id|self.receivers.get(id.0).ok_or(Error::StaleHandle)?.event_value_references.checked_add(1).ok_or(Error::Capacity)).transpose()?;
        let id=EventValueId(self.event_values.insert(EventValue{snapshot:EventValueSnapshot{plan:turn.plan,processor:turn.processor,owner:turn.owner,logical_channel:turn.logical_channel,original_frame:turn.original_frame,original:turn.event,event_origin:turn.event_origin,expression},receiver:turn.receiver,captured:state.captured,vm_owned:true,revoked:false,posts:0})?);
        self.plans.get_mut(turn.plan.0).ok_or(Error::StaleHandle)?.event_values=plan_refs;
        if let Some(id)=expression{self.expressions.get_mut(id.0).ok_or(Error::StaleHandle)?.captures=captures.ok_or(Error::InvalidInput)?;}
        if let Some(id)=turn.receiver{self.receivers.get_mut(id.0).ok_or(Error::StaleHandle)?.event_value_references=receiver_refs.ok_or(Error::InvalidInput)?;}
        Ok(id)
    }
    pub fn event_value(&self,id:EventValueId)->Result<EventValueSnapshot,Error>{Ok(self.event_values.get(id.0).ok_or(Error::StaleHandle)?.snapshot)}
    /// Ordered VM GC/cancel acknowledgement, exactly once per real original
    /// object. Aliases of a Lua table must share this one semantic value guard.
    pub fn release_event_value(&mut self,id:EventValueId)->Result<(),Error>{
        let value=self.event_values.get_mut(id.0).ok_or(Error::StaleHandle)?;
        if !value.vm_owned{return Err(Error::StaleHandle);}value.vm_owned=false;
        self.reclaim_event_value(id);Ok(())
    }
    fn reclaim_event_value(&mut self,id:EventValueId){
        let Some(value)=self.event_values.get(id.0).copied()else{return;};
        if value.vm_owned||value.posts!=0{return;}
        self.event_values.remove(id.0);
        if let Some(generation)=self.plans.get_mut(value.snapshot.plan.0){generation.event_values-=1;}
        if let Some(receiver)=value.receiver&&let Some(owner)=self.receivers.get_mut(receiver.0){owner.event_value_references-=1;}
        if let Some(expression)=value.snapshot.expression{
            if let Some(owner)=self.expressions.get_mut(expression.0){owner.captures-=1;}
            self.retire_expression_chain(expression);
        }
    }
    /// Real future output receipt. Current Controller/Program transparent repost
    /// semantics are established by their captured downstream continuation path.
    /// Retained original-note native expression/frontier law remains unproved:
    /// do not silently convert such a value into a new independent note.
    pub fn schedule_event_value(&mut self,value:EventValueId,at:u64)->Result<EventPostId,Error>{
        if self.external_execution_aborted{return Err(Error::Canceled);}
        if at<self.now{return Err(Error::PastEvent);}
        let row=self.event_values.get(value.0).ok_or(Error::StaleHandle)?;
        if !row.vm_owned{return Err(Error::StaleHandle);}
        if row.revoked{return Err(Error::Canceled);}
        if !matches!(row.captured,CapturedChannelAction::Controller(_)|CapturedChannelAction::Program(_)){return Err(Error::InvalidInput);}
        let posts=row.posts.checked_add(1).ok_or(Error::Capacity)?;
        if self.event_posts.available()==0||self.available_commands()==0{return Err(Error::Capacity);}
        let id=EventPostId(self.event_posts.insert(EventPost{value,outcome:EventPostOutcome::Pending})?);
        self.event_values.get_mut(value.0).ok_or(Error::StaleHandle)?.posts=posts;
        self.queue(at,crate::Action::EventValuePost(id));Ok(id)
    }
    pub fn event_post_outcome(&self,id:EventPostId)->Result<EventPostOutcome,Error>{Ok(self.event_posts.get(id.0).ok_or(Error::StaleHandle)?.outcome)}
    pub(crate) fn apply_event_value_post(&mut self,id:EventPostId){
        let Some(post)=self.event_posts.get(id.0).copied()else{return;};
        if post.outcome!=EventPostOutcome::Pending{return;}
        let result=(||{
            let value=*self.event_values.get(post.value.0).ok_or(Error::StaleHandle)?;
            if value.revoked{return Err(Error::Canceled);}
            match value.captured{
                CapturedChannelAction::Controller(mut event)=>{
                    event.stage=event.stage.checked_add(1).ok_or(Error::Capacity)?;event.pending=true;event.reserved=0;
                    self.admit_controller(value.snapshot.plan,event)?;Ok(())
                },
                CapturedChannelAction::Program(mut event)=>{
                    event.stage=event.stage.checked_add(1).ok_or(Error::Capacity)?;event.pending=true;event.reserved=0;
                    self.admit_program_event(value.snapshot.plan,event)?;Ok(())
                },_=>Err(Error::InvalidInput)
            }
        })();
        if let Some(post)=self.event_posts.get_mut(id.0){post.outcome=match result{Ok(())=>EventPostOutcome::Committed,Err(error)=>EventPostOutcome::Refused(error)};}
    }
    pub fn cancel_event_post(&mut self,id:EventPostId)->Result<(),Error>{
        let post=self.event_posts.get_mut(id.0).ok_or(Error::StaleHandle)?;
        if post.outcome==EventPostOutcome::Pending{post.outcome=EventPostOutcome::Cancelled;}
        self.commands.retain(|command|!matches!(command.action,crate::Action::EventValuePost(target)if target==id));
        Ok(())
    }
    /// Genuine post acknowledgement releases one reference exactly once. Pending
    /// work cannot be acknowledged and cannot lose its last semantic value.
    pub fn acknowledge_event_post(&mut self,id:EventPostId)->Result<(),Error>{
        let post=*self.event_posts.get(id.0).ok_or(Error::StaleHandle)?;
        if post.outcome==EventPostOutcome::Pending{return Err(Error::Busy);}
        self.event_posts.remove(id.0);
        let value=self.event_values.get_mut(post.value.0).ok_or(Error::StaleHandle)?;value.posts-=1;
        self.reclaim_event_value(post.value);Ok(())
    }
    /// Ordinary panic purges actual queued commands, not VM ownership. Retain a
    /// terminal receipt for every removed Pending post until its real ACK; do
    /// not revoke otherwise reusable values or overwrite a terminal outcome.
    /// Iterate the bounded command roster once without allocating or applying
    /// due work, rather than guessing posts from a source tuple or timestamp.
    pub(crate) fn cancel_queued_event_value_posts_for_panic(&mut self){
        for command in &self.commands{
            let crate::Action::EventValuePost(id)=command.action else{continue;};
            if let Some(post)=self.event_posts.get_mut(id.0)
                && post.outcome==EventPostOutcome::Pending{
                post.outcome=EventPostOutcome::Cancelled;
            }
        }
    }
    /// Fatal/source-owner cancellation keeps completed receipts queryable until
    /// their real ACK, but prevents a future queued post at an old execution F.
    pub(crate) fn cancel_event_values_for_abort(&mut self){
        for slot in &mut self.event_values.slots{if let Some(value)=slot.value.as_mut(){value.revoked=true;}}
        for slot in &mut self.event_posts.slots{if let Some(post)=slot.value.as_mut()&&post.outcome==EventPostOutcome::Pending{post.outcome=EventPostOutcome::Cancelled;}}
    }
}

impl Runtime {
    /// Explicit actual source-generation fence. It cancels pending outputs but
    /// retains independently acknowledgeable receipts and VM-owned values.
    pub fn revoke_event_values(&mut self,plan:PlanId)->Result<usize,Error>{
        self.plans.get(plan.0).ok_or(Error::StaleHandle)?;let mut count=0;
        for slot in &mut self.event_values.slots{if let Some(value)=slot.value.as_mut()&&value.snapshot.plan==plan{if !value.revoked{count+=1;}value.revoked=true;}}
        for slot in &mut self.event_posts.slots{if let Some(post)=slot.value.as_mut()&&post.outcome==EventPostOutcome::Pending&&self.event_values.get(post.value.0).is_some_and(|value|value.snapshot.plan==plan){post.outcome=EventPostOutcome::Cancelled;}}
        self.commands.retain(|command|match command.action{crate::Action::EventValuePost(id)=>self.event_posts.get(id.0).is_some_and(EventPost::is_pending),_=>true});
        Ok(count)
    }
    /// Call only after the actual VM/source owner has been revoked. Late VM GC
    /// then gets StaleHandle instead of decrementing any new owner's references.
    pub fn release_revoked_event_values(&mut self,plan:PlanId)->Result<usize,Error>{
        self.plans.get(plan.0).ok_or(Error::StaleHandle)?;let mut count=0;
        for index in 0..self.event_values.slots.len(){
            if self.event_values.slots[index].value.is_some_and(|value|value.snapshot.plan==plan&&value.revoked&&value.vm_owned){
                let id=EventValueId(self.event_values.id(index));self.release_event_value(id)?;count+=1;
            }
        }Ok(count)
    }
    /// Owner acknowledgements are bounded by the actual post arena; execution
    /// ordering remains the real timestamp/submission ordering of Core commands.
    /// Rejection leaves this and later real receipts present and queryable.
    pub fn flush_event_posts(&mut self,mut accept:impl FnMut(EventPostId,EventPostOutcome)->bool)->usize{
        let mut count=0;
        for index in 0..self.event_posts.slots.len(){
            let Some(post)=self.event_posts.slots[index].value else{continue;};
            if post.outcome==EventPostOutcome::Pending{continue;}
            let id=EventPostId(self.event_posts.id(index));
            if !accept(id,post.outcome){break;}
            if self.acknowledge_event_post(id).is_ok(){count+=1;}
        }count
    }
}
pub use kontra_events::EventValueMemory;
impl Runtime {
    pub fn event_value_memory(&self)->Result<EventValueMemory,Error>{
        Ok(EventValueMemory{values:self.event_values.slots.len(),posts:self.event_posts.slots.len(),
            arena_bytes:arena_bytes::<EventValue>(self.event_values.slots.len())?.checked_add(arena_bytes::<EventPost>(self.event_posts.slots.len())?).ok_or(Error::Capacity)?,
            expression_capture_bytes:std::mem::size_of::<usize>().checked_mul(self.expressions.slots.len()).ok_or(Error::Capacity)?})
    }
}
