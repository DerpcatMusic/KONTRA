//! Unit and integration tests for `kontra-format-decentsampler`.

use std::io;
use std::path::Path;

use kontra_format::header::{AudioHeader, AudioLoop};
use kontra_format_decentsampler::{Error, translate_xml};
use kontra_ir as ir;

fn fake_probe(path: &Path) -> io::Result<AudioHeader> {
    if path.to_string_lossy().contains("missing") {
        return Err(io::Error::new(io::ErrorKind::NotFound, "not found"));
    }
    let loops = if path.to_string_lossy().contains("embedded_loop") {
        vec![AudioLoop {
            start: 100,
            end: 900,
            count: 0,
            ping_pong: false,
        }]
    } else {
        Vec::new()
    };
    Ok(AudioHeader {
        sample_rate: 48_000,
        channels: 2,
        frames: 48_000,
        loops,
    })
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn translates_groups_inheritance_volume_pan_adsr_and_samples() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DecentSampler>
  <groups volume="-3dB" attack="0.02" decay="0.15" sustain="0.7" release="0.4" ampVelTrack="0.8">
    <group name="Soft" pan="-50" tuning="0.5">
      <sample path="Samples/C4.wav" rootNote="C4" loNote="C3" hiNote="B4" loVel="1" hiVel="64" volume="0.5" pan="25" start="100" end="24000" loopEnabled="true" loopStart="500" loopEnd="10000" loopCrossfade="64"/>
    </group>
    <group name="Loud" volume="0.5" release="0.9" ampVelTrack="0.0">
      <sample path="Samples/C4.wav" rootNote="60" loNote="60" hiNote="72" loVel="65" hiVel="127"/>
    </group>
  </groups>
</DecentSampler>"#;

    let inst = translate_xml(xml, Path::new("/lib"), &fake_probe).unwrap();
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
    assert_eq!(inst.samples.len(), 1, "shared sample deduplicated");
    assert_eq!(inst.groups.len(), 2);

    let g0 = &inst.groups[0];
    assert_eq!(g0.name, "Soft");
    assert!(near(inst.output.volume_db, -3.0));
    assert!(near(g0.volume_db, 0.0));
    assert!(near(g0.pan, -0.5));
    assert!((g0.tune - 0.5).abs() < 1e-6);
    assert!(near(g0.envelope.attack, 0.02));
    assert!(near(g0.envelope.decay, 0.15));
    assert!(near(g0.envelope.sustain, 0.7));
    assert!(near(g0.envelope.release, 0.4));
    assert_eq!(g0.mods.len(), 1);

    let z0 = &g0.zones[0];
    assert_eq!(z0.root, 60);
    assert_eq!(z0.keys, 48..=71);
    assert_eq!(z0.velocities, 1..=64);
    assert!(near(z0.volume_db, -6.0206), "linear 0.5 -> -6.02 dB");
    assert!(near(z0.pan, 0.25));
    assert_eq!((z0.start, z0.end), (100, 24_000));
    assert_eq!(
        z0.loops,
        vec![ir::Loop {
            start: 500,
            end: 10_000,
            crossfade: 64,
            ..ir::Loop::default()
        }]
    );

    let g1 = &inst.groups[1];
    assert_eq!(g1.name, "Loud");
    assert!(near(g1.volume_db, -6.0206));
    assert!(near(inst.output.volume_db + g1.volume_db, -9.0206));
    assert!(near(g1.envelope.release, 0.9));
    assert!(g1.mods.is_empty(), "ampVelTrack=0 disables velocity mod");
}

