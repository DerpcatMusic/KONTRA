//! Actual document-note routing over the one prepared owner table. The linear
//! Kontakt script-stage router remains independent and unchanged.
use crate::{BehaviorId,BehaviorOwner,Error,Inheritance,Input,NoteId,NotePitch,Prepared,
    PreparedSourceOwnerIndex,Runtime};

#[derive(Clone,Copy,Debug)]
pub(crate) struct SourceNoteRoute {
    pub owner:PreparedSourceOwnerIndex,
    /// Full authored event-list ordinal, including non-script entries.
    pub next:u32,
    pub channel:u8,
    pub source:Input,
    /// Actual root event identity, opaque after retirement; never a parent lease.
    pub event_origin:NoteId,
    pub entered:bool,
}

impl Prepared {
    /// Compile Workstation-style Program/Layer ScriptProcessor note boundaries.
    /// This is an explicit owner installation operation before publication. It
    /// never guesses source_slot or flattens sibling Layers into linear stages.
    pub fn with_source_note_hierarchy(mut self)->Result<Self,Error>{
        use sampler_ir::{SourceOwnerKind as K,SourceOwnerId};
        if !self.programs.is_empty() || self.source_owners.note_root.is_some(){return Err(Error::InvalidInput);}
        let root=self.source_owners.rows().iter().position(|owner|owner.parent.is_none()).ok_or(Error::InvalidInput)?;
        let root=PreparedSourceOwnerIndex(u32::try_from(root).map_err(|_|Error::Capacity)?);
        let owner=self.source_owners.get(root).ok_or(Error::InvalidInput)?;
        if owner.kind!=K::Instrument || !matches!(owner.id,SourceOwnerId::UviNode(_)){return Err(Error::InvalidInput);}
        let mut count=0usize;
        for row in self.source_owners.rows(){
            if !matches!(row.kind,K::Instrument|K::Layer){continue;}
            if row.kind==K::Layer && row.parent!=Some(root){return Err(Error::InvalidInput);}
            for &child in &row.children {
                let child=self.source_owners.get(child).ok_or(Error::InvalidInput)?;
                let valid=match row.kind {K::Instrument=>matches!(child.kind,K::Layer|K::Aux),K::Layer=>matches!(child.kind,K::Keygroup|K::Aux),_=>false};
                if !valid{return Err(Error::InvalidInput);}
            }
            for processor in &row.event_processors {
                // Metadata retains all unsupported rows, but this profile does
                // not silently skip arpeggiators or invent their execution law.
                if processor.kind.as_ref()!="ScriptProcessor"{return Err(Error::InvalidInput);}
                count=count.checked_add(1).ok_or(Error::Capacity)?;
            }
        }
        let mut programs=Vec::new();programs.try_reserve_exact(count.checked_add(1).ok_or(Error::Capacity)?).map_err(|_|Error::Capacity)?;
        for row in self.source_owners.rows(){
            if !matches!(row.kind,K::Instrument|K::Layer){continue;}
            for processor in &row.event_processors {
                programs.push(crate::Program::new(vec![crate::Instruction::AwaitExternal{processor:processor.id},crate::Instruction::End])?
                    .with_wait_lifetime(crate::WaitLifetime::Callback));
            }
        }
        let entry=programs.len();
        programs.push(crate::Program::new(vec![crate::Instruction::AdvanceSourceNote,crate::Instruction::End])?
            .with_wait_lifetime(crate::WaitLifetime::Callback));
        self=self.with_programs(programs,None)?;
        let mut program=0usize;
        for row in self.source_owners.rows_mut(){
            if !matches!(row.kind,K::Instrument|K::Layer){continue;}
            for processor in &mut row.event_processors {processor.note_program=Some(program);program+=1;}
        }
        self.source_owners.note_root=Some(root);self.source_owners.note_entry=Some(entry);Ok(self)
    }
    /// Actual compiled callback binding, not a document or source-slot guess.
    pub fn source_note_program(&self,id:sampler_ir::SourceEventProcessorId)->Option<usize>{
        self.source_owners.processor(id)?.note_program
    }
}

