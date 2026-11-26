//! A callback keeps its kind and payload across `wait(...)`: what it reads
//! after the wait is what it read before.
#![cfg(test)]

use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, HostPattern, Plan, Region, SampleTime, Timed, Zone};
use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};

const RATE: u32 = 48_000;

fn zone(store: &mut Store) -> Zone {
    let fmt = SampleFmt {
        encoding: Encoding::F32,
        channels: 1,
    };
    let frames = 96_000;
    let bytes = (0..frames).flat_map(|_| 0.5f32.to_le_bytes()).collect();
    let key = BlobKey {
        file: "dc".into(),
        offset: 0,
        fmt,
    };
    let sample = store
        .load(
            &key,
            Arc::new(MemSource::new(fmt, bytes)),
            frames,
            std::slice::from_ref(&(0..frames)),
        )
        .unwrap();
    let region = Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: Default::default(),
        reverse: false,
    };
    let mut z = Zone::new(region, 60, RATE);
    z.keys = 0..=127;
    z
}

fn engine(sources: &[&str]) -> (Store, Engine) {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store)], 16);
    plan.scripts = (sources.iter())
        .map(|s| kontra_host::script::plan(&kontra_ksp::compile(s, &[]).unwrap(), &[], &[]))
        .collect();
    let e = Engine::new(Arc::new(plan), store.client(), RATE);
    (store, e)
}

fn ev(body: Event) -> Timed<Event> {
    Timed {
        at: SampleTime(0),
        body,
    }
}

fn note(key: u8, on: bool) -> Timed<Event> {
    let note = HostNote {
        port: 0,
        channel: 0,
        key,
        id: -1,
        clap: true,
    };
    ev(if on {
        Event::NoteOn {
            note,
            velocity: 1.0,
            tune: 0.0,
        }
    } else {
        Event::NoteOff(HostPattern {
            port: 0,
            channel: 0,
            key: i32::from(key),
            id: -1,
            clap: true,
        })
    })
}

fn cc(n: u8, v: u8) -> Timed<Event> {
    ev(Event::midi1(0xB0, n, v))
}

/// One block per event, then one more for the waits to end.
fn blocks(e: &mut Engine, input: &[Timed<Event>]) {
    let mut out = vec![[0.0; 2]; 4096];
    for event in input {
        e.process(&mut out, std::slice::from_ref(event));
    }
    e.process(&mut out, &[]);
}

/// Each callback waits, then reports what it is and what it was called for.
#[test]
fn every_callback_keeps_its_context_across_a_wait() {
    let report = "message($NI_CALLBACK_TYPE";
    type Case<'a> = (&'a str, &'a [&'a str], Vec<Timed<Event>>, &'a str);
    let cases: [Case; 7] = [
        (
            "init",
            &["on init\n wait(1)\n message($NI_CALLBACK_TYPE)\n end on"],
            vec![],
            "5",
        ),
        (
            "persistence_changed",
            &["on persistence_changed\n wait(1)\n message($NI_CALLBACK_TYPE)\n end on"],
            vec![],
            "14",
        ),
        (
            "controller",
            &[&format!(
                "on controller\n wait(1)\n {report} & \",\" & $CC_NUM & \",\" & %CC_TOUCHED[74])\n end on"
            )],
            vec![cc(74, 90)],
            "3,74,1",
        ),
        (
            "release",
            &[&format!(
                "on release\n wait(1)\n ignore_event($EVENT_ID)\n {report} & \",\" & $EVENT_NOTE)\n end on"
            )],
            vec![note(62, true), note(62, false)],
            "2,62",
        ),
        (
            "nrpn",
            &[&format!(
                "on nrpn\n wait(1)\n {report} & \",\" & $RPN_ADDRESS & \",\" & $RPN_VALUE)\n end on"
            )],
            vec![cc(99, 1), cc(98, 5), cc(6, 3), cc(38, 4)],
            "11,133,388",
        ),
        (
            "midi_in",
            &[&format!(
                "on midi_in\n wait(1)\n {report} & \",\" & $MIDI_COMMAND & \",\" & $MIDI_BYTE_1)\n end on"
            )],
            vec![note(64, true)],
            "9,144,64",
        ),
        (
            "note_controller",
            &[
                "on note\n set_note_controller(7, $EVENT_NOTE, 77)\n end on",
                "on note_controller\n wait(1)\n message($NC_NUM & \",\" & $NC_NOTE & \",\" & $NC_VALUE)\n end on",
            ],
            vec![note(60, true)],
            "7,60,77",
        ),
    ];
    for (name, sources, input, want) in cases {
        let (_store, mut e) = engine(sources);
        blocks(&mut e, &input);
        assert_eq!(e.last_message(), want, "{name}");
    }
}

/// A routed knob followed at control rate, and a resumed `set_controller`
/// moving a MIDI-learned slider, allocate nothing on the audio thread. The
/// host links kontra-lua's global allocator, whose audio scope counts every
/// allocation made inside it.
#[test]
fn knob_routes_and_resumed_links_allocate_nothing() {
    use kontra_engine::ir::{self, Linked};
    let source = "on init
            declare ui_slider $s (0, 1000)
            declare ui_knob $k (0, 100, 1)
        end on
        on controller
            if ($CC_NUM = 74)
                wait(1)
                set_controller(122, %CC[74])
            end if
        end on
        on ui_control ($s)
            message($s)
        end on
        on ui_control ($k)
        end on";
    let script = kontra_ksp::compile(source, &[]).unwrap();
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store)], 16);
    let mut planned = kontra_host::script::plan(&script, &[], &[]);
    let link = ir::CcLink {
        cc: 122,
        control: Linked::Slider(0),
        from: 0.0,
        to: 1.0,
    };
    planned.links = kontra_host::script::links(&script, &[link]);
    assert_eq!(planned.links.len(), 1);
    plan.scripts = vec![planned];
    plan.knob_mods = vec![kontra_engine::KnobModPlan {
        slot: 0,
        id: 1,
        source: ir::Source::Controller(1),
        scale: false,
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0; 2]; 512];
    let input = |v| [cc(74, v), cc(1, v)];
    // Warm up: first touches size what grows once.
    for v in [10, 20, 30] {
        e.process(&mut out, &input(v));
    }
    let arena = kontra_lua::alloc::Claim::arena();
    kontra_lua::probe::take();
    {
        let _audio = arena.audio();
        for v in [40, 50, 60, 70] {
            e.process(&mut out, &input(v));
        }
    }
    assert_eq!(
        kontra_lua::probe::take().allocs,
        0,
        "audio-thread allocations"
    );
    // The resumed `set_controller` reached the slider through its link.
    assert_eq!(e.last_message(), "551");
}