#[test]
fn translates_round_robin_random_trigger_and_choke_tags() {
    let xml = r#"
<DecentSampler>
  <tags>
    <tag name="closed_hat" polyphony="2"/>
  </tags>
  <groups seqMode="round_robin" seqLength="2" silencingMode="normal">
    <group name="OpenHat" tags="open_hat" silencedByTags="closed_hat" seqPosition="1">
      <sample path="oh1.wav" rootNote="46" loNote="46" hiNote="46"/>
    </group>
    <group name="OpenHatRR2" tags="open_hat" silencedByTags="closed_hat" seqPosition="2">
      <sample path="oh2.wav" rootNote="46" loNote="46" hiNote="46"/>
    </group>
    <group name="ClosedHat" tags="closed_hat" seqMode="true_random" seqLength="2">
      <sample path="ch1.wav" rootNote="42" loNote="42" hiNote="42" seqPosition="1"/>
      <sample path="ch2.wav" rootNote="42" loNote="42" hiNote="42" seqPosition="2"/>
    </group>
    <group name="ReleaseNoise" seqMode="always" trigger="release">
      <sample path="rel.wav" rootNote="60" loNote="0" hiNote="127" pitchKeyTrack="0.0"/>
    </group>
  </groups>
</DecentSampler>"#;

    let inst = translate_xml(xml, Path::new("/drums"), &fake_probe).unwrap();
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
    // OpenHat, OpenHatRR2, ClosedHat split into 2 RR sample groups, ReleaseNoise -> 5 IR groups.
    assert_eq!(inst.groups.len(), 5);

    // Choke voice groups: closed_hat (0, polyphony 2), open_hat (1, silencingMode=normal -> release=true)
    assert_eq!(inst.voice_groups.len(), 2);
    assert_eq!(inst.voice_groups[0].voices, 2);
    assert!(inst.voice_groups[1].release);

    assert_eq!(inst.groups[0].voice_group, Some(1));
    assert_eq!(inst.groups[0].off_by, Some(0));
    assert_eq!(
        inst.groups[0].start,
        vec![(
            ir::Join::And,
            ir::StartRule::RoundRobin {
                scope: ir::RrScope::Global,
                position: 0,
                length: 2,
                random: false,
            }
        )]
    );
    assert_eq!(
        inst.groups[1].start,
        vec![(
            ir::Join::And,
            ir::StartRule::RoundRobin {
                scope: ir::RrScope::Global,
                position: 1,
                length: 2,
                random: false,
            }
        )]
    );
    assert_eq!(
        inst.groups[2].start,
        vec![(
            ir::Join::And,
            ir::StartRule::RoundRobin {
                scope: ir::RrScope::Global,
                position: 0,
                length: 2,
                random: true,
            }
        )]
    );
    assert_eq!(inst.groups[4].trigger, ir::Trigger::Release);
    assert_eq!(inst.groups[4].keytrack, 0.0);
}

#[test]
fn translates_effects_lfo_envelope_and_midi_cc_bindings() {
    let xml = r#"
<DecentSampler>
  <groups>
    <group name="Synth">
      <effects>
        <effect type="lowpass_4pl" frequency="1800" resonance="2.0"/>
      </effects>
      <sample path="saw.wav" rootNote="C4"/>
    </group>
  </groups>
  <effects>
    <effect type="chorus" modRate="0.4" modDepth="0.6" mix="0.5"/>
    <effect type="delay" delayTime="0.25" feedback="0.35" wetLevel="0.4"/>
    <effect type="reverb" roomSize="0.7" damping="0.4" wetLevel="0.3"/>
    <effect type="gain" level="-2.0"/>
  </effects>
  <modulators>
    <lfo frequency="4.5" modAmount="0.5" waveform="triangle" scope="voice">
      <binding type="effect" level="group" position="0" effectIndex="0" parameter="FX_FILTER_FREQUENCY"/>
      <binding type="amp" level="group" position="0" parameter="PITCH" translationOutputMin="-0.5" translationOutputMax="0.5"/>
    </lfo>
    <envelope attack="0.05" decay="0.3" sustain="0.2" release="0.5" modAmount="0.75">
      <binding type="effect" level="group" position="0" effectIndex="0" parameter="FX_FILTER_FREQUENCY"/>
    </envelope>
  </modulators>
  <midi>
    <cc number="1">
      <binding type="effect" level="group" position="0" effectIndex="0" parameter="FX_FILTER_FREQUENCY"/>
    </cc>
    <cc number="7">
      <binding type="amp" level="instrument" parameter="AMP_VOLUME"/>
    </cc>
  </midi>
</DecentSampler>"#;

    let inst = translate_xml(xml, Path::new("/synth"), &fake_probe).unwrap();
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);

    // Global inserts: chorus (Ensemble), delay, reverb, gain
    assert_eq!(inst.inserts.len(), 4);
    assert!(matches!(
        inst.inserts[0].clone().map(|e| e.kind),
        Some(ir::EffectKind::Ensemble(_))
    ));
    assert!(matches!(
        inst.inserts[1].clone().map(|e| e.kind),
        Some(ir::EffectKind::Delay(_))
    ));
    assert!(matches!(
        inst.inserts[2].clone().map(|e| e.kind),
        Some(ir::EffectKind::Reverb(_))
    ));
    assert!(matches!(
        inst.inserts[3].clone().map(|e| e.kind),
        Some(ir::EffectKind::Gain(_))
    ));

    // Group 0: legacy lowpass_4pl (the 2-pole lowpass) + velocity mod + LFO cutoff + LFO pitch + Env cutoff + CC1 cutoff + CC7 volume
    let g = &inst.groups[0];
    let Some(Some(fx)) = g.inserts.first() else {
        panic!("expected group filter");
    };
    let ir::EffectKind::Filter(f) = fx.kind else {
        panic!("expected filter, got {:?}", fx.kind);
    };
    assert_eq!((f.pass, f.stages), (ir::Pass::Low, 1));
    assert!(near(f.hz, 1800.0));
    assert!(near(f.damping[0], 0.5));

    assert_eq!(g.lfos.len(), 2);
    assert_eq!(g.lfos[0].wave, ir::Wave::Triangle);
    assert_eq!(g.envelopes.len(), 1);
    assert!(g.mods.iter().any(|m| matches!(
        m.kind,
        ir::ModKind::Cutoff {
            source: ir::Source::Lfo(0),
            insert: 0,
            ..
        }
    )));
    assert!(g.mods.iter().any(|m| matches!(
        m.kind,
        ir::ModKind::Pitch {
            source: ir::Source::Lfo(1),
            ..
        }
    )));
    assert!(g.mods.iter().any(|m| matches!(
        m.kind,
        ir::ModKind::Cutoff {
            source: ir::Source::Envelope(0),
            insert: 0,
            ..
        }
    )));
    assert!(g.mods.iter().any(|m| matches!(
        m.kind,
        ir::ModKind::Cutoff {
            source: ir::Source::Controller(1),
            insert: 0,
            ..
        }
    )));
    assert!(g.mods.iter().any(|m| matches!(
        m.kind,
        ir::ModKind::Volume {
            source: ir::Source::Controller(7),
            ..
        }
    )));
}

