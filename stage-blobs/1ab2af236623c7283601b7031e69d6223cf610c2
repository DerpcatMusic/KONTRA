//! Own synthetic scripts through the production Core block boundary; no native host.
use super::*;
use crate::sound::Transport;
use sampler_core::ScriptInstanceId;

const SNAPSHOT: &str = "on init
    declare $quarter declare $eighth declare $sixteenth
    declare $quarter_triplet declare $eighth_triplet declare $sixteenth_triplet
    declare $bar declare $position declare $numerator declare $denominator declare $running
end on
on note
    $quarter := $DURATION_QUARTER
    $eighth := $DURATION_EIGHTH
    $sixteenth := $DURATION_SIXTEENTH
    $quarter_triplet := $DURATION_QUARTER_TRIPLET
    $eighth_triplet := $DURATION_EIGHTH_TRIPLET
    $sixteenth_triplet := $DURATION_SIXTEENTH_TRIPLET
    $bar := $DURATION_BAR
    $position := $NI_SONG_POSITION
    $numerator := $SIGNATURE_NUM
    $denominator := $SIGNATURE_DENOM
    $running := $NI_TRANSPORT_RUNNING
end on";

fn part(source: &str) -> Box<Part> {
    let script = sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let plan = script
        .bind(Prepared::new(48000, vec![], vec![], 1).unwrap())
        .unwrap();
    let runtime_limits = limits(&plan).0;
    Box::new(
        Part::new(
            Runtime::new(plan, runtime_limits).unwrap(),
            MixTree::instrument("host fixture"),
        )
        .unwrap(),
    )
}

fn core(source: &str) -> V2Core {
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, Some(part(source)));
    core
}

fn block(
    core: &mut V2Core,
    playing: bool,
    tempo: f64,
    beats: f64,
    signature: (u8, u8),
    offline: bool,
) {
    core.begin_block(&BlockInfo {
        frames: 128,
        offline,
        transport: Transport {
            playing,
            tempo,
            beats,
            signature,
        },
    });
}

fn cells<const N: usize>(core: &V2Core, index: usize) -> [i64; N] {
    let runtime = &core.parts[index].as_ref().unwrap().runtime;
    std::array::from_fn(|cell| {
        runtime
            .script_cell(runtime.active_plan(), ScriptInstanceId(0), cell as u32)
            .unwrap()
    })
}

fn capture(core: &mut V2Core) -> [i64; 11] {
    core.event(0, Event::midi1(0x90, 60, 100));
    core.render(1);
    for index in 0..core.parts.len() {
        assert_eq!(core.problems(index).fault_program, 0);
    }
    cells(core, 0)
}

fn render_to(core: &mut V2Core, until: u64) {
    while core.parts[0].as_ref().unwrap().runtime.now() < until {
        let now = core.parts[0].as_ref().unwrap().runtime.now();
        core.render((until - now).min(128) as usize);
    }
    assert_eq!(core.problems(0).fault_program, 0);
}

#[test]
fn host_block_publishes_microseconds_960_ppq_and_raw_signature_to_every_part() {
    for offline in [false, true] {
        let mut core = core(SNAPSHOT);
        let mut grown = V2Core::with_parts(2, 48000.);
        core.adopt(&mut grown);
        core.install(1, Some(part(SNAPSHOT)));
        block(&mut core, true, 240., 12.5, (7, 8), offline);
        let expected = [
            250_000, 125_000, 62_500, 166_666, 83_333, 41_666, 875_000, 12_000, 7, 8, 1,
        ];
        assert_eq!(capture(&mut core), expected);
        assert_eq!(cells::<11>(&core, 1), expected);
    }
}

