use super::*;

/// Every name is a sample of its own, 1000 frames long.
struct Stub;

impl Library for Stub {
    fn sample(&self, _: &str, reference: &str) -> Result<ir::Sample, String> {
        if reference.contains("missing") {
            return Err(format!("{reference}: not in this bank"));
        }
        Ok(ir::Sample {
            name: reference.into(),
            file: "bank.ufs".into(),
            offset: reference
                .bytes()
                .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b))),
            len: None,
            channels: 1,
            frames: 1000,
            rate: 48_000,
            keystream: None,
        })
    }

    fn modules(&self) -> Vec<(String, Vec<u8>)> {
        Vec::new()
    }
}

#[test]
fn keygroups_become_groups_bundles_place_members_and_silent_layers_are_reported() {
    let xml = r#"<Program DisplayName="Synthetic" Gain="1" Polyphony="4">
      <Layers>
        <Layer Gain="1" Pan="0">
          <Keygroups>
            <Keygroup Gain="0.5" Pan="0" LowKey="60" HighKey="62" LowVelocity="1" HighVelocity="127">
              <Oscillators>
                <SamplePlayer Bypass="0" BaseNote="61" CoarseTune="1" SamplePath="./a.wav"/>
                <SamplePlayer Bypass="0" BaseNote="61" SamplePath="./missing.wav"/>
              </Oscillators>
            </Keygroup>
            <Keygroup Gain="1" Pan="0" LowKey="70" HighKey="70" LowVelocity="1" HighVelocity="127">
              <Inserts>
                <GainMatrix Bypass="0" Gain_1_1="1" Gain_1_2="0" Gain_2_1="0" Gain_2_2="1" Gain_3_1="0" Gain_3_2="0"/>
              </Inserts>
              <Oscillators>
                <SamplePlayer Bypass="0" BaseNote="70" SamplePath="./bundle/*l.wav*r.wav*c.wav"/>
              </Oscillators>
            </Keygroup>
          </Keygroups>
        </Layer>
        <Layer Gain="0" Pan="0">
          <Keygroups>
            <Keygroup LowKey="0" HighKey="127">
              <Oscillators>
                <SamplePlayer Bypass="0" BaseNote="60" SamplePath="./silent.wav"/>
              </Oscillators>
            </Keygroup>
          </Keygroups>
        </Layer>
      </Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    assert_eq!(out.name, "Synthetic");
    // 4 UVI voices, each a keygroup's zones: at most the bundle's three.
    assert_eq!(out.voices, 12);
    // One group per enabled keygroup; the silent layer's keygroup makes none.
    assert_eq!(out.groups.len(), 2);
    let first = &out.groups[0];
    assert!((first.volume_db - (-6.0206)).abs() < 1e-3);
    assert_eq!(first.zones.len(), 1);
    let zone = &first.zones[0];
    assert_eq!(
        (zone.keys.clone(), zone.root, zone.tune),
        (60..=62, 61, 1.0)
    );
    // Bundle: the matrix places L → left, R → right, the third member nowhere.
    let bundle = &out.groups[1];
    assert_eq!(bundle.zones.len(), 2);
    assert_eq!(bundle.zones[0].mix, vec![[1.0, 0.0]]);
    assert_eq!(bundle.zones[1].mix, vec![[0.0, 1.0]]);
    // a.wav, then the two placed bundle members; the unplaced and missing ones add none.
    assert_eq!(out.samples.len(), 3);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.feature.starts_with("silent layer"))
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.feature == "missing sample")
    );
}

#[test]
fn a_layer_sounds_through_its_aux_chain() {
    let xml = r#"<Program Gain="1">
      <Layers>
        <Layer Name="L" Gain="0" Pan="0">
          <BusRouters>
            <BusRouter Bypass="0" Gain="0.5" Destination="../../Aux0" PreFader="1"/>
          </BusRouters>
          <Keygroups>
            <Keygroup LowKey="0" HighKey="127">
              <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
            </Keygroup>
          </Keygroups>
        </Layer>
      </Layers>
      <Auxs>
        <AuxEffect Name="Aux0" Bypass="0" Gain="2" Pan="0" PreInsert="1">
          <Inserts>
            <DigitalEq Bypass="0"/>
            <Gain Bypass="0" Volume="0.5"/>
            <Convolver Bypass="0" Dry="0" Wet="1" SamplePath="./body.flac"/>
          </Inserts>
        </AuxEffect>
      </Auxs>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    // Main, the layer at its fader, the aux at its gain.
    assert_eq!(out.buses.len(), 3);
    assert_eq!(out.groups[0].bus, 1);
    assert!(out.groups[0].volume_db.abs() < 1e-3);
    let send = out.buses[1].sends[0];
    assert_eq!((send.bus, send.level, send.post_fader), (2, 0.5, false));
    let aux = &out.buses[2];
    assert!((aux.volume_db - 6.0206).abs() < 1e-3);
    // An EQ with no bands enabled is flat.
    assert_eq!(
        aux.inserts[0].map(|e| e.kind),
        Some(ir::EffectKind::Eq([ir::Bell::FLAT; 4]))
    );
    assert!(matches!(aux.inserts[1].map(|e| e.kind), Some(ir::EffectKind::Gain(g)) if g == 0.5));
    assert!(matches!(
        aux.inserts[2].map(|e| e.kind),
        Some(ir::EffectKind::Convolution { impulse: 0 })
    ));
    assert_eq!(out.impulses.len(), 1);
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.feature.starts_with("silent layer"))
    );
}

#[test]
fn an_effect_rack_fans_out_to_its_chains_and_back() {
    let xml = r#"<Program Gain="1">
      <Layers><Layer Gain="0.5" Pan="0">
        <Inserts>
          <EffectRack Bypass="0"><Chains>
            <AuxEffect Bypass="0" Gain="0.5" Pan="-1" DisplayName="Dry"/>
            <AuxEffect Bypass="0" Gain="0.25" Pan="0" DisplayName="Room">
              <Inserts><Gain Bypass="0" Volume="2"/></Inserts>
            </AuxEffect>
            <AuxEffect Bypass="1" Gain="1" DisplayName="Off"/>
          </Chains></EffectRack>
          <Gain Bypass="0" Volume="3"/>
        </Inserts>
        <Keygroups><Keygroup>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup></Keygroups>
      </Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    // Main, the layer's entry, its three chains, then the join.
    let b = &out.buses;
    assert_eq!(b.len(), 6);
    assert_eq!(out.groups[0].bus, 1);
    assert!(b[1].sends.iter().map(|s| s.bus).eq(2..=4));
    assert!(!b[1].to_main && !b[2].to_main);
    // A chain's gain is its fader into the join; a bypassed one is silent.
    let fader = |n: usize| b[n].volume_db;
    assert!((fader(2) + 6.0206).abs() < 1e-3 && (fader(3) + 12.0412).abs() < 1e-3);
    assert!(fader(4) < -100.0);
    assert!((2..=4).all(|n| b[n].sends[0].bus == 5 && b[n].sends[0].post_fader));
    // The panned chain ends on a balance; the other runs its own Gain.
    assert!(
        matches!(b[2].inserts[0].map(|e| e.kind), Some(ir::EffectKind::Stereo { pan, .. }) if pan == -1.0)
    );
    assert!(matches!(b[3].inserts[0].map(|e| e.kind), Some(ir::EffectKind::Gain(g)) if g == 2.0));
    // The rest of the inserts and the layer's fader are on the join.
    assert!(matches!(b[5].inserts[0].map(|e| e.kind), Some(ir::EffectKind::Gain(g)) if g == 3.0));
    assert!(b[5].to_main && (b[5].volume_db + 6.0206).abs() < 1e-3);
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.feature.starts_with("insert"))
    );
}

#[test]
fn a_keygroup_one_pole_and_its_velocity_route_reach_the_group() {
    let xml = r#"<Program Gain="1">
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Inserts>
            <DigitalEq Bypass="0"/>
            <OnePole Name="InsertFX1" Bypass="0" Freq="20000" KeyTracking="0" Mode="0">
              <Connections>
                <SignalConnection Ratio="-0.6" Source="@VoiceParam Velocity" Destination="Freq" Mapper="" ConnectionMode="0" Bypass="0" Inverted="1"/>
                <SignalConnection Ratio="-0.6" Source="@MIDI CC 1" Destination="Freq" Mapper="" ConnectionMode="0" Bypass="1" Inverted="1"/>
              </Connections>
            </OnePole>
          </Inserts>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    assert!(g.inserts[0].is_none());
    let Some(ir::EffectKind::Filter(f)) = g.inserts[1].map(|e| e.kind) else {
        panic!("{:?}", g.inserts);
    };
    // Inverted: 20 kHz × 1000^(−0.6(1 − v)), so 20 kHz × 1000^−0.6 at
    // velocity 0, rising 0.6 of the range's 9.97 octaves to 20 kHz at 1.
    assert_eq!(f.pass, ir::Pass::Low);
    assert_eq!(f.hz, 20_000.0);
    assert!((f.octaves - 1000f32.log2()).abs() < 1e-4);
    assert_eq!(
        g.shapers,
        [ir::Transfer {
            source: ir::Source::Velocity,
            table: vec![1.0, 0.0],
        }]
    );
    assert_eq!(
        g.mods.iter().map(|m| m.kind).collect::<Vec<_>>(),
        [ir::ModKind::Cutoff {
            source: ir::Source::Shaped(0),
            insert: 1,
            depth: -0.6,
        }]
    );
    assert!(g.mods[0].name.starts_with('#'), "{:?}", g.mods[0].name);
    let features: Vec<_> = out.diagnostics.iter().map(|d| d.feature.as_str()).collect();
    assert!(features.contains(&"insert DigitalEq"), "{features:?}");
    assert!(
        !features
            .iter()
            .any(|f| f.contains("OnePole") || f.contains("Freq")),
        "{features:?}"
    );
}