#[test]
fn reports_unsupported_features_and_missing_samples_without_silent_drops() {
    let xml = r#"
<DecentSampler>
  <ui width="812" height="375"/>
  <mysterySection/>
  <groups>
    <group>
      <sample path="missing_piano.wav" rootNote="C4"/>
      <sample path="embedded_loop.wav" rootNote="D4" loopEnabled="true"/>
    </group>
  </groups>
  <effects>
    <effect type="tape_warp"/>
  </effects>
  <modulators>
    <lfo frequency="1.0">
      <binding parameter="UNSUPPORTED_TARGET"/>
      <futureCurve/>
    </lfo>
  </modulators>
</DecentSampler>"#;

    let inst = translate_xml(xml, Path::new("/lib"), &fake_probe).unwrap();
    let features: Vec<&str> = inst
        .diagnostics
        .iter()
        .map(|d| d.feature.as_str())
        .collect();
    assert!(features.contains(&"element <ui>"), "{features:?}");
    assert!(
        features.contains(&"element <mysterySection>"),
        "{features:?}"
    );
    assert!(features.contains(&"samples not found"), "{features:?}");
    assert!(features.contains(&"effect tape_warp"), "{features:?}");
    assert!(
        features.contains(&"binding UNSUPPORTED_TARGET"),
        "{features:?}"
    );

    assert!(
        inst.diagnostics.iter().any(|d| {
            d.feature == "LFO element <futureCurve>" && d.impact == ir::Impact::Audible
        })
    );

    // Embedded loop picked up when loopEnabled="true" omits explicit loopStart/loopEnd
    let z1 = &inst.groups[0].zones[1];
    assert_eq!(z1.loops.len(), 1);
    assert_eq!((z1.loops[0].start, z1.loops[0].end), (100, 900));
}

