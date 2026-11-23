//! The MIDI object and zone commands a script calls after init allocate
//! nothing: they run on the audio thread.

use kontra_engine::ir::Sample;
use kontra_engine::{Action, Engine, Plan, SavedZoneDesc, Text};
use kontra_stream::{Store, StoreConfig};
use kontra_vm::{HostApi, HostCall, HostValue, event};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

struct Counting;
thread_local! { static COUNT: Cell<Option<usize>> = const { Cell::new(None) }; }
#[global_allocator]
static ALLOCATOR: Counting = Counting;

// SAFETY: all allocation operations are forwarded to the system allocator with
// their original arguments. The thread-local counter never allocates.
#[expect(unsafe_code, reason = "the isolated test allocator forwards to System")]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|n| n.set(n.get().map(|v| v + 1)));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNT.with(|n| n.set(n.get().map(|v| v + 1)));
        unsafe { System.realloc(ptr, layout, size) }
    }
}

fn call(e: &mut Engine, action: Action, ints: &[i64], texts: &[Text]) -> (HostValue, Text) {
    let mut out = Text::EMPTY;
    let value = e.call(HostCall {
        action,
        context: 0,
        ints,
        texts,
        cells: &mut [],
        text_cells: &mut [],
        text_out: &mut out,
    });
    (value, out)
}

#[test]
fn midi_object_and_zone_commands_after_init_allocate_nothing() {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![], 0);
    // Zone 99's sample, and zone 98's saved with Windows separators.
    for (id, file) in [
        (99, "Lib/Samples/Piano C3.wav"),
        (98, r"Lib\Samples\Piano C3.wav"),
    ] {
        plan.push_saved_zone(
            SavedZoneDesc {
                id,
                group: 0,
                keys: 0..=127,
                velocities: 1..=127,
                root: 60,
                start: 0,
                end: 48_000,
                start_mod: 0,
                sample_rate: 48_000,
                volume_db: 0.0,
                pan: 0.0,
                tune_semitones: 0.0,
                key_fade: (0, 0),
                velocity_fade: (0, 0),
                sample_idx: None,
                custom_name: None,
                loops: &[],
            },
            Some(Arc::new(Sample {
                name: "Piano C3".into(),
                file: file.into(),
                offset: 0,
                len: None,
                channels: 2,
                frames: 48_000,
                rate: 48_000,
                keystream: None,
                raw: None,
            })),
        );
    }
    plan.make_user_zones(1);
    let user = i64::from(plan.user_zone_ids().next().unwrap());
    let mut e = Engine::new(Arc::new(plan), store.client(), 48_000);
    // Sizing the buffer reserves, as a script does in `on init`.
    call(&mut e, Action::MfSetBufferSize, &[16], &[]);
    let path = Text::from_str_cut("other.wav").0;

    COUNT.with(|n| n.set(Some(0)));
    let sample = |e: &mut Engine, zone, what| call(e, Action::GetSample, &[zone, what], &[]).1;
    let texts = [0, 1, 2, 3].map(|what| sample(&mut e, 99, what));
    let forward = sample(&mut e, 98, 1);
    let inserted = [0, 1, 2].map(|track| {
        let event = [track, 960 * track, 0x90, 60, 100];
        call(&mut e, Action::MfInsertEvent, &event, &[]).0
    });
    call(&mut e, Action::MfGetFirst, &[-1], &[]);
    call(&mut e, Action::MfSetByteTwo, &[90], &[]);
    let all = i64::from(event::ALL);
    call(
        &mut e,
        Action::MfSetEventPar,
        &[all, i64::from(event::MIDI_CHANNEL), 3],
        &[],
    );
    let marks = i64::from(event::BY_MARKS | 1);
    call(&mut e, Action::MfSetMark, &[all, 1, 1], &[]);
    call(&mut e, Action::MfSetNumExportAreas, &[512], &[]);
    call(&mut e, Action::MfRemoveEvent, &[marks], &[]);
    let buffer = call(&mut e, Action::MfGetBufferSize, &[], &[]).0;
    call(&mut e, Action::MfReset, &[], &[]);
    call(&mut e, Action::SetSample, &[user], &[path]);
    let allocated = COUNT.with(|n| n.replace(None));

    assert_eq!(allocated, Some(0), "audio-thread allocations");
    assert_eq!(
        texts.map(|t| t.as_str().to_owned()),
        [
            "Piano C3".to_owned(),
            "Lib/Samples/Piano C3.wav".to_owned(),
            "Lib/Samples/Piano C3.wav".to_owned(),
            "wav".to_owned(),
        ]
    );
    // $NI_FILE_FULL_PATH uses forward slashes.
    assert_eq!(forward.as_str(), "Lib/Samples/Piano C3.wav");
    assert_eq!(
        inserted.map(|v| matches!(v, HostValue::Int(id) if id > 0)),
        [true; 3]
    );
    // Every event was marked and removed: the buffer is whole again.
    assert_eq!(buffer, HostValue::Int(16));
}
