#!/usr/bin/env python3
"""Compile the actual first-failure source with std-only endpoint stubs; no Cargo build."""
import pathlib
import subprocess
import tempfile

root = pathlib.Path(__file__).resolve().parents[1]
plugin = (root / 'src/plugin.rs').read_text()
uvi = (root / 'src/plugin/uvi.rs').read_text()
shared = plugin[plugin.index('#[derive(Default)]\npub(crate) struct PartShared'):plugin.index('pub struct Shared {')]
protocol = uvi[uvi.index('#[derive(Debug, Clone, Copy, PartialEq, Eq)]'):uvi.index('#[derive(Clone, Copy)]\nstruct Owner')]
# These are the production packet/bridge error declarations, including all variants.
bridge_path = root / 'src/uvi/bridge.rs'
worker_path = root / 'src/uvi/worker.rs'
if not bridge_path.exists():
    raise SystemExit('Run in a complete repository snapshot (needs src/uvi/bridge.rs and worker.rs).')
bridge = bridge_path.read_text()
worker = worker_path.read_text()
bridge_enum = bridge[bridge.index('#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub enum BridgeError'):bridge.index('#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]')]
packet_enum = worker[worker.index('#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum PacketError'):worker.index('impl std::fmt::Display for PacketError')]
def method(source, signature):
    start = source.index(signature)
    opening = source.index('{', start)
    depth = 1
    end = opening + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end]