#[test]
fn effect_units_carry_seconds_musical_time_and_linear_gain() {
    let xml = r#"<DecentSampler><effects>
        <effect type="delay" delayTimeFormat="musical_time" delayTime="10"/>
        <effect type="delay" delayTimeFormat="seconds" delayTime="0.25"/>
        <effect type="gain" levelUnit="linear" level="0.5"/>
        <effect type="gain" level="-6"/>
        <effect type="gain" levelUnit="linear"/>
    </effects></DecentSampler>"#;
    let inst = translate_xml(xml, Path::new("/lib"), &fake_probe).unwrap();
    let fx = |i: usize| inst.inserts[i].clone().unwrap();
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
    // Index 10 is an eighth note.
    assert!(matches!(fx(0).kind, ir::EffectKind::Delay(d) if d.time == ir::DelayTime::Beats(0.5)));
    assert!(matches!(fx(1).kind, ir::EffectKind::Delay(d) if d.time == ir::DelayTime::Ms(250.0)));
    assert_eq!(fx(2).kind, ir::EffectKind::Gain(0.5));
    assert!(matches!(fx(3).kind, ir::EffectKind::Gain(g) if near(g, 0.501187)));
    // The documented default level, -6 dB (0.5 linear).
    assert_eq!(fx(4).kind, ir::EffectKind::Gain(0.5));
}

#[test]
fn instrument_cutoff_bindings_share_the_same_promoted_filter_per_group() {
    for middle_rack in ["", "<effect type='gain'/>"] {
        for maximum in [2000, 6000] {
            let xml = format!(
                r#"<DecentSampler>
                <groups>
                    <group><effects><effect type="lowpass" frequency="1000"/></effects><sample path="a.wav"/></group>
                    <group><effects><effect type="lowpass" frequency="2000"/>{middle_rack}</effects><sample path="b.wav"/></group>
                </groups>
                <effects><effect type="lowpass" frequency="60"/></effects>
                <midi>
                    <cc number="1"><binding level="instrument" parameter="FX_FILTER_FREQUENCY" modBehavior="add" translationOutputMin="0" translationOutputMax="{maximum}"/></cc>
                    <cc number="2"><binding level="instrument" parameter="FX_FILTER_FREQUENCY"/></cc>
                </midi>
            </DecentSampler>"#
            );
            let inst = translate_xml(&xml, Path::new("/lib"), &fake_probe).unwrap();
            assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
            assert!(inst.inserts[0].clone().unwrap().bypass);
            for (index, group) in inst.groups.iter().enumerate() {
                let slot = if index == 1 && !middle_rack.is_empty() {
                    2
                } else {
                    1
                };
                assert_eq!(group.inserts.len(), slot + 1);
                assert!(
                    matches!(group.inserts[0].clone().unwrap().kind, ir::EffectKind::Filter(f) if f.hz == (index + 1) as f32 * 1000.0)
                );
                let filter = group.inserts[slot].clone().unwrap();
                assert!(!filter.bypass);
                assert!(matches!(filter.kind, ir::EffectKind::Filter(f) if f.hz == 60.0));
                assert!(
                    group
                        .mods
                        .iter()
                        .any(|m| matches!(m.kind, ir::ModKind::CutoffHz {
                    source: ir::Source::Controller(1), insert, min_hz: 0.0, max_hz,
                } if usize::from(insert) == slot && max_hz == maximum as f32))
                );
                assert!(
                    group
                        .mods
                        .iter()
                        .any(|m| matches!(m.kind, ir::ModKind::Cutoff {
                    source: ir::Source::Controller(2), insert, ..
                } if usize::from(insert) == slot))
                );
            }
        }
    }
}

#[test]
fn disabled_groups_keep_binding_indices_and_split_zones_keep_their_sequence() {
    let xml = r#"<DecentSampler>
        <groups volume="0.5" globalTuning="2" seqMode="round_robin">
            <group enabled="false"><sample path="disabled.wav"/></group>
            <group seqPosition="1"><sample path="first.wav"/></group>
            <group seqPosition="2" groupTuning="-12" volume="0.5">
                <sample path="second.wav" attack="0.1" tuning="3"/>
            </group>
        </groups>
        <midi>
            <cc number="1"><binding level="group" position="1" parameter="PITCH" translationOutputMin="0" translationOutputMax="24"/></cc>
            <cc number="1"><binding level="group" position="2" parameter="PITCH" translationOutputMin="-12" translationOutputMax="12"/></cc>
        </midi>
    </DecentSampler>"#;
    let inst = translate_xml(xml, Path::new("/lib"), &fake_probe).unwrap();
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
    assert_eq!(inst.groups.len(), 2);
    assert_eq!(inst.samples.len(), 2);
    assert!(near(
        inst.output.volume_db + inst.groups[1].volume_db,
        -12.0412
    ));
    assert_eq!(inst.groups[0].tune, 2.0);
    assert_eq!(inst.groups[1].tune, -22.0); // +2 global, -12 group, -12 MIDI offset
    assert_eq!(inst.groups[1].zones[0].tune, 3.0);
    for (pos, group) in inst.groups.iter().enumerate() {
        assert_eq!(
            group.start,
            vec![(
                ir::Join::And,
                ir::StartRule::RoundRobin {
                    scope: ir::RrScope::Global,
                    position: pos as u8,
                    length: 2,
                    random: false,
                }
            )]
        );
        assert!(group.mods.iter().any(|m| matches!(
            m.kind,
            ir::ModKind::Pitch {
                source: ir::Source::Controller(1),
                semitones: 24.0,
                depth: 1.0,
                ..
            }
        )));
    }
}