impl Runtime {
    pub(crate) fn source_note_work_cost(&self,note:NoteId)->Result<usize,Error>{
        let n=self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let route=self.note_events[note.0.index].source_route.ok_or(Error::InvalidInput)?;
        self.source_owner_work_cost(n.plan,route.owner)
    }
    fn source_owner_work_cost(&self,plan:crate::PlanId,owner:PreparedSourceOwnerIndex)->Result<usize,Error>{
        let table=&self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners;
        let row=table.get(owner).ok_or(Error::InvalidInput)?;
        let siblings=if row.kind==sampler_ir::SourceOwnerKind::Instrument{row.children.len()}else{0};
        // Each admitted sibling may scan the actual note arena for minimum order;
        // include the final miss and error cleanup. Callback-room lookup also
        // walks its bounded arena. Ordinary native selection retains its existing
        // independently admitted candidate/decision bounds.
        let per_sibling=self.notes.slots.len().checked_add(self.behaviors.slots.len()).ok_or(Error::Capacity)?;
        let mut cost=siblings.checked_add(2).and_then(|n|n.checked_mul(per_sibling)).ok_or(Error::Capacity)?;
        cost=cost.checked_add(row.predicates.len()).and_then(|n|n.checked_add(row.event_processors.len())).ok_or(Error::Capacity)?;
        for &child in &row.children{
            let child=table.get(child).ok_or(Error::InvalidInput)?;
            cost=cost.checked_add(child.predicates.len()).and_then(|n|n.checked_add(child.event_processors.len())).and_then(|n|n.checked_add(8)).ok_or(Error::Capacity)?;
        }
        Ok(cost.max(1))
    }
    fn reserve_external_source_work(
        &mut self,
        plan: crate::PlanId,
        owner: PreparedSourceOwnerIndex,
    ) -> Result<(), Error> {
        let cost = self.source_owner_work_cost(plan, owner)?;
        if cost > self.behavior_fuel || cost > self.behavior_budget.remaining {
            return Err(Error::Capacity);
        }
        self.behavior_budget.remaining -= cost;
        Ok(())
    }
    pub(crate) fn source_note_owner_entered(&self,note:NoteId,owner:PreparedSourceOwnerIndex)->bool{
        self.note_events.get(note.0.index).and_then(|event|event.source_route)
            .is_some_and(|route|route.entered&&route.owner==owner)
    }
    pub(crate) fn source_note_callback(&self,callback:BehaviorId)->Result<bool,Error>{
        let c=self.behaviors.get(callback.0).ok_or(Error::StaleHandle)?;
        let BehaviorOwner::Note(note)=c.owner else{return Ok(false);};
        let Some(route)=self.note_events[note.0.index].source_route else{return Ok(false);};
        let plan=self.note_plan(note)?;
        let row=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners.get(route.owner).ok_or(Error::InvalidInput)?;
        Ok(row.event_processors.iter().any(|processor|processor.note_program==Some(c.program)))
    }
    /// Enter a real already-admitted silent physical event. The caller retains
    /// ordinary physical-key and terminal ownership; this does not admit again.
    pub fn start_source_note(&mut self,note:NoteId,logical_channel:u8)->Result<(),Error>{
        if self.external_execution_aborted{return Err(Error::Canceled);}
        if logical_channel>=16{return Err(Error::InvalidInput);}
        self.apply_due();
        let n=self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if n.attack!=crate::AttackStatus::Pending || self.note_events[note.0.index].source_route.is_some(){return Err(Error::InvalidInput);}
        let source=n.input_source.or(n.input).ok_or(Error::InvalidInput)?;
        let prepared=&self.plans.get(n.plan.0).ok_or(Error::StaleHandle)?.prepared;
        let root=prepared.source_owners.note_root.ok_or(Error::InvalidInput)?;
        let entry=prepared.source_owners.note_entry.ok_or(Error::InvalidInput)?;
        if !self.behavior_room(1){return Err(Error::Capacity);}
        self.note_events[note.0.index].source_route=Some(SourceNoteRoute{owner:root,next:0,channel:logical_channel,source,event_origin:note,entered:false});
        self.start_note_context(note,entry,None)?;Ok(())
    }
    /// Authored output starts at the next actual processor on a real new child.
    /// Original immutable source and expression lineage stay separate from key,
    /// velocity and channel edits. Every successful post is a musical effect.
    pub(crate) fn post_source_note(&mut self,callback:BehaviorId,pitch:NotePitch,velocity:f64,channel:Option<u8>)->Result<NoteId,Error>{
        if !pitch.valid()||!velocity.is_finite()||!(0.0..=1.0).contains(&velocity)||channel.is_some_and(|channel|channel>=16){return Err(Error::InvalidInput);}
        if !self.source_note_callback(callback)?{return Err(Error::InvalidInput);}
        let parent=self.behaviors.get(callback.0).ok_or(Error::StaleHandle)?.owner.note()?;
        let mut route=self.note_events[parent.0.index].source_route.ok_or(Error::InvalidInput)?;
        if let Some(channel)=channel{route.channel=channel;}
        let plan=self.note_plan(parent)?;
        self.reserve_external_source_work(plan,route.owner)?;
        let child=self.child_pitched(parent,pitch,velocity,false,Inheritance::Expression)?;
        self.route_fresh_source_child(child,route.channel)?;
        self.note_events[child.0.index].source_route=Some(route);
        // A running callback's output cannot be reclaimed while this synchronous
        // admission builds/starts its actual next boundary.
        self.notes.get_mut(child.0).ok_or(Error::StaleHandle)?.pins+=1;
        let result=self.advance_source_note(child);
        if result.is_err(){self.release_now(child,crate::ReleaseCause::Discarded)?;}
        self.notes.get_mut(child.0).ok_or(Error::StaleHandle)?.pins-=1;
        result?;Ok(child)
    }
    // Only a fresh generated child may be retargeted, before callbacks, source
    // decisions or audio. Physical provenance is immutable in SourceNoteRoute;
    // the new execution address is captured by its ordinary controller snapshot.
    fn route_fresh_source_child(&mut self,note:NoteId,channel:u8)->Result<(),Error>{
        let n=self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if channel>=16||n.input.is_some()||n.attack!=crate::AttackStatus::Pending||n.work!=0||n.families!=0||n.first_decision.is_some(){return Err(Error::InvalidInput);}
        let performance=self.selections[note.0.index].performance;
        let mut address=n.address;address.channel=channel;
        let snapshot=self.performance_state.capture_channel(performance,address);
        self.performance_state.release(self.selections[note.0.index].snapshot);
        self.selections[note.0.index].snapshot=snapshot;
        self.selections[note.0.index].controller_epoch=self.performance_state.epochs(performance,crate::ChannelScope{protocol:address.protocol,port:address.port,group:address.group,channels:1<<channel})[usize::from(channel)];
        self.notes.get_mut(note.0).ok_or(Error::StaleHandle)?.address=address;Ok(())
    }
    pub(crate) fn advance_source_note(&mut self,note:NoteId)->Result<(),Error>{
        let mut route=self.note_events[note.0.index].source_route.ok_or(Error::InvalidInput)?;
        let n=self.notes.get(note.0).ok_or(Error::StaleHandle)?;let plan=n.plan;
        let properties=self.note_event(note)?;
        if !route.entered {
            if !self.source_owner_input_eligible(plan,route.owner,properties,route.channel)?{
                self.suppress_attack(note)?;return Ok(());
            }
            route.entered=true;self.note_events[note.0.index].source_route=Some(route);
        }
        // Binding is immutable; a bound typed control target is read at this
        // frame (not advertised as an instantaneous modulation-signal readback). Unbound ScriptProcessor saved bypass is NOT a cached authority:
        // the actual VM delivers the accepted bool at the delivery boundary.
        loop {
            let row=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners.get(route.owner).ok_or(Error::InvalidInput)?;
            let processor=row.event_processors.get(route.next as usize);
            let Some(processor)=processor else{break;};
            let program=processor.note_program.ok_or(Error::InvalidInput)?;
            let control=processor.bypass.control;
            route.next=route.next.checked_add(1).ok_or(Error::Capacity)?;
            if let Some(control)=control {
                match self.control_value(plan,control)? {
                    crate::ControlValue::Toggle(true)=>{self.note_events[note.0.index].source_route=Some(route);continue;}
                    crate::ControlValue::Toggle(false)=>{},_=>return Err(Error::InvalidInput),
                }
            }
            if !self.behavior_room(1){return Err(Error::Capacity);}
            self.note_events[note.0.index].source_route=Some(route);
            self.start_note_context(note,program,None)?;return Ok(());
        }
        let kind=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners.get(route.owner).ok_or(Error::InvalidInput)?.kind;
        match kind {
            sampler_ir::SourceOwnerKind::Instrument=>self.fork_source_layers(note,route),
            sampler_ir::SourceOwnerKind::Layer=>{
                self.commit_source_frontier(note,route.owner,0,route.channel)?;
                self.forward_attack(note)?;
                if self.notes.get(note.0).ok_or(Error::StaleHandle)?.input.is_none(){self.retire_when_silent(note)?;}
                Ok(())
            }
            _=>Err(Error::InvalidInput),
        }
    }
    fn fork_source_layers(&mut self,note:NoteId,route:SourceNoteRoute)->Result<(),Error>{
        let n=self.notes.get(note.0).ok_or(Error::StaleHandle)?;let plan=n.plan;let properties=self.note_event(note)?;
        let row=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners.get(route.owner).ok_or(Error::InvalidInput)?;
        let mut count=0usize;
        for &child in &row.children{
            let child_row=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners.get(child).ok_or(Error::InvalidInput)?;
            if child_row.kind==sampler_ir::SourceOwnerKind::Layer&&self.source_owner_input_eligible(plan,child,properties,route.channel)?{count=count.checked_add(1).ok_or(Error::Capacity)?;}
        }
        // All sibling note/expression owners must fit before publishing any.
        // Each generated branch owns a distinct expression follower and real ID.
        let physical_room=self.notes.slots.iter().take(self.note_params.capacity()).filter(|slot|slot.value.is_none()).count();
        if count>self.notes.available()||count>physical_room||count>self.expressions.available(){return Err(Error::Capacity);}
        self.order.checked_add(u64::try_from(count).map_err(|_|Error::Capacity)?).ok_or(Error::ClockOverflow)?;
        let expression=self.expressions.get(n.expression.0).ok_or(Error::StaleHandle)?;
        let followed=expression.follows.unwrap_or(n.expression);
        self.expressions.get(followed.0).ok_or(Error::StaleHandle)?.followers.checked_add(count).ok_or(Error::Capacity)?;
        let children=row.children.len();
        let first_order=self.order;
        for ordinal in 0..children{
            let owner=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners.get(route.owner).ok_or(Error::InvalidInput)?.children[ordinal];
            let row=self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.source_owners.get(owner).ok_or(Error::InvalidInput)?;
            if row.kind!=sampler_ir::SourceOwnerKind::Layer||!self.source_owner_input_eligible(plan,owner,properties,route.channel)?{continue;}
            let child=self.child_pitched(note,properties.pitch,properties.velocity,true,Inheritance::Expression)?;
            self.route_fresh_source_child(child,route.channel)?;
            self.note_events[child.0.index].source_route=Some(SourceNoteRoute{owner,next:0,entered:true,..route});
            self.notes.get_mut(child.0).ok_or(Error::StaleHandle)?.pins+=1;
        }
        let last_order=self.order;
        // All real sibling owners exist before any callback or audio starts.
        // Scan their fixed note arena in actual admission order, without a Vec.
        let mut after=first_order;
        loop {
            let next=self.notes.slots.iter().enumerate().filter_map(|(index,slot)|{
                let child=slot.value.as_ref()?;
                (child.parent==Some(note)&&child.order>after&&child.order<=last_order&&self.note_events[index].source_route.is_some())
                    .then_some((child.order,NoteId(self.notes.id(index))))
            }).min_by_key(|row|row.0);
            let Some((order,child))=next else{break;};after=order;
            let result=self.advance_source_note(child);
            self.notes.get_mut(child.0).ok_or(Error::StaleHandle)?.pins-=1;
            if let Err(error)=result {
                // No external owner can observe a partially admitted sibling
                // batch before this synchronous call returns. Pending, silent
                // branch inputs have no musical output to preserve; Discarded
                // cancels their callback-lifetime Ready/Wait work too. Earlier
                // sounding outputs keep the existing caller fault/tail law.
                for index in 0..self.notes.slots.len(){
                    let Some(state)=self.notes.slots[index].value.as_ref()else{continue;};
                    if state.parent!=Some(note)||state.order<=first_order||state.order>last_order{continue;}
                    let pending=state.attack==crate::AttackStatus::Pending&&state.families==0&&state.processing_owners==0;
                    let unstarted=state.order>after;
                    let child=NoteId(self.notes.id(index));
                    if pending{self.release_now(child,crate::ReleaseCause::Discarded)?;}
                    if unstarted{self.notes.get_mut(child.0).ok_or(Error::StaleHandle)?.pins-=1;}
                }
                return Err(error);
            }
        }
        self.suppress_attack(note)?;
        if self.notes.get(note.0).ok_or(Error::StaleHandle)?.input.is_none(){self.retire_when_silent(note)?;}
        Ok(())
    }
}