#[test]
fn a_filters_envelope_and_lfo_routes_are_named_and_a_bypassed_one_waits_for_its_script() {
    let xml = r#"<Program Gain="1">
      <Modulators>
        <LFO Name="LFO 1" Bypass="0" SyncToHost="0" DelayTime="0" RiseTime="0" Freq="2" Depth="1" Phase="0" WaveFormType="0" Retrigger="1" Bipolar="1"/>
      </Modulators>
      <Scripts><ScriptProcessor Name="EventProcessor0" Bypass="0" API_version="21"><script><![CDATA[--]]></script></ScriptProcessor></Scripts>
      <Layers><Layer Gain="1" Pan="0">
        <Modulators>
          <DAHDSR Name="DAHDSR 2" Bypass="0" DelayTime="0.1" AttackTime="0.2" AttackCurve="0" HoldTime="0" DecayTime="0.5" DecayCurve="-0.5" SustainLevel="0.25" ReleaseTime="1" ReleaseCurve="0" VelocityAmount="0" VelocitySens="0"/>
        </Modulators>
        <Keygroups><Keygroup LowKey="0" HighKey="127">
          <Inserts>
            <XpanderFilter Name="InsertFX0" Bypass="0" Freq="500" Q="0" KeyTracking="0" Mode="3" DistortionType="0" Drive="0" Fat="1" Algorithm="0" Oversampling="0">
              <Connections>
                <SignalConnection Ratio="0.5" Source="@MIDI CC 1" Destination="Freq" Mapper="" ConnectionMode="0" Bypass="1" Inverted="0"/>
                <SignalConnection Ratio="0.3" Source="$Program/LFO 1" Destination="Freq" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
                <SignalConnection Ratio="1" Source="$Layer/DAHDSR 2" Destination="Freq" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
              </Connections>
            </XpanderFilter>
          </Inserts>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup></Keygroups>
      </Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    let cutoffs: Vec<_> = g
        .mods
        .iter()
        .filter_map(|m| match m.kind {
            ir::ModKind::Cutoff { source, depth, .. } => {
                Some((source, depth, m.name.starts_with('#')))
            }
            _ => None,
        })
        .collect();
    // The bypassed controller waits at 0 for a script to turn it on.
    assert_eq!(
        cutoffs,
        [
            (ir::Source::Controller(1), 0.0, true),
            (ir::Source::Lfo(0), 0.3, true),
            (ir::Source::Envelope(0), 1.0, true),
        ]
    );
    let e = &g.envelopes[0];
    assert_eq!(e.sustain, Some(3));
    assert_eq!(
        e.points.iter().map(|p| (p.ms, p.level)).collect::<Vec<_>>(),
        [
            (100.0, 0.0),
            (200.0, 1.0),
            (0.0, 1.0),
            (500.0, 0.25),
            (1000.0, 0.0)
        ]
    );
    let features: Vec<_> = out.diagnostics.iter().map(|d| d.feature.as_str()).collect();
    assert!(!features.iter().any(|f| f.contains("Freq")), "{features:?}");
}

#[test]
fn a_keygroup_xpander_is_the_explicit_ladder_with_its_taps_q_and_drive() {
    let xml = r#"<Program Gain="1">
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Inserts>
            <XpanderFilter Name="InsertFX0" Bypass="0" Freq="1008.4754" Q="0.2" KeyTracking="0" Mode="9" DistortionType="0" Drive="-20" Fat="0" Algorithm="0" Oversampling="0">
              <Connections>
                <SignalConnection Ratio="0.5" Source="@MIDI CC 1" Destination="Freq" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
                <SignalConnection Ratio="1.5" Source="@MIDI CC 1" Destination="Q" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
              </Connections>
            </XpanderFilter>
            <XpanderFilter Name="InsertFX1" Bypass="0" Freq="500" Q="0" KeyTracking="0" Mode="3" DistortionType="0" Drive="0" Fat="1" Algorithm="1" Oversampling="0"/>
          </Inserts>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    let Some(ir::EffectKind::Filter(f)) = g.inserts[0].map(|e| e.kind) else {
        panic!("{:?}", g.inserts);
    };
    // Mode 9 is the four-pole band; Q 0.2 is the resonance, so k = 2(1 − 0.2).
    assert_eq!(f.ladder, Some(ir::Ladder::Explicit { taps: 10, fat: 0.0 }));
    assert!((f.hz - 1008.4754).abs() < 1e-3);
    assert!((f.damping[0] - 1.6).abs() < 1e-6);
    assert!((f.gain - 0.1).abs() < 1e-6);
    assert_eq!(
        g.mods.iter().map(|m| m.kind).collect::<Vec<_>>(),
        [
            ir::ModKind::Cutoff {
                source: ir::Source::Controller(1),
                insert: 0,
                depth: 0.5,
            },
            // Q scales the resonance by the connection law, its ratio clamped.
            ir::ModKind::Resonance {
                source: ir::Source::Controller(1),
                insert: 0,
                depth: 1.0,
            }
        ]
    );
    // Algorithm 1 (the zero-delay ladder) is not carried yet.
    assert!(g.inserts[1].is_none());
    let features: Vec<_> = out.diagnostics.iter().map(|d| d.feature.as_str()).collect();
    assert_eq!(features, ["insert XpanderFilter"], "{features:?}");
}

#[test]
fn a_keygroup_ms20_is_the_sallen_key_with_its_morph_q_and_trimmed_curve() {
    let xml = r#"<Program Gain="1">
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Inserts>
            <MS20 Name="InsertFX2" Bypass="0" Freq="500" Q="0.4" KeyTracking="0" ResonanceTrim="0.55" ReferenceVoltage="2" Morph="0.25">
              <Connections>
                <SignalConnection Ratio="-0.5" Source="@MIDI CC 1" Destination="Freq" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
              </Connections>
            </MS20>
          </Inserts>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    let Some(ir::EffectKind::Filter(f)) = g.inserts[0].map(|e| e.kind) else {
        panic!("{:?}", g.inserts);
    };
    let Some(ir::Ladder::SallenKey { morph, curve }) = f.ladder else {
        panic!("{:?}", f.ladder);
    };
    // Trim 0.55 sits halfway between the sixth and seventh pairs.
    assert!((morph - 0.25).abs() < 1e-6);
    assert!((curve[0] - 2.067_022).abs() < 1e-5 && (curve[1] - 1.808_917).abs() < 1e-5);
    assert!((f.damping[0] - 1.2).abs() < 1e-6 && (f.gain - 2.0).abs() < 1e-6);
    assert_eq!(g.mods.len(), 1);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn an_oscillators_bend_and_controller_pitch_routes_reach_its_group() {
    let xml = r#"<Program Gain="1">
      <Modulators>
        <LFO Name="LFO 1" Bypass="0" SyncToHost="1" DelayTime="0" RiseTime="0" Freq="0.5" Depth="0.5" Phase="0.25" WaveFormType="0" Retrigger="0" Bipolar="1"/>
        <MultiLFO Name="Multi LFO 1" Bypass="0" SyncToHost="0" SineDepth="1" TriangleDepth="0.5" SawDepth="0" SquareDepth="0" NoiseDepth="0" RiseTime="0" Freq="1" Depth="1" Retrigger="1" Bipolar="1" Invert="0" NormalizeOutput="1"/>
      </Modulators>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Oscillators>
            <SamplePlayer Bypass="0" SamplePath="./a.wav">
              <Connections>
                <SignalConnection Ratio="2" Source="@PitchBend" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
                <SignalConnection Ratio="1" Source="@PitchBend" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="1"/>
                <SignalConnection Ratio="0.5" Source="@MIDI CC 1" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
                <SignalConnection Ratio="0" Source="@MIDI CC 2" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
                <SignalConnection Ratio="12" Source="@PitchBend" Destination="Pitch" Mapper="Mapper 1" ConnectionMode="0" Bypass="0" Inverted="0"/>
                <SignalConnection Ratio="0.6" Source="$Program/LFO 1" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="1"/>
                <SignalConnection Ratio="0.7" Source="$Program/Multi LFO 1" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
              </Connections>
            </SamplePlayer>
          </Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let pitch = |source, semitones| ir::ModKind::Pitch {
        source,
        semitones,
        depth: 1.0,
        tau_ms: 0.0,
    };
    assert_eq!(
        out.groups[0]
            .mods
            .iter()
            .map(|m| m.kind)
            .collect::<Vec<_>>(),
        [
            pitch(ir::Source::PitchBend, 2.0),
            pitch(ir::Source::PitchBend, -1.0),
            pitch(ir::Source::Controller(1), 0.5),
            // Inverted, at half depth.
            pitch(ir::Source::Lfo(0), -0.3),
            pitch(ir::Source::Lfo(1), 0.7),
        ]
    );
    assert_eq!(
        out.groups[0].lfos[0],
        ir::Lfo {
            wave: ir::Wave::Sine,
            rate: ir::Rate::Beats(0.5),
            phase: 0.25,
            fade_ms: 0.0,
            clock: ir::Clock::Free,
        }
    );
    // Sine and half a triangle, normalized by their peak, 1.0511.
    let ir::Wave::Multi {
        weights,
        normalize: false,
        ..
    } = out.groups[0].lfos[1].wave
    else {
        panic!("{:?}", out.groups[0].lfos[1]);
    };
    let k = 1.0 / 1.051_102;
    let expected = [-k, 0.0, -0.5 * k, 0.0, 0.0, 0.0];
    assert!(
        weights
            .iter()
            .zip(expected)
            .all(|(w, e)| (w - e).abs() < 1e-4),
        "{weights:?}"
    );
    // Only the mapped connection is left to report.
    let pitch_notes: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.feature == "modulation Pitch")
        .map(|d| d.detail.as_str())
        .collect();
    assert_eq!(
        pitch_notes,
        // The mapped bend, its mapper missing.
        ["1× (first: @PitchBend)"],
        "{:?}",
        out.diagnostics
    );
}