#[test]
fn promoting_global_filter_after_delay_reports_reordering_approximation() {
    let xml = r#"<DecentSampler>
        <groups>
            <group><sample path="a.wav"/></group>
        </groups>
        <effects>
            <effect type="delay" delayTime="0.25"/>
            <effect type="lowpass" frequency="800"/>
        </effects>
        <midi>
            <cc number="1">
                <binding level="instrument" effectIndex="1" parameter="FX_FILTER_FREQUENCY"/>
            </cc>
        </midi>
    </DecentSampler>"#;
    let inst = translate_xml(xml, Path::new("/lib"), &fake_probe).unwrap();
    assert!(!inst.inserts[0].clone().unwrap().bypass);
    assert!(inst.inserts[1].clone().unwrap().bypass);
    assert_eq!(inst.groups[0].inserts.len(), 1);
    assert!(
        inst.diagnostics.iter().any(|d| {
            d.impact == ir::Impact::Audible
                && d.feature == "promoted global filter reordered before preceding global effects"
        }),
        "{:?}",
        inst.diagnostics
    );
}

fn effects(xml: &str) -> Result<(Vec<ir::Effect>, ir::Instrument), Error> {
    let inst = translate_xml(xml, Path::new("/lib"), &fake_probe)?;
    Ok((inst.inserts.iter().flatten().cloned().collect(), inst))
}

#[test]
fn filters_reverb_and_delay_take_the_documented_defaults() {
    let (fx, inst) = effects(
        r#"<DecentSampler><effects>
        <effect type="notch"/>
        <effect type="notch" frequency="300" Q="4"/>
        <effect type="reverb"/>
        <effect type="delay" stereoOffset="0.02" feedbackCutoff="3000"/>
        <effect type="delay"/>
    </effects></DecentSampler>"#,
    )
    .unwrap();
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
    let ir::EffectKind::Filter(notch) = fx[0].kind else {
        panic!("{:?}", fx[0].kind)
    };
    assert_eq!((notch.pass, notch.hz), (ir::Pass::Notch, 10_000.0));
    assert!(near(notch.damping[0], 1.0 / 0.7));
    assert!(
        matches!(fx[1].kind, ir::EffectKind::Filter(f) if f.hz == 300.0 && near(f.damping[0], 0.25))
    );
    let ir::EffectKind::Reverb(r) = &fx[2].kind else {
        panic!()
    };
    assert!(near(r.damping, 0.3) && near(r.seconds, 0.4 + 0.7 * 5.6));
    assert_eq!((fx[2].output, fx[2].dry), (0.0, 1.0));
    let ir::EffectKind::Delay(d) = &fx[3].kind else {
        panic!()
    };
    assert_eq!(
        (d.time, d.spread_ms, d.damping_hz),
        (ir::DelayTime::Ms(700.0), 10.0, 3000.0)
    );
    assert!(matches!(&fx[4].kind, ir::EffectKind::Delay(d) if d.damping_hz.is_infinite()));
}

