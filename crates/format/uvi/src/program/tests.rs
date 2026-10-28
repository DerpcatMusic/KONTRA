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
    assert_eq!(out.voices, 4);
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
    assert!(
        (f.hz - 20_000.0 * 1000f32.powf(-0.6)).abs() < 0.1,
        "{}",
        f.hz
    );
    assert!((f.octaves - 1000f32.log2()).abs() < 1e-4);
    assert_eq!(
        g.mods.iter().map(|m| m.kind).collect::<Vec<_>>(),
        [ir::ModKind::Cutoff {
            source: ir::Source::Velocity,
            insert: 1,
            depth: 0.6,
        }]
    );
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
    // Bipolar Macro 1 at 0 reads its mappers halfway: variant a's bell is 0
    // there, b's is 0.5 (between 0.4 and 0.6); c rides at half ratio.
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
    // a is silent and not loaded; b at 0.5; c at 1 - 0.5 + 0.5 * 0.5.
    assert_eq!(mixes, vec![vec![[0.0, 0.5]], vec![[0.75, 0.0]]]);
    assert_eq!(out.samples.len(), 2);
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.feature.starts_with("matrix cell"))
    );
}

#[test]
fn a_macro_a_controller_moves_crossfades_its_cells_live() {
    // Macro 1 = 0 + CC 102 × 0.5 (bipolar: position 0.5 + CC × 0.25), its
    // ratio scaled by CC 104 through a mapper; the cell reads Mapper 0.
    let xml = r#"<Program Gain="1">
      <ControlSignalSources>
        <ConstantModulation Name="Macro 1" Bipolar="1" Value="0">
          <Connections>
            <SignalConnection Ratio="0.5" Source="@MIDI CC 102" Destination="Value">
              <Connections>
                <SignalConnection Ratio="1" Source="@MIDI CC 104" Destination="Ratio" Mapper="Mapper 1"/>
              </Connections>
            </SignalConnection>
          </Connections>
        </ConstantModulation>
      </ControlSignalSources>
      <Mappers>
        <ControlSignalMapper Name="Mapper 0" Min="0" Max="1">0,0 1,0</ControlSignalMapper>
        <ControlSignalMapper Name="Mapper 1" Min="0" Max="1">0,0 0,5 1,0</ControlSignalMapper>
      </Mappers>
      <Layers>
        <Layer Gain="1" Pan="0">
          <Keygroups>
            <Keygroup LowKey="0" HighKey="127">
              <Inserts>
                <GainMatrix Bypass="0" Gain_1_1="1" Gain_2_2="1">
                  <Connections>
                    <SignalConnection Ratio="1" Source="$Program/Macro 1" Destination="Gain_2_2" Mapper="Mapper 0"/>
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
    assert_eq!(blend.base, 0.5);
    let term = &blend.terms[0];
    assert_eq!(term.input.source, ir::Source::Controller(102));
    assert_eq!(term.amount, 0.25);
    assert_eq!(term.factors[0].input.source, ir::Source::Controller(104));
    assert_eq!(term.factors[0].input.table, vec![0.0, 0.5, 1.0]);
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.feature.starts_with("matrix"))
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
                <ControlSignalMapper Name="Mapper 51" Min="0" Max="1">1,0 0,0 1,0</ControlSignalMapper>
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
fn velocity_scales_a_keygroups_gain() {
    let xml = r#"<Program Gain="1">
      <Layers><Layer Gain="1" Pan="0"><Keygroups>
        <Keygroup LowKey="0" HighKey="127">
          <Connections>
            <SignalConnection Ratio="0.75" Source="@VoiceParam Velocity" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
          </Connections>
          <Oscillators><SamplePlayer Bypass="0" SamplePath="./a.wav"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program>"#;
    let doc = roxmltree::Document::parse(xml).unwrap();
    let out = translate(doc.root_element(), "Presets", &Stub, &|_| Vec::new()).unwrap();
    assert_eq!(
        out.groups[0]
            .mods
            .iter()
            .map(|m| m.kind)
            .collect::<Vec<_>>(),
        [ir::ModKind::Volume {
            source: ir::Source::Velocity,
            intensity: 0.75,
            curve: ir::Curve::Depth { power: 1 },
        }]
    );
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
    let levels: Vec<_> = g.envelopes[0].points.iter().map(|p| (p.ms, p.level)).collect();
    assert_eq!(levels, [(500.0, 0.0), (1000.0, 1.0)]);
    // 1.5 cycles a second, half a second late: three quarters back.
    assert!((g.lfos[0].phase - 0.25).abs() < 1e-6, "{:?}", g.lfos[0]);
    assert!(!out.diagnostics.iter().any(|d| d.feature.contains("Pitch")));
}

