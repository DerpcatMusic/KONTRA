//! Source-only production-path witnesses. NOT_RUN / NOT_COMPILED; native UNKNOWN.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::*;

const QUERY: &str = "on init
  declare $id declare $parameter := $EVENT_PAR_ZONE_ID
  declare $pending declare $literal declare $dynamic
end on
on note
  $id := $EVENT_ID
  $pending := get_event_par($id,$EVENT_PAR_ZONE_ID)
  wait(1)
  $literal := get_event_par($id,$EVENT_PAR_ZONE_ID)
  $dynamic := get_event_par($id,$parameter)
end on
on controller
  $literal := get_event_par($id,$EVENT_PAR_ZONE_ID)
  $dynamic := get_event_par($id,$parameter)
end on";

fn prepared(zones: &[(u32, u8)], frames: usize) -> Prepared {
    Prepared::new(
        48000,
        vec![Pcm::new(48000, vec![[1.; 2]; frames].into_boxed_slice()).unwrap()],
        zones
            .iter()
            .map(|&(_, key)| Region {
                sample: 0,
                key_low: key,
                key_high: key,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::new(0, 0, 0, 1., 128).unwrap(),
                playback: Playback::default(),
            })
            .collect(),
        128,
    )
    .unwrap()
    .with_source_zones(zones.iter().map(|&(zone, _)| zone).collect())
    .unwrap()
}

fn runtime_with_note_capacity(source: &str, prepared: Prepared, notes: usize) -> Runtime {
    let script = sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let plan = script.bind(prepared).unwrap();
    let limits = Limits {
        notes,
        channels: 1,
        performances: 1,
        families: 16,
        voices: 8,
        expressions: 8,
        decisions: 0,
        commands: 32,
        behaviors: 16,
        behavior_fuel: 4096,
        behavior_cells: plan.behavior_local_count() * 16,
        note_cells: plan.note_cell_count() * notes,
    };
    Runtime::new(plan, limits).unwrap()
}

fn runtime(source: &str, prepared: Prepared) -> Runtime {
    runtime_with_note_capacity(source, prepared, 8)
}

fn input(key: u8) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: Some(900_000 + i32::from(key)),
    }
}

fn cell(rt: &Runtime, index: u32) -> i64 {
    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), index)
        .unwrap()
}

fn query(rt: &mut Runtime) {
    let domain = rt.performance(0).unwrap();
    rt.dispatch_controller(domain, input(60).channel_address(), 1, 1, 1)
        .unwrap();
}

#[test]
fn highest_physical_zone_is_independent_of_voice_order_and_shared_sample() {
    for ids in [[9001, 73, 501], [73, 9001, 501], [73, 501, 9001]] {
        let zones = ids.map(|id| (id, 60));
        let mut rt = runtime(QUERY, prepared(&zones, 4800));
        support::without_heap(|| {
            let note = rt.trigger(input(60), 60, 1.).unwrap();
            assert_eq!(cell(&rt, 0), i64::from(rt.source_event_id(note).unwrap()));
            assert_ne!(cell(&rt, 0), i64::from(input(60).external_id.unwrap()));
            assert_eq!(cell(&rt, 2), -1);
            rt.render(&mut [[0.; 2]; 8]).unwrap();
            assert_eq!(rt.voice_count(), 3);
            assert_eq!((cell(&rt, 3), cell(&rt, 4)), (9001, 9001));
        });
    }
}

#[test]
fn live_unmapped_or_suppressed_event_has_no_zone_after_wait() {
    for (source, zones) in [
        (QUERY.to_owned(), vec![(73, 61)]),
        (
            QUERY.replace(
                "$id := $EVENT_ID",
                "ignore_event($EVENT_ID) $id := $EVENT_ID",
            ),
            vec![(73, 60)],
        ),
    ] {
        let mut rt = runtime(&source, prepared(&zones, 4800));
        support::without_heap(|| {
            let note = rt.trigger(input(60), 60, 1.).unwrap();
            rt.render(&mut [[0.; 2]; 8]).unwrap();
            assert!(
                rt.resolve_source_event(rt.active_plan(), cell(&rt, 0) as i32)
                    .unwrap()
                    == Some(note)
            );
            assert_eq!(rt.voice_count(), 0);
            assert_eq!((cell(&rt, 2), cell(&rt, 3), cell(&rt, 4)), (-1, -1, -1));
        });
    }
}

