//! Bounded host identity, independent of a finite attack voice or MIDI key row.
use super::{EventId, Expression};
const _: () = assert!(crate::ksp::EVENT_CAPACITY <= u16::MAX as usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostNote {
    pub port: u8,
    pub channel: u8,
    pub key: u8,
    pub id: i32,
    /// CLAP supports a plugin-to-host NOTE_END; VST3 does not.
    pub clap: bool,
}

/// Each -1 axis is a host wildcard. Invalid axes never match an owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostPattern {
    pub port: i32,
    pub channel: i32,
    pub key: i32,
    pub id: i32,
    pub clap: bool,
}

impl HostPattern {
    pub fn matches(self, note: HostNote) -> bool {
        self.clap == note.clap && (self.port == -1 || self.port == i32::from(note.port))
            && (self.channel == -1 || self.channel == i32::from(note.channel))
            && (self.key == -1 || self.key == i32::from(note.key))
            && (self.id == -1 || self.id == note.id)
    }
}

/// An internal generation, never a host-provided note ID or a key index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostRef { index: u16, generation: u32 }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HostExpression { Gain(f32), Tune(f32), Pan(f32) }

#[derive(Clone, Copy)]
pub(super) struct Owner {
    pub note: HostNote,
    pub channel: u8,
    pub key: u8,
    pub velocity: u8,
    pub event: EventId,
    pub held: bool,
    pub silenced: bool,
    pub sostenuto: bool,
    pub start: u64,
    pub stop: Option<u64>,
    overlay: Expression,
    generation: u32,
    live: bool,
    pub pinned: bool,
}

pub(super) struct Owners {
    slots: Box<[Owner]>,
    free: Vec<u16>,
    pub active: Vec<HostRef>,
    /// Duplicate or full ownership cannot fall back to a different note.
    pub dropped: u64,
}

impl Owners {
    pub fn new() -> Self {
        let count = crate::ksp::EVENT_CAPACITY;
        let empty = Owner { note: HostNote { port: 0, channel: 0, key: 0, id: -1, clap: true },
            channel: 0, key: 0, velocity: 0, event: EventId(0), held: false,
            silenced: false, sostenuto: false, start: 0, stop: None, overlay: Expression::default(),
            generation: 0, live: false, pinned: false };
        Self { slots: vec![empty; count].into_boxed_slice(),
            free: (0..count as u16).rev().collect(), active: Vec::with_capacity(count), dropped: 0 }
    }

    pub fn get(&self, id: HostRef) -> Option<&Owner> {
        self.slots.get(id.index as usize).filter(|o| o.live && o.generation == id.generation)
    }
    pub fn get_mut(&mut self, id: HostRef) -> Option<&mut Owner> {
        self.slots.get_mut(id.index as usize).filter(|o| o.live && o.generation == id.generation)
    }
    pub fn admits(&self, note:HostNote) -> bool {
        !self.free.is_empty() && (note.id == -1 || !self.active.iter().any(|r| self.get(*r).is_some_and(|o| o.note == note)))
    }
    pub fn allocate(&mut self, note: HostNote, channel: u8, key: u8, velocity: u8, event: EventId, start: u64) -> Option<HostRef> {
        // Anonymous host notes can overlap. A concrete ID must remain unique
        // for its tuple’s entire sounding lifetime, including released voices.
        if note.id != -1 && self.active.iter().any(|r| self.get(*r).is_some_and(|o| o.note == note)) {
            self.dropped = self.dropped.saturating_add(1); return None;
        }
        let Some(index) = self.free.pop() else { self.dropped = self.dropped.saturating_add(1); return None };
        let slot = &mut self.slots[index as usize];
        let generation = slot.generation.wrapping_add(1).max(1);
        *slot = Owner { note, channel, key, velocity, event, held: true, silenced: false, sostenuto: false,
            start, stop: None, overlay: Expression::default(), generation, live: true, pinned: true };
        let id = HostRef { index, generation };
        self.active.push(id);
        Some(id)
    }
    pub fn overlay(&self, id: Option<HostRef>, mut expression: Expression) -> Expression {
        if let Some(o) = id.and_then(|id| self.get(id)) {
            expression.gain *= o.overlay.gain;
            expression.tune += o.overlay.tune;
            expression.pan += o.overlay.pan;
        }
        expression
    }
    pub fn change(&mut self, id: HostRef, expression: HostExpression) -> bool {
        let Some(o) = self.get_mut(id) else { return false };
        let before = o.overlay;
        match expression {
            HostExpression::Gain(v) if v.is_finite() && (0.0..=4.0).contains(&v) => o.overlay.gain = v,
            HostExpression::Tune(v) if v.is_finite() => o.overlay.tune = v,
            HostExpression::Pan(v) if v.is_finite() && (-1.0..=1.0).contains(&v) => o.overlay.pan = v,
            _ => {}
        }
        before != o.overlay
    }
    pub fn pin(&mut self, id: Option<HostRef>) {
        if let Some(o) = id.and_then(|id| self.get_mut(id)) { o.pinned = true; }
    }
    pub fn retire(&mut self, id: HostRef) {
        let Some(o) = self.get_mut(id) else { return };
        o.live = false;
        self.active.retain(|r| *r != id);
        self.free.push(id.index);
    }
    pub fn silence(&mut self, logical: Option<u8>, physical: Option<u16>, now: u64) {
        for &r in &self.active {
            let o = &mut self.slots[r.index as usize];
            if logical.is_none_or(|c| o.channel == c) && physical.is_none_or(|m| m & (1 << o.note.channel) != 0) {
                o.silenced = true; o.stop.get_or_insert(now);
            }
        }
    }
    pub fn reset(&mut self) {
        if self.active.is_empty() { return; }
        for &r in &self.active { self.slots[r.index as usize].live=false; }
        self.active.clear(); self.free.clear();
        self.free.extend((0..self.slots.len() as u16).rev());
    }
    pub fn stop(&mut self, logical:Option<u8>, physical:Option<u16>, now:u64) {
        self.silence(logical,physical,now);
        for &r in &self.active {
            let o=&mut self.slots[r.index as usize];
            if logical.is_none_or(|c| o.channel==c) && physical.is_none_or(|m| m & (1 << o.note.channel)!=0) { o.held=false; }
        }
    }
}