#[test]
fn a_held_macro_on_a_bends_ratio_sets_its_range() {
    // VWinds: 12 semitones scaled by "Pitch bend amount" (Macro 3 = 1/12);
    // a controller on the ratio is live, and stays reported.
    let xml = r#"<Program Gain="1">
      <ControlSignalSources>
        <ConstantModulation Name="Macro 3" Bypass="0" Bipolar="0" Value="0.083333336"/>
      </ControlSignalSources>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Oscillators>
            <SamplePlayer Bypass="0" SamplePath="./a.wav">
              <Connections>
                <SignalConnection Ratio="12" Source="@PitchBend" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0">
                  <Connections>
                    <SignalConnection Ratio="1" Source="$Program/Macro 3" Destination="Ratio" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
                  </Connections>
                </SignalConnection>
                <SignalConnection Ratio="2" Source="@PitchBend" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0">
                  <Connections>
                    <SignalConnection Ratio="1" Source="@MIDI CC 1" Destination="Ratio" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
                  </Connections>
                </SignalConnection>
              </Connections>
            </SamplePlayer>
          </Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let kinds: Vec<_> = out.groups[0].mods.iter().map(|m| m.kind).collect();
    let [
        ir::ModKind::Pitch {
            source: ir::Source::PitchBend,
            semitones,
            ..
        },
    ] = kinds[..]
    else {
        panic!("{kinds:?}");
    };
    assert!((semitones - 1.0).abs() < 1e-6, "{semitones}");
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.feature == "modulation Pitch"),
        "{:?}",
        out.diagnostics
    );
}

#[test]
fn a_layer_ahd_bends_its_oscillators_pitch_through_breakpoints() {
    let xml = r#"<Program Gain="1">
      <Layers><Layer Gain="1" Pan="0">
        <Modulators>
          <AHD Name="AHD 1" Bypass="0" AttackTime="0" AttackCurve="0" HoldTime="0" DecayTime="0.03" DecayCurve="-0.5" VelocityAmount="0" Retrigger="1"/>
        </Modulators>
        <Keygroups><Keygroup LowKey="0" HighKey="127">
          <Oscillators>
            <SamplePlayer Bypass="0" SamplePath="./a.wav">
              <Connections>
                <SignalConnection Ratio="-2" Source="$Layer/AHD 1" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
              </Connections>
            </SamplePlayer>
          </Oscillators>
        </Keygroup></Keygroups>
      </Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    assert_eq!(
        g.mods.iter().map(|m| m.kind).collect::<Vec<_>>(),
        [ir::ModKind::Pitch {
            source: ir::Source::Envelope(0),
            semitones: -2.0,
            depth: 1.0,
            tau_ms: 0.0,
        }]
    );
    let e = &g.envelopes[0];
    let (ms, levels): (Vec<_>, Vec<_>) = e.points.iter().map(|p| (p.ms, p.level)).unzip();
    assert_eq!(
        (e.start, ms, levels, e.sustain),
        (0.0, vec![0.0, 0.0, 30.0], vec![1.0, 1.0, 0.0], None)
    );
    // c = −0.5 passes (1 − c)/2 = 3/4 of its fall at half time.
    let k = e.points[2].curve;
    assert!(((0.5 * k).exp_m1() / k.exp_m1() - 0.75).abs() < 1e-5, "{k}");
    assert!(
        !out.diagnostics.iter().any(|d| d.feature.contains("Pitch")),
        "{:?}",
        out.diagnostics
    );
}

#[test]
fn a_step_envelope_bends_pitch_through_a_held_step_table() {
    let xml = r#"<Program Gain="1">
      <Modulators>
        <StepEnvelope Name="Step Envelope 1" Bypass="0" SyncToHost="1" NumSteps="4" Freq="0.25" Retrigger="0" Bipolar="0" InterpolationMode="0" Smooth="0" Levels="0,000000 1,000000 0,500000 0,250000"/>
      </Modulators>
      <Layers><Layer Gain="1" Pan="0"><Keygroups><Keygroup LowKey="0" HighKey="127">
        <Oscillators>
          <SamplePlayer Bypass="0" SamplePath="./a.wav">
            <Connections>
              <SignalConnection Ratio="12" Source="$Program/Step Envelope 1" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
            </Connections>
          </SamplePlayer>
        </Oscillators>
      </Keygroup></Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    assert_eq!(
        g.mods.iter().map(|m| m.kind).collect::<Vec<_>>(),
        [ir::ModKind::Pitch {
            source: ir::Source::Shaped(0),
            semitones: 12.0,
            depth: 1.0,
            tau_ms: 0.0,
        }]
    );
    // Four sixteenths a cycle, on the song clock.
    assert_eq!(
        (g.lfos[0].wave, g.lfos[0].rate, g.lfos[0].clock),
        (ir::Wave::Saw, ir::Rate::Beats(1.0), ir::Clock::Free)
    );
    let shaper = &g.shapers[0];
    assert_eq!(shaper.source, ir::Source::Lfo(0));
    // The saw reads 1 − 2p, entering the table at 1 − p: each step's middle.
    for (p, level) in [(0.125, 0.0), (0.375, 1.0), (0.625, 0.5), (0.875, 0.25)] {
        let x = 1.0 - 2.0 * p;
        assert!((shaper.apply((x + 1.0) / 2.0) - level).abs() < 1e-6, "{p}");
    }
}

#[test]
fn a_macro_holds_its_crossfade_cells_where_its_mappers_put_them() {
    // Bipolar Macro 1 at 0 reads its mappers at their start, as a unipolar
    // one: variant a's bell is 1 there, b's is 0 (dropped); c rides at half
    // ratio.
    let xml = r#"<Program Gain="1">
      <ControlSignalSources>
        <ConstantModulation Name="Macro 1" Bipolar="1" Value="0"/>
      </ControlSignalSources>
      <Mappers>
        <ControlSignalMapper Name="Mapper 0" Min="0" Max="1">1,0 0,0 0,0</ControlSignalMapper>
        <ControlSignalMapper Name="Mapper 1" Min="0" Max="1">0,0 0,4 0,6 0,0</ControlSignalMapper>
      </Mappers>
      <Layers>
        <Layer Gain="1" Pan="0">
          <Keygroups>
            <Keygroup LowKey="0" HighKey="127">
              <Inserts>
                <GainMatrix Bypass="0" Gain_1_1="1" Gain_2_2="1" Gain_3_1="1">
                  <Connections>
                    <SignalConnection Ratio="1" Source="$Program/Macro 1" Destination="Gain_1_1" Mapper="Mapper 0"/>
                    <SignalConnection Ratio="1" Source="$Program/Macro 1" Destination="Gain_2_2" Mapper="Mapper 1"/>
                    <SignalConnection Ratio="0.5" Source="$Program/Macro 1" Destination="Gain_3_1" Mapper="Mapper 1"/>
                  </Connections>
                </GainMatrix>
              </Inserts>
              <Oscillators>
                <SamplePlayer Bypass="0" SamplePath="./v/*a.wav*b.wav*c.wav"/>
              </Oscillators>
            </Keygroup>
          </Keygroups>
        </Layer>
      </Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let mixes: Vec<_> = out.groups[0].zones.iter().map(|z| z.mix.clone()).collect();
    // a at 1; b is silent and not loaded; c at 1 - 0.5 + 0.5 * 0.
    assert_eq!(mixes, vec![vec![[1.0, 0.0]], vec![[0.5, 0.0]]]);
    assert_eq!(out.samples.len(), 2);
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.feature.starts_with("matrix cell"))
    );
}

/// VWinds' release: the script ramps its Release modulation to 1, which
/// takes the bipolar brightness macro (CC 102 at ratio 1 − mapper·release)
/// back to its Value 0, where the lowest bell is silent, while the same
/// modulation drifts the pitch up (Clarinet A: 1.4 semitones). Read at
/// `(x + 1)/2` the macro rested halfway, on loud bells: the sustain rang on
/// over its release sample a semitone sharp.
#[test]
fn a_released_vwinds_sustain_falls_silent_as_its_pitch_drifts() {
    let xml = r#"<Program Gain="1">
      <ControlSignalSources>
        <ScriptEventModulation Name="Script Event Modulation 1" EventId="2" Bipolar="0"/>
        <ConstantModulation Name="Macro 1" Bipolar="1" Value="0">
          <Connections>
            <SignalConnection Ratio="1" Source="@MIDI CC 102" Destination="Value">
              <Connections>
                <SignalConnection Ratio="-1" Source="$Program/Script Event Modulation 1" Destination="Ratio"/>
              </Connections>
            </SignalConnection>
          </Connections>
        </ConstantModulation>
      </ControlSignalSources>
      <Mappers>
        <ControlSignalMapper Name="Mapper 0" Min="0" Max="1">0,0 1,0 0,0 0,0 0,0</ControlSignalMapper>
        <ControlSignalMapper Name="Mapper 1" Min="0" Max="1">0,0 0,0 1,0 1,0 0,0</ControlSignalMapper>
      </Mappers>
      <Layers>
        <Layer Gain="1" Pan="0">
          <Keygroups>
            <Keygroup LowKey="0" HighKey="127">
              <Inserts>
                <GainMatrix Bypass="0" Gain_1_1="1" Gain_2_2="1">
                  <Connections>
                    <SignalConnection Ratio="1" Source="$Program/Macro 1" Destination="Gain_1_1" Mapper="Mapper 0"/>
                    <SignalConnection Ratio="1" Source="$Program/Macro 1" Destination="Gain_2_2" Mapper="Mapper 1"/>
                  </Connections>
                </GainMatrix>
              </Inserts>
              <Oscillators>
                <SamplePlayer Bypass="0" SamplePath="./v/*a.wav*b.wav"/>
              </Oscillators>
            </Keygroup>
          </Keygroups>
        </Layer>
      </Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    let ir::Source::Blend(b) = g.shapers[0].source else {
        panic!("{:?}", g.shapers)
    };
    let blend = &g.blends[usize::from(b)];
    // The macro's position at a wind (CC 102) and a release ramp.
    let macro_at = |wind: f32, release: f32| {
        let term = &blend.terms[0];
        let factor = term.factors[0].at(term.factors[0].input.apply(release));
        blend.base + term.amount * term.input.apply(wind) * factor
    };
    let cells = |x: f32| -> Vec<f32> { g.shapers.iter().map(|s| s.apply(x)).collect() };
    // Played: the wind picks its bell; released, every bell is closed.
    assert_eq!(cells(macro_at(0.25, 0.0)), [1.0, 0.0]);
    assert_eq!(cells(macro_at(0.6, 0.0)), [0.0, 1.0]);
    assert_eq!(cells(macro_at(0.6, 1.0)), [0.0, 0.0]);
}