#[test]
fn exhausted_voices_report_no_zone_until_alias_retirement_not_slot_reuse() {
    let source = QUERY.replace("$id := $EVENT_ID", "if ($id = 0) $id := $EVENT_ID end if");
    let mut rt = runtime_with_note_capacity(&source, prepared(&[(73, 60)], 16), 1);
    support::without_heap(|| {
        let old = rt.trigger(input(60), 60, 1.).unwrap();
        let alias = rt.source_event_id(old).unwrap();
        rt.render(&mut [[0.; 2]; 64]).unwrap();
        query(&mut rt);
        assert_eq!(rt.voice_count(), 0);
        assert_eq!(
            rt.resolve_source_event(rt.active_plan(), alias).unwrap(),
            Some(old)
        );
        assert_eq!((cell(&rt, 3), cell(&rt, 4)), (-1, -1));

        let mut finished_note_callbacks = 0;
        rt.visit_behavior_progress(|progress| {
            if progress.owner == BehaviorOwner::Note(old) {
                assert_eq!(progress.outcome, Some(Outcome::Finished));
                finished_note_callbacks += 1;
            }
        });
        assert_eq!(finished_note_callbacks, 1);
        let mut flushed_note_callbacks = 0;
        rt.flush_behaviors(|_, owner, outcome| {
            if owner == BehaviorOwner::Note(old) {
                assert_eq!(outcome, Outcome::Finished);
                flushed_note_callbacks += 1;
            }
            true
        });
        assert_eq!(flushed_note_callbacks, 1);

        assert_eq!(rt.note_off(input(60), None), Ok(old));
        assert!(!rt.input_held(old).unwrap());
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
        assert_eq!(
            rt.resolve_source_event(rt.active_plan(), alias).unwrap(),
            None
        );
        let new = rt.trigger(input(60), 60, 1.).unwrap();
        assert_ne!(new, old);
        assert_eq!(rt.note_count(), 1);
        assert_ne!(rt.source_event_id(new).unwrap(), alias);
        assert_eq!(
            rt.resolve_source_event(rt.active_plan(), alias).unwrap(),
            None
        );
        query(&mut rt);
        assert_eq!((cell(&rt, 3), cell(&rt, 4)), (0, 0));
    });
}

#[test]
fn generated_alias_queries_its_own_zone_not_the_suppressed_parent() {
    let source = "on init declare $child declare $parameter := $EVENT_PAR_ZONE_ID
      declare $parent_zone declare $literal declare $dynamic declare $creator end on
      on note
        ignore_event($EVENT_ID)
        $child := play_note(61,100,0,0)
        wait(1)
        $parent_zone := get_event_par($EVENT_ID,$EVENT_PAR_ZONE_ID)
        $literal := get_event_par($child,$EVENT_PAR_ZONE_ID)
        $dynamic := get_event_par($child,$parameter)
        $creator := get_event_par($child,$EVENT_PAR_SOURCE)
      end on";
    let mut rt = runtime(source, prepared(&[(73, 60), (9001, 61)], 4800));
    support::without_heap(|| {
        let parent = rt.trigger(input(60), 60, 1.).unwrap();
        rt.render(&mut [[0.; 2]; 8]).unwrap();
        let child = rt
            .resolve_source_event(rt.active_plan(), cell(&rt, 0) as i32)
            .unwrap()
            .unwrap();
        assert_ne!(child, parent);
        assert_eq!(
            (cell(&rt, 2), cell(&rt, 3), cell(&rt, 4), cell(&rt, 5)),
            (-1, 9001, 9001, 0)
        );
    });
}

#[test]
fn highest_zone_spans_attack_and_release_families_while_tails_remain() {
    let plan = prepared(&[(9001, 60), (73, 60)], 4800)
        .with_releases(
            vec![Trigger::Attack, Trigger::GateRelease],
            ReleaseOptions::default(),
            ReleaseOptions::default(),
        )
        .unwrap();
    let mut rt = runtime(QUERY, plan);
    support::without_heap(|| {
        let note = rt.trigger(input(60), 60, 1.).unwrap();
        rt.render(&mut [[0.; 2]; 8]).unwrap();
        rt.key_up(note, None).unwrap();
        assert_eq!(rt.note_families(note).unwrap().count(), 2);
        assert_eq!(rt.voice_count(), 2);
        query(&mut rt);
        assert_eq!((cell(&rt, 3), cell(&rt, 4)), (9001, 9001));
    });
}

#[test]
fn stolen_fading_voices_do_not_supply_a_zone() {
    let source = QUERY.replace("$id := $EVENT_ID", "if ($id = 0) $id := $EVENT_ID end if");
    let mut rt = runtime(&source, prepared(&[(9001, 60), (73, 61)], 4800));
    rt.set_voice_stealing(Some(Stealing {
        headroom: 7,
        fade: 128,
    }))
    .unwrap();
    support::without_heap(|| {
        let old = rt.trigger(input(60), 60, 1.).unwrap();
        rt.render(&mut [[0.; 2]; 8]).unwrap();
        let alias = rt.source_event_id(old).unwrap();
        let new = rt.trigger(input(61), 61, 1.).unwrap();
        rt.render(&mut [[0.; 2]; 8]).unwrap();
        assert_eq!(rt.stolen_voices(), 1);
        assert_eq!(rt.voice_count(), 2);
        assert_ne!(rt.source_event_id(new).unwrap(), alias);
        assert_eq!(
            rt.resolve_source_event(rt.active_plan(), alias).unwrap(),
            Some(old)
        );
        query(&mut rt);
        assert_eq!((cell(&rt, 3), cell(&rt, 4)), (-1, -1));
    });
}