#[test]
fn stopped_block_zeroes_only_bar_and_running_not_tempo_or_host_position() {
    let mut core = core(SNAPSHOT);
    block(&mut core, false, 120., -0.25, (3, 4), false);
    assert_eq!(
        capture(&mut core),
        [
            500_000, 250_000, 125_000, 333_333, 166_666, 83_333, 0, -240, 3, 4, 0
        ]
    );
    block(&mut core, true, 60., 9., (5, 8), true);
    assert_eq!(
        capture(&mut core),
        [
            1_000_000, 500_000, 250_000, 666_666, 333_333, 166_666, 2_500_000, 8640, 5, 8, 1
        ]
    );
    block(&mut core, false, 60., 9., (5, 8), true);
    assert_eq!(capture(&mut core)[6..], [0, 8640, 5, 8, 0]);
}

#[test]
fn missing_tempo_and_signature_use_defaults_then_retain_last_valid_pair() {
    let mut core = core(SNAPSHOT);
    block(&mut core, false, f64::NAN, f64::NAN, (0, 0), false);
    assert_eq!(
        capture(&mut core),
        [
            500_000, 250_000, 125_000, 333_333, 166_666, 83_333, 0, 0, 4, 4, 0
        ]
    );
    block(&mut core, true, 240., 1., (7, 8), false);
    assert_eq!(capture(&mut core)[6..], [875_000, 960, 7, 8, 1]);
    for (tempo, signature) in [
        (f64::NAN, (0, 4)),
        (f64::INFINITY, (4, 0)),
        (0., (0, 0)),
        (-10., (0, 8)),
    ] {
        block(&mut core, true, tempo, 2., signature, true);
        assert_eq!(
            capture(&mut core),
            [
                250_000, 125_000, 62_500, 166_666, 83_333, 41_666, 875_000, 1920, 7, 8, 1
            ]
        );
    }
}

#[test]
fn positive_signature_pair_is_raw_not_restricted_to_power_of_two_denominators() {
    let mut core = core(SNAPSHOT);
    block(&mut core, true, 120., 0., (255, 3), false);
    assert_eq!(capture(&mut core)[6..], [170_000_000, 0, 255, 3, 1]);
    block(&mut core, true, 120., 0., (7, 0), false);
    assert_eq!(
        capture(&mut core)[6..],
        [170_000_000, 0, 255, 3, 1],
        "an invalid pair cannot partly replace the signature"
    );
}

#[test]
fn stopped_block_state_is_visible_to_midi_flushed_from_alignment() {
    let mut core = core(SNAPSHOT);
    let mut mix = Mix::default();
    mix.timing = Arc::new(crate::timing::Plan {
        on: true,
        transport_only: true,
        ..Default::default()
    });
    core.set_mix(&mix);
    block(&mut core, true, 120., 0., (4, 4), false);
    core.event(0, Event::midi1(0x90, 60, 100));
    block(&mut core, false, 240., 2., (7, 8), true);
    core.render(1);
    assert_eq!(
        cells::<11>(&core, 0),
        [
            250_000, 125_000, 62_500, 166_666, 83_333, 41_666, 0, 1920, 7, 8, 0
        ]
    );
    assert_eq!(core.problems(0).fault_program, 0);
}

#[test]
fn missing_position_advances_rendered_time_then_stop_holds_and_seek_overrides() {
    let mut core = core(SNAPSHOT);
    block(&mut core, true, 120., 4., (4, 4), false);
    render_to(&mut core, 24_000);
    // Elapsed frames use the previous tempo, not the new tempo.
    block(&mut core, false, 240., f64::NAN, (4, 4), true);
    assert_eq!(capture(&mut core)[6..], [0, 4800, 4, 4, 0]);
    render_to(&mut core, 48_000);
    block(&mut core, true, 240., f64::INFINITY, (4, 4), false);
    assert_eq!(capture(&mut core)[7], 4800);
    block(&mut core, true, 240., -0.0001, (4, 4), false);
    assert_eq!(
        capture(&mut core)[7],
        -1,
        "negative preroll floors at 960 PPQ"
    );
    block(&mut core, true, 240., 100., (4, 4), true);
    assert_eq!(capture(&mut core)[7], 96_000, "host seek is authoritative");
}