#[test]
fn a_macro_a_controller_moves_crossfades_its_cells_live() {
    // Macro 1 = 0 + CC 102 × 0.5 (bipolar, read as its value), its
    // ratio scaled by CC 104 through a mapper; the cell reads Mapper 0.
    let xml = r#"<Program Gain="1">
      <ControlSignalSources>
        <ConstantModulation Name="Macro 1" Bipolar="1" Value="0">
          <Connections>
            <SignalConnection Ratio="0.5" Source="@MIDI CC 102" Destination="Value">
              <Connections>
                <SignalConnection Ratio="1" Source="@MIDI CC 104" Destination="Ratio" Mapper="Mapper 1"/>
                <SignalConnection Ratio="1" Source="@MIDI CC 106" Destination="Ratio" Mapper="Mapper 0" Inverted="1"/>
                <SignalConnection Ratio="0.6" Source="@PitchBend" Destination="Ratio">
                  <Connections>
                    <SignalConnection Ratio="1" Source="$Program/Macro 6" Destination="Ratio"/>
                  </Connections>
                </SignalConnection>
                <SignalConnection Ratio="1" Source="$Program/Script Event Modulation 10" Destination="Ratio" Mapper="Mapper 2"/>
              </Connections>
            </SignalConnection>
          </Connections>
        </ConstantModulation>
        <ConstantModulation Name="Macro 6" Bipolar="0" Value="0.5"/>
        <ScriptEventModulation Name="Script Event Modulation 10" EventId="14" Bipolar="1"/>
      </ControlSignalSources>
      <Mappers>
        <ControlSignalMapper Name="Mapper 0" Min="0" Max="1">0,0 1,0</ControlSignalMapper>
        <ControlSignalMapper Name="Mapper 1" Min="0" Max="1">0,0 0,5 1,0</ControlSignalMapper>
        <ControlSignalMapper Name="Mapper 2" Min="0" Max="1">1,0 0,8 0,6 0,2</ControlSignalMapper>
      </Mappers>
      <Layers>
        <Layer Gain="1" Pan="0">
          <Keygroups>
            <Keygroup LowKey="0" HighKey="127">
              <Inserts>
                <GainMatrix Bypass="0" Gain_1_1="1" Gain_2_2="1">
                  <Connections>
                    <SignalConnection Ratio="1" Source="$Program/Macro 1" Destination="Gain_2_2" Mapper="Mapper 0"/>
                    <SignalConnection Ratio="1" Source="@MIDI CC 102" Destination="Gain_3_1"/>
                  </Connections>
                </GainMatrix>
                <GainMatrix Bypass="1" Gain_1_1="1">
                  <Connections>
                    <SignalConnection Ratio="1" Source="@MIDI CC 102" Destination="Gain_1_1"/>
                  </Connections>
                </GainMatrix>
              </Inserts>
              <Oscillators>
                <SamplePlayer Bypass="0" SamplePath="./v/*a.wav*b.wav"/>
              </Oscillators>
            </Keygroup>
          </Keygroups>
        </Layer>
      </Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    // b keeps its saved cell; the route moves it.
    assert_eq!(g.zones[1].mix, vec![[0.0, 1.0]]);
    let route = g.mods.iter().find_map(|m| match m.kind {
        ir::ModKind::Mix {
            source,
            zone,
            row,
            side,
        } => Some((source, zone, row, side)),
        _ => None,
    });
    assert_eq!(route, Some((ir::Source::Shaped(0), 1, 0, 1)));
    assert_eq!(g.shapers[0].source, ir::Source::Blend(0));
    assert_eq!(g.shapers[0].table, vec![0.0, 1.0]);
    let blend = &g.blends[0];
    assert_eq!(blend.base, 0.0);
    let term = &blend.terms[0];
    assert_eq!(term.input.source, ir::Source::Controller(102));
    assert_eq!(term.amount, 0.5);
    assert_eq!(term.factors[0].input.source, ir::Source::Controller(104));
    assert_eq!(term.factors[0].input.table, vec![0.0, 0.5, 1.0]);
    // An inverted controller reads its mapper at `1 − x`.
    assert_eq!(term.factors[1].input.source, ir::Source::Controller(106));
    assert_eq!(term.factors[1].input.table, vec![1.0, 0.0]);
    // Macro 6 holds the bend's ratio at half: 0.6 × 0.5.
    assert_eq!(term.factors[2].input.source, ir::Source::PitchBend);
    assert!((term.factors[2].depth - 0.3).abs() < 1e-6);
    // A bipolar script modulation reads its mapper at `(x + 1)/2`.
    assert_eq!(term.factors[3].input.source, ir::Source::Script(14));
    let half = [0.7, 0.6, 0.4, 0.2];
    for (got, want) in term.factors[3].input.table.iter().zip(half) {
        assert!(
            (got - want).abs() < 1e-5,
            "{:?}",
            term.factors[3].input.table
        );
    }
    // A row past the two-channel bundle and a bypassed matrix move nothing.
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.feature.starts_with("matrix") || d.feature.starts_with("modulation")),
        "{:?}",
        out.diagnostics
    );
}

#[test]
fn an_aux_eqs_gain_scale_connections_move_its_bands_live() {
    let xml = r#"<Program Gain="1">
      <Modulations>
        <ScriptEventModulation Name="Script Event Modulation 8" EventId="8" Bipolar="0"/>
        <ConstantModulation Name="Macro 6" Bipolar="0" Value="0.5"/>
      </Modulations>
      <Auxs>
        <AuxEffect Name="Aux0" Bypass="0" Gain="1" Pan="0" PreInsert="1">
          <Inserts>
            <DigitalEq Bypass="0" GainScale="0"
              Enabled1="1" Type1="6" Slope1="0" Freq1="261.6" Gain1="-6" Bandwidth1="0.4"
              Enabled2="0" Type2="6" Gain2="-9"
              Enabled3="1" Type3="5" Slope3="1" Freq3="3139.5" Gain3="-6.5">
              <Mappers>
                <ControlSignalMapper Name="Mapper 51" Min="-1" Max="1">-1,0 0,0 1,0</ControlSignalMapper>
              </Mappers>
              <Connections>
                <SignalConnection Ratio="1" Source="$Program/Script Event Modulation 8" Destination="GainScale" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
                <SignalConnection Ratio="0.3" Source="@PitchBend" Destination="GainScale" Mapper="Mapper 51" ConnectionMode="0" Bypass="0" Inverted="0">
                  <Connections>
                    <SignalConnection Ratio="1" Source="$Program/Macro 6" Destination="Ratio" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
                  </Connections>
                </SignalConnection>
              </Connections>
            </DigitalEq>
          </Inserts>
        </AuxEffect>
      </Auxs>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let aux = out.buses.last().unwrap();
    let Some(ir::EffectKind::Eq(bells)) = aux.inserts[0].map(|e| e.kind) else {
        panic!("{:?}", aux.inserts);
    };
    // GainScale 0: flat at rest, the enabled bands in order.
    assert_eq!(
        (bells[0].shape, bells[1].shape),
        (ir::Shape::Peak, ir::Shape::HighShelf)
    );
    assert!(bells.iter().all(|b| b.gain_db == 0.0));
    // A 0.4-octave peak: Q 1/(2^0.2 − 2^−0.2).
    assert!((bells[0].q - 3.596).abs() < 1e-2, "{}", bells[0].q);
    // One blend of both connections moves each band by its own gain.
    let depths: Vec<_> = aux
        .mods
        .iter()
        .map(|m| match *m {
            ir::ModKind::EffectParam {
                source: ir::Source::Blend(0),
                insert: 0,
                param,
                depth,
                ..
            } => (param, depth),
            _ => panic!("{m:?}"),
        })
        .collect();
    use ir::EffectParam::BellGain;
    assert_eq!(depths, [(BellGain(0), -6.0), (BellGain(1), -6.5)]);
    let terms = &aux.blends[0].terms;
    assert_eq!(terms[0].input.source, ir::Source::Script(8));
    // The macro at 0.5 halves the bend's ratio.
    assert_eq!(terms[1].input.source, ir::Source::PitchBend);
    assert!((terms[1].amount - 0.15).abs() < 1e-6);
    // GainScale adds the mapper's output as it is (0x141429650): a bipolar
    // mapper is not re-centred, so the bend rests at 0.
    assert_eq!(terms[1].input.table, [-1.0, 0.0, 1.0]);
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.feature.contains("GainScale"))
    );
}

#[test]
fn a_step_envelope_pans_its_keygroup() {
    let xml = r#"<Program Gain="1">
      <Modulators>
        <StepEnvelope Name="Step Envelope 1" Bypass="0" SyncToHost="1" NumSteps="2" Freq="0.5" Retrigger="1" Bipolar="1" InterpolationMode="0" Smooth="0" Levels="-1,000000 1,000000"/>
      </Modulators>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127" Pan="0">
          <Connections>
            <SignalConnection Ratio="0.5" Source="$Program/Step Envelope 1" Destination="Pan" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    assert_eq!(
        g.mods.iter().map(|m| m.kind).collect::<Vec<_>>(),
        [ir::ModKind::Pan {
            source: ir::Source::Shaped(0),
            amount: 0.5,
        }]
    );
    assert_eq!(
        (g.lfos[0].rate, g.lfos[0].clock),
        (ir::Rate::Beats(1.0), ir::Clock::Voice)
    );
    assert!(
        !out.diagnostics.iter().any(|d| d.feature.contains("Pan")),
        "{:?}",
        out.diagnostics
    );
}

#[test]
fn a_mapped_bend_bends_pitch_through_its_mapper() {
    let xml = r#"<Program Gain="1">
      <Mappers><ControlSignalMapper Name="Mapper 1" Min="-1" Max="2" Integer="0" Discrete="0">-1,0 0,0 0,5 1,0</ControlSignalMapper></Mappers>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav">
            <Connections>
              <SignalConnection Ratio="2" Source="@PitchBend" Destination="Pitch" Mapper="Mapper 1" ConnectionMode="0" Bypass="0" Inverted="0"/>
            </Connections>
          </SamplePlayer></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    assert_eq!(
        g.mods.iter().map(|m| m.kind).collect::<Vec<_>>(),
        [ir::ModKind::Pitch {
            source: ir::Source::Shaped(0),
            semitones: 2.0,
            depth: 1.0,
            tau_ms: 0.0,
        }]
    );
    assert_eq!(g.shapers[0].source, ir::Source::PitchBend);
    assert_eq!(g.shapers[0].table, vec![-1.0, 0.0, 1.0, 2.0]);
}

