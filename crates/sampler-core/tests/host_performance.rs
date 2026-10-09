mod support;
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

fn input_tone_ir() -> ir::Instrument {
    let mut i=source(0);
    i.groups.push(ir::Group { output: ir::Output::Bus(ir::BusRef(0)), ..Default::default() });
    i.zones[0].group=Some(ir::GroupRef(0));
    i.input_bus=Some(ir::BusRef(1));
    i.chains.push(ir::Chain { scope:ir::Scope::Bus(ir::BusRef(1)),
        pre_amplitude:vec![ir::Processor::Rectify(ir::Rectifier::Full)], post_amplitude:vec![] });
    i.buses=vec![
        ir::Bus { name:"upstream".into(), chain:None, sends:vec![], output:ir::Output::Bus(ir::BusRef(1)), gain:ir::Gain::UNITY },
        ir::Bus { name:"insert".into(), chain:Some(ir::ChainRef(0)), sends:vec![], output:ir::Output::Bus(ir::BusRef(2)), gain:ir::Gain::UNITY },
        ir::Bus { name:"downstream".into(), chain:None, sends:vec![], output:ir::Output::Master, gain:ir::Gain::UNITY },
    ];
    i
}

#[test]
fn input_tone_precedes_nonlinear_insert_and_direct_descendants_filter_once() {
    let i=input_tone_ir();
    let samples:Vec<_>=(0..2048).map(|n| [(n as f32*0.17).sin()*0.5;2]).collect();
    let run=|cutoff,direct:Option<usize>| {
        let pcm=Pcm::new(48_000,samples.clone().into_boxed_slice()).unwrap();
        let plan=sampler_core::lower::lower_with(&i,48_000,vec![pcm],&sampler_core::lower::Options{mpe:None},|_,_|unreachable!()).unwrap();
        let limits=Limits::for_plan(&plan,4,4);
        let mut rt=Runtime::new(plan,limits).unwrap(); rt.set_part_tone_cutoff(cutoff).unwrap();
        if let Some(bus)=direct { rt.set_bus_mix(bus,sampler_core::BusMix { gain:[1.;2],output:Some(0) }).unwrap(); }
        rt.trigger(input(),60,1.).unwrap();
        let mut main=vec![[0.;2];512];let mut separate=vec![[0.;2];512];
        rt.render_split(&mut main,&mut [&mut separate]).unwrap();
        (main,separate)
    };
    let mut expected=samples[..512].to_vec();
    OutputLowPass::new(48_000).unwrap().process(&mut expected,&mut [[0.;2];2],1000.,0).unwrap();
    for f in &mut expected { *f=f.map(f32::abs); }
    let (wet,_)=run(1000.,None);
    let error=wet.iter().flatten().zip(expected.iter().flatten()).map(|(a,b)|(a-b).abs()).fold(0f32,f32::max);
    assert!(error<1e-6,"input Tone precedes rectification, error={error}");
    let dry=run(20_000.,None).0;
    assert_eq!(dry,samples[..512].iter().map(|f|f.map(f32::abs)).collect::<Vec<_>>());
    for bus in [1,2] {
        let (main,direct)=run(1000.,Some(bus));
        assert!(main.iter().all(|f|*f==[0.;2]));
        assert_eq!(direct,wet,"Tone must reach downstream direct bus{bus} exactly once");
    }
    let (main,direct)=run(1000.,Some(0));
    assert!(main.iter().all(|f|*f==[0.;2]));
    assert_eq!(direct,&samples[..512],"upstream direct escapes Tone and inserts, as v1 does");
    let mut invalid=i.clone();invalid.input_bus=Some(ir::BusRef(3));
    assert!(invalid.validate().is_err());
}

fn tone_plan(value:f32,traced:bool) -> sampler_core::Prepared {
    let pcm=Pcm::new(48_000,vec![[value;2];48000].into_boxed_slice()).unwrap();
    let plan=sampler_core::lower::lower_with(&input_tone_ir(),48_000,vec![pcm],&sampler_core::lower::Options{mpe:None},|_,_|unreachable!()).unwrap();
    if traced { plan.with_signal_trace(1024).unwrap() } else { plan }
}

