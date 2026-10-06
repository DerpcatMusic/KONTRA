//! Audio-thread heap guard: after preparation, realistic playback at full
//! polyphony must neither allocate nor free on the rendering thread.
use kontakto::{
    audio::Sample,
    engine::{Bank, Engine, MAX_BLOCK, Rack, load_scripts},
    import::{Group, Instrument, Loop, Zone},
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::PathBuf;

struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    // Per thread, so tests running in parallel do not count each other.
    static CALLS: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    if COUNTING.with(Cell::get) {
        CALLS.with(|n| n.set(n.get() + 1));
    }
}

// SAFETY: forwards every call unchanged to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count();
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Fails if `f` allocates, reallocates or frees on this thread.
fn without_heap(what: &str, f: impl FnOnce()) {
    let before = CALLS.with(Cell::get);
    COUNTING.with(|c| c.set(true));
    f();
    COUNTING.with(|c| c.set(false));
    let calls = CALLS.with(Cell::get) - before;
    assert_eq!(calls, 0, "{what}: the audio thread made {calls} heap calls");
}

const BLOCK: usize = MAX_BLOCK;

/// `groups` layered groups, each one looping zone over every key.
fn instrument(groups: usize, scripts: Vec<String>) -> Instrument {
    let zones = (0..groups)
        .map(|g| Zone {
            group: g,
            sample: PathBuf::from(g.to_string()),
            loop_range: Some(Loop {
                start: 0,
                end: 4000,
                alternating: g % 2 == 1,
                until_release: false,
                crossfade: 0,
            }),
            ..Zone::default()
        })
        .collect();
    Instrument {
        name: "heap guard".into(),
        groups: vec![Group::default(); groups],
        zones,
        scripts,
        ..Default::default()
    }
}

fn bank(instrument: &Instrument, polyphony: usize) -> Bank {
    let samples = (0..instrument.groups.len())
        .map(|g| {
            let frames = (0..4000)
                .map(|i| {
                    let x = (i as f32 * (0.01 + g as f32 * 0.003)).sin() * 0.1;
                    [x, -x]
                })
                .collect();
            (PathBuf::from(g.to_string()), Sample { rate: 44100, frames })
        })
        .collect();
    let mut bank =
        Bank::from_samples(instrument.groups.clone(), instrument.zones.clone(), samples).unwrap();
    bank.set_polyphony(polyphony);
    bank
}

fn engine(groups: usize, polyphony: usize, script: Option<&str>) -> Engine {
    let scripts = script.map(|s| vec![s.to_owned()]).unwrap_or_default();
    let instrument = instrument(groups, scripts);
    let mut e = Engine::default();
    e.attack = 0.002;
    e.release = 0.05;
    e.reset(48000.0);
    e.set_bank(Some(Box::new(bank(&instrument, polyphony))));
    if script.is_some() {
        let (rt, errors) = load_scripts(&instrument, Vec::new(), 48000.0);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(e.set_script(rt).is_none());
    }
    e
}

/// Chords on all 16 channels with sustain and sostenuto, bends, pressure and
/// CCs, so voices pile up past the polyphony limit and get stolen.
fn perform(e: &mut Engine, left: &mut [f32; BLOCK], right: &mut [f32; BLOCK], rounds: usize) {
    for round in 0..rounds {
        e.begin_audio_block(BLOCK, 1, false);
        let channel = (round % 16) as u8;
        if round % 8 == 0 {
            e.cc(channel, 64, 127);
        }
        if round % 13 == 0 {
            e.cc(channel, 66, 127);
        }
        for i in 0..12u8 {
            let note = 24 + ((round as u8).wrapping_mul(7).wrapping_add(i * 5) % 96);
            e.note_on(channel, note, 40 + (i * 7) % 87);
        }
        e.pitch_bend(channel, (round as u16 * 517) & 0x3fff);
        e.channel_pressure(channel, (round % 128) as u8);
        e.cc(channel, 1, (round % 128) as u8);
        e.render(left, right);
        for i in 0..12u8 {
            let note = 24 + ((round as u8).wrapping_mul(7).wrapping_add(i * 5) % 96);
            e.note_off(channel, note);
        }
        if round % 8 == 7 {
            e.cc(channel, 64, 0);
        }
        if round % 13 == 12 {
            e.cc(channel, 66, 0);
        }
        e.render(left, right);
    }
}

