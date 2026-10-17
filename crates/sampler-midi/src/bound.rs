//! Explicit Core receiver capability; no mutable protocol state lives here.
use crate::{Applied,ApplyError,Packet};
use sampler_core::{Error,Runtime,RuntimeId,ReceiverId,Input,NoteId,OwnedReceiver};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct BoundMpe { runtime:RuntimeId, receiver:ReceiverId, manager:u8 }
impl BoundMpe {
    pub(crate) fn new(runtime:RuntimeId,receiver:ReceiverId,manager:u8)->Self {Self{runtime,receiver,manager}}
    pub fn id(self)->ReceiverId {self.receiver}
    /// Selected Lower/Upper profile is immutable decoder routing, finalized
    /// before binding; MCM changes membership, never this selected manager.
    pub fn manager_channel(&self)->u8{self.manager}
    fn owner<'r>(&self,runtime:&'r Runtime)->Result<&'r OwnedReceiver,Error>{
        if runtime.id()!=self.runtime{return Err(Error::StaleHandle);}runtime.receiver(self.receiver)
    }
    pub fn configuration(&self,runtime:&Runtime)->Option<[u8;2]>{self.owner(runtime).ok().and_then(OwnedReceiver::configuration)}
    pub fn accepts_channel(&self,runtime:&Runtime,channel:u8)->bool{self.owner(runtime).is_ok_and(|owner|owner.accepts_channel(channel))}
    pub fn pitch_ranges(&self,runtime:&Runtime)->Result<(u8,u8),Error>{Ok(self.owner(runtime)?.pitch_ranges())}
    pub fn pitch_sensitivities(&self,runtime:&Runtime)->Result<(f64,f64),Error>{Ok(self.owner(runtime)?.pitch_sensitivities())}
    pub fn pitch_sensitivities_for(&self,runtime:&Runtime,version:crate::Version)->Result<(f64,f64),Error>{Ok(self.owner(runtime)?.pitch_sensitivities_for(version))}
    pub fn channel_pitch_sensitivity(&self,runtime:&Runtime,version:crate::Version,channel:u8)->Result<f64,Error>{self.owner(runtime)?.channel_pitch_sensitivity(version,channel)}
    pub fn set_manual_active(&self,runtime:&mut Runtime,active:bool)->Result<(),Error>{runtime.set_receiver_manual_active(self.receiver,active)}
    pub fn set_timbre_controller(&self,runtime:&mut Runtime,controller:Option<u8>)->Result<(),Error>{runtime.set_receiver_timbre_controller(self.receiver,controller)}
    pub fn set_bend_range(&self,runtime:&mut Runtime,range:Option<u8>)->Result<(),Error>{runtime.set_receiver_bend_range(self.receiver,range)}
    pub fn apply(&self,runtime:&mut Runtime,packet:Packet<'_>)->Result<Applied,ApplyError>{runtime.apply_receiver(self.receiver,packet.channel_voice(),false,None)}
    pub fn apply_silent(&self,runtime:&mut Runtime,packet:Packet<'_>)->Result<Applied,ApplyError>{runtime.apply_receiver(self.receiver,packet.channel_voice(),true,None)}
    pub fn apply_with_source(&self,runtime:&mut Runtime,packet:Packet<'_>,source:Input)->Result<Applied,ApplyError>{runtime.apply_receiver(self.receiver,packet.channel_voice(),false,Some(source))}
    pub fn apply_silent_with_source(&self,runtime:&mut Runtime,packet:Packet<'_>,source:Input)->Result<Applied,ApplyError>{runtime.apply_receiver(self.receiver,packet.channel_voice(),true,Some(source))}
    pub fn trigger(&self,runtime:&mut Runtime,channel:u8,input:Input,velocity:f64)->Result<NoteId,Error>{runtime.trigger_receiver(self.receiver,channel,input,None,velocity,0.,false)}
    pub fn trigger_with_tuning(&self,runtime:&mut Runtime,channel:u8,input:Input,velocity:f64,tuning:f64)->Result<NoteId,Error>{runtime.trigger_receiver(self.receiver,channel,input,None,velocity,tuning,false)}
    pub fn trigger_silent(&self,runtime:&mut Runtime,channel:u8,input:Input,velocity:f64)->Result<NoteId,Error>{runtime.trigger_receiver(self.receiver,channel,input,None,velocity,0.,true)}
    pub fn trigger_silent_with_tuning(&self,runtime:&mut Runtime,channel:u8,input:Input,velocity:f64,tuning:f64)->Result<NoteId,Error>{runtime.trigger_receiver(self.receiver,channel,input,None,velocity,tuning,true)}
    pub fn trigger_with_source(&self,runtime:&mut Runtime,channel:u8,input:Input,source:Input,velocity:f64,tuning:f64)->Result<NoteId,Error>{runtime.trigger_receiver(self.receiver,channel,input,Some(source),velocity,tuning,false)}
    pub fn trigger_silent_with_source(&self,runtime:&mut Runtime,channel:u8,input:Input,source:Input,velocity:f64,tuning:f64)->Result<NoteId,Error>{runtime.trigger_receiver(self.receiver,channel,input,Some(source),velocity,tuning,true)}
    pub fn set_note_tuning(&self,runtime:&mut Runtime,note:NoteId,tuning:f64)->Result<(),Error>{runtime.set_receiver_note_tuning(self.receiver,note,tuning)}
    pub fn transpose(&self,runtime:&mut Runtime,semitones:f64)->Result<usize,Error>{runtime.transpose_receiver(self.receiver,semitones)}
    pub fn native_physical_slots(&self,runtime:&Runtime)->usize{self.owner(runtime).map_or(0,OwnedReceiver::native_physical_slots)}
    pub fn native_physical_at(&self,runtime:&Runtime,index:usize)->Option<(NoteId,Input,bool)>{self.owner(runtime).ok().and_then(|owner|owner.native_physical_at(index))}
    pub fn has_native_physical_note(&self,runtime:&Runtime,note:NoteId)->bool{runtime.receiver_has_native_note(self.receiver,note)}
    pub fn has_native_physical_source(&self,runtime:&Runtime,source:Input)->bool{self.owner(runtime).is_ok_and(|owner|owner.has_native_physical_source(source))}
    pub fn validate_native_physical_releases(&self,runtime:&Runtime,matches:impl Fn(NoteId,Input)->bool)->Result<(),Error>{runtime.validate_receiver_native_releases(self.receiver,matches)}
    pub fn release_native_physical(&self,runtime:&mut Runtime,matches:impl Fn(NoteId,Input)->bool,velocity:Option<f64>)->Result<usize,Error>{runtime.release_receiver_native(self.receiver,matches,velocity)}
    pub fn choke_native_physical(&self,runtime:&mut Runtime,matches:impl Fn(NoteId,Input)->bool)->Result<usize,Error>{runtime.choke_receiver_native(self.receiver,matches)}
    pub fn validate_all_physical_releases(&self,runtime:&Runtime)->Result<(),Error>{runtime.validate_receiver_all_releases(self.receiver)}
    pub fn queue_all_physical_releases(&self,runtime:&mut Runtime)->Result<usize,Error>{runtime.queue_receiver_all_releases(self.receiver)}
    pub fn next_terminal_release(&self,runtime:&mut Runtime)->Result<Option<sampler_core::ReceiverTerminalDelivery>,Error>{runtime.next_receiver_terminal(self.receiver)}
    pub fn complete_terminal_release(&self,runtime:&mut Runtime,delivery:sampler_core::ReceiverTerminalDelivery)->Result<(),Error>{runtime.complete_receiver_terminal(delivery)}
    pub fn unbind(self,runtime:&mut Runtime)->Result<crate::Mpe,Error>{runtime.unbind_receiver(self.receiver).map(|state|crate::Mpe{state})}
}