#[test]
fn peak_and_shelves_are_eq_bands_in_linear_or_db_gain() {
    let (fx, _) = effects(
        r#"<DecentSampler><effects>
        <effect type="peak" frequency="2000" Q="2" gain="2.0"/>
        <effect type="low_shelf" gain="-6" gainUnit="decibels"/>
        <effect type="high_shelf" q="0.7071" gain="0.5"/>
    </effects></DecentSampler>"#,
    )
    .unwrap();
    let band = |i: usize| match fx[i].kind {
        ir::EffectKind::Eq(b) => {
            assert_eq!(&b[1..], &[ir::Bell::FLAT; 3]);
            b[0]
        }
        ref k => panic!("{k:?}"),
    };
    let peak = band(0);
    assert_eq!(
        (peak.shape, peak.hz, peak.q),
        (ir::Shape::Peak, 2000.0, 2.0)
    );
    assert!(near(peak.gain_db, 6.0206));
    let low = band(1);
    assert_eq!(
        (low.shape, low.hz, low.gain_db),
        (ir::Shape::LowShelf, 200.0, -6.0)
    );
    let high = band(2);
    assert_eq!((high.shape, high.hz), (ir::Shape::HighShelf, 5000.0));
    // Q 1/√2 is the cookbook slope 1, whatever the gain.
    assert!(near(high.q, 1.0) && near(high.gain_db, -6.0206));
}

#[test]
fn compressor_folds_input_gain_into_threshold_and_makeup() {
    let (fx, _) = effects(
        r#"<DecentSampler><effects>
        <effect type="compressor"/>
        <effect type="compressor" threshold="-20" ratio="8" attack="1" release="300" inputGain="6" outputGain="-3"/>
    </effects></DecentSampler>"#,
    )
    .unwrap();
    let ir::EffectKind::Compressor(c) = fx[0].kind else {
        panic!()
    };
    assert_eq!(
        (
            c.threshold_db,
            c.ratio,
            c.attack_ms,
            c.release_ms,
            c.makeup_db,
            c.knee_db
        ),
        (-12.0, 4.0, 5.0, 100.0, 0.0, 0.0)
    );
    assert!(c.link);
    let ir::EffectKind::Compressor(c) = fx[1].kind else {
        panic!()
    };
    assert_eq!(
        (
            c.threshold_db,
            c.ratio,
            c.attack_ms,
            c.release_ms,
            c.makeup_db
        ),
        (-26.0, 8.0, 1.0, 300.0, 3.0)
    );
}

#[test]
fn phaser_sweeps_its_centre_by_a_thousandfold_per_unit_depth() {
    let (fx, _) = effects(
        r#"<DecentSampler><effects><effect type="phaser" modDepth="0.5" feedback="-0.4" mix="0.25"/></effects></DecentSampler>"#,
    )
    .unwrap();
    let ir::EffectKind::Phaser(p) = fx[0].kind else {
        panic!()
    };
    assert_eq!(
        (p.stages, p.hz, p.spread_hz, p.feedback),
        (6, 400.0, 400.0, -0.4)
    );
    assert_eq!(p.rate, ir::Rate::Hz(0.2));
    // At the LFO's peak the corner is the centre times 1000^(depth/2).
    assert!(near(400.0 * p.exponent.exp(), 400.0 * 1000f32.powf(0.25)));
    assert_eq!((fx[0].output, fx[0].dry), (0.25, 0.75));
}

#[test]
fn convolution_loads_its_ir_file_as_a_sample_and_impulse() {
    let (fx, inst) = effects(
        r#"<DecentSampler><effects>
        <effect type="convolution" mix="0.3" irFile="Samples\Hall 3.wav"/>
        <effect type="convolution" irFile="missing.wav"/>
    </effects></DecentSampler>"#,
    )
    .unwrap();
    assert_eq!(fx[0].kind, ir::EffectKind::Convolution { impulse: 0 });
    assert_eq!((fx[0].output, fx[0].dry), (0.3, 0.7));
    assert_eq!(inst.impulses.len(), 1);
    let sample = &inst.samples[inst.impulses[0].sample];
    assert!(
        sample.file.ends_with("Samples/Hall 3.wav"),
        "{:?}",
        sample.file
    );
    assert_eq!(sample.channels, 2);
    // A missing response plays only the dry share.
    assert_eq!(
        (fx[1].kind.clone(), fx[1].dry),
        (ir::EffectKind::OtherWet, 0.5)
    );
}

