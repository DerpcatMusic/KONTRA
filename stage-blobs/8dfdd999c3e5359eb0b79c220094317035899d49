//! An authored clear program and generated WAV: translate, load and render.

use sampler_core::{Input, Limits, Protocol, Runtime};
use sampler_ir as ir;

#[test]
fn large_falcon_program_xml_is_bounded_without_rejecting_installed_node_counts() {
    let xml = format!("<Program>{}</Program>", "<Properties/>".repeat(210_000));
    assert!(sampler_uvi::parse_program_xml(&xml).is_ok());
    let excessive = format!("<Program>{}</Program>", "<p/>".repeat(1_000_000));
    assert!(sampler_uvi::parse_program_xml(&excessive).is_err());
    assert!(sampler_uvi::parse_program_xml(&" ".repeat((32 << 20) + 1)).is_err());
    assert!(sampler_uvi::parse_program_xml("<!DOCTYPE Program><Program/>").is_err());
}

const PROGRAM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<UVI4>
  <Program Name="Fixture" Gain="0.5">
    <ControlSignalSources>
      <DAHDSR Name="Amp Env" AttackTime="0.01" DecayTime="0.2" SustainLevel="0.5" ReleaseTime="0.3" VelocityAmount="1" VelocitySens="0.5" DecayCurve="0.5"/>
      <LFO Name="Vibrato" Freq="5"/>
      <AHD Name="Steps"/>
    </ControlSignalSources>
    <EventProcessors>
      <ScriptProcessor Name="Arp"><script><![CDATA[function onNote(e) playNote(e.note, e.velocity) end]]></script></ScriptProcessor>
    </EventProcessors>
    <Layers>
      <Layer Name="Main">
        <Keygroups>
          <Keygroup Name="Low" LowKey="48" HighKey="72" LowVelocity="1" HighVelocity="127" HighKeyFade="2">
            <Connections>
              <SignalConnection Source="$Program/Amp Env" Destination="Gain" Ratio="1"/>
              <SignalConnection Source="$Program/Vibrato" Destination="Pitch" Ratio="0.1"/>
              <SignalConnection Source="$Program/Steps" Destination="Gain" Ratio="0.5"/>
            </Connections>
            <Oscillators>
              <SamplePlayer Name="Osc" SamplePath="samples/sine.wav" BaseNote="60" CoarseTune="2" FineTune="-50" Pitch="0.25" Gain="0.8">
                <PlaybackOptions Start="10" Stop="47000"><Loop Start="1000" End="9000" Type="0"/></PlaybackOptions>
              </SamplePlayer>
              <SamplePlayer Name="Bank" SamplePath="$Bank.ufs/Samples/x.wav"/>
              <SamplePlayer Name="Off" SamplePath="samples/sine.wav" Bypass="1"/>
            </Oscillators>
          </Keygroup>
        </Keygroups>
      </Layer>
      <Layer Name="Muted" Mute="1"/>
    </Layers>
  </Program>
</UVI4>"#;