#[cfg(all(test,feature="plugin"))]
mod tests {
    use super::*;
    use crate::{audio::Sample, engine::{Bank, Engine}, import::{Group, Loop, Zone}};
    use std::path::PathBuf;

    fn note(id:i32) -> HostNote { HostNote { port:0, channel:4, key:60, id, clap:true } }
    fn pattern(id:i32) -> HostPattern { HostPattern { port:0, channel:4, key:60, id, clap:true } }
    fn bank(finite:bool) -> Bank {
        let attack = PathBuf::from("attack"); let tail = PathBuf::from("tail");
        Bank::from_samples(vec![Group::default(),Group { release_trigger:true,..Default::default() }],
            vec![Zone { sample:attack.clone(),root:48,loop_range:(!finite).then_some(Loop { start:0,end:128,alternating:false,until_release:false,crossfade:0 }),..Default::default() },
                Zone { group:1,sample:tail.clone(),root:48,..Default::default() }],
            vec![(attack,Sample { rate:48000,frames:vec![[0.2;2];128] }),
                (tail,Sample { rate:48000,frames:vec![[0.1;2];4800] })]).unwrap()
    }
    fn engine(finite:bool) -> Engine {
        let mut e = Engine::default(); e.reset(48000.); e.attack=0.0001; e.release=0.03;
        e.set_bank(Some(Box::new(bank(finite)))); e
    }
    fn render(e:&mut Engine, blocks:usize) {
        let (mut l,mut r)=([0.;128],[0.;128]);
        for _ in 0..blocks { e.render(&mut l,&mut r); }
    }

