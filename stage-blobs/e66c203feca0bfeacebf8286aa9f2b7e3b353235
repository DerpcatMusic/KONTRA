use sampler_core::{Input, Limits, OutputLowPass, Pcm, Protocol, Runtime};
use sampler_ir as ir;

fn input() -> Input { Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: None } }

fn source(kind: usize) -> ir::Instrument {
    let mut i = ir::Instrument::default();
    i.assets.push(ir::Asset { location: ir::AssetLocation::Path("generated.wav".into()),
        encoding: ir::Encoding::Wav, root_key: None, loops: Vec::new() });
    i.zones.push(ir::Zone { pitch: ir::KeyTracking::Fixed, velocity: ir::VelocityResponse::None,
        ..ir::Zone::new(ir::AssetRef(0)) });
    if kind != 0 {
        let source = if kind == 2 {
            ir::ModulationSource::Breakpoints(ir::Breakpoints {
                points: vec![ir::Breakpoint { time: ir::Time::Milliseconds(1.), level: 1., shape: ir::Curve::Linear }],
                sustain: Some(0),
            })
        } else {
            ir::ModulationSource::Envelope(ir::Envelope { attack: ir::Time::Milliseconds(2.),
                release: ir::Time::Milliseconds(10.), ..Default::default() })
        };
        i.modulators.push(ir::Modulator { scope: ir::Scope::Voice, source });
        if kind == 1 { i.zones[0].amplitude = Some(ir::ModulatorRef(0)); }
        else {
            i.routes.push(ir::Route::new(ir::ModulatorRef(0), ir::Target::Amplitude, ir::Depth::Normalized(1.)));
            i.zones[0].routes.push(ir::RouteRef(0));
        }
    }
    i
}

fn render(i: &ir::Instrument, defaults: bool) -> Vec<[f32; 2]> {
    let pcm = Pcm::new(48_000, vec![[0.5; 2]; 48_000].into_boxed_slice()).unwrap();
    let options = sampler_core::lower::Options { mpe: None };
    let plan = sampler_core::lower::lower_with(i, 48_000, vec![pcm], &options, |_, _| unreachable!()).unwrap();
    let limits = Limits::for_plan(&plan, 4, 4);
    let mut rt = Runtime::new(plan, limits).unwrap();
    if defaults { rt.set_fallback_envelope(0.1, 0.2).unwrap(); }
    rt.trigger(input(), 60, 1.).unwrap();
    let mut out = vec![[0.; 2]; 256];
    rt.render(&mut out).unwrap();
    rt.note_off(input(), None).unwrap();
    let mut tail = vec![[0.; 2]; 512];
    rt.render(&mut tail).unwrap();
    out.extend(tail);
    out
}

#[test]
fn v1_global_fallback_excludes_authored_ahdsr_flex_and_amplitude_routes() {
    let fallback = source(0);
    assert_ne!(render(&fallback, false), render(&fallback, true));
    for kind in 1..=3 {
        let i = source(kind);
        let saved = i.clone();
        let dry = render(&i, false);
        assert!(dry.iter().any(|f| f[0] > 0.1));
        assert_eq!(dry, render(&i, true), "authored amplitude kind {kind} excludes global defaults");
        assert_eq!(i, saved, "source IR stays immutable");
    }
}

#[test]
fn v1_global_tone_dry_seed_stereo_history_and_f32_comparison() {
    let mut filter = OutputLowPass::new(48_000).unwrap();
    let mut state = [[0.; 2]; 2];
    let mut dry = [[0.25, -0.5]; 64];
    let original = dry;
    filter.process(&mut dry, &mut state, 20_000., 0).unwrap();
    assert_eq!(dry, original, "default bypass is exactly dry");
    let mut enable = [[0.25, -0.5]; 64];
    filter.process(&mut enable, &mut state, 20., 64).unwrap();
    assert_eq!(enable, original, "seeding preserves a steady signal on enable");

    let mut frames: Vec<_> = (0..2048).map(|n| [
        (n as f32 * 0.17).sin() * 0.7, (n as f32 * 0.31).cos() * 0.3]).collect();
    let mut reference = frames.clone();
    let cutoff = 1000f32;
    let a = 1.0 - (-std::f32::consts::TAU * cutoff / 48_000.).exp();
    let mut v1 = [0f32; 2];
    for frame in &mut reference {
        for (x, history) in frame.iter_mut().zip(&mut v1) { *history += a * (*x - *history); *x = *history; }
    }
    filter.process(&mut frames, &mut [[0.; 2]; 2], f64::from(cutoff), 128).unwrap();
    let error = frames.iter().flatten().zip(reference.iter().flatten()).map(|(a,b)| (*a - *b).abs()).fold(0f32, f32::max);
    eprintln!("v1 f32 vs shared f64 Tone max absolute error: {error}");
    assert!(error < 4e-6, "same one-pole law, without a bit-exact claim");
    let held = state;
    assert!(filter.process(&mut dry, &mut state, f64::NAN, 2200).is_err());
    assert_eq!(state, held);
}
