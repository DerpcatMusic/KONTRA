//! Wave 2A Deliverable 1 & 4: Two-effect RT spike (`Tape` + `ThreeBandShelves`)
//! and RT-paranoid verification across coupled Kontakt & UVI effect models.
//!
//! Measures and compares under the counting allocator (`GlobalAlloc`):
//! 1. `build -> Effect` (in-place DSP slot update via `ModelId` / `Action::EffectKnob` & `SetEnginePar`)
//! 2. `build -> param writes` (per-field `Action::EffectField` updates)
//!
//! Proves ZERO audio-thread heap allocations for both Kontakt (`Tape`) and UVI
//! (`ThreeBandShelves`, `SparkVerb`, `DigitalEq`) live knob updates plus audio rendering.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

use kontra_engine::ir;
use kontra_engine::{Action, Engine, Plan};
use kontra_kontakt_effects::tape;
use kontra_plan::{EnginePar, EnginePars, ScriptPlan};
use kontra_stream::{Store, StoreConfig};
use kontra_vm::{HostApi, HostCall, HostValue, Program, Text};

struct Counting;
thread_local! { static COUNT: Cell<Option<usize>> = const { Cell::new(None) }; }
#[global_allocator]
static ALLOCATOR: Counting = Counting;

// SAFETY: forwards all allocation calls directly to System; thread-local counter never allocates.
#[expect(unsafe_code, reason = "isolated RT-paranoid counting test allocator")]
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

fn call(e: &mut Engine, action: Action, ints: &[i64]) -> HostValue {
    let mut out = Text::EMPTY;
    e.call(HostCall {
        action,
        context: 0,
        ints,
        texts: &[],
        cells: &mut [],
        text_cells: &mut [],
        text_out: &mut out,
    })
}

fn measure_zero_alloc<T>(label: &str, f: impl FnOnce() -> T) -> T {
    COUNT.with(|n| n.set(Some(0)));
    let out = f();
    let allocs = COUNT.with(|n| n.replace(None)).unwrap_or(usize::MAX);
    assert_eq!(allocs, 0, "{label}: audio thread allocated {allocs} times");
    out
}

