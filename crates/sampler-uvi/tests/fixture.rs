//! An authored clear program and generated WAV: translate, load and render.

use sampler_core::{Input, Limits, Protocol, Runtime};
use sampler_ir as ir;

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
        "modulation source",
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
    assert_eq!(zone.routes.len(), 1);
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