    #[test]
    fn exact_roots_keep_finite_attacks_pedals_children_and_late_expressions_without_heap() {
        let mut e=engine(false);
        assert_eq!(crate::test_support::allocations(|| {
            assert!(e.host_note_on(note(10),7,48,100,0.)); render(&mut e,1);
            e.host_note_off(pattern(10));
            assert!(e.host_note_on(note(11),7,48,100,0.));
            e.host_expression(pattern(10),HostExpression::Gain(0.)); render(&mut e,2);
            let old=e.player.voices.iter().filter(|v| e.player.host_notes.get(v.host_note.unwrap()).unwrap().note.id==10);
            for v in old { assert!(v.gains.iter().all(|x| x.abs()<1e-7)); }
            assert!(e.player.voices.iter().any(|v| e.player.host_notes.get(v.host_note.unwrap()).unwrap().note.id==11 && v.gains[0]>0.01));
            assert!(e.key_down(7,48));
            e.mark_host_notes(); assert!(e.host_note_pending(note(10))); assert!(e.host_note_pending(note(11)));
            // Old-id choke does not cancel the new same-key owner.
            e.host_note_choke(pattern(10)); render(&mut e,4); e.mark_host_notes();
            assert!(!e.host_note_pending(note(10))); assert!(e.host_note_pending(note(11)));
            let stale=e.player.host_notes.active[0]; e.retire_host_note(note(10));
            assert!(e.host_note_on(note(12),7,48,100,0.));
            assert!(!e.player.host_notes.change(stale,HostExpression::Gain(0.)));
            e.host_note_off(HostPattern { id:-1,..pattern(10) }); render(&mut e,64); e.mark_host_notes();
            assert!(!e.host_note_pending(note(11))); assert!(!e.host_note_pending(note(12)));
        }),0);

        let mut e=engine(true);
        assert_eq!(crate::test_support::allocations(|| {
            assert!(e.host_note_on(note(20),7,48,100,0.)); render(&mut e,8);
            assert_eq!(e.active_voices(),0); e.mark_host_notes(); assert!(e.host_note_pending(note(20)));
            e.cc(7,66,127); e.host_note_off(pattern(20));
            assert_eq!(e.player.pending_releases.len(),1); e.mark_host_notes(); assert!(e.host_note_pending(note(20)));
            e.host_expression(pattern(20),HostExpression::Tune(12.));
            e.cc(7,66,0); render(&mut e,1);
            let voice=e.player.voices.iter().find(|v| v.release_trigger).unwrap();
            assert_eq!(e.player.host_notes.get(voice.host_note.unwrap()).unwrap().note.id,20);
            assert!((voice.pitch.1-2.).abs()<1e-6);
            e.cc(7,120,0); render(&mut e,8); e.mark_host_notes(); assert!(!e.host_note_pending(note(20)));
        }),0);

        let mut e=engine(false);
        e.set_mpe_zone(Some((0,1<<1)));
        let first=HostNote { channel:1,..note(25) };
        let old=HostPattern { channel:1,..pattern(25) };
        assert_eq!(crate::test_support::allocations(|| {
            e.set_expression_on(1,48,Expression { tune:2.,..Default::default() });
            assert!(e.host_note_on(first,1,48,100,0.)); render(&mut e,1);
            e.host_note_off(old);
            e.set_expression_on(1,48,Expression { tune:8.,..Default::default() });
            assert!(e.host_note_on(HostNote { id:26,..first },1,48,100,0.));
            e.host_expression(old,HostExpression::Tune(12.)); render(&mut e,1);
            for voice in &e.player.voices {
                let id=e.player.host_notes.get(voice.host_note.unwrap()).unwrap().note.id;
                let semitones=if id==25 { 14. } else { 8. };
                assert!((voice.pitch.1-2f64.powf(semitones/12.)).abs()<1e-6,"MPE member reuse replaced the old owner default");
            }
        }),0);

        let mut e=engine(false);
        let script="on note\nignore_event($EVENT_ID)\nplay_note($EVENT_NOTE+12,100,0,-1)\nwait(10000)\nplay_note($EVENT_NOTE+13,100,0,20000)\nend on\non release\nwait(5000)\nplay_note(72,100,0,0)\nend on";
        let (rt,errors)=crate::ksp::Runtime::with_scripts(&[script],&mut crate::ksp::LogEngine::default(),0,Vec::new());
        assert!(errors.iter().all(Option::is_none)); e.set_script(Some(Box::new(rt)));
        assert_eq!(crate::test_support::allocations(|| {
            assert!(e.host_note_on(note(30),7,48,100,0.)); render(&mut e,1);
            e.host_note_off(pattern(30));
            assert!(e.host_note_on(note(31),7,48,100,0.));
            e.host_expression(pattern(30),HostExpression::Gain(0.)); render(&mut e,8); e.mark_host_notes();
            assert!(e.host_note_pending(note(30)),"waiting root and independent child retain NOTE_END ownership");
            assert!(e.script().unwrap().key_down_from(4,7,48));
            assert!(e.player.voices.iter().any(|v| e.player.host_notes.get(v.host_note.unwrap()).unwrap().note.id==31));
            e.host_note_choke(pattern(30)); render(&mut e,16); e.mark_host_notes();
            assert!(!e.host_note_pending(note(30))); assert!(e.host_note_pending(note(31)));
            e.panic(); render(&mut e,32); e.mark_host_notes(); assert!(!e.host_note_pending(note(31)));
            assert!(e.host_note_on(note(32),7,48,100,0.)); render(&mut e,1);
            e.all_sound_off_from(7,1<<4); render(&mut e,8); e.mark_host_notes();
            assert!(e.host_note_pending(note(32)),"CC120 retains the physical root for its eventual cleanup");
            e.host_note_off(pattern(32)); render(&mut e,32); e.mark_host_notes();
            assert!(!e.host_note_pending(note(32)),"exact key-up failed to clean a silenced root");
            assert_eq!(e.active_voices(),0,"late key-up resurrected a stopped child/release");
            e.reset(48000.);
            assert!(e.host_note_on(note(31),7,48,100,0.),"host reset must allow the same ID immediately");
        }),0);
    }