fn wav(rate: u32, frames: &[i16]) -> Vec<u8> {
    let data = (frames.len() * 2) as u32;
    let mut out = Vec::new();
    for chunk in [
        b"RIFF".as_slice(),
        &(36 + data).to_le_bytes(),
        b"WAVEfmt ",
        &16u32.to_le_bytes(),
        &1u16.to_le_bytes(),
        &1u16.to_le_bytes(),
    ] {
        out.extend_from_slice(chunk);
    }
    for chunk in [rate.to_le_bytes(), (rate * 2).to_le_bytes()] {
        out.extend_from_slice(&chunk);
    }
    for chunk in [2u16.to_le_bytes(), 16u16.to_le_bytes()] {
        out.extend_from_slice(&chunk);
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    frames
        .iter()
        .for_each(|s| out.extend_from_slice(&s.to_le_bytes()));
    out
}

#[test]
fn authored_program_translates_loads_and_renders() {
    let dir = std::env::temp_dir().join(format!("sampler-uvi-fixture-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("samples")).unwrap();
    let sine: Vec<i16> = (0..48000)
        .map(|i| ((i as f64 * 440.0 * std::f64::consts::TAU / 48000.0).sin() * 16000.0) as i16)
        .collect();
    std::fs::write(dir.join("samples/sine.wav"), wav(48000, &sine)).unwrap();
    let program = dir.join("Fixture.uvip");
    std::fs::write(&program, PROGRAM).unwrap();

    let uvi = sampler_uvi::read(&program).unwrap();
    let ir = &uvi.instrument;
    assert_eq!(
        (ir.name.as_str(), &ir.source),
        ("Fixture", &ir::SourceFormat::Uvi)
    );
    assert_eq!(
        (ir.groups.len(), ir.zones.len(), ir.assets.len()),
        (2, 1, 1), // the script may pick one of the keygroup's two oscillators
    );
    let zone = &ir.zones[0];
    assert_eq!((zone.keys.low, zone.keys.high), (48, 72));
    assert_eq!(zone.pitch, ir::KeyTracking::Tracked { root: 60 });
    assert_eq!(zone.tune, ir::Pitch::Semitones(1.75));
    assert_eq!(zone.gain, ir::Gain::Linear(0.8));
    assert_eq!(zone.velocity, ir::VelocityResponse::Power(2.0));
    assert_eq!((zone.playback.start, zone.playback.end), (10, Some(47000)));
    assert!(matches!(
        zone.playback.looping,
        ir::Looping::Continuous(ir::LoopRange {
            start: 1000,
            end: 9000,
            ..
        })
    ));
    assert!(zone.amplitude.is_some());
    let ir::ModulationSource::Envelope(env) = &ir.modulators[zone.amplitude.unwrap().0].source
    else {
        panic!()
    };
    let k = 2.0 * ((1.5_f64) / (0.5)).ln();
    assert_eq!(env.decay_shape, ir::Curve::Exponential(k));
    assert_eq!(env.attack_shape, ir::Curve::Linear);
    assert_eq!(ir.behaviors.len(), 1);
    assert_eq!(ir.behaviors[0].language, ir::Language::Lua);
    let features: Vec<&str> = ir.unsupported.iter().map(|u| u.feature.as_str()).collect();
    for expected in [
        "HighKeyFade",
        "AHD note-off release (default release used)",
        "sample outside the program's bank",
        "keygroup oscillators all play (the script may pick one per note)",
    ] {
        assert!(
            features.contains(&expected),
            "{expected} missing from {features:?}"
        );
    }
    // The vibrato is a route: +0.1 semitone per unit of a 5 Hz sine.
    let route = &ir.routes[zone.routes[0].0];
    assert_eq!(zone.routes.len(), 2, "vibrato and the AHD gain route");
    assert_eq!(route.target, ir::Target::Pitch);
    assert_eq!(route.depth, ir::Depth::Pitch(ir::Pitch::Semitones(0.1)));
    assert!(matches!(
        ir.modulators[route.source.0].source,
        ir::ModulationSource::Lfo(ir::Lfo {
            shape: ir::LfoShape::Sine,
            rate: ir::Frequency::Hertz(5.0),
            ..
        })
    ));

    let loaded = sampler_uvi::load(&program, 48000).unwrap();
    assert!(
        loaded
            .instrument
            .unsupported
            .iter()
            .any(|u| u.feature == "script"),
        "Lua is reported, not run"
    );
    let limits = Limits {
        notes: 4,
        channels: 1,
        performances: 1,
        families: 4,
        expressions: 4,
        voices: 8,
        decisions: 16,
        commands: 16,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    };
    let mut rt = Runtime::new(loaded.plan, limits).unwrap();
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    };
    rt.trigger(input, 60, 1.0).unwrap();
    let mut out = vec![[0.0f32; 2]; 4800];
    rt.render(&mut out).unwrap();
    let peak = out.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
    assert!(peak > 0.05, "audible: peak {peak}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn protected_and_foreign_xml_is_rejected() {
    let folder = std::path::Path::new(".");
    assert!(sampler_uvi::translate(r#"<Program Name="x" Password="y"/>"#, folder).is_err());
    assert!(sampler_uvi::translate("<Patch/>", folder).is_err());
}

/// A sine keygroup, either direct to the output or sent pre-fader to an aux
/// bus with the given inserts (the layer's own fader is shut).
fn insert_program(aux: Option<&str>) -> String {
    let (layer_gain, router, auxs) = match aux {
        Some(inserts) => (
            0.0,
            r#"<BusRouters><BusRouter Name="Send" Bypass="0" Gain="1" PreFader="1" Destination="../../Aux0"/></BusRouters>"#,
            format!(r#"<Auxs><AuxEffect Name="Aux0" Bypass="0" Gain="1"><Inserts>{inserts}</Inserts></AuxEffect></Auxs>"#),
        ),
        None => (1.0, "", String::new()),
    };
    format!(
        r#"<UVI4><Program Name="Inserts">{auxs}<Layers><Layer Name="Main" Gain="{layer_gain}">{router}<Keygroups>
        <Keygroup Name="K"><Oscillators><SamplePlayer Name="Osc" SamplePath="samples/sine.wav" BaseNote="60"/></Oscillators></Keygroup>
        </Keygroups></Layer></Layers></Program></UVI4>"#
    )
}

#[test]
fn aux_inserts_gain_matrix_and_convolver_shape_the_bus() {
    let dir = std::env::temp_dir().join(format!("sampler-uvi-inserts-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("samples")).unwrap();
    let sine: Vec<i16> = (0..48000)
        .map(|i| ((i as f64 * 440.0 * std::f64::consts::TAU / 48000.0).sin() * 16000.0) as i16)
        .collect();
    std::fs::write(dir.join("samples/sine.wav"), wav(48000, &sine)).unwrap();
    // A unit-at-half impulse response: a convolution through it halves the signal.
    std::fs::write(dir.join("samples/half.wav"), wav(48000, &[16384, 0, 0, 0])).unwrap();
    let peaks = |aux: Option<&str>| {
        let program = dir.join("Inserts.uvip");
        std::fs::write(&program, insert_program(aux)).unwrap();
        let loaded = sampler_uvi::load(&program, 48000).unwrap();
        let mut rt = Runtime::new(
            loaded.plan,
            Limits {
                notes: 4,
                channels: 1,
                performances: 1,
                families: 4,
                expressions: 4,
                voices: 8,
                decisions: 16,
                commands: 16,
                behaviors: 0,
                behavior_fuel: 0,
                behavior_cells: 0,
                note_cells: 0,
            },
        )
        .unwrap();
        let input = Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: None };
        rt.trigger(input, 60, 1.0).unwrap();
        let mut out = vec![[0.0f32; 2]; 4800];
        rt.render(&mut out).unwrap();
        out[1000..].iter().fold([0f32; 2], |p, f| [p[0].max(f[0].abs()), p[1].max(f[1].abs())])
    };
    let plain = peaks(None);
    assert!(plain[0] > 0.05, "plain {plain:?}");
    // Input 1 feeds output 2 at 0.5, then the impulse halves it again.
    let inserts = r#"<GainMatrix Gain_1_1="0" Gain_1_2="0.5" Gain_2_1="0" Gain_2_2="0"/>
        <Convolver Dry="0" Wet="1" SamplePath="samples/half.wav"/>"#;
    let [left, right] = peaks(Some(inserts));
    assert!(left < plain[0] * 0.001, "left {left}");
    assert!((right / plain[1] - 0.25).abs() < 0.02, "right {right}, plain {plain:?}");
    // A bypassed insert does nothing.
    let bypassed = peaks(Some(r#"<GainMatrix Bypass="1" Gain_1_1="0"/>"#));
    assert!((bypassed[0] / plain[0] - 1.0).abs() < 0.01, "{bypassed:?}");
    std::fs::remove_dir_all(&dir).ok();
}

struct NoFiles;
impl sampler_uvi::script::Files for NoFiles {
    fn script(&self, _: &str) -> Option<String> {
        None
    }
}

/// A note a script plays follows its parent's per-note expression live (a
/// bend moves it), while the parent's own gain, silenced here, does not.
#[test]
fn script_played_notes_follow_the_parents_expression() {
    use sampler_core::Expression;
    let dir = std::env::temp_dir().join(format!("sampler-uvi-expr-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("samples")).unwrap();
    let sine: Vec<i16> = (0..48000)
        .map(|i| ((i as f64 * 440.0 * std::f64::consts::TAU / 48000.0).sin() * 16000.0) as i16)
        .collect();
    std::fs::write(dir.join("samples/sine.wav"), wav(48000, &sine)).unwrap();
    let program = dir.join("Fixture.uvip");
    std::fs::write(&program, PROGRAM).unwrap();
    let limits = Limits {
        notes: 4,
        channels: 1,
        performances: 1,
        families: 4,
        expressions: 4,
        voices: 8,
        decisions: 16,
        commands: 16,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    };
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    };
    let bent = |gain| Expression {
        pitch_semitones: 12.0,
        gain,
        ..Expression::default()
    };
    // The reference plays the note itself: one block plain, one bent.
    let mut reference = Runtime::new(sampler_uvi::load(&program, 48000).unwrap().plan, limits).unwrap();
    let note = reference.trigger(input, 60, 1.0).unwrap();
    let id = reference.expression_id(note).unwrap();
    let mut expected = vec![[0.0f32; 2]; 960];
    reference.render(&mut expected[..480]).unwrap();
    reference.set_expression(id, bent(1.0)).unwrap();
    reference.render(&mut expected[480..]).unwrap();

    // The script's child follows a muted parent.
    let mut rt = Runtime::new(sampler_uvi::load(&program, 48000).unwrap().plan, limits).unwrap();
    let host = sampler_uvi::script::ScriptHost::new(PROGRAM, NoFiles, Default::default()).unwrap();
    let mut driver = sampler_uvi::scripted::Driver::new(host, Vec::new(), 48000);
    assert!(driver.handles_notes());
    let parent = rt.note_on(input, 60, 1.0).unwrap();
    driver.note_on(&mut rt, parent, 60, 1.0).unwrap();
    let id = rt.expression_id(parent).unwrap();
    let mut audio = vec![[0.0f32; 2]; 960];
    driver.wake(&mut rt).unwrap();
    rt.render(&mut audio[..480]).unwrap();
    rt.set_expression(id, bent(0.0)).unwrap();
    driver.wake(&mut rt).unwrap();
    rt.render(&mut audio[480..]).unwrap();
    let peak = audio.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
    assert!(peak > 0.05, "the child sounds: peak {peak}");
    let worst = audio
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
        .fold(0f32, |w, (a, b)| w.max((a - b).abs()));
    assert!(worst < 1e-4, "child differs from the bent parent by {worst}");
    std::fs::remove_dir_all(&dir).ok();
}

/// `setParameter` on the program reaches the runtime: Polyphony limits the
/// voices, Gain moves the instrument's level.
#[test]
fn set_parameter_reaches_the_runtime() {
    let dir = std::env::temp_dir().join(format!("sampler-uvi-param-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("samples")).unwrap();
    std::fs::write(dir.join("samples/sine.wav"), wav(48000, &vec![8000i16; 4800])).unwrap();
    let xml = PROGRAM.replace(
        "function onNote(e) playNote(e.note, e.velocity) end",
        "function onNote(e) Program:setParameter('Polyphony', 3); Program:setParameter('Gain', 0.25); playNote(e.note, e.velocity) end",
    );
    let program = dir.join("Fixture.uvip");
    std::fs::write(&program, &xml).unwrap();
    let limits = Limits {
        notes: 4,
        channels: 1,
        performances: 1,
        families: 4,
        expressions: 4,
        voices: 8,
        decisions: 16,
        commands: 16,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    };
    let mut rt = Runtime::new(sampler_uvi::load(&program, 48000).unwrap().plan, limits).unwrap();
    let host = sampler_uvi::script::ScriptHost::new(&xml, NoFiles, Default::default()).unwrap();
    let mut driver = sampler_uvi::scripted::Driver::new(host, Vec::new(), 48000);
    let input = Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: None };
    let note = rt.note_on(input, 60, 1.0).unwrap();
    driver.note_on(&mut rt, note, 60, 1.0).unwrap();
    // 8 slots, 3 voices: 5 are headroom for fading stolen voices.
    assert_eq!(rt.voice_stealing().map(|s| s.headroom), Some(5));
    std::fs::remove_dir_all(&dir).ok();
}

/// One note of the sine through `inserts` on an aux bus, 4800 frames.
fn through(inserts: &str, name: &str) -> Vec<[f32; 2]> {
    let dir = std::env::temp_dir().join(format!("sampler-uvi-{name}-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("samples")).unwrap();
    let sine: Vec<i16> = (0..48000)
        .map(|i| ((i as f64 * 440.0 * std::f64::consts::TAU / 48000.0).sin() * 16000.0) as i16)
        .collect();
    std::fs::write(dir.join("samples/sine.wav"), wav(48000, &sine)).unwrap();
    let program = dir.join("Inserts.uvip");
    std::fs::write(&program, insert_program(Some(inserts))).unwrap();
    let loaded = sampler_uvi::load(&program, 48000).unwrap();
    let mut rt = Runtime::new(
        loaded.plan,
        Limits {
            notes: 4,
            channels: 1,
            performances: 1,
            families: 4,
            expressions: 4,
            voices: 8,
            decisions: 16,
            commands: 16,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap();
    let input = Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: None };
    rt.trigger(input, 60, 1.0).unwrap();
    let mut out = vec![[0.0f32; 2]; 4800];
    rt.render(&mut out).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    out
}

#[test]
fn wave_shaper_rectifies_and_comp_exp_compresses() {
    let translated = sampler_uvi::translate(&insert_program(Some(
        r#"<CompExp CompThreshold="-30" CompRatio="30" CompAttack="0" CompRelease="100"/>"#,
    )), std::path::Path::new(".")).unwrap();
    for (role, default) in [(ir::ProcessorParameter::Threshold, -30.),
        (ir::ProcessorParameter::Ratio, 30.), (ir::ProcessorParameter::Attack, 0.),
        (ir::ProcessorParameter::Release, 0.1)] {
        let binding = translated.instrument.processor_controls.iter()
            .find(|b| b.parameter == role).expect("compressor role missing");
        let ir::ControlValue::Continuous { min, max, default: actual, .. } =
            translated.instrument.controls[binding.control.0].value else { panic!("physical owner") };
        assert_eq!(actual, default);
        assert!((min..=max).contains(&default));
    }
    let peak = |out: &[[f32; 2]]| out[1000..].iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
    let plain = through(r#"<Gain Volume="1"/>"#, "plain");
    let full = through(r#"<WaveShaper Mode="6"/>"#, "full");
    let half = through(r#"<WaveShaper Mode="7"/>"#, "half");
    assert!(full.iter().flatten().all(|x| *x >= 0.0));
    assert!(half.iter().flatten().all(|x| *x >= 0.0));
    assert!((peak(&full) / peak(&plain) - 1.0).abs() < 0.01);
    assert!(full[1000..].iter().flatten().sum::<f32>() > half[1000..].iter().flatten().sum::<f32>());
    let squashed = through(
        r#"<CompExp CompThreshold="-30" CompRatio="30" CompAttack="0" CompRelease="100"/>"#,
        "comp",
    );
    assert!(peak(&squashed) < peak(&plain) * 0.2, "{} vs {}", peak(&squashed), peak(&plain));
}

#[test]
fn track_delay_moves_the_impulse_without_feedback() {
    let plain = through(r#"<Gain Volume="1"/>"#, "delay-plain");
    let delayed = through(r#"<TrackDelay DelayTime="0.01"/>"#, "track-delay");
    assert!(delayed[..480].iter().flatten().all(|x| x.abs() < 1e-7));
    for (actual, expected) in delayed[480..].iter().zip(&plain) {
        for c in 0..2 {
            assert!((actual[c] - expected[c]).abs() < 2e-6);
        }
    }
}

#[test]
fn program_and_layer_inserts_are_distinct_summed_scopes() {
    let xml = insert_program(None)
        .replace("<Layers>", r#"<Inserts><GainMatrix Gain_1_1="0" Gain_2_1="1" Gain_1_2="0" Gain_2_2="0"/></Inserts><Layers>"#)
        .replace("<Keygroups>", r#"<Inserts><GainMatrix Gain_1_1="0" Gain_2_1="0" Gain_1_2="1" Gain_2_2="0"/></Inserts><Keygroups>"#);
    let instrument = sampler_uvi::translate(&xml, std::path::Path::new(".")).unwrap().instrument;
    let layer = instrument.groups[0].output;
    let ir::Output::Bus(layer) = layer else { panic!("missing layer bus") };
    let ir::Output::Bus(program) = instrument.buses[layer.0].output else { panic!("missing program bus") };
    assert_ne!(layer, program);
    let chain = |bus: ir::BusRef| &instrument.chains[instrument.buses[bus.0].chain.unwrap().0];
    assert!(matches!(chain(layer).pre_amplitude[0], ir::Processor::StereoMatrix([[0., 0.], [1., 0.]])));
    assert!(matches!(chain(program).pre_amplitude[0], ir::Processor::StereoMatrix([[0., 1.], [0., 0.]])));
}

#[test]
fn three_band_shelves_uniform_gain_is_a_level_change() {
    let plain = through(r#"<Gain Volume="1"/>"#, "shelves-plain");
    let raised = through(r#"<ThreeBandShelves GainLow="6" GainMid="6" GainHigh="6"/>"#, "shelves-level");
    let ratio = 10f32.powf(6.0 / 20.0);
    for (actual, expected) in raised.iter().zip(&plain) {
        for c in 0..2 {
            assert!((actual[c] - ratio * expected[c]).abs() < 2e-6);
        }
    }
}

#[test]
fn one_pole_frequency_connections_keep_the_native_three_decade_law() {
    let dir = std::env::temp_dir().join(format!("sampler-uvi-cutoff-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("samples")).unwrap();
    std::fs::write(dir.join("samples/sine.wav"), wav(48000, &[0; 64])).unwrap();
    let xml = insert_program(None).replace("<Oscillators>", r#"<Inserts><OnePole Name="Tone" Freq="1000"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Freq" Ratio="0.5"/></Connections></OnePole></Inserts><Oscillators>"#);
    let ir = sampler_uvi::translate(&xml, &dir).unwrap().instrument;
    let (index, route) = ir.routes.iter().enumerate().find(|(_, r)| matches!(r.target, sampler_ir::Target::Processor { parameter: sampler_ir::ProcessorParameter::Cutoff, .. })).expect("missing OnePole cutoff route");
    let sampler_ir::Depth::Pitch(depth) = route.depth else { panic!("cutoff depth must be exponential"); };
    assert!((depth.semitones() - 6.0 * 1000f64.log2()).abs() < 1e-10);
    assert!(ir.zones.iter().any(|zone| zone.routes.contains(&sampler_ir::RouteRef(index))));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn one_pole_connections_address_each_stage_in_a_shared_voice_chain() {
    let dir = std::env::temp_dir().join(format!("sampler-uvi-cutoff-stages-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("samples")).unwrap();
    std::fs::write(dir.join("samples/sine.wav"), wav(48000, &[0; 64])).unwrap();
    let xml = insert_program(None).replace("<Oscillators>", r#"<Inserts><OnePole Name="Low" Freq="1000"><Connections><SignalConnection Source="@MIDI CC 1" Destination="Freq" Ratio="0.5"/></Connections></OnePole><OnePole Name="High" Freq="2000" Mode="1"><Connections><SignalConnection Source="@MIDI CC 2" Destination="Freq" Ratio="0.25"/></Connections></OnePole></Inserts><Oscillators>"#);
    let ir = sampler_uvi::translate(&xml, &dir).unwrap().instrument;
    let stages: Vec<_> = ir.routes.iter().filter_map(|r| match r.target {
        ir::Target::Processor { index, parameter: ir::ProcessorParameter::Cutoff, .. } => Some(index),
        _ => None,
    }).collect();
    assert_eq!(stages, vec![0, 1]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn typed_one_pole_catalog_controls_reach_distinct_live_processors() {
    let xml = insert_program(None).replace("<Oscillators>", r#"<Inserts><OnePole Freq="500"/><OnePole Freq="8000"/></Inserts><Oscillators>"#);
    let instrument = sampler_uvi::translate(&xml, std::path::Path::new(".")).unwrap().instrument;
    assert_eq!(instrument.processor_controls.len(), 2, "catalog controls must bind actual processor lanes");
    assert_ne!(instrument.processor_controls[0].index, instrument.processor_controls[1].index);
    for control in &instrument.controls {
        assert!(matches!(control.value, ir::ControlValue::Continuous { min:20., max:20000., unit:ir::ControlUnit::Hertz, .. }));
    }
}


#[test]
fn initialized_controller_and_widget_cutoff_writes_change_production_pcm() {
    use sampler_core::{EngineParameterAddress, EngineParameterLaw, ControlValue};
    use sampler_uvi::{script::Config, scripted::HostInput};
    let dir = std::env::temp_dir().join(format!("sampler-uvi-live-catalog-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("samples")).unwrap();
    let sine: Vec<i16> = (0..48000).map(|i| ((i as f64 * 5000. * std::f64::consts::TAU / 48000.).sin() * 16000.) as i16).collect();
    std::fs::write(dir.join("samples/sine.wav"), wav(48000, &sine)).unwrap();
    let xml = insert_program(None)
        .replace("<Oscillators>", r#"<Inserts><OnePole Freq="500"/><OnePole Freq="8000"/></Inserts><Oscillators>"#)
        .replace("<Layers>", r#"<EventProcessors><ScriptProcessor><script>
          local filter=Program.layers[1].keygroups[1].inserts[1]
          local knob=Knob('Cutoff',1000,20,20000)
          function knob:changed() filter:setParameter('Freq',self.value) end
          function onInit() filter:setParameter('Freq',1000) end
          function onController(e) filter:setParameter('Freq',10000) end
        </script></ScriptProcessor></EventProcessors><Layers>"#);
    let path = dir.join("Live.uvip");
    std::fs::write(&path, xml).unwrap();
    let mut translated = sampler_uvi::translate_path(&path).unwrap();
    let mut attached = translated.attach_script(48000,Config::default()).unwrap().unwrap();
    assert!(attached.findings.is_empty(), "{:?}", attached.findings);
    let ids:Vec<_> = translated.inserts.iter().map(|i| i.node).collect();
    let control = sampler_core::lower::ir_control_id(&translated.instrument.controls[0].key);
    let loaded = sampler_uvi::assemble_translated(translated,48000).unwrap();
    let limits = Limits::for_plan(&loaded.plan,4,4);
    let mut rt = Runtime::new(loaded.plan,limits).unwrap();
    // onInit is installed before the first script pump or note.
    assert_eq!(rt.control_value(rt.active_plan(),control).unwrap(), ControlValue::Real(1000.));
    let address = |node| EngineParameterAddress { parameter:sampler_core::engine_parameter_id("ENGINE_PAR_CUTOFF").unwrap(),group:-1,slot:-1,generic:node as i32 };
    let law = EngineParameterLaw::Exponential {low:20.,high:20000.};
    let wait = |driver:&mut sampler_uvi::scripted::Driver<sampler_uvi::scripted::ScriptThread>, rt:&mut Runtime, native| {
        let deadline = std::time::Instant::now()+std::time::Duration::from_secs(5);
        loop {
            driver.wake(rt).unwrap();
            if rt.engine_parameter(address(ids[0])).unwrap()==law.normalized_value(native).unwrap() { break; }
            assert!(std::time::Instant::now()<deadline,"live cutoff command did not reach the DSP control");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(rt.engine_parameter(address(ids[1])).unwrap(),law.normalized_value(8000.).unwrap());
    };
    wait(&mut attached.driver,&mut rt,1000.);
    let input=Input { protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:None };
    rt.trigger(input,60,1.).unwrap();
    let mut closed=vec![[0.;2];2048];
    rt.render(&mut closed).unwrap();
    attached.driver.input(&rt,HostInput::Controller {cc:1,value:127,channel:0});
    wait(&mut attached.driver,&mut rt,10000.);
    let mut open=vec![[0.;2];2048];
    rt.render(&mut open).unwrap();
    let power=|audio:&[[f32;2]]| audio[1024..].iter().flatten().map(|x| x*x).sum::<f32>();
    assert!(power(&open)>power(&closed)*9.,"controller write must change actual PCM");
    let ui=attached.driver.ui();
    let knob=ui.values()[0].0;
    assert!(ui.edit(knob,100.));
    wait(&mut attached.driver,&mut rt,100.);
    let mut widget=vec![[0.;2];2048];
    rt.render(&mut widget).unwrap();
    assert!(power(&widget)<power(&closed)*0.1,"widget write must change actual PCM");
    std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn streamed_bus_gain_has_live_catalog_defaults_and_controller_readback() {
    use sampler_core::{EngineParameterAddress, EngineParameterLaw, ControlValue};
    use sampler_uvi::{script::Config, scripted::HostInput};
    let dir=std::env::temp_dir().join(format!("sampler-uvi-streamed-catalog-{}",std::process::id()));
    std::fs::create_dir_all(dir.join("samples")).unwrap();
    std::fs::write(dir.join("samples/sine.wav"),wav(48000,&vec![16000;48000])).unwrap();
    let xml=insert_program(None).replace("<Layers>",r#"<Inserts><Gain Volume="1"/></Inserts><EventProcessors><ScriptProcessor><script>
      local gain=Program.inserts[1]
      function onInit() gain:setParameter('Volume',0.25) end
      function onController(e) gain:setParameter('Volume',0.5) end
    </script></ScriptProcessor></EventProcessors><Layers>"#);
    let path=dir.join("Bus.uvip");
    std::fs::write(&path,xml).unwrap();
    let mut translated=sampler_uvi::translate_path(&path).unwrap();
    let mut attached=translated.attach_script(48000,Config::default()).unwrap().unwrap();
    assert!(attached.findings.is_empty(),"{:?}",attached.findings);
    let node=translated.inserts[0].node;
    let control=sampler_core::lower::ir_control_id(&translated.instrument.controls[0].key);
    let streamed=sampler_uvi::assemble_translated_streamed(translated,48000,&Default::default()).unwrap();
    let limits=Limits::for_plan(&streamed.loaded.plan,4,4);
    let mut rt=Runtime::new(streamed.loaded.plan,limits).unwrap().with_stream_cache(streamed.cache);
    assert_eq!(rt.control_value(rt.active_plan(),control).unwrap(),ControlValue::Real(0.25));
    let address=EngineParameterAddress {parameter:sampler_core::engine_parameter_id("ENGINE_PAR_VOLUME").unwrap(),group:-1,slot:-1,generic:node as i32};
    let law=EngineParameterLaw::Linear {low:0.,high:3.981};
    attached.driver.wake(&mut rt).unwrap();
    rt.trigger(Input {protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:None},60,1.).unwrap();
    let mut quiet=vec![[0.;2];128];
    rt.render(&mut quiet).unwrap();
    attached.driver.input(&rt,HostInput::Controller {cc:1,value:127,channel:0});
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
    loop {
        attached.driver.wake(&mut rt).unwrap();
        if rt.engine_parameter(address).unwrap()==law.normalized_value(0.5).unwrap() {break;}
        assert!(std::time::Instant::now()<deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let mut loud=vec![[0.;2];128];
    rt.render(&mut loud).unwrap();
    assert!(quiet[100][0]>0.01);
    assert!((loud[100][0]/quiet[100][0]-2.).abs()<0.01,"summed bus gain must change actual PCM");
    drop(rt);
    drop(streamed.streamer);
    std::fs::remove_dir_all(dir).unwrap();
}