#[test]
fn a_mapped_script_modulation_bends_pitch_through_its_mapper() {
    let xml = r#"<Program Gain="1">
      <Modulators><ScriptEventModulation Name="Script Event Modulation 1" EventId="2" Bipolar="0"/></Modulators>
      <Mappers><ControlSignalMapper Name="Mapper 1" Min="0" Max="1" Integer="0" Discrete="0">0,0 0,25 1,0</ControlSignalMapper></Mappers>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav">
            <Connections>
              <SignalConnection Ratio="2" Source="$Program/Script Event Modulation 1" Destination="Pitch" Mapper="Mapper 1" ConnectionMode="0" Bypass="0" Inverted="0"/>
            </Connections>
          </SamplePlayer></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    assert_eq!(
        g.mods.iter().map(|m| m.kind).collect::<Vec<_>>(),
        [ir::ModKind::Pitch {
            source: ir::Source::Shaped(0),
            semitones: 2.0,
            depth: 1.0,
            tau_ms: 0.0,
        }]
    );
    assert_eq!(g.shapers[0].source, ir::Source::Script(2));
    assert_eq!(g.shapers[0].table, vec![0.0, 0.25, 1.0]);
}

#[test]
fn an_aux_matrix_spreads_the_first_channel_to_both_sides() {
    // VWinds: keygroups sum into channel 1; the aux spreads it at 0.7.
    let xml = r#"<Program Gain="1">
      <Auxs>
        <AuxEffect Name="Aux0" Bypass="0" Gain="1" Pan="0" PreInsert="1">
          <Inserts>
            <GainMatrix Bypass="0" Gain_1_1="0.7" Gain_1_2="0.7" Gain_2_2="0" Gain_3_3="1"/>
            <TrackDelay Bypass="0" SyncToHost="0" DelayTime="0"/>
          </Inserts>
        </AuxEffect>
      </Auxs>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let aux = out.buses.last().unwrap();
    assert_eq!(
        aux.inserts[0].map(|e| e.kind),
        Some(ir::EffectKind::Matrix([[0.7, 0.0], [0.7, 0.0]]))
    );
    assert!(aux.inserts[1].is_none());
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn an_aux_eq_past_four_bands_runs_as_eqs_in_series() {
    let bands: String = (1..=6)
        .map(|n| format!(r#" Enabled{n}="1" Type{n}="6" Slope{n}="0" Freq{n}="{}" Gain{n}="{n}" Bandwidth{n}="0.1""#, n * 500))
        .collect();
    let xml = format!(
        r#"<Program Gain="1"><Auxs><AuxEffect Name="Aux0" Bypass="0" Gain="1" Pan="0" PreInsert="1"><Inserts>
          <DigitalEq Bypass="0" GainScale="1"{bands}>
            <Connections>
              <SignalConnection Ratio="1" Source="@MIDI CC 107" Destination="GainScale" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
            </Connections>
          </DigitalEq>
          <Gain Bypass="0" Volume="0.5"/>
        </Inserts></AuxEffect></Auxs></Program>"#
    );
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let aux = out.buses.last().unwrap();
    let gains: Vec<f32> = aux.inserts[..2]
        .iter()
        .flat_map(|e| match e.map(|e| e.kind) {
            Some(ir::EffectKind::Eq(b)) => b.map(|b| b.gain_db),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(gains, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 0.0, 0.0]);
    assert!(matches!(
        aux.inserts[2].map(|e| e.kind),
        Some(ir::EffectKind::Gain(_))
    ));
    // Band 5 is the second EQ's first.
    assert!(aux.mods.iter().any(|m| matches!(
        *m,
        ir::ModKind::EffectParam { insert: 1, param: ir::EffectParam::BellGain(0), depth, .. } if depth == 5.0
    )));
    assert!(!out.diagnostics.iter().any(|d| d.feature.contains("fourth")));
}

#[test]
fn velocity_controllers_and_lfos_scale_a_keygroups_gain_by_the_signed_law() {
    let xml = r#"<Program Gain="1">
      <Modulators>
        <LFO Name="LFO 1" Bypass="0" SyncToHost="0" DelayTime="0" RiseTime="0" Freq="2" Depth="0.5" Phase="0" WaveFormType="0" Retrigger="1" Bipolar="1"/>
      </Modulators>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Connections>
            <SignalConnection Ratio="0.75" Source="@VoiceParam Velocity" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
            <SignalConnection Ratio="-0.4" Source="@MIDI CC 1" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
            <SignalConnection Ratio="0.6" Source="$Program/LFO 1" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    let volume = |source, intensity| ir::ModKind::Volume {
        source,
        intensity,
        curve: ir::Curve::Signed,
        tau_ms: 0.0,
    };
    assert_eq!(
        g.mods.iter().map(|m| m.kind).collect::<Vec<_>>(),
        [
            volume(ir::Source::Velocity, 0.75),
            volume(ir::Source::Controller(1), -0.4),
            volume(ir::Source::Shaped(0), 0.6),
        ]
    );
    // Depth 0.5 inside `(m + 1)/2`: 0.25 at the trough, 0.75 at the crest.
    assert_eq!(g.shapers[0].table, [0.25, 0.75]);
    assert_eq!(g.shapers[0].source, ir::Source::Lfo(0));
    // −0.4 at CC 1 = 1: 1 − 0.4; 0.75 at velocity 0: 1 − 0.75.
    assert!((ir::Curve::Signed.gain(-0.4, 1.0) - 0.6).abs() < 1e-6);
    assert!((ir::Curve::Signed.gain(0.75, 0.0) - 0.25).abs() < 1e-6);
    assert!(out.diagnostics.iter().all(|d| !d.feature.contains("Gain")));
}

#[test]
fn an_amp_envelopes_curves_shape_its_stages() {
    let xml = r#"<Program Gain="1">
      <Modulators>
        <DAHDSR Name="DAHDSR 1" Bypass="0" DelayTime="0" AttackTime="0.1" AttackCurve="0.5" HoldTime="0" DecayTime="0" DecayCurve="-0.97" SustainLevel="1" ReleaseTime="5" ReleaseCurve="-0.97"/>
      </Modulators>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Connections>
            <SignalConnection Ratio="1" Source="$Program/DAHDSR 1" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let e = out.groups[0].envelope;
    // Half way through a stage, (1 − c)/2 of it is done: 1/(1 + e^(k/2)).
    let half = |k: f32| 1.0 / (1.0 + (k / 2.0).exp());
    assert!((half(e.attack_shape) - 0.25).abs() < 1e-4, "{e:?}");
    assert!((half(e.fall_shape) - 0.985).abs() < 1e-4, "{e:?}");
    assert!(!out.diagnostics.iter().any(|d| d.feature.contains("curve")));
}

#[test]
fn a_delayed_rising_lfo_reaches_pitch_through_its_rise() {
    let xml = r#"<Program Gain="1">
      <Modulators>
        <LFO Name="LFO 1" Bypass="0" SyncToHost="0" WaveFormType="0" Freq="1.5" Depth="1" Phase="0" Retrigger="1" Bipolar="1" DelayTime="0.5" RiseTime="1"/>
      </Modulators>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav">
            <Connections>
              <SignalConnection Ratio="0.5" Source="$Program/LFO 1" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
            </Connections>
          </SamplePlayer></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    assert!(g.mods.iter().any(|m| m.kind
        == ir::ModKind::Intensity {
            source: ir::Source::Envelope(0),
            of: ir::Source::Lfo(0),
            depth: 1.0,
        }));
    let levels: Vec<_> = g.envelopes[0]
        .points
        .iter()
        .map(|p| (p.ms, p.level))
        .collect();
    assert_eq!(levels, [(500.0, 0.0), (1000.0, 1.0)]);
    // 1.5 cycles a second, half a second late: three quarters back.
    assert!((g.lfos[0].phase - 0.25).abs() < 1e-6, "{:?}", g.lfos[0]);
    assert!(!out.diagnostics.iter().any(|d| d.feature.contains("Pitch")));
}

#[test]
fn a_controller_on_an_lfos_depth_scales_its_routes_by_the_connection_law() {
    let xml = r#"<Program Gain="1">
      <Modulators>
        <LFO Name="LFO 1" Bypass="0" SyncToHost="0" WaveFormType="0" Freq="5" Depth="1" Phase="0" Retrigger="1" Bipolar="1" DelayTime="0" RiseTime="0">
          <Connections>
            <SignalConnection Ratio="1" Source="@MIDI CC 1" Destination="Depth" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
            <SignalConnection Ratio="-0.5" Source="@VoiceParam Velocity" Destination="Depth" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
            <SignalConnection Ratio="0.4" Source="$Program/LFO 2" Destination="Depth" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
        </LFO>
        <LFO Name="LFO 2" Bypass="0" SyncToHost="0" WaveFormType="0" Freq="0.5" Depth="0.5" Phase="0" Retrigger="1" Bipolar="1" DelayTime="0" RiseTime="0"/>
      </Modulators>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav">
            <Connections>
              <SignalConnection Ratio="0.5" Source="$Program/LFO 1" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
            </Connections>
          </SamplePlayer></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    let scales: Vec<_> = g
        .mods
        .iter()
        .filter_map(|m| match m.kind {
            ir::ModKind::Intensity { source, of, depth } => {
                Some((source, of, depth, m.name.starts_with('#')))
            }
            _ => None,
        })
        .collect();
    let of = ir::Source::Lfo(0);
    // The wheel: 1 − (1 − x) at ratio 1, named for the script's knob.
    assert!(
        scales.contains(&(ir::Source::Controller(1), of, 1.0, true)),
        "{scales:?}"
    );
    // Velocity at −0.5: 1 − 0.5·v, which is Intensity 0.5 on 1 − v.
    let falling = scales.iter().find(|s| s.2 == 0.5).expect("velocity scale");
    let ir::Source::Shaped(n) = falling.0 else {
        panic!("{falling:?}")
    };
    assert_eq!(g.shapers[usize::from(n)].source, ir::Source::Velocity);
    assert_eq!(g.shapers[usize::from(n)].table, [1.0, 0.0]);
    // LFO 2 (depth 0.5) enters at (0.5·x + 1)/2.
    let wobble = scales
        .iter()
        .find(|s| (s.2 - 0.4).abs() < 1e-6)
        .expect("LFO scale");
    let ir::Source::Shaped(n) = wobble.0 else {
        panic!("{wobble:?}")
    };
    assert_eq!(g.shapers[usize::from(n)].table, [0.25, 0.75]);
    assert!(
        !out.diagnostics.iter().any(|d| d.feature.contains("Depth")),
        "{:?}",
        out.diagnostics
    );
}

#[test]
fn an_lfo_on_an_lfos_freq_moves_its_rate_live() {
    let xml = r#"<Program Gain="1">
      <Modulators>
        <LFO Name="LFO 1" Bypass="0" SyncToHost="0" WaveFormType="0" Freq="12" Depth="1" Phase="0" Retrigger="1" Bipolar="1" DelayTime="0" RiseTime="0">
          <Connections>
            <SignalConnection Ratio="1" Source="$Program/LFO 2" Destination="Freq" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
        </LFO>
        <LFO Name="LFO 2" Bypass="0" SyncToHost="0" WaveFormType="6" Freq="20" Depth="0.14" Phase="0" Retrigger="1" Bipolar="1" DelayTime="0" RiseTime="0"/>
      </Modulators>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav">
            <Connections>
              <SignalConnection Ratio="0.5" Source="$Program/LFO 1" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
            </Connections>
          </SamplePlayer></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    let rates: Vec<_> = g
        .mods
        .iter()
        .filter_map(|m| match m.kind {
            ir::ModKind::LfoHz {
                source,
                lfo,
                depth,
                range_hz,
            } => Some((source, lfo, depth, range_hz)),
            _ => None,
        })
        .collect();
    // LFO 2 at depth 0.14 adds 0.14 × 20 Hz per unit to LFO 1's rate.
    let [(ir::Source::Lfo(wobble), 0, depth, [0.0, 20.0])] = rates[..] else {
        panic!("{rates:?}")
    };
    assert!((depth - 0.14).abs() < 1e-6);
    // Random S&H draws once a cycle: the random wave at half the Hz.
    let wobble = g.lfos[usize::from(wobble)];
    assert_eq!(
        (wobble.wave, wobble.rate),
        (ir::Wave::Random, ir::Rate::Hz(10.0))
    );
    assert!(
        !out.diagnostics.iter().any(|d| d.feature.contains("Freq")),
        "{:?}",
        out.diagnostics
    );
}

#[test]
fn a_macro_a_script_can_set_is_read_live_on_a_ratio_too() {
    let xml = r#"<Program Gain="1">
      <ControlSignalSources>
        <ConstantModulation Name="Macro 6" Bypass="0" Bipolar="0" Value="0.5"/>
        <ConstantModulation Name="Macro 7" Bypass="0" Bipolar="0" Value="0"/>
        <LFO Name="LFO 1" Bypass="0" SyncToHost="0" WaveFormType="9" Freq="5" Depth="1" Phase="0" Retrigger="1" Bipolar="1" DelayTime="0" RiseTime="0">
          <Connections>
            <SignalConnection Ratio="1" Source="$Program/Macro 7" Destination="Depth" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
          <UserTable>0 1 0 -1</UserTable>
        </LFO>
      </ControlSignalSources>
      <Scripts><ScriptProcessor Name="EventProcessor0" Bypass="0" API_version="21"><script><![CDATA[--]]></script></ScriptProcessor></Scripts>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav">
            <Connections>
              <SignalConnection Ratio="0.5" Source="$Program/LFO 1" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
              <SignalConnection Ratio="2" Source="@MIDI CC 1" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0">
                <Connections>
                  <SignalConnection Ratio="1" Source="$Program/Macro 6" Destination="Ratio" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
                </Connections>
              </SignalConnection>
            </Connections>
          </SamplePlayer></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    // Macro 7 scales the (user table) LFO's depth live, Macro 6 the
    // wheel's pitch range (2 semitones at full) live: each a factor.
    let factor_of = |input: ir::Source| {
        let b = g.blends.iter().find(|b| b.terms[0].input.source == input)?;
        let [f] = &b.terms[0].factors[..] else {
            return None;
        };
        Some(f.input.source)
    };
    let Some(ir::Source::Script(depth)) = factor_of(ir::Source::Controller(1)) else {
        panic!("{:?}", g.blends)
    };
    assert!(depth >= 0x8000, "{depth:#x}");
    let lfo = g
        .blends
        .iter()
        .flat_map(|b| &b.terms)
        .flat_map(|t| &t.factors);
    let id = lfo
        .map(|f| f.input.source)
        .find(|&s| s != ir::Source::Script(depth));
    let Some(ir::Source::Script(id)) = id else {
        panic!("{:?}", g.blends)
    };
    // The tree the script sees names it.
    assert!(out.scripts[0].data.iter().any(|(name, text)| {
        name == "__program.lua" && String::from_utf8_lossy(text).contains(&format!("mod={id}"))
    }));
    let wheel = g.mods.iter().find_map(|m| match m.kind {
        ir::ModKind::Pitch {
            source: ir::Source::Blend(n),
            semitones,
            ..
        } if g.blends[usize::from(n)].terms[0].input.source == ir::Source::Controller(1) => {
            Some(semitones)
        }
        _ => None,
    });
    assert_eq!(wheel, Some(2.0));
}

#[test]
fn chaos_lfos_are_their_attractors() {
    for (wave, lorenz) in [("7", true), ("8", false)] {
        let node = format!(
            r#"<LFO Name="LFO 1" Bypass="0" SyncToHost="0" WaveFormType="{wave}" Freq="4.5" Depth="0.7" Phase="0" Retrigger="0" Bipolar="1" DelayTime="0" RiseTime="0"/>"#
        );
        let doc = roxmltree::Document::parse(&node).unwrap();
        let (lfo, depth) = modulation::lfo(doc.root_element()).expect("carried");
        let ir::Wave::Chaos { attractor, .. } = lfo.wave else {
            panic!("{lfo:?}")
        };
        assert_eq!(matches!(attractor, ir::Attractor::Lorenz { .. }), lorenz);
        assert_eq!(
            (lfo.rate, lfo.clock, depth),
            (ir::Rate::Hz(4.5), ir::Clock::Free, 0.7)
        );
    }
}

#[test]
fn a_wheel_on_a_send_scales_its_level_live() {
    // AO Crusader MW: CC 1 at ratio 1 on the send's Gain; the step
    // envelope (a note's) stays reported.
    let xml = r#"<Program Gain="1">
      <Modulators><StepModulator Name="Step Envelope 1" Bypass="0"/></Modulators>
      <Layers><Layer Gain="1" Pan="0">
        <BusRouters><BusRouter Bypass="0" Gain="0.5" Destination="../../Aux0" PreFader="0">
          <Connections>
            <SignalConnection Ratio="1" Source="@MIDI CC 1" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
            <SignalConnection Ratio="1" Source="$Program/Step Envelope 1" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
        </BusRouter></BusRouters>
        <Keygroups><Keygroup LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup></Keygroups>
      </Layer></Layers>
      <Auxs><AuxEffect Name="Aux0" Bypass="0" Gain="1" PreInsert="1"/></Auxs>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let layer = &out.buses[1];
    assert_eq!(layer.sends[0].level, 0.5);
    let [ir::ModKind::Send { source, send: 0 }] = layer.mods[..] else {
        panic!("{:?}", layer.mods);
    };
    assert_eq!(source, ir::Source::Blend(0));
    let factor = &layer.blends[0].terms[0].factors[0];
    assert_eq!(
        (factor.input.source, factor.depth),
        (ir::Source::Controller(1), 1.0)
    );
    let gaps = out
        .diagnostics
        .iter()
        .filter(|d| d.feature == "modulation Gain");
    assert_eq!(
        gaps.map(|d| d.detail.as_str()).collect::<Vec<_>>(),
        ["1× (first: $Program/Step Envelope 1)"]
    );
}

#[test]
fn an_effect_on_an_aux_no_send_reaches_is_latent() {
    let program = |gain: &str| {
        format!(
            r#"<Program Gain="1">
      <Layers><Layer Gain="1" Pan="0">
        <BusRouters><BusRouter Bypass="0" Gain="{gain}" Destination="../../Aux3" PreFader="1"/></BusRouters>
        <Keygroups><Keygroup LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup></Keygroups>
      </Layer></Layers>
      <Auxs><AuxEffect Name="Aux3" Bypass="0" Gain="1" PreInsert="1">
        <Inserts><CombFilter Name="InsertFX0" Bypass="0" Freq="40" Q="0.9"/></Inserts>
      </AuxEffect></Auxs>
    </Program>"#
        )
    };
    for (gain, impact) in [("0", ir::Impact::Latent), ("0.5", ir::Impact::Audible)] {
        let xml = program(gain);
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
        let comb = out
            .diagnostics
            .iter()
            .find(|d| d.feature == "insert CombFilter");
        assert_eq!(
            comb.map(|d| d.impact),
            Some(impact),
            "{gain}: {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn a_unipolar_lfo_scales_a_keygroups_gain_by_its_own_m() {
    // `m = (w + 1)·d/2` takes the plain law; inverted `1 − m`; a rising saw
    // runs the table backwards. Its depth routes are reported.
    let xml = r#"<Program Gain="1">
      <Modulators>
        <LFO Name="LFO 1" Bypass="0" SyncToHost="0" DelayTime="0" RiseTime="0" Freq="2" Depth="0.5" Phase="0" WaveFormType="0" Retrigger="1" Bipolar="0">
          <Connections>
            <SignalConnection Ratio="1" Source="@MIDI CC 1" Destination="Depth" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
        </LFO>
        <LFO Name="LFO 2" Bypass="0" SyncToHost="0" DelayTime="0" RiseTime="0" Freq="2" Depth="0.25" Phase="0" WaveFormType="3" Retrigger="1" Bipolar="0"/>
      </Modulators>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Connections>
            <SignalConnection Ratio="-1" Source="$Program/LFO 1" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
            <SignalConnection Ratio="0.5" Source="$Program/LFO 1" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="1"/>
            <SignalConnection Ratio="0.5" Source="$Program/LFO 2" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    let tables: Vec<_> = g
        .shapers
        .iter()
        .map(|t| (t.source, t.table.clone()))
        .collect();
    assert_eq!(
        tables,
        [
            (ir::Source::Lfo(0), vec![0.0, 0.5]),
            (ir::Source::Lfo(0), vec![1.0, 0.5]),
            (ir::Source::Lfo(1), vec![0.25, 0.0]),
        ]
    );
    // At rest (wave −1) the first route leaves the gain alone: `1 + r·0`.
    assert!((ir::Curve::Signed.gain(-1.0, tables[0].1[0]) - 1.0).abs() < 1e-6);
    assert!(
        !g.mods
            .iter()
            .any(|m| matches!(m.kind, ir::ModKind::Intensity { .. }))
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.feature == "unipolar LFO depth route")
    );
}

#[test]
fn a_rising_lfo_scales_a_keygroups_gain_through_its_rise() {
    // The rise scales the wave under the gain's table: `(1 + ρ·d·w)/2`.
    let xml = r#"<Program Gain="1">
      <Modulators>
        <LFO Name="LFO 1" Bypass="0" SyncToHost="0" DelayTime="0" RiseTime="0.5" Freq="2" Depth="0.5" Phase="0" WaveFormType="0" Retrigger="1" Bipolar="1"/>
      </Modulators>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Connections>
            <SignalConnection Ratio="-1" Source="$Program/LFO 1" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    assert_eq!(g.shapers[0].source, ir::Source::Lfo(0));
    assert_eq!(g.shapers[0].table, [0.25, 0.75]);
    assert_eq!(
        g.envelopes[0].points.last().map(|p| (p.ms, p.level)),
        Some((500.0, 1.0))
    );
    let kinds: Vec<_> = g.mods.iter().map(|m| m.kind).collect();
    assert!(kinds.contains(&ir::ModKind::Intensity {
        source: ir::Source::Envelope(0),
        of: ir::Source::Lfo(0),
        depth: 1.0,
    }));
    assert!(kinds.iter().any(|k| matches!(
        k,
        ir::ModKind::Volume { source: ir::Source::Shaped(0), intensity, .. } if *intensity == -1.0
    )));
}

#[test]
fn a_program_plays_in_a_part_that_cc_7_10_and_11_move() {
    let xml = r#"<Program Gain="1"><Layers/></Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let c = out.output.controllers.unwrap();
    assert_eq!(
        (c.volume, c.pan, c.power, c.expression),
        (7, 10, 1, Some(11))
    );
    // Edition mode over the part's Gain, linear 0..10^(6/20).
    assert_eq!(c.top_db, 6.0);
    // A stereo pan turns the far side to `(1 + cos π·pan)/2`.
    assert_eq!(out.output.balance, ir::Balance::Cosine);
}

#[test]
fn a_key_on_a_routes_ratio_scales_it_per_voice() {
    // VWinds' vibrato pitch: CC 103's route scaled by KeyFollow (bipolar,
    // read at `(k + 1)/2`) and here by Key at −0.5 (`1 − 0.5·x/127`).
    let xml = r#"<Program Gain="1">
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav">
            <Connections>
              <SignalConnection Ratio="3" Source="@MIDI CC 103" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0">
                <Connections>
                  <SignalConnection Ratio="1" Source="@VoiceParam KeyFollow" Destination="Ratio" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
                  <SignalConnection Ratio="-0.5" Source="@VoiceParam Key" Destination="Ratio" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
                </Connections>
              </SignalConnection>
            </Connections>
          </SamplePlayer></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    assert!(g.mods.iter().any(|m| matches!(
        m.kind,
        ir::ModKind::Pitch { source: ir::Source::Blend(0), semitones, .. } if semitones == 3.0
    )));
    let term = &g.blends[0].terms[0];
    assert_eq!(term.input.source, ir::Source::Controller(103));
    let [follow, key] = &term.factors[..] else {
        panic!("{:?}", term.factors)
    };
    assert_eq!(
        (follow.input.source, follow.depth, key.depth),
        (ir::Source::Key, 1.0, -0.5)
    );
    let t = &follow.input.table;
    assert_eq!(
        (t[0], t[30], t[60], t[120], t[127]),
        (0.0, 0.125, 0.5, 1.0, 1.0)
    );
    assert!((t[75] - (0.25f32.sqrt() + 1.0) / 2.0).abs() < 1e-6);
    assert_eq!((key.input.table[0], key.input.table[127]), (0.0, 1.0));
    // At key 60 the bend is 3 × 0.5 × (1 − 0.5·60/127).
    assert!((key.at(key.input.table[60]) - (1.0 - 0.5 * 60.0 / 127.0)).abs() < 1e-6);
    assert!(
        out.diagnostics.iter().all(|d| !d.feature.contains("Pitch")),
        "{:?}",
        out.diagnostics
    );
}

#[test]
fn a_user_table_lfo_growls_on_every_player_through_its_table_and_depth() {
    // VWinds' growl: LFO 1 reads its 256-point table, a script modulation
    // scales its depth, and its routes sit on each player's Gain (ratio
    // scaled by Key) and Pitch alike.
    let points: Vec<String> = (0..256)
        .map(|j| format!("{:.6}", j as f32 / 256.0).replace('.', ","))
        .collect();
    let routes = r#"<Connections>
        <SignalConnection Name="a" Ratio="0.5" Source="$Program/LFO 1" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0">
          <Connections><SignalConnection Ratio="-1" Source="@VoiceParam Key" Destination="Ratio" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/></Connections>
        </SignalConnection>
        <SignalConnection Name="b" Ratio="2" Source="$Program/LFO 1" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
      </Connections>"#;
    let xml = format!(
        r#"<Program Gain="1">
      <Modulators>
        <LFO Name="LFO 1" Bypass="0" SyncToHost="0" DelayTime="0" RiseTime="0" Freq="10" Depth="1" Phase="0" WaveFormType="9" Retrigger="1" Bipolar="1">
          <Connections>
            <SignalConnection Ratio="1" Source="$Program/Script Event Modulation 7" Destination="Depth" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
          <UserTable>{}</UserTable>
        </LFO>
        <ScriptEventModulation Name="Script Event Modulation 7" EventId="12" Bipolar="0"/>
      </Modulators>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127"><Oscillators>
          <SamplePlayer Bypass="0" SamplePath="./a.wav">{routes}</SamplePlayer>
          <SamplePlayer Bypass="0" SamplePath="./b.wav">{routes}</SamplePlayer>
        </Oscillators></Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#,
        points.join(" ")
    );
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    let blend = |want: fn(&ir::ModKind) -> Option<ir::Source>| {
        let source = g.mods.iter().find_map(|m| want(&m.kind)).unwrap();
        let ir::Source::Blend(n) = source else {
            panic!("{source:?}")
        };
        g.blends[usize::from(n)].clone()
    };
    let pitch = blend(|k| match k {
        ir::ModKind::Pitch {
            source, semitones, ..
        } if *semitones == 2.0 => Some(*source),
        _ => None,
    });
    // The saw reads `1 − p`: entry 1 is the table's last point.
    let growl = &pitch.terms[0];
    assert_eq!(
        (growl.input.source, growl.amount, pitch.base),
        (ir::Source::Lfo(0), 1.0, 0.0)
    );
    let t = &growl.input.table;
    assert_eq!((t.len(), t[0], t[256]), (257, 0.0, 0.0));
    assert!((t[1] - 255.0 / 256.0).abs() < 1e-5);
    assert_eq!(growl.factors[0].input.source, ir::Source::Script(12));
    assert_eq!(growl.factors[0].depth, 1.0);
    // Gain: `1 − rk + rk·(m + 1)/2` as `1 − r + r·x'`, `x' = 1 − k/2 + (k/2)·m`.
    let gain = blend(|k| match k {
        ir::ModKind::Volume {
            source, intensity, ..
        } if *intensity == 0.5 => Some(*source),
        _ => None,
    });
    assert_eq!(gain.base, 1.0);
    let [growl, rest] = &gain.terms[..] else {
        panic!("{:?}", gain.terms)
    };
    assert_eq!(
        (growl.amount, rest.amount, rest.input.source),
        (0.5, -0.5, ir::Source::Constant)
    );
    assert_eq!((growl.factors.len(), rest.factors.len()), (2, 1));
    assert_eq!(
        (rest.factors[0].input.source, rest.factors[0].depth),
        (ir::Source::Key, -1.0)
    );
    // Both players' copies are carried once.
    assert_eq!(
        g.mods
            .iter()
            .filter(|m| matches!(m.kind, ir::ModKind::Volume { .. }))
            .count(),
        1
    );
    assert!(
        out.diagnostics
            .iter()
            .all(|d| !d.feature.starts_with("modulation")),
        "{:?}",
        out.diagnostics
    );
}

#[test]
fn a_mapped_bend_on_a_keygroups_gain_enters_at_half_past_its_mapper() {
    // VWinds' pitch-bend crossfade mappers swing −1..=1: the law takes them at `(y + 1)/2`.
    let xml = r#"<Program Gain="1">
      <Mappers><ControlSignalMapper Name="Mapper 1" Min="-1" Max="1">-1,0 1,0 1,0</ControlSignalMapper></Mappers>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Connections>
            <SignalConnection Ratio="1" Source="@PitchBend" Destination="Gain" Mapper="Mapper 1" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    assert_eq!(g.shapers[0].source, ir::Source::PitchBend);
    assert_eq!(g.shapers[0].table, [0.0, 1.0, 1.0]);
}

#[test]
fn a_mapped_gain_enters_at_half_past_its_mapper_only_when_the_mapper_is_bipolar() {
    // VWinds' AttacksMapper (`Min` −1) opens 0..=1 only re-centred; its
    // brightness bells (`Min` 0) sum to ~1 only as they are.
    let xml = r#"<Program Gain="1">
      <Mappers>
        <ControlSignalMapper Name="Mapper 1" Min="-1" Max="1">-1,0 1,0</ControlSignalMapper>
        <ControlSignalMapper Name="Mapper 2" Min="0" Max="1">0,5 1,0</ControlSignalMapper>
      </Mappers>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Connections>
            <SignalConnection Ratio="1" Source="@MIDI CC 1" Destination="Gain" Mapper="Mapper 1" ConnectionMode="0" Bypass="0" Inverted="0"/>
            <SignalConnection Ratio="1" Source="@MIDI CC 2" Destination="Gain" Mapper="Mapper 2" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let tables: Vec<_> = out.groups[0]
        .shapers
        .iter()
        .map(|s| (s.source, s.table.clone()))
        .collect();
    assert_eq!(
        tables,
        [
            (ir::Source::Controller(1), vec![0.0, 1.0]),
            (ir::Source::Controller(2), vec![0.5, 1.0]),
        ]
    );
}

#[test]
fn a_mapped_key_follow_on_a_ratio_centres_the_mapper_output() {
    // The binary centres bipolar KeyFollow even through a unipolar mapper:
    // VWinds' clarinet has this on CC 103's ratio (0x141429a80).
    let xml = r#"<Program Gain="1">
      <Mappers><ControlSignalMapper Name="Mapper 1" Min="0" Max="1">0,0 1,0</ControlSignalMapper></Mappers>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav">
            <Connections>
              <SignalConnection Ratio="3" Source="@MIDI CC 103" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0">
                <Connections>
                  <SignalConnection Ratio="1" Source="@VoiceParam KeyFollow" Destination="Ratio" Mapper="Mapper 1" ConnectionMode="0" Bypass="0" Inverted="0"/>
                </Connections>
              </SignalConnection>
            </Connections>
          </SamplePlayer></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let t = &out.groups[0].blends[0].terms[0].factors[0].input.table;
    assert_eq!((t[0], t[60], t[120]), (0.5, 0.75, 1.0));

    // The two saved Mapper 28 values surrounding key 49 in Bb Clarinet 2.
    // The binary evaluator and connection law return 0.61297661 there.
    let mut points = vec!["0"; 128];
    points[42] = "0,22258";
    points[43] = "0,232191";
    let saved = xml.replace("0,0 1,0", &points.join(" "));
    let doc = roxmltree::Document::parse(&saved).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let table = &out.groups[0].blends[0].terms[0].factors[0].input.table;
    assert!((table[49] - 0.612_976_6).abs() < 1e-6);
}

#[test]
fn a_live_macro_on_a_players_gain_scales_it_through_its_bipolar_mapper() {
    // VWinds' AttacksMapper: brightness (a bipolar macro CC 102 moves) on an
    // attack player's Gain through a `Min` −1 mapper, closed until the top.
    let xml = r#"<Program Gain="1">
      <Modulators><ConstantModulation Name="Macro 1" Bipolar="1" Value="0">
        <Connections><SignalConnection Ratio="1" Source="@MIDI CC 102" Destination="Value" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/></Connections>
      </ConstantModulation></Modulators>
      <Mappers><ControlSignalMapper Name="Mapper 1" Min="-1" Max="1">-1,0 -1,0 1,0</ControlSignalMapper></Mappers>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav">
            <Connections>
              <SignalConnection Ratio="1" Source="$Program/Macro 1" Destination="Gain" Mapper="Mapper 1" ConnectionMode="0" Bypass="0" Inverted="0"/>
            </Connections>
          </SamplePlayer></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    assert!(g.mods.iter().any(|m| matches!(
        m.kind,
        ir::ModKind::Volume { source: ir::Source::Shaped(0), intensity, curve: ir::Curve::Signed, .. }
            if intensity == 1.0
    )));
    assert_eq!(g.shapers[0].table, [0.0, 0.0, 1.0]);
    let ir::Source::Blend(b) = g.shapers[0].source else {
        panic!("{:?}", g.shapers[0])
    };
    assert_eq!(g.blends[usize::from(b)].base, 0.0);
    assert_eq!(
        g.blends[usize::from(b)].terms[0].input.source,
        ir::Source::Controller(102)
    );
}

#[test]
fn a_gain_one_player_has_alone_scales_its_zones_cells() {
    // VWinds' noise crossfade: CC 102 on each player through its own mapper.
    let xml = r#"<Program Gain="1">
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Oscillators>
            <SamplePlayer Bypass="0" SamplePath="./a.wav">
              <Connections>
                <SignalConnection Ratio="0.5" Source="@MIDI CC 102" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
              </Connections>
            </SamplePlayer>
            <SamplePlayer Bypass="0" SamplePath="./b.wav"/>
          </Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let g = &out.groups[0];
    let cells: Vec<_> = g
        .mods
        .iter()
        .filter_map(|m| match m.kind {
            ir::ModKind::Mix {
                source,
                zone,
                row,
                side,
            } => Some((source, zone, row, side)),
            _ => None,
        })
        .collect();
    let s = ir::Source::Shaped(0);
    assert_eq!(cells, [(s, 0, 0, 0), (s, 0, 0, 1)], "{:?}", g.zones[0].mix);
    assert_eq!(g.zones[0].mix, [[1.0, 1.0]]);
    assert!(g.zones[1].mix.is_empty());
    // `1 − 0.5 + 0.5·x` over the controller.
    assert_eq!(g.shapers[0].source, ir::Source::Controller(102));
    assert_eq!(g.shapers[0].table, [0.5, 1.0]);
    assert!(
        out.diagnostics.iter().all(|d| !d.feature.contains("Gain")),
        "{:?}",
        out.diagnostics
    );
}

#[test]
fn a_macro_held_on_a_script_modulations_ratio_scales_its_bend() {
    // VWinds' release pitch drift: 2 st, scaled by "ReleasePitchDrift" at 0.7.
    let xml = r#"<Program Gain="1">
      <Modulators>
        <ScriptEventModulation Name="Script Event Modulation 1" EventId="2" Bipolar="0"/>
        <ConstantModulation Name="Macro 2" Bipolar="0" Value="0.7"/>
      </Modulators>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav">
            <Connections>
              <SignalConnection Ratio="2" Source="$Program/Script Event Modulation 1" Destination="Pitch" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0">
                <Connections>
                  <SignalConnection Ratio="1" Source="$Program/Macro 2" Destination="Ratio" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
                </Connections>
              </SignalConnection>
            </Connections>
          </SamplePlayer></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let semitones: Vec<_> = out.groups[0]
        .mods
        .iter()
        .filter_map(|m| match m.kind {
            ir::ModKind::Pitch {
                source: ir::Source::Script(2),
                semitones,
                ..
            } => Some(semitones),
            _ => None,
        })
        .collect();
    assert_eq!(semitones.len(), 1);
    assert!((semitones[0] - 1.4).abs() < 1e-6, "{semitones:?}");
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn a_discrete_mapper_is_read_at_each_of_its_points() {
    // VWinds' pitch-bend crossfade mappers are Discrete: 0x1411391a6 reads
    // point ⌊n·x⌋, at each saved point's own `x` that point.
    let xml = r#"<Program Gain="1">
      <Mappers><ControlSignalMapper Name="Mapper 1" Min="0" Max="1" Integer="0" Discrete="1">0,0 0,5 1,0</ControlSignalMapper></Mappers>
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Connections>
            <SignalConnection Ratio="1" Source="@MIDI CC 2" Destination="Gain" Mapper="Mapper 1" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    assert_eq!(out.groups[0].shapers[0].table, [0.0, 0.5, 1.0]);
}

#[test]
fn an_inverted_mapped_bend_reads_its_mapper_backwards() {
    // VWinds V2's bend on its EQ: `−x` before a mapper read at `(x + 1)/2`.
    let xml = r#"<Program Gain="1">
      <Auxs>
        <AuxEffect Name="Aux0" Bypass="0" Gain="1" Pan="0" PreInsert="1">
          <Inserts>
            <DigitalEq Bypass="0" GainScale="0" Enabled1="1" Type1="6" Slope1="0" Freq1="261.6" Gain1="-6" Bandwidth1="0.4">
              <Mappers>
                <ControlSignalMapper Name="Mapper 51" Min="-1" Max="1">-1,0 0,5 1,0</ControlSignalMapper>
              </Mappers>
              <Connections>
                <SignalConnection Ratio="0.3" Source="@PitchBend" Destination="GainScale" Mapper="Mapper 51" ConnectionMode="0" Bypass="0" Inverted="1"/>
              </Connections>
            </DigitalEq>
          </Inserts>
        </AuxEffect>
      </Auxs>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let term = &out.buses.last().unwrap().blends[0].terms[0];
    assert_eq!(term.input.source, ir::Source::PitchBend);
    assert_eq!(term.input.table, [1.0, 0.5, -1.0]);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn a_wheel_opens_a_phasor_through_its_power_law() {
    // AO Dancing Bartok: Depth 0, the wheel adds up to 1; out is
    // `(x + d·y)/√(1 + d²)`, so wet and dry both end at 1/√2.
    let xml = r#"<Program Gain="1">
      <Layers><Layer Gain="1" Pan="0">
        <Inserts><Phasor Bypass="0" SyncToHost="0" Speed="0.5" MinFreq="200" MaxFreq="3000" Feedback="0" Depth="0" Spread="1" Order="4" LFOSHape="0">
          <Connections>
            <SignalConnection Ratio="1" Source="@MIDI CC 1" Destination="Depth" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
        </Phasor></Inserts>
        <Keygroups><Keygroup LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup></Keygroups>
      </Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    let b = &out.buses[1];
    let tables: Vec<_> = b
        .mods
        .iter()
        .map(|m| {
            let ir::ModKind::EffectParam {
                source: ir::Source::Blend(n),
                param,
                ..
            } = *m
            else {
                panic!("{m:?}");
            };
            let t = &b.blends[usize::from(n)].terms[0].input;
            assert_eq!(t.source, ir::Source::Controller(1));
            (param, t.table[0], *t.table.last().unwrap())
        })
        .collect();
    let half = std::f32::consts::FRAC_1_SQRT_2;
    assert_eq!(tables.len(), 2, "{tables:?}");
    let (wet, dry) = (tables[0], tables[1]);
    assert_eq!(
        (wet.0, dry.0),
        (ir::EffectParam::Output, ir::EffectParam::Dry)
    );
    assert!(wet.1 == 0.0 && (wet.2 - half).abs() < 1e-6, "{wet:?}");
    assert!(
        dry.1 == 0.0 && (dry.2 - (half - 1.0)).abs() < 1e-6,
        "{dry:?}"
    );
    assert!(
        out.diagnostics
            .iter()
            .all(|d| d.feature != "modulation Depth")
    );
}