#[test]
fn block_growth_and_part_replacement_preserve_missing_host_state() {
    let mut core = core(SNAPSHOT);
    block(&mut core, false, 240., 10., (3, 8), false);
    capture(&mut core);
    let mut grown = V2Core::with_parts(2, 48000.);
    core.adopt(&mut grown);
    let retired = core.install(0, Some(part(SNAPSHOT)));
    block(&mut core, true, f64::NAN, f64::NAN, (0, 0), true);
    assert_eq!(
        capture(&mut core),
        [
            250_000, 125_000, 62_500, 166_666, 83_333, 41_666, 375_000, 9600, 3, 8, 1
        ]
    );
    drop(retired);
}

#[test]
fn host_tempo_changes_affect_new_waits_not_an_already_scheduled_wait() {
    for offline in [false, true] {
        let mut core =
            core("on init declare $done end on on note wait_ticks(960) inc($done) end on");
        block(&mut core, true, 240., 0., (4, 4), offline);
        core.event(0, Event::midi1(0x90, 60, 100));
        render_to(&mut core, 1024);
        block(&mut core, true, 60., 1., (4, 4), offline);
        render_to(&mut core, 11_936);
        assert_eq!(cells::<1>(&core, 0), [0]);
        render_to(&mut core, 12_064);
        assert_eq!(cells::<1>(&core, 0), [1], "the fast wait was not retimed");
        core.event(0, Event::midi1(0x90, 61, 100));
        render_to(&mut core, 60_000);
        assert_eq!(cells::<1>(&core, 0), [1]);
        render_to(&mut core, 60_128);
        assert_eq!(
            cells::<1>(&core, 0),
            [2],
            "the new wait uses the slow host tempo"
        );
    }
}

#[test]
fn existing_beat_listener_consumes_each_block_tempo_even_when_stopped_offline() {
    let mut core = core(
        "on init declare $ticks declare $quarter declare $running
        set_listener($NI_SIGNAL_TIMER_BEAT,4) end on
        on listener inc($ticks) $quarter:=$DURATION_QUARTER $running:=$NI_TRANSPORT_RUNNING end on",
    );
    block(&mut core, false, 240., 0., (4, 4), true);
    render_to(&mut core, 48_000);
    let fast = cells::<3>(&core, 0);
    assert!((14..=16).contains(&fast[0]), "fast ticks: {fast:?}");
    assert_eq!(fast[1..], [250_000, 0]);
    block(&mut core, false, 60., 0., (4, 4), true);
    render_to(&mut core, 96_000);
    let slow = cells::<3>(&core, 0);
    assert!(
        (3..=5).contains(&(slow[0] - fast[0])),
        "slow ticks: {slow:?}"
    );
    assert_eq!(slow[1..], [1_000_000, 0]);
}

#[test]
fn extreme_finite_inputs_saturate_ksp_integer_values_without_faults() {
    let mut core = core(SNAPSHOT);
    block(&mut core, true, f64::MIN_POSITIVE, f64::MAX, (255, 1), true);
    let huge = capture(&mut core);
    assert_eq!(huge[0], i64::from(i32::MAX));
    assert_eq!(huge[6], i64::from(i32::MAX));
    assert_eq!(huge[7], i64::from(i32::MAX));
    block(&mut core, true, f64::MAX, -f64::MAX, (1, 255), true);
    let tiny = capture(&mut core);
    assert_eq!(tiny[0], 1);
    assert_eq!(tiny[7], i64::from(i32::MIN));
}

#[cfg(feature = "plugin")]
#[test]
fn host_block_updates_and_script_consumption_make_no_audio_heap_calls() {
    let mut core = core(SNAPSHOT);
    block(&mut core, false, 120., 0., (4, 4), false);
    capture(&mut core);
    let heap_calls = crate::plugin::tests::allocations(|| {
        for n in 0..16 {
            block(
                &mut core,
                n % 2 == 0,
                60. + f64::from(n) * 10.,
                f64::from(n),
                (7, 8),
                n % 2 != 0,
            );
            capture(&mut core);
        }
    });
    assert_eq!(
        heap_calls, 0,
        "block publication or callback allocated/freed"
    );
}