#[test]
fn wrapped_negative_literal_is_an_invalid_alias_and_selectors_keep_zero_fallback() {
    let source = "on init declare $parameter := $EVENT_PAR_ZONE_ID
      declare $all := $ALL_EVENTS declare $marks declare $unknown declare $wrapped_negative
      declare $all_zone declare $mark_zone end on
      on note
        $marks := by_marks($MARK_1)
        set_event_mark($EVENT_ID,$MARK_1)
        wait(1)
        $unknown := get_event_par(99999,$EVENT_PAR_ZONE_ID)
        $wrapped_negative := get_event_par(2147483648,$parameter)
        $all_zone := get_event_par($all,$EVENT_PAR_ZONE_ID)
        $mark_zone := get_event_par($marks,$parameter)
      end on";
    let mut rt = runtime(source, prepared(&[(73, 60)], 4800));
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap();
        rt.render(&mut [[0.; 2]; 8]).unwrap();
        assert_eq!(
            (cell(&rt, 3), cell(&rt, 4), cell(&rt, 5), cell(&rt, 6)),
            (0, 0, 0, 0)
        );
    });
}

#[test]
fn play_position_reports_microseconds_into_the_zone() {
    // One voice at unit ratio: the cursor advances one frame per output frame.
    let source = "on init declare $pos end on
      on note
        wait(20000)
        $pos := get_event_par($EVENT_ID,$EVENT_PAR_PLAY_POS)
      end on";
    let mut rt = runtime(source, prepared(&[(73, 60)], 48000));
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap();
        rt.render(&mut [[0.; 2]; 9600]).unwrap();
        // wait is in microseconds: 20 ms = 960 frames at 48 kHz; allow for block slack.
        let pos = cell(&rt, 0);
        assert!((19_000..=25_000).contains(&pos), "play position {pos} us");
    });
}

#[test]
fn zone_id_is_the_native_identity_when_the_instrument_has_one() {
    const NATIVE: &str = "on init declare $id declare $zone end on
on note
  $id := $EVENT_ID
  wait(1)
  $zone := get_event_par($id,$EVENT_PAR_ZONE_ID)
end on";
    for native in [9727, 0] {
        let prepared = prepared(&[(73, 60)], 4800)
            .with_groups(1, vec![Some(0)])
            .unwrap()
            .with_source_zone_identities(
                vec![SourceZoneIdentity {
                    id: Some(native),
                    home: SourceZoneHome { group: 0, low_key: 60, high_key: 60 },
                    region: Some(0),
                }],
                4,
                4096,
            )
            .unwrap();
        let mut rt = runtime(NATIVE, prepared);
        rt.trigger(input(60), 60, 1.).unwrap();
        rt.render(&mut [[0.; 2]; 8]).unwrap();
        assert_eq!(cell(&rt, 1), i64::from(native));
    }
}

#[test]
fn sample_length_is_the_event_zones_sample_in_microseconds() {
    const LENGTH: &str = "on init
  declare $id declare $zone declare $length declare $unknown
end on
on note
  $id := $EVENT_ID
  wait(1)
  $zone := get_event_par($id,$EVENT_PAR_ZONE_ID)
  $length := get_sample_length($zone)
  $unknown := get_sample_length($zone + 1)
end on";
    // Half a second recorded at 44.1 kHz, played by a 48 kHz host.
    let prepared = Prepared::new(
        48000,
        vec![Pcm::new(44100, vec![[1.; 2]; 22050].into_boxed_slice()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::new(0, 0, 0, 1., 128).unwrap(),
            playback: Playback::default(),
        }],
        128,
    )
    .unwrap()
    .with_source_zones(vec![73])
    .unwrap();
    let mut rt = runtime(LENGTH, prepared);
    rt.trigger(input(60), 60, 1.).unwrap();
    rt.render(&mut [[0.; 2]; 8]).unwrap();
    assert_eq!(cell(&rt, 1), 73);
    assert_eq!(cell(&rt, 2), 500_000);
    assert_eq!(cell(&rt, 3), -1);
}

#[test]
fn sample_length_takes_the_native_identity_get_zone_id_returns() {
    const LENGTH: &str = "on init
  declare $id declare $by_ordinal declare $by_event
end on
on note
  $id := $EVENT_ID
  wait(1)
  $by_ordinal := get_sample_length(get_zone_id(0))
  $by_event := get_sample_length(get_event_par($id,$EVENT_PAR_ZONE_ID))
end on";
    let prepared = prepared(&[(73, 60)], 24000)
        .with_groups(1, vec![Some(0)])
        .unwrap()
        .with_source_zone_identities(
            vec![SourceZoneIdentity {
                id: Some(9727),
                home: SourceZoneHome { group: 0, low_key: 60, high_key: 60 },
                region: Some(0),
            }],
            4,
            4096,
        )
        .unwrap();
    let mut rt = runtime(LENGTH, prepared);
    rt.trigger(input(60), 60, 1.).unwrap();
    rt.render(&mut [[0.; 2]; 8]).unwrap();
    // 24000 frames at 48 kHz.
    assert_eq!((cell(&rt, 1), cell(&rt, 2)), (500_000, 500_000));
}