/// Builds a minimal engine with a Tape Saturator in instrument insert slot 0
/// wired to KSP-style `SetEnginePar` knobs `0..=3` through `tape::MODULE`.
fn tape_engine() -> (Engine, Store) {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut bytes = Vec::new();
    for v in [0.5f32, 0.4, 0.6] {
        bytes.extend(v.to_le_bytes());
    }
    bytes.push(0); // quality = false
    let model = tape::model(&bytes).expect("tape model");
    let kind = model.build(&model.saved).expect("tape initial build");
    let mut plan = Plan::new(vec![], 0);
    plan.inserts = vec![Some(ir::Effect {
        kind,
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    plan.slot_models = vec![(ir::RackAt::Inserts, 0, model)];
    let script = ScriptPlan {
        program: Program::default(),
        callbacks: kontra_plan::Callbacks::default(),
        coroutines: 4,
        data: Vec::new(),
        text_data: Vec::new(),
        source_folder: None,
        library_folder: None,
        rc_folder: None,
        factory_folder: None,
        restore: Vec::new(),
        restore_texts: Vec::new(),
        controls: kontra_plan::Controls::default(),
        module_pars: EnginePars {
            first: 0,
            by_id: vec![
                Some(EnginePar::Knob(0)),
                Some(EnginePar::Knob(1)),
                Some(EnginePar::Knob(2)),
                Some(EnginePar::Knob(3)),
            ]
            .into(),
            readouts: Box::default(),
            owners: Box::default(),
            accepts: Box::default(),
        },
        view: None,
        reverb_laws: ir::ReverbControls::from_fn(|_| ir::Law::Linear { lo: 0.0, hi: 1.0 }),
        reverb_networks: Vec::new(),
        lfo_units: Vec::new(),
        warp_modes: Vec::new(),
        mod_kinds: Vec::new(),
        knobs: tape::rows()
            .into_iter()
            .map(|(_, target, law)| (target, law))
            .collect(),
        foreign: None,
        files: None,
        arrays: Vec::new(),
        hears_own_releases: false,
        held_release_ends_children: false,
        links: Vec::new(),
    };
    plan.scripts = vec![script];
    let engine = Engine::new(Arc::new(plan), store.client(), 48_000);
    (engine, store)
}

/// Loads a real UVI program containing `ThreeBandShelves`, `SparkVerb`, and
/// `DigitalEq` with script references, prepares it, and builds an `Engine`.
fn uvi_coupled_engine() -> (Engine, Store) {
    let xml = r#"<Program Gain="1">
      <Scripts><ScriptProcessor Bypass="0"><script><![CDATA[
        local s = Program.inserts[1]
        s:setParameter("FreqLowMid", 250)
        s:setParameter("GainLow", 3)
        s:setParameter("GainMid", 1)
        s:setParameter("GainHigh", 4)
        local v = Program.inserts[2]
        v:setParameter("RoomSize", 25)
        v:setParameter("DecayTime", 2.5)
        v:setParameter("Mix", 0.5)
        v:setParameter("MixMode", 1)
        local eq = Program.inserts[3]
        eq:setParameter("GainScale", 1.5)
        eq:setParameter("Gain1", 4.0)
      ]]></script></ScriptProcessor></Scripts>
      <Inserts>
        <ThreeBandShelves FreqLowMid="200" FreqMidHigh="4000" GainLow="3" GainMid="-1" GainHigh="4"/>
        <SparkVerb RoomSize="20" DecayTime="2" Mix="0.5" MixMode="1" PreDelay="10" Rolloff="8000" Width="0.8"/>
        <DigitalEq GainScale="1" Enabled1="1" Type1="6" Slope1="1" Freq1="1000" Gain1="3" Bandwidth1="1"/>
      </Inserts>
    </Program>"#;
    let dir = std::env::temp_dir().join(format!("kontra-rt-spike-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("spike.uvip");
    std::fs::write(&path, xml).unwrap();
    let inst = kontra_format_uvi::load(&path).expect("load uvi spike");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(inst.slot_models.len(), 3, "all 3 UVI inserts registered");

    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (plan, _) = kontra_prepare::prepare(&inst, 48_000, &mut store, &|_| {
        Err(std::io::Error::other("unused"))
    });
    let engine = Engine::new(Arc::new(plan), store.client(), 48_000);
    (engine, store)
}

#[test]
fn rt_spike_tape_and_uvi_coupled_models_zero_alloc_and_benchmark() {
    let (mut tape_eng, _s1) = tape_engine();
    let (mut uvi_eng, _s2) = uvi_coupled_engine();
    let mut audio = [[0.0f32; 2]; 128];

    // Warm up one block off-measurement so any one-time lazy state is settled.
    tape_eng.process(&mut audio, &[]);
    uvi_eng.process(&mut audio, &[]);

    // 1. Kontakt Tape Saturator via ModelId (in-place DSP slot update).
    const ITERS: usize = 2_000;
    let t0 = Instant::now();
    measure_zero_alloc("Kontakt Tape SetEnginePar + process", || {
        for i in 0..ITERS {
            let v = ((i * 500) % 1_000_000) as i64;
            call(&mut tape_eng, Action::SetEnginePar, &[0, v, -1, 0, 1]);
            call(
                &mut tape_eng,
                Action::SetEnginePar,
                &[1, 1_000_000 - v, -1, 0, 1],
            );
            call(&mut tape_eng, Action::SetEnginePar, &[2, v, -1, 0, 1]);
            if i % 64 == 0 {
                tape_eng.process(&mut audio, &[]);
            }
        }
    });
    let tape_ns = t0.elapsed().as_nanos() as f64 / (ITERS * 3) as f64;

    // 2. UVI ThreeBandShelves via ModelId (Action::EffectKnob, single pure build -> Effect).
    let t1 = Instant::now();
    measure_zero_alloc("UVI ThreeBandShelves Action::EffectKnob + process", || {
        for i in 0..ITERS {
            let mid = -6.0f32 + (i % 120) as f32 * 0.1;
            let low = 3.0f32 + (i % 60) as f32 * 0.05;
            call(
                &mut uvi_eng,
                Action::EffectKnob,
                &[2, 0, 0, 2, i64::from(low.to_bits())],
            );
            call(
                &mut uvi_eng,
                Action::EffectKnob,
                &[2, 0, 0, 3, i64::from(mid.to_bits())],
            );
            if i % 64 == 0 {
                uvi_eng.process(&mut audio, &[]);
            }
        }
    });
    let model_eq_ns = t1.elapsed().as_nanos() as f64 / (ITERS * 2) as f64;

    // 3. Comparison baseline: UVI ThreeBandShelves via 3 separate param writes (Action::EffectField).
    let g0 = i64::from(ir::EffectParam::BellGain(0).code().unwrap());
    let g1 = i64::from(ir::EffectParam::BellGain(1).code().unwrap());
    let out_code = i64::from(ir::EffectParam::Output.code().unwrap());
    let t2 = Instant::now();
    measure_zero_alloc("UVI ThreeBandShelves Action::EffectField baseline", || {
        for i in 0..ITERS {
            let mid = -6.0f32 + (i % 120) as f32 * 0.1;
            let gain0 = ((3.0 - mid) * 1000.0) as i64;
            let gain1 = ((4.0 - mid) * 1000.0) as i64;
            let out_lin = (10f32.powf(mid / 20.0) * 1000.0) as i64;
            call(&mut uvi_eng, Action::EffectField, &[2, 0, 0, g0, gain0]);
            call(&mut uvi_eng, Action::EffectField, &[2, 0, 0, g1, gain1]);
            call(
                &mut uvi_eng,
                Action::EffectField,
                &[2, 0, 0, out_code, out_lin],
            );
        }
    });
    let field_eq_ns = t2.elapsed().as_nanos() as f64 / ITERS as f64;

    // 4. UVI SparkVerb (coupled RoomSize * DecayTime + equal-power Mix/MixMode) & DigitalEq (GainScale).
    measure_zero_alloc(
        "UVI SparkVerb + DigitalEq Action::EffectKnob + process",
        || {
            for i in 0..256 {
                let room = 10.0f32 + (i % 40) as f32;
                let mix = (i % 100) as f32 / 100.0;
                let scale = 0.5f32 + (i % 20) as f32 * 0.1;
                // SparkVerb in slot 1
                call(
                    &mut uvi_eng,
                    Action::EffectKnob,
                    &[2, 0, 1, 0, i64::from(room.to_bits())],
                );
                call(
                    &mut uvi_eng,
                    Action::EffectKnob,
                    &[2, 0, 1, 5, i64::from(mix.to_bits())],
                );
                // DigitalEq in slot 2
                call(
                    &mut uvi_eng,
                    Action::EffectKnob,
                    &[2, 0, 2, 0, i64::from(scale.to_bits())],
                );
                if i % 32 == 0 {
                    uvi_eng.process(&mut audio, &[]);
                }
            }
        },
    );

    eprintln!(
        "RT SPIKE RESULTS (0 allocs everywhere):\n  \
         Kontakt Tape (ModelId in-place build->Effect): {tape_ns:.1} ns/knob\n  \
         UVI ThreeBandShelves (ModelId build->Effect):  {model_eq_ns:.1} ns/knob\n  \
         UVI ThreeBandShelves (3x EffectField writes):  {field_eq_ns:.1} ns/coupled-move"
    );
}