#[test]
fn input_tone_trace_exposes_pre_insert_levels_without_heap() {
    let plan=tone_plan(0.5,true);let limits=Limits::for_plan(&plan,4,4);
    let mut rt=Runtime::new(plan,limits).unwrap();let reader=rt.signal_trace_reader().unwrap();
    rt.set_part_tone_cutoff(1000.).unwrap();rt.trigger(input(),60,1.).unwrap();
    support::without_heap(|| rt.render(&mut [[0.;2];64]).unwrap());
    let rows=reader.drain();
    let node=reader.graph.nodes.iter().find(|n|n.kind=="part_tone").unwrap();
    let tone=rows.iter().find(|r|r.node==node.id).unwrap();
    assert!((tone.input.dc[0]-0.5).abs()<1e-9);
    assert!(tone.output.dc[0]>0. && tone.output.dc[0]<tone.input.dc[0]);
    let input_node=reader.graph.nodes.iter().find(|n|n.kind=="bus_input" && n.bus==Some(1)).unwrap();
    let summed=rows.iter().find(|r|r.node==input_node.id).unwrap();
    assert_eq!(summed.output.dc,tone.input.dc,"sum is observed before Tone");
    let fx=reader.graph.nodes.iter().find(|n|n.kind=="bus_fx" && n.bus==Some(1)).unwrap();
    let effected=rows.iter().find(|r|r.node==fx.id).unwrap();
    assert_eq!(effected.input.dc,tone.output.dc,"native FX receives Tone once");
    assert!(reader.graph.edges.iter().any(|e| e.from==input_node.id && e.to==node.id));
}

#[test]
fn input_tone_retained_generations_keep_histories_and_live_cutoff_until_tail_retires() {
    let old=tone_plan(0.5,false);let limits=Limits::for_plan(&old,4,4);
    let (mut rt,mut control)=Runtime::with_plan_updates(old,limits,2,1).unwrap();
    let mut old_ref=Runtime::new(tone_plan(0.5,false),limits).unwrap();
    for r in [&mut rt,&mut old_ref] {r.set_part_tone_cutoff(1000.).unwrap();r.trigger(input(),60,1.).unwrap();r.render(&mut [[0.;2];128]).unwrap();}
    let new=tone_plan(0.25,false);let request=control.submit(Box::new(new)).unwrap();
    let mut new_ref=Runtime::new(tone_plan(0.25,false),limits).unwrap();
    let mut other=input();other.key=61;
    support::without_heap(|| {
        assert_eq!(rt.poll_plan_update(),Ok(Some(request)));
        assert!(rt.has_input_tone());rt.set_part_tone_cutoff(20.).unwrap();
        rt.trigger(other,61,1.).unwrap();rt.render(&mut [[0.;2];0]).unwrap();
    });
    old_ref.set_part_tone_cutoff(20.).unwrap();new_ref.set_part_tone_cutoff(20.).unwrap();new_ref.trigger(other,61,1.).unwrap();
    let (mut combined,mut old,mut new)=([[0.;2];128],[[0.;2];128],[[0.;2];128]);
    support::without_heap(|| rt.render(&mut combined).unwrap());old_ref.render(&mut old).unwrap();new_ref.render(&mut new).unwrap();
    for n in 0..128 {for c in 0..2 {assert!((combined[n][c]-old[n][c]-new[n][c]).abs()<1e-6,"per-generation Tone history");}}
    support::without_heap(|| {
        rt.note_off(input(),None).unwrap();rt.render(&mut [[0.;2];64]).unwrap();rt.flush_ended(|_|true);
        assert_eq!(rt.note_count(),1,"only the new generation note remains");
        assert_eq!(rt.collect_retired_plans(),0,"Tone's zero-input tail retains old generation");
        for _ in 0..300 {rt.render(&mut [[0.;2];64]).unwrap();}
        rt.flush_ended(|_|true);
        assert_eq!(rt.collect_retired_plans(),1,"old Tone tail eventually retires");
        rt.panic();rt.render(&mut [[0.;2];64]).unwrap();
    });
    assert!(!rt.has_input_tone() || rt.plan_count()==1);
}