#[test]
fn full_polyphony_stealing_and_pedals_do_not_touch_the_heap() {
    let mut e = engine(4, 256, None);
    let (mut left, mut right) = ([0.0; BLOCK], [0.0; BLOCK]);
    // Warm-up sizes nothing: the guard covers the first note too.
    without_heap("stealing at the instrument limit", || {
        perform(&mut e, &mut left, &mut right, 400);
    });
    assert!(e.active_voices() >= 200, "{} voices", e.active_voices());
    assert!(left.iter().any(|x| *x != 0.0));
    without_heap("all notes off and tails", || {
        for channel in 0..16 {
            e.cc(channel, 64, 0);
            e.cc(channel, 123, 0);
        }
        for _ in 0..200 {
            e.render(&mut left, &mut right);
        }
    });
    assert_eq!(e.active_voices(), 0);
}

#[test]
fn voice_storage_full_of_tails_cuts_without_touching_the_heap() {
    // A limit at the engine's own storage makes every note a new voice until
    // storage is full and only hard cuts make room.
    let mut e = engine(4, kontakto::engine::MAX_VOICES, None);
    e.release = 5.0;
    let (mut left, mut right) = ([0.0; BLOCK], [0.0; BLOCK]);
    without_heap("filling voice storage", || {
        perform(&mut e, &mut left, &mut right, 200);
        e.panic();
        e.render(&mut left, &mut right);
    });
    assert_eq!(e.active_voices(), 0);
}

#[test]
fn scripted_waits_and_generated_notes_at_polyphony_do_not_touch_the_heap() {
    let script = "on init
declare $count
declare %held[128]
end on
on note
inc($count)
%held[$EVENT_NOTE] := $EVENT_ID
wait(1500)
play_note($EVENT_NOTE + 12, $EVENT_VELOCITY, 0, 30000)
wait(700)
change_vol($EVENT_ID, -3000, 1)
end on
on release
wait(900)
play_note($EVENT_NOTE, 30, 0, 5000)
end on
on controller
if ($CC_NUM = 1)
wait(300)
end if
end on";
    let mut e = engine(2, 192, Some(script));
    let (mut left, mut right) = ([0.0; BLOCK], [0.0; BLOCK]);
    // One warm-up pass lets first-use script buffers settle, as the existing
    // playback guards do; everything after it must be heap-free.
    perform(&mut e, &mut left, &mut right, 4);
    without_heap("scripted waits under load", || {
        perform(&mut e, &mut left, &mut right, 300);
        for _ in 0..400 {
            e.render(&mut left, &mut right);
        }
    });
    assert!(e.script().unwrap().diagnostics().is_empty(), "{:?}", e.script().unwrap().diagnostics());
}

#[test]
fn rack_with_every_part_playing_does_not_touch_the_heap() {
    let mut rack = Rack::default();
    for part in &mut rack.parts {
        *part = engine(2, 64, None);
    }
    without_heap("rack render", || {
        for round in 0..300usize {
            let channel = (round % 16) as u8;
            for i in 0..8u8 {
                rack.note_on(channel, 36 + ((round as u8).wrapping_mul(3).wrapping_add(i * 7) % 80), 100);
            }
            if round % 10 == 0 {
                rack.cc(channel, 64, 127);
            }
            rack.render(BLOCK);
            for i in 0..8u8 {
                rack.note_off(channel, 36 + ((round as u8).wrapping_mul(3).wrapping_add(i * 7) % 80));
            }
            if round % 10 == 9 {
                rack.cc(channel, 64, 0);
            }
            rack.render(BLOCK);
        }
        rack.panic();
        rack.render(BLOCK);
    });
}