    #[test]
    fn budget_paused_scoped_cleanup_keeps_live_key_visibility_without_heap() {
        let script="on init\ndeclare $i\ndeclare $b := -1\ndeclare $c := -1\nmake_persistent($i)\nmake_persistent($b)\nmake_persistent($c)\nend on\non release\nif ($EVENT_NOTE=48)\nwhile ($i<2000)\ninc($i)\nend while\n$b := %KEY_DOWN[50]\n$c := %KEY_DOWN[52]\nend if\nend on";
        for full_sound_off in [false,true] {
            let (rt,errors)=crate::ksp::Runtime::with_scripts(&[script],&mut crate::ksp::LogEngine::default(),0,Vec::new());
            assert!(errors.iter().all(Option::is_none));
            let mut e=engine(false); e.set_script(Some(Box::new(rt)));
            let b=HostNote { key:62,..note(11) }; let bp=HostPattern { key:62,..pattern(11) };
            let c=HostNote { key:64,..note(12) };
            assert!(e.host_note_on(note(10),7,48,100,0.)); assert!(e.host_note_on(b,7,50,100,0.)); render(&mut e,1);
            let spent=e.script().unwrap().env.spent;
            assert_eq!(crate::test_support::allocations(|| {
                // Offline mode removes wall-clock variability; the fixed fuel
                // forces A's release to yield inside its loop, before reads.
                e.begin_audio_block(128,1,true);
                e.script.as_deref_mut().unwrap().env.block_fuel=64;
                if full_sound_off {
                    e.host_note_off(pattern(10)); e.cc(7,120,0);
                } else { e.host_note_choke(pattern(10)); }
                assert_eq!(e.script().unwrap().env.block_fuel,0);
                assert!(e.script().unwrap().env.spent>spent);
                e.host_note_off(bp);
                assert!(e.host_note_on(c,7,52,100,0.));
                e.mark_host_notes(); assert!(e.host_note_pending(note(10)));
            }),0);
            let paused=e.script().unwrap().persistence();
            assert!(matches!(paused[0]["$i"],crate::ksp::Value::Int(i) if i>0 && i<2000));
            assert_eq!(paused[0]["$b"],crate::ksp::Value::Int(-1));
            assert_eq!(paused[0]["$c"],crate::ksp::Value::Int(-1));
            assert_eq!(crate::test_support::allocations(|| {
                e.begin_audio_block(128,1,true); render(&mut e,4);
                assert!(e.key_down(7,52));
            }),0);
            let saved=e.script().unwrap().persistence();
            assert_eq!(saved[0]["$b"],crate::ksp::Value::Int(0));
            assert_eq!(saved[0]["$c"],crate::ksp::Value::Int(i32::from(!full_sound_off)),"scoped input visibility leaked across budget pause, or full sound-off lost suppression");
        }
    }