slot_fail = method(uvi, '    #[track_caller]\n    fn fail(')
slot_origin = method(uvi, '    pub(crate) fn error_origin(')
source = '''#![allow(dead_code)]
use std::sync::{Arc, atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering}};
const OUTS: usize = 2;
'''+packet_enum+bridge_enum+'''\nmod uvi {
use super::{BridgeError,PacketError};
'''+protocol+'''\n}
mod uvi_control {
use super::uvi;
pub(crate) struct Slot { pub(crate) error: Option<(uvi::Error,u64,u32)> }
impl Slot { pub(crate) fn error_origin(&self)->Option<(uvi::Error,u64,u32)> { self.error } }
pub(crate) struct Audio { pub(crate) epoch:u64, pub(crate) generation:u64, pub(crate) part_generation:u64, pub(crate) slot:Slot }
impl Audio {
 pub(crate) fn epoch(&self)->u64 { self.epoch }
 pub(crate) fn generation(&self)->u64 { self.generation }
 pub(crate) fn part_generation(&self)->u64 { self.part_generation }
 pub(crate) fn slot(&self)->&Slot { &self.slot }
}
}
'''+shared+'''
mod actual_slot_failure {
 use super::{uvi::Error, BridgeError, PacketError};
 #[derive(Default)] pub(crate) struct Bridge { error:Option<BridgeError> }
 impl Bridge { fn abort(&mut self,error:BridgeError) {self.error.get_or_insert(error);} }
 #[derive(Default)] pub(crate) struct Notes { aborted:bool }
 impl Notes {fn abort(&mut self) {self.aborted=true;} }
 #[derive(Default)] pub(crate) struct Slot {error:Option<Error>,frame:u64,error_frame:u64,error_line:u32,bridge:Bridge,notes:Notes}
 impl Slot {
'''+slot_fail+slot_origin+'''
 }
 #[test] fn actual_slot_retains_original_frame_line_and_error() {
  let mut slot=Slot {frame:23,..Default::default()};
  let error=Error::Bridge(BridgeError::Worker(PacketError::Full));
  assert_eq!(slot.fail(error),error);
  let first=slot.error_origin().unwrap();
  assert_eq!((first.0,first.1),(error,23));
  assert!(first.2>0);
  slot.frame=600;
  assert_eq!(slot.fail(Error::Aborted),error);
  assert_eq!(slot.error_origin(),Some(first));
  assert!(slot.notes.aborted);
 }
}

fn audio(epoch:u64,generation:u64,part_generation:u64)->uvi_control::Audio {
 uvi_control::Audio { epoch,generation,part_generation,slot:uvi_control::Slot {error:None} }
}
fn adopt(atoms:&PartShared, generation:u64, part_generation:u64) {
 atoms.uvi_generation.store(0,Ordering::Release);
 atoms.uvi_failure.reset();
 atoms.uvi_failed.store(false,Ordering::Release);
 atoms.uvi_part_generation.store(part_generation,Ordering::Release);
 atoms.uvi_generation.store(generation,Ordering::Release);
}
#[test] fn error_and_stage_roundtrip() {
 use uvi::{Error as E,FailureStage as S};
 let errors=[E::InvalidInput,E::Capacity,E::DuplicateNote,E::TokenExhausted,E::UnsupportedMpe,E::UnsupportedExpression,E::UnsupportedChoke,E::UnsupportedInitialTuning,E::Aborted,E::UnsupportedRouterInput,
 E::Bridge(BridgeError::InvalidConfig),E::Bridge(BridgeError::NotReady),E::Bridge(BridgeError::InvalidBuffer),E::Bridge(BridgeError::TimelineOverflow),E::Bridge(BridgeError::RequestCapacity),
 E::Bridge(BridgeError::Worker(PacketError::WrongEpoch)),E::Bridge(BridgeError::Worker(PacketError::WrongGeneration)),E::Bridge(BridgeError::Worker(PacketError::WrongFrame)),E::Bridge(BridgeError::Worker(PacketError::InvalidInput)),E::Bridge(BridgeError::Worker(PacketError::TooManyInputs)),E::Bridge(BridgeError::Worker(PacketError::Full)),E::Bridge(BridgeError::Worker(PacketError::Underrun)),E::Bridge(BridgeError::Worker(PacketError::Failed)),E::Bridge(BridgeError::Worker(PacketError::Stopped)),E::Bridge(BridgeError::Worker(PacketError::PortTaken))];
 for error in errors { assert_eq!(E::from_code(error.code()),Some(error)); }
 let unique:std::collections::HashSet<_>=errors.iter().map(|e|e.code()).collect();
 assert_eq!(unique.len(),errors.len());
 for invalid in [0,11,31,37,63,74,u32::MAX] { assert_eq!(E::from_code(invalid),None); }
 for stage in [S::Feed,S::Router,S::Completions,S::Transport,S::UiEdit,S::AuditionRelease,S::AuditionNote,S::AuditionEnd,S::Process,S::Panic] {
  let atoms=uvi::FailureAtoms::default();
  let failure=uvi::Failure {error:E::Capacity,frame:u64::MAX,stage,epoch:7,generation:8,source:"src/plugin/uvi.rs",line:999};
  assert!(atoms.record(failure));
  assert_eq!(atoms.snapshot(7,8),Some(failure));
  assert!(!stage.as_str().is_empty());
 }
}
#[test] fn actual_shared_first_wins_reset_and_stale_endpoint() {
 use uvi::{Error as E,FailureStage as S};
 let atoms=PartShared::default();
 adopt(&atoms,20,3);
 let mut endpoint=audio(10,20,3);
 endpoint.slot.error=Some((E::Bridge(BridgeError::Worker(PacketError::Full)),12345,321));
 atoms.record_uvi_failure(&endpoint,E::Aborted,S::Process,99999);
 let first=atoms.uvi_failure(10,20).unwrap();
 assert_eq!(first.error,E::Bridge(BridgeError::Worker(PacketError::Full)));
 assert_eq!((first.frame,first.source,first.line),(12345,"src/plugin/uvi.rs",321));
 endpoint.slot.error=Some((E::UnsupportedMpe,54321,123));
 atoms.record_uvi_failure(&endpoint,E::InvalidInput,S::Feed,100000);
 assert_eq!(atoms.uvi_failure(10,20),Some(first));
 assert_eq!(atoms.uvi_failure(11,20),None);
 assert_eq!(atoms.uvi_failure(10,21),None);
 adopt(&atoms,21,4);
 assert!(!atoms.uvi_failed.load(Ordering::Acquire));
 assert_eq!(atoms.uvi_failure(10,20),None);
 atoms.record_uvi_failure(&endpoint,E::InvalidInput,S::Feed,42);
 assert!(!atoms.uvi_failed.load(Ordering::Acquire));
 let endpoint=audio(11,21,4);
 atoms.record_uvi_failure(&endpoint,E::UnsupportedRouterInput,S::Router,u64::MAX);
 let next=atoms.uvi_failure(11,21).unwrap();
 assert_eq!((next.error,next.frame,next.stage,next.source),(E::UnsupportedRouterInput,u64::MAX,S::Router,"src/plugin.rs"));
 assert!(next.line>0);
}
#[test] fn published_flag_exposes_complete_cause() {
 use uvi::{Error as E,FailureStage as S};
 for generation in 1..=128 {
  let atoms=Arc::new(PartShared::default());
  adopt(&atoms,generation,generation);
  let writer=atoms.clone();
  let thread=std::thread::spawn(move|| {
   let mut endpoint=audio(generation+1000,generation,generation);
   endpoint.slot.error=Some((E::Bridge(BridgeError::Worker(PacketError::TooManyInputs)),u64::MAX-generation,4321));
   writer.record_uvi_failure(&endpoint,E::Aborted,S::Transport,0);
  });
  while !atoms.uvi_failed.load(Ordering::Acquire) { std::hint::spin_loop(); }
  let result=atoms.uvi_failure(generation+1000,generation).unwrap();
  assert_eq!((result.frame,result.line,result.stage),(u64::MAX-generation,4321,S::Transport));
  assert_eq!(result.error,E::Bridge(BridgeError::Worker(PacketError::TooManyInputs)));
  thread.join().unwrap();
 }
}
#[test] fn concurrent_replacement_never_mixes_generations() {
 use uvi::{Error as E,FailureStage as S};
 let atoms=Arc::new(PartShared::default());
 let writer=atoms.clone();
 let done=Arc::new(AtomicBool::new(false));
 let writer_done=done.clone();
 let thread=std::thread::spawn(move|| {
  for generation in 1..=100000 {
   adopt(&writer,generation,generation);
   let mut endpoint=audio(generation+1,generation,generation);
   endpoint.slot.error=Some((E::Capacity,generation*17,777));
   writer.record_uvi_failure(&endpoint,E::Capacity,S::Feed,0);
  }
  writer_done.store(true,Ordering::Release);
 });
 while !done.load(Ordering::Acquire) {
  let generation=atoms.uvi_generation.load(Ordering::Acquire);
  if let Some(fault)=atoms.uvi_failure(generation+1,generation) {
   assert_eq!(fault.frame,generation*17);
   assert_eq!(fault.epoch,generation+1);
   assert_eq!(fault.line,777);
  }
 }
 thread.join().unwrap();
}
'''
with tempfile.TemporaryDirectory(prefix='kontakto-uvi-fault-') as temp:
    rust = pathlib.Path(temp) / 'fault.rs'
    binary = pathlib.Path(temp) / 'fault-tests'
    rust.write_text(source)
    subprocess.run(['rustc','--edition=2024','--test','--cfg','feature="uvi"',str(rust),'-o',str(binary)],check=True)
    subprocess.run([str(binary)],check=True)