#[test]
fn wave_folder_and_shaper_are_shaper_curves() {
    let (fx, inst) = effects(
        r#"<DecentSampler><effects>
        <effect type="wave_folder" drive="10" threshold="0.5"/>
        <effect type="wave_shaper" drive="40" outputLevel="0.3"/>
    </effects></DecentSampler>"#,
    )
    .unwrap();
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
    let curve = |i: usize| match &fx[i].kind {
        ir::EffectKind::Shaper(s) => s.curve.clone(),
        k => panic!("{k:?}"),
    };
    // Folds at ±0.5: d = π·10/(2·0.5), scaled back by 2·0.5/π.
    let ir::DriveCurve::Fold(d) = curve(0) else {
        panic!()
    };
    assert!(near(d, 10.0 * std::f32::consts::PI) && near(fx[0].output, 1.0 / std::f32::consts::PI));
    assert_eq!((curve(1), fx[1].output), (ir::DriveCurve::Tanh(40.0), 0.3));
}

#[test]
fn bit_crusher_holds_its_reduction_factor_and_bits() {
    let (fx, _) = effects(
        r#"<DecentSampler><effects>
        <effect type="bit_crusher" bitDepth="8" sampleRateReduction="4" mix="0.5"/>
        <effect type="bit_crusher"/>
    </effects></DecentSampler>"#,
    )
    .unwrap();
    let crush = |i: usize| match fx[i].kind {
        ir::EffectKind::Crush(c) => c,
        ref k => panic!("{k:?}"),
    };
    // The engine holds 1/rate + 1 frames: 4.
    assert_eq!((crush(0).bits, crush(0).rate), (8.0, 1.0 / 3.0));
    assert_eq!((fx[0].output, fx[0].dry), (0.5, 0.5));
    assert_eq!((crush(1).bits, crush(1).rate, fx[1].dry), (24.0, 1.0, 0.0));
}

#[test]
fn pitch_shift_glides_its_heads_by_the_semitone_ratio() {
    let (fx, _) = effects(
        r#"<DecentSampler><effects><effect type="pitch_shift" pitchShift="-12"/></effects></DecentSampler>"#,
    )
    .unwrap();
    let ir::EffectKind::Shifter(s) = fx[0].kind else {
        panic!()
    };
    assert_eq!((s.speed, s.feedback), ([0.5, 0.5], 0.0));
    assert_eq!((fx[0].output, fx[0].dry), (0.5, 0.5));
}

#[test]
fn effect_bindings_move_voice_eq_crusher_and_folder_controls() {
    let xml = r#"
<DecentSampler>
  <groups>
    <group>
      <sample path="a.wav"/>
      <effects>
        <effect type="bit_crusher"/>
        <effect type="wave_folder" threshold="0.5"/>
      </effects>
    </group>
  </groups>
  <effects><effect type="high_shelf" gain="0" gainUnit="decibels"/></effects>
  <midi>
    <cc number="20"><binding type="effect" level="instrument" position="0" parameter="FX_FILTER_GAIN" translationOutputMin="-6" translationOutputMax="6"/></cc>
    <cc number="21"><binding type="effect" level="group" groupIndex="0" effectIndex="0" parameter="FX_BIT_DEPTH" translationOutputMin="1" translationOutputMax="24"/></cc>
    <cc number="22"><binding type="effect" level="group" position="0" effectIndex="1" parameter="FX_DRIVE" translationOutputMin="1" translationOutputMax="11"/></cc>
  </midi>
</DecentSampler>"#;
    let inst = translate_xml(xml, Path::new("/lib"), &fake_probe).unwrap();
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
    // The instrument shelf runs per voice now; its global copy is bypassed.
    assert!(inst.inserts[0].as_ref().unwrap().bypass);
    let g = &inst.groups[0];
    let route = |cc: u8| {
        g.mods
            .iter()
            .find_map(|m| match m.kind {
                ir::ModKind::EffectParam {
                    source: ir::Source::Controller(c),
                    insert,
                    param,
                    depth,
                    ..
                } if c == cc => Some((insert, param, depth)),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(route(20), (2, ir::EffectParam::BellGain(0), 12.0));
    assert_eq!(route(21), (0, ir::EffectParam::CrushBits, 23.0));
    let (insert, param, depth) = route(22);
    assert_eq!((insert, param), (1, ir::EffectParam::ShaperAmount));
    assert!(near(depth, 10.0 * std::f32::consts::PI));
    // Each controller sets its control outright from the range's bottom.
    let fx = |i: usize| g.inserts[i].clone().unwrap().kind;
    assert!(matches!(fx(0), ir::EffectKind::Crush(c) if c.bits == 1.0));
    assert!(matches!(fx(2), ir::EffectKind::Eq(b) if b[0].gain_db == -6.0));
}
