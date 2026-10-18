//! Mixed generation/filter topology and all-thread callback heap contract.
//! Frozen baseline/candidate receipts supplement in-process serial/parallel PCM/state checks.
use sampler_core::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};

struct CountAll;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static FREES: AtomicUsize = AtomicUsize::new(0);
#[allow(
    unsafe_code,
    reason = "Dedicated test allocator forwards unchanged to System; counts every render lane"
)]
unsafe impl GlobalAlloc for CountAll {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(SeqCst) {
            ALLOCS.fetch_add(1, SeqCst);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if COUNTING.load(SeqCst) {
            FREES.fetch_add(1, SeqCst);
        }
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static GLOBAL: CountAll = CountAll;
fn without_heap(f: impl FnOnce()) {
    struct End;
    impl Drop for End {
        fn drop(&mut self) {
            COUNTING.store(false, SeqCst);
        }
    }
    assert!(!COUNTING.swap(true, SeqCst));
    let before = (ALLOCS.load(SeqCst), FREES.load(SeqCst));
    let end = End;
    f();
    drop(end);
    assert_eq!(
        (ALLOCS.load(SeqCst), FREES.load(SeqCst)),
        before,
        "callback or render worker allocated/deallocated"
    );
}
const NOTES: usize = 128;
fn plan(filtered: bool) -> Prepared {
    let assets = (0..3)
        .map(|asset| {
            Pcm::new(
                48000,
                (0..4096)
                    .map(|i| {
                        [
                            (i as f32 * (0.031 + asset as f32 * 0.007)).sin() * 0.125,
                            (i as f32 * (0.043 + asset as f32 * 0.003)).cos() * -0.1,
                        ]
                    })
                    .collect(),
            )
            .unwrap()
        })
        .collect();
    let regions = (0..NOTES)
        .map(|key| Region {
            sample: key % 3,
            key_low: key as u8,
            key_high: key as u8,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 0.0078125,
            envelope: Envelope::new(7, 3, 31, 0.6, 19).unwrap(),
            playback: Playback {
                loop_range: Some(Loop {
                    start: 0,
                    end: 4096,
                    passes: None,
                    mode: LoopMode::Continuous,
                    shape: LoopShape::Wrap,
                }),
                ..Playback::default()
            },
        })
        .collect();
    let base = Prepared::new(48000, assets, regions, NOTES)
        .unwrap()
        .with_velocity_curves(vec![VelocityCurve::Constant; NOTES])
        .unwrap();
    let svf = |hz| {
        Processor::StateVariable(StateVariableFilter {
            mode: SvfMode::LowPass,
            cutoff_hz: Parameter::Constant(hz),
            q: Parameter::Constant(0.7),
        })
    };
    let program = |target, depth| ModProgram {
        sources: vec![ModSource::Constant],
        routes: vec![ModRoute::new(0, target, depth)],
        ..ModProgram::default()
    };
    if filtered {
        base.with_voice_chains(
            vec![
                VoiceChain::new(vec![], vec![svf(900.)], 0).unwrap(),
                VoiceChain::new(vec![], vec![svf(2200.)], 0).unwrap(),
            ],
            (0..NOTES)
                .map(|i| match i % 5 {
                    0 | 3 => Some(0),
                    2 => Some(1),
                    _ => None,
                })
                .collect(),
        )
        .unwrap()
        .with_voice_modulation(
            vec![
                program(ModTarget::ProcessorCutoff(0), 12.),
                program(ModTarget::ProcessorResonance(1), -6.),
                program(ModTarget::Decibels, -3.),
            ],
            (0..NOTES)
                .map(|i| match i % 5 {
                    0 => Some(0),
                    1 => Some(2),
                    2 => Some(1),
                    _ => None,
                })
                .collect(),
            vec![0; NOTES],
        )
        .unwrap()
    } else {
        base.with_voice_chains(
            vec![VoiceChain::new(vec![], vec![Processor::Gain(0.75)], 0).unwrap()],
            (0..NOTES).map(|i| (i % 3 == 0).then_some(0)).collect(),
        )
        .unwrap()
        .with_voice_modulation(
            vec![program(ModTarget::Pan, 0.25)],
            (0..NOTES).map(|i| (i % 2 == 0).then_some(0)).collect(),
            vec![0; NOTES],
        )
        .unwrap()
    }
}
fn limits() -> Limits {
    Limits {
        notes: NOTES * 4,
        channels: 0,
        performances: 1,
        families: NOTES * 4,
        voices: NOTES * 4,
        expressions: NOTES * 4,
        decisions: 0,
        commands: NOTES * 4,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    }
}
fn input(generation: usize, key: usize) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: key as u8,
        external_id: Some((generation * NOTES + key) as i32),
    }
}
fn on(rt: &mut Runtime, generation: usize, notes: &mut Vec<NoteId>) {
    assert!(notes.is_empty() && notes.capacity() >= NOTES);
    for key in 0..NOTES {
        notes.push(rt.trigger(input(generation, key), key as u8, 1.).unwrap());
    }
}
fn off(rt: &mut Runtime, notes: &[NoteId]) {
    for &note in notes {
        rt.release(note).unwrap();
    }
}
#[derive(Debug, PartialEq, Eq)]
struct State {
    now: u64,
    voices: usize,
    audible: usize,
    notes: usize,
    plans: usize,
    voice_capacity: usize,
    note_capacity: usize,
    taps: [Option<(u32, u8, u8, u8, u32)>; 16],
    problems: [u64; 11],
}
fn state(rt: &Runtime) -> State {
    let s = rt.stats();
    State {
        now: rt.now(),
        voices: rt.voice_count(),
        audible: rt.audible_voice_count(),
        notes: rt.note_count(),
        plans: rt.plan_count(),
        voice_capacity: s.voice_capacity,
        note_capacity: rt.note_capacity(),
        taps: rt
            .voice_taps()
            .map(|v| v.map(|v| (v.group, v.key, v.velocity, v.phase, v.level.to_bits()))),
        problems: [
            s.voice_drops,
            s.voice_pool_refusals,
            s.waiting_voice_replacements,
            s.refused_starts,
            s.stretch_fallbacks,
            s.cold_starts,
            s.nonfinite_frames,
            s.stream_underruns,
            s.stream_capacity_refusals,
            s.stream_disconnected_refusals,
            s.growth_failures,
        ],
    }
}
struct Receipt {
    pcm: Vec<[u32; 2]>,
    states: Vec<State>,
    pooled: u64,
}
fn chunks(rt: &mut Runtime, count: usize, output: &mut Receipt) {
    for _ in 0..count {
        let mut block = [[0.; 2]; 64];
        without_heap(|| rt.render(&mut block).unwrap());
        assert!(block.iter().flatten().all(|x| x.is_finite()));
        output.pcm.extend(block.map(|x| x.map(f32::to_bits)));
        let s = state(rt);
        assert_eq!(s.problems, [0; 11]);
        assert_eq!(s.voice_capacity, NOTES * 4);
        assert_eq!(s.note_capacity, NOTES * 4);
        output.states.push(s);
    }
}
fn run_with(threads: usize, make_plan: fn(bool) -> Prepared) -> Receipt {
    let (mut rt, mut control) = Runtime::with_plan_updates(make_plan(true), limits(), 3, 1).unwrap();
    rt.set_threads(Threads::Fixed(threads));
    let mut first = Vec::with_capacity(NOTES);
    let mut second = Vec::with_capacity(NOTES);
    let mut third = Vec::with_capacity(NOTES);
    // All thread construction and first-use setup precede the all-thread audit.
    on(&mut rt, 0, &mut first);
    for _ in 0..8 {
        rt.render(&mut [[0.; 2]; 64]).unwrap();
    }
    let mut result = Receipt {
        pcm: Vec::new(),
        states: Vec::new(),
        pooled: 0,
    };
    chunks(&mut rt, 40, &mut result);
    let old = rt.active_plan();
    let request = control.submit(Box::new(make_plan(false))).unwrap();
    without_heap(|| {
        assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
        on(&mut rt, 1, &mut second);
    });
    assert_ne!(rt.active_plan(), old);
    assert_eq!(rt.voice_count(), NOTES * 2);
    chunks(&mut rt, 40, &mut result);
    without_heap(|| off(&mut rt, &first));
    chunks(&mut rt, 4, &mut result);
    without_heap(|| {
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    // Returned generations are consumed/dropped off the guarded callback domain.
    assert_eq!(control.retired().unwrap().request, 0);
    let request = control.submit(Box::new(make_plan(true))).unwrap();
    without_heap(|| {
        assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
        on(&mut rt, 2, &mut third);
    });
    assert_eq!(rt.voice_count(), NOTES * 2);
    chunks(&mut rt, 40, &mut result);
    without_heap(|| {
        off(&mut rt, &second);
        off(&mut rt, &third);
    });
    chunks(&mut rt, 4, &mut result);
    without_heap(|| {
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    drop(control.retired().unwrap());
    assert_eq!(rt.voice_count(), 0);
    assert_eq!(rt.note_count(), 0);
    assert_eq!(rt.plan_count(), 1);
    result.pooled = rt.parallel_blocks();
    if threads > 1 {
        assert!(result.pooled > 10, "fixture silently stayed serial");
    }
    // This counter proves pooled dispatch only. Actual lane>0 task execution
    // must also be witnessed by the separate cfg(test) unit oracle before promotion.
    result
}
fn run(threads: usize) -> Receipt {
    run_with(threads, plan)
}
#[test]
fn mixed_generations_keep_pcm_state_and_all_thread_heap_across_filter_shape_changes() {
    let reference = run(1);
    for threads in [2, 4] {
        let got = run(threads);
        assert_eq!(got.pcm, reference.pcm, "threads {threads}: PCM");
        assert_eq!(
            got.states, reference.states,
            "threads {threads}: semantic state"
        );
        let hash = got
            .pcm
            .iter()
            .flatten()
            .fold(0xcbf29ce484222325u64, |h, x| {
                (h ^ u64::from(*x)).wrapping_mul(0x100000001b3)
            });
        println!(
            "FILTER_PROJECTION_ORACLE threads={threads} hash={hash:016x} frames={} states={} pooled={}",
            got.pcm.len(),
            got.states.len(),
            got.pooled
        );
    }
    kontakt_generations_keep_exact_pcm_state_and_all_thread_heap();
}

// Use all three actual Kontakt color kernels in every generation. The existing
// CountAll allocator observes every rendering thread, not just this test thread.
fn kontakt_plan(first: bool) -> Prepared {
    use sampler_ir::fx::filter::KontaktFx;
    let knobs = if first {
        [0.63, 0.18, 0.27, 0.4, 0.22]
    } else {
        [0.31, 0.42, 0.71, 0.2, 0.61]
    };
    let filters = [
        KontaktFx::SimpleLpHp { lp: knobs[0], hp: knobs[1] },
        KontaktFx::DualSkf(knobs),
        KontaktFx::Monark {
            mode: if first { 0 } else { 3 },
            variant: if first { 0 } else { 2 },
            knobs,
        },
    ];
    let chains = filters.into_iter().map(|f| {
        VoiceChain::new(vec![], vec![Processor::Filter(FilterSettings::Kontakt(f))], 0).unwrap()
    }).collect();
    let route = |target, depth| ModProgram {
        sources: vec![ModSource::Constant],
        routes: vec![ModRoute::new(0, target, depth)],
        ..ModProgram::default()
    };
    plan(false)
        .with_voice_chains(chains, (0..NOTES).map(|i| Some(i % 3)).collect())
        .unwrap()
        .with_voice_modulation(
            vec![route(ModTarget::ProcessorNativeCutoff(0), 0.125),
                 route(ModTarget::ProcessorNativeResonance(1), -0.0625),
                 route(ModTarget::Decibels, -3.)],
            (0..NOTES).map(|i| Some(i % 3)).collect(),
            vec![0; NOTES],
        )
        .unwrap()
}
fn kontakt_generations_keep_exact_pcm_state_and_all_thread_heap() {
    let reference = run_with(1, kontakt_plan);
    assert!(reference.pcm.iter().flatten().any(|bits| f32::from_bits(*bits) != 0.));
    for threads in [2, 4] {
        let got = run_with(threads, kontakt_plan);
        assert_eq!(got.pcm, reference.pcm, "Kontakt threads {threads}: PCM");
        assert_eq!(got.states, reference.states, "Kontakt threads {threads}: semantic state");
        let hash = got.pcm.iter().flatten().fold(0xcbf29ce484222325u64,
            |h, x| (h ^ u64::from(*x)).wrapping_mul(0x100000001b3));
        println!("KONTAKT_PROJECTION_ORACLE threads={threads} hash={hash:016x} frames={} states={} pooled={}",
            got.pcm.len(), got.states.len(), got.pooled);
    }
}