    #[test]
    fn scoped_choke_exposes_unselected_keys_and_bank_only_replacement_closes_waits() {
        let script="on release\nif ($EVENT_NOTE=48)\nmessage(%KEY_DOWN[50])\nend if\nend on";
        let (rt,errors)=crate::ksp::Runtime::with_scripts(&[script],&mut crate::ksp::LogEngine::default(),0,Vec::new());
        assert!(errors.iter().all(Option::is_none));
        let mut e=engine(false); e.set_script(Some(Box::new(rt)));
        assert_eq!(crate::test_support::allocations(|| {
            assert!(e.host_note_on(note(10),7,48,100,0.));
            assert!(e.host_note_on(HostNote { key:62,..note(11) },7,50,100,0.)); render(&mut e,1);
            e.host_note_choke(pattern(10));
            assert_eq!(e.script().unwrap().last_message(),"1","scoped cleanup hid another held key from on release");
            assert!(e.script().unwrap().key_down_from(4,7,50));
            assert!(e.key_down(7,50));
        }),0);
        let script="on note\nignore_event($EVENT_ID)\nwait(100000)\nplay_note($EVENT_NOTE,100,0,-1)\nend on\non release\nwait(10000)\nplay_note(72,100,0,0)\nend on";
        let (rt,errors)=crate::ksp::Runtime::with_scripts(&[script],&mut crate::ksp::LogEngine::default(),0,Vec::new());
        assert!(errors.iter().all(Option::is_none));
        let mut e=engine(false); e.set_script(Some(Box::new(rt)));
        let mut old=None;
        assert_eq!(crate::test_support::allocations(|| {
            assert!(e.host_note_on(note(20),7,48,100,0.)); render(&mut e,1);
            e.mark_host_notes(); assert!(e.host_note_pending(note(20)));
            old=e.set_bank(None);
            render(&mut e,64); e.mark_host_notes();
            assert!(!e.script().unwrap().key_down_from(4,7,48));
            assert!(!e.host_note_pending(note(20)),"bank-only replacement retained an old held/waiting root");
            assert_eq!(e.active_voices(),0);
            e.host_note_off(pattern(20)); e.mark_host_notes();
            assert!(!e.host_note_pending(note(20)));
        }),0);
        assert!(old.is_some());
    }

    #[test]
    fn anonymous_key_up_preserves_exact_roots_but_exact_wildcard_releases_them() {
        for scripted in [false,true] {
            let mut e=engine(false);
            if scripted {
                let (rt,errors)=crate::ksp::Runtime::with_scripts(&["on note\nend on"],&mut crate::ksp::LogEngine::default(),0,Vec::new());
                assert!(errors.iter().all(Option::is_none)); e.set_script(Some(Box::new(rt)));
            }
            assert_eq!(crate::test_support::allocations(|| {
                assert!(e.host_note_on(note(10),7,48,100,0.));
                assert!(e.host_note_on(note(11),7,48,100,0.));
                e.note_on_from(7,4,48,100); render(&mut e,1);
                e.note_off_from(7,4,48); render(&mut e,1);
                assert!(e.key_down(7,48),"anonymous MIDI key-up cleared an exact root");
                assert!(e.host_key_held(4,60));
                for id in [10,11] {
                    assert!(e.player.voices.iter().any(|v| v.held && !v.released
                        && v.host_note.and_then(|r| e.player.host_notes.get(r)).is_some_and(|o| o.note.id==id)),"anonymous key-up released exact id {id}");
                }
                assert!(e.player.voices.iter().filter(|v| v.host_note.is_none()).all(|v| !v.held));
                if let Some(rt)=e.script() { assert!(rt.key_down_from(4,7,48)); }
                // CLAP id=-1 explicitly matches every exact owner on the PCK.
                e.host_note_off(HostPattern { id:-1,..pattern(10) }); render(&mut e,1);
                assert!(!e.host_key_held(4,60)); assert!(!e.key_down(7,48));
                assert!(e.player.voices.iter().all(|v| !v.held || v.release_trigger));
            }),0);
        }
    }

    #[test]
    fn ownership_exhaustion_and_duplicate_tuples_do_not_steal_generations() {
        let mut owners=Owners::new();
        assert!(HostPattern { port:-1,channel:-1,key:-1,id:-1,clap:true }.matches(note(10)));
        assert!(!HostPattern { port:1,..pattern(10) }.matches(note(10)));
        assert!(!HostPattern { clap:false,..pattern(10) }.matches(note(10)));
        let first=owners.allocate(note(0),7,48,100,EventId(1),0).unwrap();
        assert!(owners.allocate(HostNote { channel:5,..note(0) },7,48,100,EventId(2),0).is_some(),"ID alone is not a complete host tuple");
        assert!(owners.allocate(note(0),7,48,100,EventId(3),0).is_none());
        assert_eq!(crate::test_support::allocations(|| {
            for id in 1..crate::ksp::EVENT_CAPACITY-1 { assert!(owners.allocate(note(id as i32),7,48,100,EventId(id as u32),0).is_some()); }
            assert!(owners.allocate(note(99999),7,48,100,EventId(4),0).is_none());
            assert_eq!(owners.dropped,2); assert_eq!(owners.get(first).unwrap().note,note(0));
            owners.retire(first);
            let next=owners.allocate(note(99999),7,48,100,EventId(4),0).unwrap();
            assert_ne!(next,first); assert!(owners.get(first).is_none());
        }),0);
    }
}
