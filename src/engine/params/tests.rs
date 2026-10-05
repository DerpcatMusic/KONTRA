#[test]
fn solid_geq_callback_captions_show_signed_dsp_gain() {
    use crate::ksp::{KspEngine, LogEngine, Runtime, Value};
    for name in [
        "$ENGINE_PAR_SEQ_LF_GAIN",
        "$ENGINE_PAR_SEQ_LMF_GAIN",
        "$ENGINE_PAR_SEQ_HMF_GAIN",
        "$ENGINE_PAR_SEQ_HF_GAIN",
    ] {
        let source = format!(
            "on init\nmake_perfview\ndeclare ui_slider $gain(0,1000000)\ndeclare ui_label $caption(1,1)\nend on\non ui_control($gain)\nset_engine_par({name},$gain,-1,2,$NI_BUS_OFFSET)\nset_control_par_str(get_ui_id($gain),$CONTROL_PAR_LABEL,get_engine_par_disp({name},-1,2,$NI_BUS_OFFSET) & \" dB\")\nset_text($caption,get_engine_par_disp_ext({name},$gain,-1,2,$NI_BUS_OFFSET) & \" dB\")\nend on"
        );
        let mut host = LogEngine::new(Vec::new(), 48000.);
        let (mut rt, errors) = Runtime::with_scripts(&[&source], &mut host, 8, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        let id = (ENGINE_PAR_BASE..ENGINE_PAR_BASE + 512)
            .find(|&id| crate::ksp::engine_par_name(id) == Some(name))
            .unwrap();
        for (value, caption) in [
            (0, "-15.0 dB"),
            (500000, "0.0 dB"),
            (750000, "7.5 dB"),
            (1000000, "15.0 dB"),
        ] {
            rt.ui_control(&mut host, 0, 0, value);
            let ui = rt.interface(0);
            assert_eq!(
                ui.controls[0].properties["$CONTROL_PAR_LABEL"],
                Value::Text(caption.into()),
                "{name}"
            );
            assert_eq!(
                ui.controls[1].properties["$CONTROL_PAR_TEXT"],
                Value::Text(caption.into()),
                "{name}"
            );
            assert_eq!(
                host.engine_par(EnginePar {
                    id,
                    group: -1,
                    slot: 2,
                    generic: 1000
                }),
                Some(value)
            );
        }
    }
}

#[test]
fn reverb_time_callback_displays_the_effective_decay_in_milliseconds() {
    let ui = crate::ksp::initialize("on init\nmake_perfview\ndeclare ui_slider $time(0,127)\ndeclare ui_label $caption(1,1)\nset_engine_par($ENGINE_PAR_RV2_TIME,370078,-1,0,0)\nset_control_par_str(get_ui_id($time),$CONTROL_PAR_LABEL,get_engine_par_disp($ENGINE_PAR_RV2_TIME,-1,0,0) & \" ms\")\nset_text($caption,get_engine_par_disp($ENGINE_PAR_RV2_TIME,-1,0,0) & \" ms\")\nend on", 0, 8).unwrap();
    assert_eq!(
        ui.controls[0].properties["$CONTROL_PAR_LABEL"],
        crate::ksp::Value::Text("1099.5 ms".into())
    );
    assert_eq!(
        ui.controls[1].properties["$CONTROL_PAR_TEXT"],
        crate::ksp::Value::Text("1099.5 ms".into())
    );
    for (value, milliseconds) in [(0, 200.), (500000, 2000.), (1000000, 20000.)] {
        let Some(Disp::Num(shown, _)) = display(id::RV2_TIME, value) else {
            panic!("RV2_TIME display absent")
        };
        assert_eq!(shown, milliseconds);
    }
}

#[test]
fn effect_time_callback_labels_use_dsp_milliseconds() {
    let ui = crate::ksp::initialize("on init\nmake_perfview\ndeclare ui_slider $release(0,1000000)\n$release := 230000\nset_control_par_str(get_ui_id($release),$CONTROL_PAR_LABEL,get_engine_par_disp_ext($ENGINE_PAR_LIM_RELEASE,$release,-1,4,0) & \" ms\")\nend on", 0, 0).unwrap();
    assert_eq!(
        ui.controls[0].properties["$CONTROL_PAR_LABEL"],
        crate::ksp::Value::Text("28.8 ms".into())
    );
    for name in [
        "$ENGINE_PAR_COMP_ATTACK",
        "$ENGINE_PAR_COMP_DECAY",
        "$ENGINE_PAR_LIM_RELEASE",
        "$ENGINE_PAR_DL_TIME",
    ] {
        let id = (ENGINE_PAR_BASE..ENGINE_PAR_BASE + 512)
            .find(|&id| crate::ksp::engine_par_name(id) == Some(name))
            .unwrap();
        let (kind, field) = crate::fx::blocks::engine_par(name).unwrap();
        for value in [0, 230000, 1000000] {
            let Some(Disp::Num(shown, _)) = display(id, value) else {
                panic!("{name}")
            };
            assert_eq!(
                shown,
                crate::fx::blocks::stored(kind, field, value as f32 / 1e6)
            );
        }
    }
}

use super::*;
use crate::import::{Modulator, ShaperCurve};

#[test]
fn equal_curves_share_one_copy() {
    let mut other = group();
    other.mods[0].shaper = Some(ShaperCurve::Table(vec![0.5; 128]));
    let mut tables = [&group(), &group(), &other].map(ModTable::from);
    share_curves(tables.iter_mut());
    let curve = |t: &ModTable| t.mods[0].curve.clone().unwrap();
    assert!(Arc::ptr_eq(&curve(&tables[0]), &curve(&tables[1])));
    assert!(!Arc::ptr_eq(&curve(&tables[0]), &curve(&tables[2])));
    assert_eq!(
        tables[0],
        ModTable::from(&group()),
        "sharing changes no value"
    );
}

#[test]
fn release_counter_moves_the_release_sample_start() {
    // Pacific's RTC_PITCH: counter -> sample start at 0.5, shaped from 1
    // (counter 0) down to 0.42 (counter 1). Dropping the route plays the
    // release's loud onset, about 4 dB over the whole audit render.
    let group = Group {
        mods: vec![ModAssignment {
            cc_default: None,
            name: "RTC_PITCH".into(),
            source: ModSource::ReleaseTriggerCounter,
            target: ModTarget::SampleStart,
            intensity: 0.5,
            invert: false,
            lag_ms: 0,
            shaper: Some(ShaperCurve::Table(
                (0..128).map(|i| 1.0 - 0.58 * i as f32 / 127.0).collect(),
            )),
        }],
        ..Group::default()
    };
    let table = ModTable::from(&group);
    let cc = [crate::midi::ControlValue::ZERO; 128];
    let at = |counter| {
        table.start_offset(&Inputs {
            cc_received: None,
            cc74_received: None,
            per_note_cc: None,
            script_mods: &[],
            cc74: None,
            cc: &cc,
            bend: 0.0,
            pressure: 0.,
            note: 60,
            velocity: crate::midi::Velocity::from_midi1(100),
            release_velocity: None,
            counter,
        })
    };
    // A 700 ms note under T = 1500 ms leaves 0.53 of the counter.
    let x = release_counter(1500, 700.0);
    assert!((x - 0.533).abs() < 1e-3);
    assert!((at(x) - 0.5 * (1.0 - 0.58 * x)).abs() < 1e-2, "{}", at(x));
    let (low, high) = table.start_offset_range(&[0; 128], 60..=60, 100..=100);
    assert!(
        (low - 0.21).abs() < 1e-2 && (high - 0.5).abs() < 1e-3,
        "{low}..{high}"
    );
}

#[test]
fn lagged_values_preserve_positive_lag_recurrence() {
    for (start, target) in [(0.2f64, 0.7f64), (5e-7, 0.)] {
        let (mut whole, mut split) = (start, start);
        approach64(&mut whole, target, 0.01, 128, 48_000.);
        approach64(&mut split, target, 0.01, 64, 48_000.);
        approach64(&mut split, target, 0.01, 64, 48_000.);
        let expected = target + (start - target) * (-128.0 / (f64::from(0.01f32) * 48_000.)).exp();
        let scale = (start - target).abs();
        assert!((whole - expected).abs() < 1e-14 * scale);
        assert!((split - expected).abs() < 1e-14 * scale);
    }
    let mut value = 5e-7;
    approach64(&mut value, 0., 0.1, 0, 48_000.);
    assert_eq!(value, 5e-7, "zero elapsed frames retain positive lag state");
    approach64(&mut value, 0.7, 0., 128, 48_000.);
    assert_eq!(value, 0.7);
    approach64(&mut value, 0.7, 0.1, 128, 48_000.);
    assert_eq!(value, 0.7, "an exactly reached target needs no work");
}

#[test]
fn ids_match_the_runtime_table() {
    use crate::ksp::engine_par_name as name;
    for (id, expected) in [
        (id::VOLUME, "VOLUME"),
        (id::PAN, "PAN"),
        (id::TUNE, "TUNE"),
        (id::OUTPUT_CHANNEL, "OUTPUT_CHANNEL"),
        (id::ATTACK, "ATTACK"),
        (id::DECAY, "DECAY"),
        (id::SUSTAIN, "SUSTAIN"),
        (id::RELEASE, "RELEASE"),
        (id::HOLD, "HOLD"),
        (id::ATK_CURVE, "ATK_CURVE"),
        (id::MOD_TARGET_INTENSITY, "MOD_TARGET_INTENSITY"),
        (id::MOD_TARGET_MP_INTENSITY, "MOD_TARGET_MP_INTENSITY"),
        (id::INTMOD_INTENSITY, "INTMOD_INTENSITY"),
        (id::INTMOD_BYPASS, "INTMOD_BYPASS"),
        (id::EFFECT_BYPASS, "EFFECT_BYPASS"),
        (id::EFFECT_TYPE, "EFFECT_TYPE"),
        (id::EFFECT_SUBTYPE, "EFFECT_SUBTYPE"),
        (id::SEND_EFFECT_TYPE, "SEND_EFFECT_TYPE"),
        (id::RV2_PREDELAY, "RV2_PREDELAY"),
        (id::RV2_TIME, "RV2_TIME"),
        (id::RV2_TYPE, "RV2_TYPE"),
        (id::RV2_SIZE, "RV2_SIZE"),
        (id::RV2_DAMPING, "RV2_DAMPING"),
        (id::RV2_DIFF, "RV2_DIFF"),
        (id::RV2_MOD, "RV2_MOD"),
        (id::RV2_STEREO, "RV2_STEREO"),
        (id::RV2_FREEZE, "RV2_FREEZE"),
        (id::RV2_EQ_LOW_FREQ, "RV2_EQ_LOW_FREQ"),
        (id::RV2_EQ_LOW_GAIN, "RV2_EQ_LOW_GAIN"),
        (id::RV2_EQ_HIGH_FREQ, "RV2_EQ_HIGH_FREQ"),
        (id::RV2_EQ_HIGH_GAIN, "RV2_EQ_HIGH_GAIN"),
        (id::SEND_EFFECT_BYPASS, "SEND_EFFECT_BYPASS"),
        (id::SEND_EFFECT_DRY_LEVEL, "SEND_EFFECT_DRY_LEVEL"),
        (id::SEND_EFFECT_OUTPUT_GAIN, "SEND_EFFECT_OUTPUT_GAIN"),
        (id::INSERT_EFFECT_OUTPUT_GAIN, "INSERT_EFFECT_OUTPUT_GAIN"),
        (id::SENDLEVEL_0, "SENDLEVEL_0"),
        (id::SENDLEVEL_7, "SENDLEVEL_7"),
        (id::STEREO, "STEREO"),
        (id::STEREO_PAN, "STEREO_PAN"),
        (id::FREQ1, "FREQ1"),
        (id::FREQ3, "FREQ3"),
        (id::BW1, "BW1"),
        (id::BW3, "BW3"),
        (id::GAIN1, "GAIN1"),
        (id::GAIN3, "GAIN3"),
    ] {
        assert_eq!(name(id), Some(format!("$ENGINE_PAR_{expected}").as_str()));
    }
}

#[test]
fn value_laws_match_reference_points() {
    let volume = Address::Instrument(GroupPar::Volume);
    assert!((volume.decode(630_859) - 1.0).abs() < 1e-3, "0 dB");
    // The cube law is Kontakt's to within 0.005 dB, not bit-exact.
    assert!((volume.encode(1.0) - 630_859).abs() < 200);
    // Areia sets 704316 where its presets store intensity 0.4961.
    let intensity = Address::Intensity {
        group: 0,
        index: 0,
        bipolar: false,
        cubic: None,
    };
    assert!((intensity.decode(704_316) - 0.4961).abs() < 1e-3);
    // Areia sets these at init; its saved envelopes hold the same times.
    let attack = Address::Envelope(0, Stage::Attack);
    let release = Address::Envelope(0, Stage::Release);
    assert!((attack.decode(465_229) - 0.125_013).abs() < 1e-4);
    assert!((release.decode(512_668) - 0.250_001).abs() < 1e-4);
    // Afflatus sets these on a group insert and its send; the presets store the gains.
    let output = Address::Filter(0, 1, Knob::Output);
    assert!((output.decode(396_851) - 1.000_005_6).abs() < 1e-4);
    assert!((Address::Fx(Rack::Send, 0, FxParam::Wet).decode(125_919) - 0.031_944).abs() < 1e-5);
    assert!((output.encode(1.0) - 396_851).abs() < 2);
    let spread = Address::Filter(0, 0, Knob::Spread);
    assert!((spread.decode(434_210) + 0.131_58).abs() < 1e-4);
    assert!((release.decode(1_000_000) - 25.000_04).abs() < 1e-3);
    // Solo sets these where its presets store attack curves 0.5 and -0.33.
    let curve = Address::Envelope(0, Stage::Curve);
    assert_eq!(curve.decode(750_000), 0.5);
    assert!((curve.decode(333_333) + 0.333_33).abs() < 1e-5);
    assert_eq!(curve.encode(1.0), 1_000_000);
    assert_eq!(release.encode(0.250_001), 512_668);
    // Areia's send level, stored as 0.25 (−12 dB).
    let send = Address::Fx(Rack::Insert, 7, FxParam::SendLevel(0));
    assert!((send.decode(396_820) - 0.25).abs() < 2e-3);
    let output = Address::Group(0, GroupPar::Output);
    assert_eq!(output.decode(1003), 3.0);
    assert_eq!(output.encode(output.decode(-1)), -1);
    // Past the instrument output: channel 1 ("Out 2"), both ways.
    assert_eq!(output.decode(1), f32::from(DIRECT) + 1.0);
    assert_eq!(output.encode(output.decode(1)), 1);
    let bus_out = Address::Fx(Rack::Bus(2), 0, FxParam::Output);
    assert_eq!(
        (bus_out.decode(3), bus_out.decode(-1), bus_out.encode(3.0)),
        (3.0, -1.0, 3)
    );
}

fn group() -> Group {
    let assignment = |source, lag_ms, shaper| ModAssignment {
        cc_default: None,
        name: String::new(),
        source,
        target: ModTarget::Volume,
        intensity: 1.0,
        invert: false,
        lag_ms,
        shaper,
    };
    Group {
        mods: vec![
            // Velocity squared by a table shaper.
            assignment(
                ModSource::Velocity,
                0,
                Some(ShaperCurve::Table(
                    (0..128).map(|i| (i as f32 / 127.0).powi(2)).collect(),
                )),
            ),
            assignment(ModSource::MidiCc(11), 100, None),
        ],
        modulators: vec![
            Modulator {
                name: "ENV_AHDSR".into(),
                targets: vec!["ENV_AHDSR_VOLUME".into()],
                assignments: None,
                volume_env: true,
                bypassed: false,
                flex: false,
                primary_flex: false,
                envelope: None,
                kind: String::new(),
            },
            Modulator {
                name: "VEL_VOLUME".into(),
                targets: vec![String::new()],
                assignments: Some(0),
                volume_env: false,
                bypassed: false,
                flex: false,
                primary_flex: false,
                envelope: None,
                kind: String::new(),
            },
            Modulator {
                name: "CC_VOLUME".into(),
                targets: vec![String::new()],
                assignments: Some(1),
                volume_env: false,
                bypassed: false,
                flex: false,
                primary_flex: false,
                envelope: None,
                kind: String::new(),
            },
        ],
        ..Group::default()
    }
}

#[test]
fn group_insert_types_and_reverb_eq_freeze_resolve() {
    use crate::fx::{Effect, Kind, Params};
    let mut g = group();
    g.fx.slots.push(Effect {
        slot: 2,
        kind: Kind::StereoModeller,
        version: 0,
        bypass: false,
        output_gain: 1.0,
        dry_level: 0.0,
        params: Params::Opaque { bytes: 0 },
    });
    let groups = [g];
    let par = |id, group, slot, generic| EnginePar {
        id,
        group,
        slot,
        generic,
    };
    let ty = |slot| Address::resolve(par(id::EFFECT_TYPE, 0, slot, -1), &groups);
    assert_eq!((ty(2), ty(8)), (Some(Address::GroupType(0, 2)), None));
    assert_eq!(
        (group_type(&groups, 0, 2), group_type(&groups, 0, 1)),
        (Some(31.0), Some(0.0))
    );
    // A filter's type is its subtype; it decodes as the raw type id.
    let sub = Address::resolve(par(id::EFFECT_SUBTYPE, 0, 1, -1), &groups);
    assert_eq!(sub, Some(Address::Filter(0, 1, Knob::Type)));
    assert_eq!(
        (
            Address::Filter(0, 1, Knob::Type).decode(106),
            Address::Filter(0, 1, Knob::Type).encode(106.0)
        ),
        (106.0, 106)
    );
    // Reverb: the stored EQ values are the cut amounts; freeze is a switch.
    let rv = |id| {
        Address::resolve(par(id, -1, 0, 0), &[]).map(|a| match a {
            Address::Fx(_, _, FxParam::Reverb(n)) => n,
            _ => u8::MAX,
        })
    };
    let unresolved = |id| Address::resolve(par(id, -1, 0, 0), &[]).is_none();
    assert_eq!(
        [id::RV2_EQ_HIGH_GAIN, id::RV2_EQ_LOW_GAIN, id::RV2_FREEZE].map(rv),
        [Some(7), Some(8), Some(10)]
    );
    assert!(unresolved(id::RV2_EQ_LOW_FREQ) && unresolved(id::RV2_EQ_HIGH_FREQ));
    let freeze = Address::Fx(Rack::Send, 0, FxParam::Reverb(10));
    assert_eq!(
        (
            freeze.decode(1_000_000),
            freeze.decode(0),
            freeze.encode(1.0)
        ),
        (1.0, 0.0, 1)
    );
}

#[test]
fn saved_mixed_ahd_preserves_early_release_and_complete_pcm_without_heap() {
    use crate::{
        audio::Sample,
        engine::{Bank, Engine, Expression, HostNote, HostPattern},
        fx::{Chain, Effect, Kind, Params, params::Filter},
        import::{Group, Zone},
        modulation::read_group,
    };
    use ni_file::kontakt::{Chunk, StructuredObject, objects::Group as RawGroup};
    fn name(out: &mut Vec<u8>, value: &str) {
        out.extend((value.len() as u32).to_le_bytes());
        out.extend(value.as_bytes());
    }
    fn object(id: u16, version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Chunk {
        let mut data = vec![1];
        data.extend(version.to_le_bytes());
        for part in [private, public, children] {
            data.extend((part.len() as u32).to_le_bytes());
            data.extend(part);
        }
        Chunk { id, data }
    }
    let env = crate::import::Ahdsr {
        attack_curve: 0.,
        attack_ms: 2.,
        hold_ms: 1.,
        decay_ms: 4.,
        sustain: 0.75,
        release_ms: 1.,
        unknown_flag: 1,
        unknown_tail: [0, 0, 128, 191, 0, 0, 0, 0, 0, 0, 128, 63, 0].repeat(4),
    };
    let raw = |mode: u8, source: [u8; 4], opaque: bool, inverted: bool, wrapper_version: u16| {
        let internal = |env: &crate::import::Ahdsr,
                        rows: &[(&str, Option<u8>, f32)],
                        source,
                        invert,
                        version| {
            let mut private = (rows.len() as u32).to_le_bytes().to_vec();
            for (param, slot, depth) in rows {
                name(&mut private, param);
                private.extend(depth.to_le_bytes());
                private.extend((-1i16).to_le_bytes());
                private.push(0x10);
                private.extend(0u16.to_le_bytes());
                name(&mut private, param);
                private.extend(*slot);
                private.push(u8::from(invert));
            }
            private.extend(std::iter::repeat_n(0, rows.len())); // disabled shapers
            private.extend(source);
            private.extend(0u32.to_le_bytes());
            name(&mut private, "Saved AHD");
            private.extend(2u32.to_le_bytes());
            let mut concrete = Vec::new();
            env.write(&mut concrete).unwrap();
            let mut wrapped = Vec::new();
            object(7, version, &[], &[0; 4], &concrete)
                .write(&mut wrapped)
                .unwrap();
            object(0x0d, 0x80, &private, &[], &wrapped)
        };
        let mut saved = env.clone();
        saved.unknown_flag = mode;
        if opaque {
            saved.unknown_tail[0] = 1;
        }
        let mixed = internal(
            &saved,
            &[("pitch", None, 0.25), ("filterCutoff", Some(0), 0.35)],
            source,
            inverted,
            wrapper_version,
        );
        // Independent amplitude lifetime keeps the sample alive while
        // physical key-up tests the nonvolume source's release policy.
        let amplitude = crate::import::Ahdsr {
            attack_ms: 0.,
            hold_ms: 0.,
            decay_ms: 16.,
            ..env.clone()
        };
        let amp = internal(
            &amplitude,
            &[("volume", None, 1.)],
            [0, 0, 1, 0],
            false,
            0x90,
        );
        let mut slots = vec![1];
        mixed.write(&mut slots).unwrap();
        slots.push(1);
        amp.write(&mut slots).unwrap();
        slots.extend([0; 14]);
        RawGroup(StructuredObject {
            version: 0x95,
            public_data: vec![],
            private_data: vec![],
            children: vec![object(0x3b, 0x10, &[], &slots, &[])],
        })
    };
    let decoded = read_group(&raw(1, [0, 0, 1, 0], false, false, 0x90)).unwrap();
    assert!(decoded.native_volume_env && decoded.envelopes[0].native_ahd_only);
    assert!(
        decoded
            .warnings
            .iter()
            .any(|w| w.contains("timing and curves remain approximate"))
    );
    let group = Group {
        volume_env: decoded.volume_env,
        native_volume_env: decoded.native_volume_env,
        envelopes: decoded.envelopes,
        modulators: decoded.modulators,
        amp_split_slot: Some(1),
        fx: Chain {
            slots: vec![Effect {
                slot: 0,
                kind: Kind::Filter,
                version: 0,
                bypass: false,
                output_gain: 1.,
                dry_level: 0.,
                params: Params::Filter(Filter {
                    filter_type: 2,
                    cutoff: 0.12,
                    resonance: 0.15,
                    extra: [0.; 3],
                    native_flag: None,
                }),
            }],
        },
        ..Default::default()
    };
    let cached = serde_json::to_value(&group).unwrap();
    let restored: Group = serde_json::from_value(cached.clone()).unwrap();
    assert_eq!(restored.envelopes, group.envelopes);
    let mut legacy = cached;
    legacy["envelopes"][0]
        .as_object_mut()
        .unwrap()
        .remove("native_ahd_only");
    let legacy: Group = serde_json::from_value(legacy).unwrap();
    let legacy_settings = GroupSettings::from(&legacy);
    assert!(!legacy_settings.pitch_envelopes[0].env.ahd_only);
    assert!(
        !legacy_settings
            .filter
            .as_ref()
            .unwrap()
            .envelope_at(0)
            .unwrap()
            .ahd_only
    );
    assert_eq!(
        legacy.envelopes[0].env.unknown_flag, 1,
        "raw mode alone cannot admit an older cache"
    );
    for (mode, source, opaque, inverted, version) in [
        (0, [0, 0, 1, 0], false, false, 0x90),
        (2, [0, 0, 1, 0], false, false, 0x90),
        (1, [0, 0, 0, 0], false, false, 0x90),
        (1, [0, 0, 1, 1], false, false, 0x90),
        (1, [2, 0, 1, 0], false, false, 0x90),
        (1, [0, 2, 1, 0], false, false, 0x90),
        (1, [0, 0, 1, 0], true, false, 0x90),
        (1, [0, 0, 1, 0], false, true, 0x90),
        (1, [0, 0, 1, 0], false, false, 0x91),
    ] {
        let rejected = read_group(&raw(mode, source, opaque, inverted, version)).unwrap();
        assert!(!rejected.envelopes[0].native_ahd_only);
        assert_eq!(rejected.envelopes[0].env.unknown_flag, mode);
        if source[2] == 0 {
            assert!(rejected.warnings.iter().any(|w| w.contains("retrigger0")));
        }
    }
    let bypassed = read_group(&raw(1, [1, 1, 1, 0], false, false, 0x90)).unwrap();
    assert!(bypassed.envelopes[0].native_ahd_only && bypassed.modulators[0].bypassed);
    let prepared = GroupSettings::from(&restored);
    let p = prepared.pitch_envelopes[0].env;
    assert!(
        p.ahd_only
            && prepared
                .filter
                .as_ref()
                .unwrap()
                .envelope_at(0)
                .unwrap()
                .ahd_only
    );
    // Independent closed-form bounds for the existing generic source,
    // not a claim about the native finite-count/curve recurrence.
    let mut clock = Envelope::new(&p, 48_000.);
    clock.use_scalar_clock_for_test();
    let mut off = clock;
    for frame in 0..512 {
        if frame == 67 {
            off.release(None);
        }
        let actual = clock.pitch_frame(48_000.);
        assert_eq!(actual, off.pitch_frame(48_000.));
        if (1..90).contains(&frame) {
            assert!((actual - (frame + 1) as f32 / 96.).abs() < 2e-5);
        }
        if (160..390).contains(&frame) {
            let reference = 0.001f64.powf((frame as f64 - 143.) / 192.);
            assert!((f64::from(actual) - reference).abs() < 0.00005);
        }
    }
    assert!(clock.done() && off.done());
    assert_eq!(clock.level(), 0.);
    let create = |explicit: bool, shared: bool, frames: usize| {
        let mut imported = restored.clone();
        if explicit {
            imported.envelopes[0].native_ahd_only = false;
        }
        let samples = (0..frames)
            .map(|i| [0.015 * (i as f32 * 0.19).sin() + 0.007 * (i as f32 * 0.071).cos(); 2])
            .collect();
        let bank = Bank::from_samples(
            vec![imported],
            vec![Zone::default()],
            vec![(
                Default::default(),
                Sample {
                    rate: 48_000,
                    frames: samples,
                },
            )],
        )
        .unwrap();
        let mut engine = Engine::default();
        engine.reset(48_000.);
        engine.player.shared = shared;
        engine.set_bank(Some(Box::new(bank)));
        if explicit {
            // Existing explicit mode control is independent of saved
            // admission, and must reach both copies of the mixed source.
            assert!(write(
                &mut engine.state.settings,
                Address::ModEnvelope(0, 0, Stage::AhdOnly),
                1.
            ));
        }
        engine
    };
    const FRAMES: usize = 2048;
    let first = HostNote {
        port: 0,
        channel: 0,
        key: 60,
        id: 61,
        clap: true,
    };
    let second = HostNote { id: 62, ..first };
    let pattern = |note: HostNote| HostPattern {
        port: 0,
        channel: 0,
        key: 60,
        id: note.id,
        clap: true,
    };
    for shared in [false, true] {
        let (mut whole, mut fragmented, mut reference, mut ordinary) = (
            create(false, shared, 4096),
            create(false, shared, 4096),
            create(true, shared, 4096),
            create(false, shared, 4096),
        );
        assert!(write(
            &mut ordinary.state.settings,
            Address::ModEnvelope(0, 0, Stage::AhdOnly),
            0.
        ));
        let (mut a, mut ar, mut b, mut br, mut oracle, mut or, mut negative, mut nr) = (
            [0.; FRAMES],
            [0.; FRAMES],
            [0.; FRAMES],
            [0.; FRAMES],
            [0.; FRAMES],
            [0.; FRAMES],
            [0.; FRAMES],
            [0.; FRAMES],
        );
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                for engine in [&mut whole, &mut fragmented, &mut reference, &mut ordinary] {
                    assert!(engine.host_note_on(first, 0, 60, 127, 0.));
                }
                let mut start = 0;
                for end in [
                    19, 67, 83, 103, 151, 211, 289, 317, 339, 359, 411, 800, 801, 1001, 1117, 1118,
                    FRAMES,
                ] {
                    whole.render(&mut a[start..end], &mut ar[start..end]);
                    reference.render(&mut oracle[start..end], &mut or[start..end]);
                    ordinary.render(&mut negative[start..end], &mut nr[start..end]);
                    let (mut at, mut part) = (start, 0);
                    while at < end {
                        fragmented.render(&mut [], &mut []);
                        let n = [1, 7, 31, 128][part % 4].min(end - at);
                        fragmented.render(&mut b[at..at + n], &mut br[at..at + n]);
                        at += n;
                        part += 1;
                    }
                    for engine in [&mut whole, &mut fragmented, &mut reference, &mut ordinary] {
                        match end {
                            19 => engine.cc(0, 64, 127),
                            83 => engine.cc(0, 64, 0),
                            103 | 151 => engine.set_expression_on(
                                0,
                                60,
                                Expression {
                                    gain: if end == 103 { 0. } else { 1. },
                                    ..Default::default()
                                },
                            ),
                            211 | 289 => {
                                assert!(write(
                                    &mut engine.state.settings,
                                    Address::InternalBypass(0, 0),
                                    if end == 211 { 1. } else { 0. }
                                ));
                            }
                            317 => {
                                assert!(engine.host_note_on(second, 0, 60, 127, 0.));
                            }
                            339 => engine.cc(0, 66, 127),
                            411 => engine.cc(0, 66, 0),
                            _ => {}
                        }
                    }
                    if matches!(end, 67 | 359) {
                        let note = pattern(if end == 67 { first } else { second });
                        whole.host_note_off(note);
                        whole.host_note_off(note);
                        fragmented.host_note_off(note);
                        ordinary.host_note_off(note);
                        assert!(!whole.key_down(0, 60) && !fragmented.key_down(0, 60));
                        assert!(reference.key_down(0, 60));
                        assert!(
                            whole.active_voices() > 0,
                            "early physical key-up must preserve natural AHD source lifetime"
                        );
                    }
                    if end == 1001 {
                        reference.host_note_off(pattern(first));
                        reference.host_note_off(pattern(second));
                    }
                    if matches!(end, 801 | 1118) {
                        for engine in [&mut whole, &mut fragmented, &mut reference, &mut ordinary] {
                            assert_eq!(engine.active_voices(), usize::from(end == 801));
                        }
                    }
                    start = end;
                }
                // Independent primary native AHD: 16 ms at 1500 points/s
                // owns24 decay points, then one32-frame interpolation tick.
                // It terminates at800; retrigger at317 terminates at1117.
                for engine in [&mut whole, &mut fragmented, &mut reference, &mut ordinary] {
                    engine.mark_host_notes();
                    assert_eq!(engine.host_note_end_at(first), Some(800));
                    assert_eq!(engine.host_note_end_at(second), Some(1117));
                }
            }),
            0
        );
        assert!(a.iter().any(|v| v.abs() > 1e-5));
        assert!(
            a.iter()
                .chain(&ar)
                .zip(negative.iter().chain(&nr))
                .any(|(saved, ordinary)| (saved - ordinary).abs() > 1e-5),
            "early-off ordinary mixed source must materially differ while independent amplitude keeps sounding"
        );
        assert!(a.iter().chain(&ar).all(|v| v.is_finite()));
        for i in 0..FRAMES {
            assert!(
                (a[i] - b[i]).abs() < 2e-6 && (ar[i] - br[i]).abs() < 2e-6,
                "fragment frame{i}"
            );
            assert!(
                (a[i] - oracle[i]).abs() < 2e-6 && (ar[i] - or[i]).abs() < 2e-6,
                "deferred-off mode oracle frame{i}"
            );
        }
        assert_eq!(whole.active_voices(), 0);
        assert_eq!(fragmented.active_voices(), 0);
        assert_eq!(reference.active_voices(), 0);
    }
    let mut eof = create(false, false, 64);
    let (mut l, mut r) = ([0.; 128], [0.; 128]);
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            eof.note_on(0, 60, 127);
            eof.render(&mut l, &mut r);
            assert_eq!(
                eof.active_voices(),
                0,
                "finite sample EOF remains independent of one-shot source mode"
            );
            assert!(eof.key_down(0, 60));
            eof.note_off(0, 60);
            assert!(!eof.key_down(0, 60));
        }),
        0
    );
}

#[test]
fn scripted_ahd_only_ignores_early_release_and_finishes_without_heap() {
    use crate::{
        engine::ScriptSetup,
        import::Instrument,
        ksp::{KspEngine, Runtime, Value},
    };
    let imported = crate::import::Ahdsr {
        attack_curve: 0.,
        attack_ms: 7.,
        hold_ms: 3.,
        decay_ms: 40.,
        sustain: 0.8,
        release_ms: 1.,
        unknown_flag: 0,
        unknown_tail: Vec::new(),
    };
    let mut volume = group();
    volume.volume_env = Some(imported.clone());
    let mut pitch = volume.clone();
    pitch.volume_env = None;
    pitch.modulators[0].volume_env = false;
    pitch.modulators[0].envelope = Some(0);
    pitch.envelopes = vec![crate::modulation::ModEnvelope {
        native_ahd_only: false,
        env: imported,
        targets: vec![ModAssignment {
            cc_default: None,
            name: "Envelope".into(),
            source: ModSource::Unassigned,
            target: ModTarget::Pitch,
            intensity: 1.,
            invert: false,
            lag_ms: 0,
            shaper: None,
        }],
    }];
    let i = Instrument {
        groups: vec![volume, pitch],
        ..Default::default()
    };
    assert_eq!(
        crate::ksp::engine_par_name(id::ENV_AHD),
        Some("$ENGINE_PAR_ENV_AHD")
    );
    let mut setup = ScriptSetup::new(&i, 1000.);
    let source = "on init\nset_engine_par($ENGINE_PAR_ENV_AHD,1,0,0,-1)\nset_engine_par($ENGINE_PAR_ENV_AHD,1,1,0,-1)\ndeclare $saved := get_engine_par($ENGINE_PAR_ENV_AHD,0,0,-1)\nmake_persistent($saved)\nend on";
    let (rt, errors) = Runtime::with_scripts(&[source], &mut setup, 0, Vec::new());
    assert!(errors.iter().all(Option::is_none));
    assert_eq!(rt.persistence()[0]["$saved"], Value::Int(1));
    let pars: [_; 2] = std::array::from_fn(|g| EnginePar {
        id: id::ENV_AHD,
        group: g as i32,
        slot: 0,
        generic: -1,
    });
    for par in pars {
        assert_eq!(setup.engine_par(par), Some(1));
    }
    let mut settings: Vec<_> = i.groups.iter().map(GroupSettings::from).collect();
    let addresses: [_; 2] = pars.map(|p| Address::resolve(p, &i.groups).unwrap());
    assert!(
        !settings[0].envelope.unwrap().ahd_only,
        "preset flags are not inferred"
    );
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for address in addresses {
                assert!(write(&mut settings, address, address.decode(1)));
                assert_eq!(address.encode(read(&settings, address).unwrap()), 1);
            }
            assert!(settings[0].envelope.unwrap().ahd_only);
            assert!(settings[1].pitch_envelopes[0].env.ahd_only);
            for curve in [-0.8, 0., 0.8] {
                for zero_times in [false, true] {
                    let mut p = settings[0].envelope.unwrap();
                    p.curve = curve;
                    if zero_times {
                        p.attack = 0.;
                        p.hold = 0.;
                        p.decay = 0.;
                    }
                    let mut held = Envelope::new(&p, 1000.);
                    let mut released = held;
                    let mut skipped = held;
                    let mut scalar = held;
                    released.release(None);
                    skipped.release(None);
                    scalar.release(None);
                    for block in 0..32 {
                        let (mut h, mut r, mut q) = ([0.; 13], [0.; 13], [0.; 13]);
                        held.render(&mut h, None, 1000.);
                        released.render(&mut r, None, 1000.);
                        for x in &mut q {
                            scalar.render(std::slice::from_mut(x), None, 1000.);
                        }
                        skipped.skip(13, None, 1000.);
                        assert_eq!(h, r, "AHD is one-shot even when released during attack");
                        assert!(h.iter().all(|v| v.is_finite()));
                        for (a, b) in h.into_iter().zip(q) {
                            assert!((a - b).abs() < 0.00002);
                        }
                        assert!((held.level() - skipped.level()).abs() < 0.00002);
                        assert_eq!(held.phase(), skipped.phase());
                        if block == 1 {
                            released.release(None);
                            skipped.release(None);
                            scalar.release(None);
                        }
                    }
                    assert!(held.done() && released.done() && skipped.done() && scalar.done());
                    assert_eq!(held.level(), 0.);
                }
            }
            // Turning it off restores the stored sustain/release behavior.
            let address = addresses[0];
            assert!(write(&mut settings, address, address.decode(0)));
            assert_eq!(address.encode(read(&settings, address).unwrap()), 0);
            let p = settings[0].envelope.unwrap();
            assert_eq!(p.sustain, 0.8);
            let mut ordinary = Envelope::new(&p, 1000.);
            ordinary.skip(300, None, 1000.);
            assert!(!ordinary.done());
            assert_eq!(ordinary.level(), 0.8);
            ordinary.release(None);
            ordinary.skip(20, None, 1000.);
            assert!(ordinary.done());
        }),
        0
    );
}

#[test]
fn prepared_voiced_routes_keep_all_gain_and_pitch_lags_in_pcm_without_heap() {
    use crate::{
        audio::Sample,
        engine::{Bank, Engine},
        import::Zone,
    };
    const ROUTES: usize = 32;
    const FRAMES: usize = 2048;
    let group = Group {
        mods: (0..ROUTES)
            .map(|i| ModAssignment {
                cc_default: None,
                name: format!("CC{}", 20 + i),
                source: ModSource::MidiCc((20 + i) as u8),
                target: if i % 2 == 0 {
                    ModTarget::Volume
                } else {
                    ModTarget::Pitch
                },
                intensity: if i % 2 == 0 { 0.03125 } else { 0.0078125 },
                invert: false,
                lag_ms: (7 + i) as u16,
                shaper: None,
            })
            .collect(),
        ..Default::default()
    };
    assert!(validate_modulation_capacity(&group).is_ok());
    let source: Vec<_> = (0..16384)
        .map(|i| [0.2 * (i as f32 * std::f32::consts::TAU * 300. / 48000.).sin(); 2])
        .collect();
    let prepare = |routed: bool| {
        let mut group = group.clone();
        if !routed {
            group.mods.clear();
        }
        Box::new(
            Bank::from_samples(
                vec![group],
                vec![Zone::default()],
                vec![(
                    Default::default(),
                    Sample {
                        rate: 48000,
                        frames: source.clone(),
                    },
                )],
            )
            .unwrap(),
        )
    };
    let (mut whole, mut split, mut oracle) =
        (Engine::default(), Engine::default(), Engine::default());
    for (engine, routed) in [(&mut whole, true), (&mut split, true), (&mut oracle, false)] {
        engine.attack = 0.;
        engine.set_bank(Some(prepare(routed)));
        engine.note_on(0, 60, 127);
    }
    assert_eq!(whole.player.voices[0].mods.len(), ROUTES);
    assert_eq!(whole.player.voices[0].mod_targets.len(), ROUTES);
    let (mut actual, mut fragmented, mut expected) =
        ([[0.; FRAMES]; 2], [[0.; FRAMES]; 2], [[0.; FRAMES]; 2]);
    let mut levels = [0_f64; ROUTES];
    let coefficients: [_; ROUTES] =
        std::array::from_fn(|i| 1. - (-1. / (f64::from((7 + i) as f32 / 1000.) * 48000.)).exp());
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for (stage, (start, end)) in [(0, 229), (229, 617), (617, FRAMES)]
                .into_iter()
                .enumerate()
            {
                let targets: [_; ROUTES] =
                    std::array::from_fn(|i| ((i * 11 + stage * 47) % 128) as u8);
                for engine in [&mut whole, &mut split] {
                    for (i, value) in targets.iter().enumerate() {
                        engine.cc(0, (20 + i) as u8, *value);
                    }
                }
                // Independent scalar recurrences and the documented product/sum
                // laws drive an unmodulated engine one actual PCM frame at a time.
                for at in start..end {
                    let (mut gain, mut pitch) = (1_f32, 0_f32);
                    for i in 0..ROUTES {
                        levels[i] +=
                            (f64::from(f32::from(targets[i]) / 127.) - levels[i]) * coefficients[i];
                        if i % 2 == 0 {
                            gain *= 1. - 0.03125 * (1. - levels[i] as f32);
                        } else {
                            pitch += 12. * 0.0078125 * levels[i] as f32;
                        }
                    }
                    oracle.state.settings[0].gain = gain;
                    oracle.state.settings[0].tune = pitch;
                    let voice = &mut oracle.player.voices[0];
                    voice.gains = [gain; 2];
                    voice.gain_start = [gain; 2];
                    voice.gain_target = [gain; 2];
                    voice.gain_remaining = 0;
                    let [l, r] = &mut expected;
                    oracle.render(&mut l[at..at + 1], &mut r[at..at + 1]);
                }
                for (engine, pcm, sizes) in [
                    (&mut whole, &mut actual, &[128][..]),
                    (&mut split, &mut fragmented, &[1, 7, 31, 128][..]),
                ] {
                    let (mut at, mut block) = (start, 0);
                    while at < end {
                        let n = sizes[block % sizes.len()].min(end - at);
                        let [l, r] = pcm;
                        engine.render(&mut l[at..at + n], &mut r[at..at + n]);
                        at += n;
                        block += 1;
                    }
                }
            }
        }),
        0
    );
    let error = |pcm: &[[f32; FRAMES]; 2]| {
        pcm.iter()
            .flatten()
            .zip(expected.iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0_f32, f32::max)
    };
    assert!(expected.iter().flatten().any(|x| x.abs() > 0.05));
    assert!(
        error(&actual) < 2e-5 && error(&fragmented) < 2e-5,
        "all {ROUTES} voiced routes must reach PCM: whole={}, fragmented={}",
        error(&actual),
        error(&fragmented)
    );
}

#[test]
fn pitch_ahdsr_frames_match_fragmented_ram_and_stream_without_heap() {
    use crate::{
        audio::Sample,
        engine::{Bank, Engine},
        import::{Instrument, Loop, Zone},
        modulation::{Ahdsr as ImportedAhdsr, ModEnvelope},
    };
    let pitch = ModAssignment {
        cc_default: None,
        name: "Pitch".into(),
        source: ModSource::Unassigned,
        target: ModTarget::Pitch,
        intensity: 0.5,
        invert: false,
        lag_ms: 0,
        shaper: None,
    };
    let group = Group {
        pitch_lfos: vec![crate::import::PitchLfo { extended_fade: false, fade_sync: None, shared: false,
            digital: None,
            start_phase: 0.23,
            slot: 7,
            count: 1.,
            note_value: 1. / 24.,
            sine: 0.5,
            fade_ms: 0.,
            depth: 0.25,
            targets: vec![],
            bypassed: false,
        }],
        envelopes: vec![ModEnvelope {
            native_ahd_only: false,
            env: ImportedAhdsr {
                attack_curve: 0.7,
                attack_ms: 7.,
                hold_ms: 3.,
                decay_ms: 9.,
                sustain: 0.25,
                release_ms: 12.,
                unknown_flag: 0,
                unknown_tail: Vec::new(),
            },
            targets: vec![pitch],
        }],
        ..Group::default()
    };
    let frames: Vec<_> = (0..4096)
        .map(|n| [(n as f32 * 0.031).sin() * 0.2; 2])
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pitch-ahdsr.wav");
    let mut wav = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for frame in &frames {
        for channel in frame {
            wav.write_sample(*channel).unwrap();
        }
    }
    wav.finalize().unwrap();
    let zone = Zone {
        sample: path.clone(),
        loop_range: Some(Loop {
            start: 0,
            end: 4096,
            crossfade: 32,
            until_release: false,
            alternating: false,
        }),
        ..Zone::default()
    };
    let create = |bank| {
        let mut e = Engine::default();
        e.attack = 0.;
        e.release = 0.02;
        e.blocking_streams = true;
        e.set_bank(Some(Box::new(bank)));
        e
    };
    let ram = || {
        Bank::from_samples(
            vec![group.clone()],
            vec![zone.clone()],
            vec![(
                path.clone(),
                Sample {
                    rate: 48_000,
                    frames: frames.clone(),
                },
            )],
        )
        .unwrap()
    };
    let bare = || {
        Bank::load_bare(&Instrument {
            groups: vec![group.clone()],
            zones: vec![zone.clone()],
            ..Default::default()
        })
        .unwrap()
    };
    let (mut full, mut split, mut stream_full, mut stream_split) =
        (create(ram()), create(ram()), create(bare()), create(bare()));
    assert_eq!(stream_full.bank().unwrap().streamed_samples(), 1);
    assert_eq!(
        stream_full.bank().unwrap().samples[0].spans[0].data.len(),
        0,
        "bare bank must exercise disk reads"
    );
    let (mut a, mut ar, mut b, mut br, mut c, mut cr, mut d, mut dr) = (
        [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128],
    );
    let equal = |a: &[f32], b: &[f32]| {
        assert!(
            a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-6),
            "pitch AHDSR partition changed PCM"
        )
    };
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for e in [&mut full, &mut split, &mut stream_full, &mut stream_split] {
                e.note_on(0, 60, 127);
            }
            for block in 0..40 {
                if block == 20 {
                    for e in [&mut full, &mut split, &mut stream_full, &mut stream_split] {
                        e.note_off(0, 60);
                    }
                }
                // Bypassing disconnects the target; it does not stop the source clock.
                if block == 2 || block == 5 {
                    for e in [&mut full, &mut split, &mut stream_full, &mut stream_split] {
                        e.state.settings[0].pitch_envelopes[0].bypass = block == 2;
                    }
                }
                full.render(&mut a, &mut ar);
                stream_full.render(&mut c, &mut cr);
                for from in 0..128 {
                    split.render(&mut b[from..from + 1], &mut br[from..from + 1]);
                    stream_split.render(&mut d[from..from + 1], &mut dr[from..from + 1]);
                }
                equal(&a, &b);
                equal(&ar, &br);
                equal(&a, &c);
                equal(&ar, &cr);
                equal(&a, &d);
                equal(&ar, &dr);
                if let Some(reference) = full.player.voices.first() {
                    for engine in [&split, &stream_full, &stream_split] {
                        let voice = engine.player.voices.first().unwrap();
                        assert!(
                            (voice.pos - reference.pos).abs() < 1e-5,
                            "block {block}: position {} vs {}, envelope {} vs {} (phase {:?}/{:?})",
                            voice.pos,
                            reference.pos,
                            voice.pitch_envs[0].level(),
                            reference.pitch_envs[0].level(),
                            voice.pitch_envs[0].phase(),
                            reference.pitch_envs[0].phase()
                        );
                        assert!(
                            (voice.pitch_envs[0].level() - reference.pitch_envs[0].level()).abs()
                                < 1e-6
                        );
                    }
                }
            }
            assert_eq!(full.active_voices(), 0);
            assert_eq!(split.active_voices(), 0);
            assert_eq!(stream_full.active_voices(), 0);
            assert_eq!(stream_split.active_voices(), 0);
        }),
        0
    );
    assert_eq!(stream_full.underruns(), 0);
    assert_eq!(stream_split.underruns(), 0);
}

#[test]
fn pitch_ahdsr_muted_and_wavetable_clocks_match_fragmentation() {
    use crate::{
        audio::Sample,
        engine::{Bank, Engine, EventChange},
        import::{Wavetable, Zone},
        modulation::{Ahdsr as ImportedAhdsr, ModEnvelope},
    };
    let group = Group {
        envelopes: vec![ModEnvelope {
            native_ahd_only: false,
            env: ImportedAhdsr {
                attack_curve: 0.,
                attack_ms: 10.,
                hold_ms: 2.,
                decay_ms: 7.,
                sustain: 0.5,
                release_ms: 10.,
                unknown_flag: 0,
                unknown_tail: Vec::new(),
            },
            targets: vec![ModAssignment {
                cc_default: None,
                name: "Pitch".into(),
                source: ModSource::Unassigned,
                target: ModTarget::Pitch,
                intensity: 0.5,
                invert: false,
                lag_ms: 0,
                shaper: None,
            }],
        }],
        ..Group::default()
    };
    for wavetable in [false, true] {
        let mut group = group.clone();
        group.wavetable = wavetable.then(Wavetable::default);
        let create = || {
            let frames: Vec<_> = (0..4096)
                .map(|n| [(n as f32 * std::f32::consts::TAU / 2048.).sin() * 0.2; 2])
                .collect();
            let bank = Bank::from_samples(
                vec![group.clone()],
                vec![Zone::default()],
                vec![(
                    Default::default(),
                    Sample {
                        rate: 48_000,
                        frames,
                    },
                )],
            )
            .unwrap();
            let mut engine = Engine::default();
            engine.attack = 0.;
            engine.set_bank(Some(Box::new(bank)));
            engine
        };
        let (mut full, mut split, mut muted) = (create(), create(), create());
        let (mut a, mut ar, mut b, mut br, mut m, mut mr) = (
            [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128],
        );
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                for engine in [&mut full, &mut split, &mut muted] {
                    engine.note_on(0, 60, 127);
                }
                let event = muted.player.voices[0].event;
                muted.change_event(event, EventChange::Volume(0.));
                for block in 0..8 {
                    full.render(&mut a, &mut ar);
                    muted.render(&mut m, &mut mr);
                    for (from, to) in [(0, 1), (1, 17), (17, 33), (33, 70), (70, 128)] {
                        split.render(&mut b[from..to], &mut br[from..to]);
                    }
                    assert!(a.iter().zip(&b).all(|(a, b)| (a - b).abs() < 1e-6));
                    assert!(m.iter().all(|x| *x == 0.) && mr.iter().all(|x| *x == 0.));
                    let reference = &full.player.voices[0];
                    for engine in [&split, &muted] {
                        let voice = &engine.player.voices[0];
                        assert!(
                            (voice.pos - reference.pos).abs() < 1e-5,
                            "wavetable {wavetable}, block {block}: position {} vs {}, envelope {} vs {} (phase {:?}/{:?})",
                            voice.pos,
                            reference.pos,
                            voice.pitch_envs[0].level(),
                            reference.pitch_envs[0].level(),
                            voice.pitch_envs[0].phase(),
                            reference.pitch_envs[0].phase()
                        );
                        assert!(
                            (voice.pitch_envs[0].level() - reference.pitch_envs[0].level()).abs()
                                < 1e-6
                        );
                        assert_eq!(voice.pitch_envs[0].phase(), reference.pitch_envs[0].phase());
                    }
                }
            }),
            0
        );
    }
}

#[test]
fn pitch_envelope_uses_ahdsr_and_script_addresses() {
    use crate::modulation::{Ahdsr as ImportedAhdsr, ModEnvelope};
    let env = ImportedAhdsr {
        attack_curve: 0.,
        attack_ms: 10.,
        hold_ms: 0.,
        decay_ms: 10.,
        sustain: 0.25,
        release_ms: 10.,
        unknown_flag: 0,
        unknown_tail: Vec::new(),
    };
    let mut pitch = group().mods.remove(0);
    pitch.source = ModSource::Unassigned;
    pitch.target = ModTarget::Pitch;
    pitch.intensity = 0.5;
    pitch.shaper = None;
    let g = Group {
        envelopes: vec![ModEnvelope {
            native_ahd_only: false,
            env,
            targets: vec![pitch],
        }],
        modulators: vec![Modulator {
            name: "Pitch".into(),
            targets: vec!["Depth".into()],
            assignments: None,
            volume_env: false,
            bypassed: false,
            flex: false,
            primary_flex: false,
            envelope: Some(0),
            kind: "ahdsr".into(),
        }],
        ..Group::default()
    };
    let mut settings = vec![GroupSettings::from(&g)];
    let p = &settings[0].pitch_envelopes[0];
    let mut state = Envelope::new(&p.env, 48_000.);
    assert!((p.pitch(&mut state, 480, 48_000.) - 6.).abs() < 1e-4);
    let peak = p.pitch(&mut state, 1, 48_000.);
    assert!((peak - 6.).abs() < 1e-4);
    let sustain = p.pitch(&mut state, 20_000, 48_000.);
    assert!((sustain - 1.5).abs() < 1e-4);
    state.release(None);
    assert!(p.pitch(&mut state, 480, 48_000.) < sustain);
    assert_eq!(p.pitch(&mut state, 20_000, 48_000.), 0.);
    let groups = [g];
    let address = Address::resolve(
        EnginePar {
            id: id::MOD_TARGET_MP_INTENSITY,
            group: 0,
            slot: 0,
            generic: 0,
        },
        &groups,
    )
    .unwrap();
    assert!(write(&mut settings, address, address.decode(250_000)));
    assert_eq!(read(&settings, address), Some(-0.125));
    assert_eq!(address.encode(read(&settings, address).unwrap()), 250_000);
    let attack = Address::resolve(
        EnginePar {
            id: id::ATTACK,
            group: 0,
            slot: 0,
            generic: -1,
        },
        &groups,
    )
    .unwrap();
    assert!(write(&mut settings, attack, 0.02));
    assert_eq!(read(&settings, attack), Some(0.02));
    let p = &settings[0].pitch_envelopes[0];
    let mut state = Envelope::new(&p.env, 48_000.);
    assert!((p.pitch(&mut state, 480, 48_000.) + 0.75).abs() < 1e-4);

    // A held +6 semitone pitch envelope must reach the actual resampler.
    let legacy = Address::resolve(
        EnginePar {
            id: id::INTMOD_INTENSITY,
            group: 0,
            slot: 0,
            generic: 0,
        },
        &groups,
    )
    .unwrap();
    assert_eq!(
        legacy, address,
        "internal pitch aliases share the same law and target"
    );
    for (value, semitones) in [
        (0, -12.),
        (250_000, -1.5),
        (500_000, 0.),
        (750_000, 1.5),
        (1_000_000, 12.),
        (1_129_961, 24.),
        (1_221_125, 36.),
        (1_293_701, 48.),
        (2_000_000, 324.),
    ] {
        let depth = legacy.decode(value);
        assert!((12. * depth - semitones).abs() < 1e-3);
        assert!((legacy.encode(depth) - value).abs() <= 1);
        assert!(write(&mut settings, legacy, depth));
        assert_eq!(read(&settings, legacy), Some(depth));
    }
    assert_eq!(address.decode(750_000), 0.125); // Both signed pitch APIs use the cubic law.
    assert!(!write(&mut settings, legacy, f32::NAN));
    assert!(write(&mut settings, legacy, -0.5));
    let bypass = Address::resolve(
        EnginePar {
            id: id::INTMOD_BYPASS,
            group: 0,
            slot: 0,
            generic: -1,
        },
        &groups,
    )
    .unwrap();
    assert!(write(&mut settings, bypass, bypass.decode(1)));
    assert_eq!(bypass.encode(read(&settings, bypass).unwrap()), 1);
    let p = &settings[0].pitch_envelopes[0];
    let mut bypassed = Envelope::new(&p.env, 48_000.);
    assert_eq!(p.pitch(&mut bypassed, 480, 48_000.), 0.);
    assert_eq!(bypassed.level(), 0., "bypass preserves the source clock");
    assert!(write(&mut settings, bypass, bypass.decode(0)));
    assert_eq!(settings[0].pitch_envelopes[0].pitch(&mut bypassed, 0, 48_000.), 0.);
    assert!((settings[0].pitch_envelopes[0].pitch(&mut bypassed, 480, 48_000.) + 3.).abs() < 1e-4);
    let mut full = groups[0].clone();
    full.envelopes = vec![full.envelopes[0].clone(); PITCH_ENVS];
    let prepared = PitchEnvelope::from_group(&full);
    assert_eq!(prepared.len(), 16);
    assert_eq!(prepared.last().unwrap().index, 15);

    let mut pitched = groups[0].clone();
    let env = &mut pitched.envelopes[0];
    (env.env.attack_ms, env.env.sustain) = (0., 1.);
    let reference = Group {
        tune: 2f64.powf(0.5),
        ..Group::default()
    };
    let render = |group| {
        let sample = crate::audio::Sample {
            rate: 48_000,
            frames: (0..4096)
                .map(|i| [(i as f32 * 0.08).sin() * 0.25; 2])
                .collect(),
        };
        let bank = super::super::Bank::from_samples(
            vec![group],
            vec![crate::import::Zone::default()],
            vec![(std::path::PathBuf::new(), sample)],
        )
        .unwrap();
        let mut engine = super::super::Engine::default();
        engine.set_bank(Some(Box::new(bank)));
        engine.note_on(0, 60, 100);
        let (mut l, mut r) = ([0.; 1024], [0.; 1024]);
        engine.render(&mut l, &mut r);
        l
    };
    for e in &mut full.envelopes {
        (e.env.attack_ms, e.env.sustain) = (0., 1.);
        e.targets[0].intensity = 0.5 / PITCH_ENVS as f32;
    }
    let all_slots = render(full.clone());
    let (pitched, reference) = (render(pitched), render(reference));
    assert!(reference.iter().any(|x| x.abs() > 0.01));
    assert!(
        pitched
            .iter()
            .zip(reference)
            .all(|(a, b)| (a - b).abs() < 1e-6)
    );
    assert!(
        all_slots
            .iter()
            .zip(reference)
            .all(|(a, b)| (a - b).abs() < 1e-6)
    );
    for (raw, st) in [(1_129_961, 24.), (1_221_125, 36.)] {
        let mut pitched = groups[0].clone();
        let env = &mut pitched.envelopes[0];
        (env.env.attack_ms, env.env.sustain) = (0., 1.);
        env.targets[0].intensity = address.decode(raw);
        let pitched = render(pitched);
        let ratio = 2f64.powf(f64::from(address.decode(raw)));
        assert!((ratio - 2f64.powf(f64::from(st) / 12.)).abs() < 1e-4);
        let reference = render(Group {
            tune: ratio,
            ..Group::default()
        });
        assert!(
            pitched
                .iter()
                .zip(reference)
                .all(|(a, b)| (a - b).abs() < 1e-4),
            "modern signed depth must reach the actual resampler at {st} semitones"
        );
    }
    full.envelopes.push(full.envelopes[0].clone());
    assert!(super::super::Bank::from_samples(vec![full], Vec::new(), Vec::new()).is_err());
}

#[test]
#[ignore = "requires the user's installed library; only aggregate modulation facts are printed"]
fn analog_strings_pitch_envelopes_are_routed() {
    let path = std::env::var_os("KONTRA_ANALOG_NKI").expect("set KONTRA_ANALOG_NKI");
    let i = crate::import::read(std::path::Path::new(&path)).unwrap();
    let mut count = 0;
    for g in &i.groups {
        let settings = GroupSettings::from(g);
        count += settings.pitch_envelopes.len();
        for m in &g.modulators {
            if m.name == "Pitch_Envelope" {
                let e = &g.envelopes[m.envelope.unwrap()];
                assert!(e.targets.iter().any(|t| t.target == ModTarget::Pitch));
                assert!(!settings.pitch_envelopes.is_empty());
            }
        }
    }
    println!("groups={} pitch_envelopes={count}", i.groups.len());
    assert_eq!(count, 483);
}

#[test]
fn velocity_shaper_and_lagged_cc_volume() {
    let table = ModTable::from(&group());
    let mut cc = [crate::midi::ControlValue::ZERO; 128];
    cc[11] = crate::midi::ControlValue::FULL;
    let mut input = Inputs {
        cc_received: None,
        cc74_received: None,
        per_note_cc: None,
        script_mods: &[],
        cc: &cc,
        cc74: None,
        bend: 0.0,
        pressure: 0.,
        note: 60,
        velocity: crate::midi::Velocity::from_midi1(64),
        release_velocity: None,
        counter: 0.0,
    };
    let mut values = vec![None; table.voiced_count()];
    table.start(&mut values, &input, None);
    let (gain, ..) = table.modulate(&mut values, &input, 128, 48000.0, None);
    let expected = (64.0f32 / 127.0).powi(2);
    assert!((gain - expected).abs() < 1e-5, "{gain} vs {expected}");

    // CC11 drops to 0: after one lag time constant 63% of the way down.
    let quiet = [crate::midi::ControlValue::ZERO; 128];
    input.cc = &quiet;
    let (gain, ..) = table.modulate(&mut values, &input, 4800, 48000.0, None);
    assert!((gain / expected - (-1.0f32).exp()).abs() < 1e-4, "{gain}");
}

#[test]
#[cfg(feature = "plugin")]
fn lagged_gain_and_pitch_pcm_follow_sample_clock_in_ram_and_stream_without_heap() {
    use crate::{
        audio::Sample,
        engine::{Bank, Engine},
        import::{Instrument, Zone},
    };
    const FRAMES: usize = 4096;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("lag-clock.wav");
    let source: Vec<_> = (0..16_384)
        .map(|n| [0.1 * (n as f32 * std::f32::consts::TAU * 300. / 48_000.).sin(); 2])
        .collect();
    let mut wav = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for frame in &source {
        for value in frame {
            wav.write_sample(*value).unwrap();
        }
    }
    wav.finalize().unwrap();
    let mut errors = [[[0_f32; 4]; 2]; 2];
    for (lag_case, lag_ms) in [13_u16, u16::MAX].into_iter().enumerate() {
        let boost = if lag_case == 0 { 1. } else { 32. };
        for (case, target) in [ModTarget::Volume, ModTarget::Pitch]
            .into_iter()
            .enumerate()
        {
            let depth = if case == 0 { 0.75 } else { 0.5 };
            let group = Group {
                mods: vec![ModAssignment {
                    cc_default: None,
                    name: "Lag".into(),
                    source: ModSource::MidiCc(1),
                    target,
                    intensity: depth,
                    invert: false,
                    lag_ms,
                    shaper: None,
                }],
                ..Group::default()
            };
            let zone = Zone {
                sample: path.clone(),
                ..Zone::default()
            };
            let make = |streamed: bool, modulated: bool| {
                let mut group = group.clone();
                if !modulated {
                    group.mods.clear();
                }
                let bank = if streamed {
                    Bank::load_bare(&Instrument {
                        groups: vec![group],
                        zones: vec![zone.clone()],
                        ..Default::default()
                    })
                    .unwrap()
                } else {
                    Bank::from_samples(
                        vec![group],
                        vec![zone.clone()],
                        vec![(
                            path.clone(),
                            Sample {
                                rate: 48_000,
                                frames: source.clone(),
                            },
                        )],
                    )
                    .unwrap()
                };
                let mut engine = Engine::default();
                engine.attack = 0.;
                engine.blocking_streams = true;
                engine.set_bank(Some(Box::new(bank)));
                engine.state.settings[0].gain = boost;
                engine
            };
            let mut engines = [
                make(false, true),
                make(false, true),
                make(true, true),
                make(true, true),
            ];
            let mut reference = make(false, false);
            assert_eq!(engines[2].bank().unwrap().streamed_samples(), 1);
            assert_eq!(
                engines[2].bank().unwrap().samples[0].spans[0].data.len(),
                0,
                "streamed case must read disk, rather than a resident head"
            );
            let mut actual = [[[0_f32; FRAMES]; 2]; 4];
            let mut expected = [[0_f32; FRAMES]; 2];
            let mut value = f64::from(96_f32 / 127.);
            let pole = (-1_f64 / (f64::from(f32::from(lag_ms) / 1000.) * 48_000.)).exp();
            let mut fragment = 0;
            assert_eq!(
                crate::plugin::tests::allocations(|| {
                    for engine in &mut engines {
                        engine.cc(0, 1, 96);
                        engine.note_on(0, 60, 127);
                    }
                    reference.note_on(0, 60, 127);
                    for (start, end, cc) in [
                        (0, 229, 96),
                        (229, 617, 19),
                        (617, 1107, 104),
                        (1107, 1753, 0),
                        (1753, FRAMES, 127),
                    ] {
                        for engine in &mut engines {
                            engine.cc(0, 1, cc);
                        }
                        // Independent one-pole oracle: bypass the destination's
                        // authored-control declick ramp, and supply one sample's
                        // mathematically lagged amplifier/pitch value at a time.
                        for at in start..end {
                            let target = f64::from(f32::from(cc) / 127.);
                            value = target + (value - target) * pole;
                            let gain = boost
                                * if case == 0 {
                                    1. - depth * (1. - value as f32)
                                } else {
                                    1.
                                };
                            let tune = if case == 1 {
                                12. * depth * value as f32
                            } else {
                                0.
                            };
                            reference.state.settings[0].gain = gain;
                            reference.state.settings[0].tune = tune;
                            let voice = &mut reference.player.voices[0];
                            voice.gains = [gain; 2];
                            voice.gain_start = [gain; 2];
                            voice.gain_target = [gain; 2];
                            voice.gain_remaining = 0;
                            let [l, r] = &mut expected;
                            reference.render(&mut l[at..at + 1], &mut r[at..at + 1]);
                        }
                        for index in [0, 2] {
                            let mut at = start;
                            while at < end {
                                let n = 128.min(end - at);
                                let [l, r] = &mut actual[index];
                                engines[index].render(&mut l[at..at + n], &mut r[at..at + n]);
                                at += n;
                            }
                        }
                        let mut at = start;
                        while at < end {
                            let n = [1, 7, 31, 128][fragment % 4].min(end - at);
                            fragment += 1;
                            for index in [1, 3] {
                                let [l, r] = &mut actual[index];
                                engines[index].render(&mut l[at..at + n], &mut r[at..at + n]);
                            }
                            at += n;
                        }
                    }
                }),
                0
            );
            for (index, pcm) in actual.iter().enumerate() {
                errors[lag_case][case][index] = pcm
                    .iter()
                    .flatten()
                    .zip(expected.iter().flatten())
                    .map(|(a, b)| (a - b).abs())
                    .fold(0_f32, f32::max);
                assert_eq!(engines[index].underruns(), 0);
            }
        }
    }
    eprintln!(
        "lag gain/pitch oracle PCM errors [RAM128,RAMirregular,stream128,streamirregular]: {errors:?}"
    );
    assert!(
        errors.iter().flatten().flatten().all(|e| *e < 2e-5),
        "gain/pitch lag advanced to fragment end instead of following sample clock: {errors:?}"
    );
}

#[test]
fn quiet_lag_residual_keeps_amplified_engine_pcm() {
    use crate::{
        audio::Sample,
        engine::{Bank, Engine},
        import::Zone,
    };
    let residual = 5e-7_f32;
    let mut group = group();
    group.mods.remove(0);
    group.modulators.clear();
    let mut engine = Engine::default();
    engine.attack = 0.;
    engine.set_bank(Some(Box::new(
        Bank::from_samples(
            vec![group],
            vec![Zone::default()],
            vec![(
                Default::default(),
                Sample {
                    rate: 48_000,
                    frames: vec![[0.25; 2]; 4096],
                },
            )],
        )
        .unwrap(),
    )));
    engine.state.settings[0].gain = 10_000.;
    engine.cc(0, 11, 0);
    engine.note_on(0, 60, 127);
    let voice = &mut engine.player.voices[0];
    // Seed a reachable quiet residue, independently from the unlagged
    // amplifier declick state. This avoids an unbounded silent warm-up.
    voice.gains = [10_000.; 2];
    voice.gain_start = voice.gains;
    voice.gain_target = voice.gains;
    voice.gain_remaining = 0;
    voice.pitch = (0., 1.);
    voice.mods[0] = Some(f64::from(residual));
    voice.settled = None;
    let (mut left, mut right) = ([0.; 64], [0.; 64]);
    engine.render(&mut left, &mut right);
    let mut peak = 0_f32;
    let mut error = 0_f32;
    for at in 0..64 {
        let value = (f64::from(residual) * (-(at as f64 + 1.) / (0.1 * 48_000.)).exp()) as f32;
        // Native destination arithmetic remains f32, even when the source
        // reference is independent double-precision sample-clock math.
        let expected = 0.25 * 10_000. * (1. - (1. - value));
        peak = peak.max(expected);
        error = error
            .max((left[at] - expected).abs())
            .max((right[at] - expected).abs());
    }
    eprintln!("quiet lag residue: reference peak={peak}, sample-clock PCM error={error}");
    assert!(
        peak > 0.001,
        "the amplified oracle must exercise audible PCM"
    );
    assert!(
        error < 1e-7,
        "lag residue was snapped away: error={error}, expected peak={peak}"
    );
}

#[test]
fn velocity_scales_envelope_attack_at_note_start() {
    let group = Group {
        mods: vec![ModAssignment {
            cc_default: None,
            name: "VEL_ATTACK".into(),
            source: ModSource::Velocity,
            target: ModTarget::Attack,
            intensity: 1.0,
            invert: false,
            lag_ms: 0,
            // Pacific's stored shaper: soft notes keep the attack, hard ones shorten it.
            shaper: Some(ShaperCurve::Table(vec![1.0, 0.59])),
        }],
        ..Group::default()
    };
    let table = ModTable::from(&group);
    let cc = [crate::midi::ControlValue::ZERO; 128];
    let attack = |velocity| {
        let mut env = Ahdsr {
            attack: 0.9,
            ..Ahdsr::UNITY
        };
        let input = Inputs {
            cc_received: None,
            cc74_received: None,
            per_note_cc: None,
            script_mods: &[],
            cc: &cc,
            cc74: None,
            bend: 0.0,
            pressure: 0.,
            note: 60,
            velocity: crate::midi::Velocity::from_midi1(velocity),
            release_velocity: None,
            counter: 0.0,
        };
        table.scale_envelope(&mut env, &input);
        (env.attack, env.release)
    };
    assert_eq!(attack(0), (0.9, f32::INFINITY));
    assert!((attack(127).0 - 0.9 * 0.59).abs() < 1e-5);
    // Envelope times are not per-block voice modulation.
    assert!(table.voiced.is_empty());
}

#[test]
#[cfg(feature = "plugin")]
fn modern_signed_pitch_depth_matches_native_writer_without_heap() {
    let mut g = group();
    g.mods[1].target = ModTarget::Pitch;
    g.modulators[1].targets = vec!["Volume".into(), "Pitch".into()];
    g.modulators.truncate(2);
    let groups = [g];
    let mut settings = [GroupSettings::from(&groups[0])];
    let par = |id, generic| EnginePar {
        id,
        group: 0,
        slot: 1,
        generic,
    };
    let pitch = Address::resolve(par(id::MOD_TARGET_MP_INTENSITY, 1), &groups).unwrap();
    let legacy = Address::resolve(par(id::INTMOD_INTENSITY, 1), &groups).unwrap();
    assert_eq!(
        legacy, pitch,
        "native parameter449 aliases resolve to the same exact target"
    );
    assert!(
        Address::resolve(par(id::INTMOD_INTENSITY, 0), &groups).is_none(),
        "unverified external volume law remains unsupported for the legacy alias"
    );
    let volume = Address::resolve(par(id::MOD_TARGET_MP_INTENSITY, 0), &groups).unwrap();
    let unipolar = Address::resolve(par(id::MOD_TARGET_INTENSITY, 1), &groups).unwrap();
    assert_eq!(
        volume.decode(750_000),
        0.5,
        "unverified non-pitch law remains unchanged"
    );
    assert_eq!(
        Address::resolve(par(id::MOD_TARGET_MP_INTENSITY, 2), &groups),
        None
    );
    // Actual Conflux PB->PITCH UP target: raw775160 and saved depth .16666558385.
    assert!((pitch.decode(775_160) - 0.166_665_58).abs() < 1e-7);
    let mut native = super::super::native_state::NativeState::default();
    native.prepare(pitch, par(id::MOD_TARGET_MP_INTENSITY, 1), pitch.encode(1.));
    native.prepare(
        unipolar,
        par(id::MOD_TARGET_INTENSITY, 1),
        unipolar.encode(1.),
    );
    native.prepare(legacy, par(id::INTMOD_INTENSITY, 1), legacy.encode(1.));
    assert_eq!(native.capacity().0, 1, "aliases share the physical target");
    let mut saved = native.snapshot();
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for (raw, st) in [
                (0, -12.),
                (250_000, -1.5),
                (500_000, 0.),
                (750_000, 1.5),
                (775_160, 2.),
                (1_000_000, 12.),
                (1_129_961, 24.),
                (1_221_125, 36.),
            ] {
                let physical = pitch.decode(raw);
                assert_eq!(legacy.decode(raw), physical);
                assert_eq!(legacy.encode(physical), pitch.encode(physical));
                assert!((physical * 12. - st).abs() < 1e-3);
                assert!((pitch.encode(physical) - raw).abs() <= 1);
                assert!(write(&mut settings, pitch, physical));
                assert_eq!(read(&settings, pitch), Some(physical));
                assert_eq!(read(&settings, unipolar), Some(physical));
                assert_eq!(
                    settings[0].mods.mods[0].intensity, 1.,
                    "adjacent volume target is unchanged"
                );
                native.capture(pitch, par(id::MOD_TARGET_MP_INTENSITY, 1), raw, physical);
            }
            assert!(!write(&mut settings, pitch, f32::NAN));
            assert!(!write(&mut settings, pitch, f32::INFINITY));
            while !native.refresh(&mut saved, 1) {}
        }),
        0
    );
    let edits = saved.saved();
    assert_eq!(edits.len(), 1);
    assert_eq!(
        (edits[0].par, edits[0].value),
        (par(id::MOD_TARGET_MP_INTENSITY, 1), 1_221_125)
    );
}

#[test]
fn constant_loop_controls_preserve_physical_cursor_stream_seams_and_clock_without_heap() {
    use super::super::map::{FOREVER, LoopMap, PlayMap};
    use crate::{
        audio::Sample,
        engine::{Bank, Engine},
        import::{Instrument, Loop, Zone},
    };
    let assignment = |param: &str, intensity| ModAssignment {
        cc_default: None,
        name: "Constant".into(),
        source: ModSource::Constant,
        target: ModTarget::Group(param.into()),
        intensity,
        invert: false,
        lag_ms: 0,
        shaper: None,
    };
    let group = Group {
        mods: vec![
            assignment("pan", 0.25),
            assignment("loopStart", 0.126_636_07),
            assignment("loopLength", -0.917_009_95),
        ],
        modulators: vec![Modulator {
            name: "Constant".into(),
            targets: vec!["Pan".into(), "Loop_Start".into(), "Loop_Length".into()],
            assignments: Some(0),
            volume_env: false,
            bypassed: false,
            flex: false,
            primary_flex: false,
            envelope: None,
            kind: "external".into(),
        }],
        ..Group::default()
    };
    let par = |id, index| EnginePar {
        id,
        group: 0,
        slot: 0,
        generic: index,
    };
    let start =
        Address::resolve(par(id::INTMOD_INTENSITY, 1), std::slice::from_ref(&group)).unwrap();
    let length = Address::resolve(
        par(id::MOD_TARGET_MP_INTENSITY, 2),
        std::slice::from_ref(&group),
    )
    .unwrap();
    assert_eq!(
        start,
        Address::resolve(
            par(id::MOD_TARGET_MP_INTENSITY, 1),
            std::slice::from_ref(&group)
        )
        .unwrap()
    );
    assert_eq!(
        length,
        Address::resolve(par(id::INTMOD_INTENSITY, 2), std::slice::from_ref(&group)).unwrap()
    );
    // Independent actual stored UI/timing + original inverse lookup produces
    // these normalized writes, not values fitted from the saved depths.
    assert!((start.decode(751_086) - 0.126_636_07).abs() < 1e-7);
    assert!((length.decode(14_233) + 0.917_009_95).abs() < 1e-7);
    assert!((start.encode(0.126_636_07) - 751_086).abs() <= 1);
    assert!((length.encode(-0.917_009_95) - 14_233).abs() <= 1);
    let saved = PlayMap {
        start: 0,
        end: 4096,
        reverse: false,
        looped: Some(LoopMap {
            start: 0,
            end: 4096,
            xfade: 32,
            until_release: true,
            alternating: false,
        }),
    };
    let initial = saved
        .controlled_loop([0.126_636_07, -0.917_009_95], 4096)
        .unwrap();
    assert_eq!(
        initial.looped.unwrap(),
        LoopMap {
            start: 518,
            end: 858,
            xfade: 32,
            until_release: true,
            alternating: false
        }
    );
    for (offsets, bounds) in [
        ([-0.125, -0.125], (0, 3584)),
        ([0.125, 0.125], (0, 4096)),
        ([0., -1.], (0, 4)),
        ([0.5, -1.], (2048, 2052)),
    ] {
        let l = saved
            .controlled_loop(offsets, 4096)
            .unwrap()
            .looped
            .unwrap();
        assert_eq!((l.start, l.end), bounds);
        assert!(l.xfade <= l.start && l.xfade < l.end - l.start);
    }
    assert!(
        (PlayMap {
            reverse: true,
            ..saved
        })
        .controlled_loop([0.; 2], 4096)
        .is_none()
    );
    assert!(
        (PlayMap { start: 1, ..saved })
            .controlled_loop([0.; 2], 4096)
            .is_none()
    );
    let alternate = PlayMap {
        looped: Some(LoopMap {
            alternating: true,
            ..saved.looped.unwrap()
        }),
        ..saved
    };
    assert!(alternate.controlled_loop([0.; 2], 4096).is_none());
    assert!(saved.controlled_loop([f32::MAX, 0.], 4096).is_none());
    assert!(saved.controlled_loop([f32::NAN, 0.], 4096).is_none());
    let mut unsupported = group.clone();
    unsupported.source_mode = Some(3);
    assert!(!ModTable::from(&unsupported).has_loop_controls());
    unsupported.source_mode = None;
    assert!(
        !ModTable::from(&unsupported).has_loop_controls(),
        "old cache must not invent a source mode"
    );
    let frames: Vec<_> = (0..4096)
        .map(|n| [(n as f32 * 0.031).sin() * 0.2; 2])
        .collect();
    let path =
        std::env::temp_dir().join(format!("kontakto-loop-controls-{}.wav", std::process::id()));
    let mut wav = b"RIFF".to_vec();
    wav.extend((36u32 + 4096 * 8).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(3u16.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(48_000u32.to_le_bytes());
    wav.extend(384_000u32.to_le_bytes());
    wav.extend(8u16.to_le_bytes());
    wav.extend(32u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((4096u32 * 8).to_le_bytes());
    for frame in &frames {
        for channel in frame {
            wav.extend(channel.to_le_bytes());
        }
    }
    std::fs::write(&path, wav).unwrap();
    let zone = Zone {
        sample: path.clone(),
        tune: 1.37,
        loop_range: Some(Loop {
            start: 0,
            end: 4096,
            crossfade: 32,
            until_release: true,
            alternating: false,
        }),
        ..Zone::default()
    };
    let instrument = Instrument {
        groups: vec![group.clone()],
        zones: vec![zone.clone()],
        ..Default::default()
    };
    let bare = Bank::load_bare(&instrument).unwrap();
    assert_eq!(bare.samples[0].frames, 4096);
    assert_eq!(bare.samples[0].spans[0].data.len(), 0);
    let ram = Bank::from_samples(
        vec![group],
        vec![zone],
        vec![(
            path.clone(),
            Sample {
                rate: 48_000,
                frames,
            },
        )],
    )
    .unwrap();
    let create = |bank| {
        let mut e = Engine::default();
        e.attack = 0.;
        e.release = 1.;
        e.blocking_streams = true;
        e.set_bank(Some(Box::new(bank)));
        e
    };
    let (mut a, mut b) = (create(ram), create(bare));
    let (mut al, mut ar, mut bl, mut br) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
    let render = |a: &mut Engine,
                  b: &mut Engine,
                  al: &mut [f32],
                  ar: &mut [f32],
                  bl: &mut [f32],
                  br: &mut [f32]| {
        a.render(al, ar);
        b.render(bl, br);
        assert_eq!(
            al, bl,
            "stream configuration shares the resident physical path"
        );
        assert_eq!(ar, br);
        assert!(al.iter().chain(ar.iter()).all(|x| x.is_finite()));
    };
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            a.note_on(0, 60, 100);
            b.note_on(0, 60, 100);
            assert_eq!(
                a.player.voices[0].map, initial,
                "saved crossfade survives start modulation"
            );
            assert_eq!(b.player.voices[0].map, initial);
            for _ in 0..10 {
                render(&mut a, &mut b, &mut al, &mut ar, &mut bl, &mut br);
            }
            render(
                &mut a,
                &mut b,
                &mut al[..13],
                &mut ar[..13],
                &mut bl[..13],
                &mut br[..13],
            );
            for e in [&mut a, &mut b] {
                let settings = &mut e.state.settings;
                assert!(write(settings, start, 0.125));
                assert!(write(settings, length, -0.875));
                assert_eq!(read(settings, length), Some(-0.875));
                assert_eq!(
                    settings[0].mods.mods[0].intensity, 0.25,
                    "adjacent target stays unchanged"
                );
                e.player.touch();
            }
            render(
                &mut a,
                &mut b,
                &mut al[..19],
                &mut ar[..19],
                &mut bl[..19],
                &mut br[..19],
            );
            assert_eq!(
                a.player.voices[0].map, initial,
                "fragment beginning at tick13 must finish the old step"
            );
            let v = &a.player.voices[0];
            let physical = v.map.run(v.pos as u64, v.wraps).unwrap().frame as f64 + v.pos.fract();
            render(&mut a, &mut b, &mut al, &mut ar, &mut bl, &mut br);
            let v = &a.player.voices[0];
            assert_eq!(v.map.looped.unwrap().start, 512);
            assert_eq!(v.map.looped.unwrap().end, 1024);
            assert!(
                (v.pos - physical - 128. * v.plan.step as f64 / 4294967296.).abs() < 1e-8,
                "a boundary edit preserves physical position and its fraction"
            );
            // The shortened-behind-cursor seam is explicitly deferred once,
            // then retried when the old loop returns to an eligible position.
            for e in [&mut a, &mut b] {
                assert!(write(&mut e.state.settings, length, -0.9));
            }
            render(&mut a, &mut b, &mut al, &mut ar, &mut bl, &mut br);
            for _ in 0..8 {
                render(&mut a, &mut b, &mut al, &mut ar, &mut bl, &mut br);
            }
            assert_eq!(a.player.voices[0].loop_deferred_edits, 1);
            assert!(
                a.player.voices[0].loop_deferred.is_none(),
                "eligible control tick retries without a new write"
            );
            let map = a.player.voices[0].map;
            a.note_off(0, 60);
            b.note_off(0, 60);
            assert_ne!(a.player.voices[0].wraps, FOREVER);
            for e in [&mut a, &mut b] {
                assert!(write(&mut e.state.settings, start, -0.5));
            }
            render(&mut a, &mut b, &mut al, &mut ar, &mut bl, &mut br);
            assert_eq!(
                a.player.voices[0].map, map,
                "release keeps the final seam and tail"
            );
            for _ in 0..30 {
                render(&mut a, &mut b, &mut al, &mut ar, &mut bl, &mut br);
            }
            assert_eq!(a.active_voices(), 0);
            assert_eq!(b.active_voices(), 0);
        }),
        0
    );
    assert_eq!(b.underruns(), 0);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn saved_pitch_lfo_and_constant_loop_are_partition_invariant_in_ram_and_stream() {
    use crate::{
        audio::Sample,
        engine::{Bank, Engine},
        import::{Instrument, Loop, PitchLfo, Zone},
    };
    let mut group = Group {
        pitch_lfos: vec![PitchLfo { extended_fade: false, fade_sync: None, shared: false,
            digital: None,
            start_phase: 0.,
            slot: 7,
            count: 1.,
            note_value: 1. / 24.,
            sine: 0.5,
            fade_ms: 0.,
            depth: 0.5,
            targets: vec![],
            bypassed: false,
        }],
        mods: ["loopStart", "loopLength"]
            .into_iter()
            .map(|param| ModAssignment {
                cc_default: None,
                name: "Constant".into(),
                source: ModSource::Constant,
                target: ModTarget::Group(param.into()),
                intensity: 0.,
                invert: false,
                lag_ms: 0,
                shaper: None,
            })
            .collect(),
        modulators: vec![Modulator {
            name: "Constant".into(),
            targets: vec!["Loop_Start".into(), "Loop_Length".into()],
            assignments: Some(0),
            volume_env: false,
            bypassed: false,
            flex: false,
            primary_flex: false,
            envelope: None,
            kind: "external".into(),
        }],
        ..Group::default()
    };
    let length = Address::resolve(
        EnginePar {
            id: id::INTMOD_INTENSITY,
            group: 0,
            slot: 0,
            generic: 1,
        },
        std::slice::from_ref(&group),
    )
    .unwrap();
    // The saved oscillator is audible, but live LFO target writes remain
    // unsupported. Do not accept a write into metadata while leaving its
    // prepared depth unchanged; both Host and ScriptSetup use this gate.
    group.modulators.resize_with(8, || Modulator {
        name: String::new(),
        targets: vec![],
        assignments: None,
        volume_env: false,
        bypassed: false,
        flex: false,
        primary_flex: false,
        envelope: None,
        kind: "unsupported".into(),
    });
    group.modulators[7] = Modulator {
        name: "LFO_Pitch_Retrigger".into(),
        targets: vec!["Pitch".into()],
        assignments: None,
        volume_env: false,
        bypassed: false,
        flex: false,
        primary_flex: false,
        envelope: None,
        kind: "lfo".into(),
    };
    for id in [
        id::INTMOD_INTENSITY,
        id::MOD_TARGET_MP_INTENSITY,
        id::MOD_TARGET_INTENSITY,
    ] {
        let par = EnginePar {
            id,
            group: 0,
            slot: 7,
            generic: 0,
        };
        assert!(Address::resolve(par, std::slice::from_ref(&group)).is_none());
        assert!(!Address::inert(par, std::slice::from_ref(&group)));
    }
    let frames: Vec<_> = (0..4096)
        .map(|n| [(n as f32 * 0.031).sin() * 0.2; 2])
        .collect();
    let path = std::env::temp_dir().join(format!("kontakto-loop-lfo-{}.wav", std::process::id()));
    let mut wav = b"RIFF".to_vec();
    wav.extend((36u32 + 4096 * 8).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(3u16.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(48_000u32.to_le_bytes());
    wav.extend(384_000u32.to_le_bytes());
    wav.extend(8u16.to_le_bytes());
    wav.extend(32u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((4096u32 * 8).to_le_bytes());
    for frame in &frames {
        for channel in frame {
            wav.extend(channel.to_le_bytes());
        }
    }
    std::fs::write(&path, wav).unwrap();
    let zone = Zone {
        sample: path.clone(),
        tune: 1.37,
        loop_range: Some(Loop {
            start: 0,
            end: 4096,
            crossfade: 32,
            until_release: true,
            alternating: false,
        }),
        ..Zone::default()
    };
    let create = |bank| {
        let mut e = Engine::default();
        e.attack = 0.;
        e.release = 0.001;
        e.blocking_streams = true;
        e.set_transport(false, 120., 0., (4, 4));
        e.set_bank(Some(Box::new(bank)));
        e
    };
    let ram = |group: Group| {
        Bank::from_samples(
            vec![group],
            vec![zone.clone()],
            vec![(
                path.clone(),
                Sample {
                    rate: 48_000,
                    frames: frames.clone(),
                },
            )],
        )
        .unwrap()
    };
    let mut full = create(ram(group.clone()));
    let mut split = create(ram(group.clone()));
    let mut stream = create(
        Bank::load_bare(&Instrument {
            groups: vec![group.clone()],
            zones: vec![zone.clone()],
            ..Default::default()
        })
        .unwrap(),
    );
    group.mods.clear();
    group.modulators.clear();
    let mut no_controls = create(ram(group));
    assert!(full.bank().unwrap().has_native_controls());
    assert!(!no_controls.bank().unwrap().has_native_controls());
    let (mut a, mut ar, mut b, mut br, mut c, mut cr, mut d, mut dr) = (
        [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128],
    );
    let equal = |a: &[f32], b: &[f32]| {
        assert!(
            a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-7),
            "planner partition changed PCM"
        )
    };
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for e in [&mut full, &mut split, &mut stream, &mut no_controls] {
                e.note_on(0, 60, 127);
            }
            full.render(&mut a, &mut ar);
            no_controls.render(&mut d, &mut dr);
            for (from, to) in [(0, 17), (17, 128)] {
                split.render(&mut b[from..to], &mut br[from..to]);
                stream.render(&mut c[from..to], &mut cr[from..to]);
            }
            equal(&a, &b);
            equal(&ar, &br);
            equal(&a, &c);
            equal(&ar, &cr);
            equal(&a, &d);
            equal(&ar, &dr);
            assert!(
                full.player.voices[0].pos < 4096.,
                "loop has not been reached"
            );
            // Same command at note frame141; only fragment boundaries differ.
            for (e, l, r) in [
                (&mut full, &mut a, &mut ar),
                (&mut split, &mut b, &mut br),
                (&mut stream, &mut c, &mut cr),
            ] {
                e.render(&mut l[..13], &mut r[..13]);
            }
            let old = full.player.voices[0].map;
            for e in [&mut full, &mut split, &mut stream] {
                assert!(write(&mut e.state.settings, length, -0.5));
                assert_eq!(read(e.settings().unwrap(), length), Some(-0.5));
                e.player.touch();
            }
            full.render(&mut a[13..], &mut ar[13..]);
            for (from, to) in [(13, 32), (32, 128)] {
                split.render(&mut b[from..to], &mut br[from..to]);
                stream.render(&mut c[from..to], &mut cr[from..to]);
                if to == 32 {
                    assert_eq!(
                        split.player.voices[0].map, old,
                        "edit waits for native tick160"
                    );
                }
            }
            equal(&a, &b);
            equal(&ar, &br);
            equal(&a, &c);
            equal(&ar, &cr);
            for e in [&mut full, &mut split, &mut stream] {
                assert_eq!(e.player.voices[0].map.looped.unwrap().end, 2048);
                e.note_off(0, 60);
            }
            for _ in 0..16 {
                full.render(&mut a, &mut ar);
                split.render(&mut b, &mut br);
                stream.render(&mut c, &mut cr);
                equal(&a, &b);
                equal(&ar, &br);
                equal(&a, &c);
                equal(&ar, &cr);
            }
            assert_eq!(full.active_voices(), 0);
            assert_eq!(split.active_voices(), 0);
            assert_eq!(
                stream.active_voices(),
                0,
                "source clock cannot extend release lifetime"
            );
        }),
        0
    );
    assert_eq!(stream.underruns(), 0);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn live_saved_pitch_lfo_targets_preserve_native_identity_phase_and_persistence_without_heap() {
    use crate::{
        audio::Sample,
        engine::{Bank, Engine, load_scripts, load_scripts_with_state},
        import::{Instrument, PitchLfo, Zone},
        ksp::Value,
    };
    let mut group = Group {
        pitch_lfos: vec![
            PitchLfo { extended_fade: false, fade_sync: None, shared: false,
                digital: None,
                start_phase: 0.,
                slot: 7,
                count: 1.,
                note_value: 1. / 24.,
                sine: 0.5,
                fade_ms: 0.,
                depth: 0.2,
                targets: vec![(1, 0.25), (3, -0.05)],
                bypassed: false,
            },
            PitchLfo { extended_fade: false, fade_sync: None, shared: false,
                digital: None,
                start_phase: 0.,
                slot: 3,
                count: 2.,
                note_value: 1. / 24.,
                sine: 0.1,
                fade_ms: 0.,
                depth: 0.1,
                targets: vec![(1, 0.1)],
                bypassed: false,
            },
        ],
        ..Group::default()
    };
    group.modulators.resize_with(8, || Modulator {
        name: String::new(),
        targets: vec![],
        assignments: None,
        volume_env: false,
        bypassed: false,
        flex: false,
        primary_flex: false,
        envelope: None,
        kind: "unsupported".into(),
    });
    for slot in [3, 7] {
        group.modulators[slot] = Modulator {
            name: format!("LFO{slot}"),
            targets: vec![
                "Cutoff".into(),
                "PitchA".into(),
                "Volume".into(),
                "PitchB".into(),
            ],
            assignments: None,
            volume_env: false,
            bypassed: false,
            flex: false,
            primary_flex: false,
            envelope: None,
            kind: "lfo".into(),
        };
    }
    let par = |id, slot, target| EnginePar {
        id,
        group: 0,
        slot,
        generic: target,
    };
    let address = |id, slot, target| {
        Address::resolve(par(id, slot, target), std::slice::from_ref(&group)).unwrap()
    };
    for id in [
        id::INTMOD_INTENSITY,
        id::MOD_TARGET_MP_INTENSITY,
        id::MOD_TARGET_INTENSITY,
    ] {
        assert!(
            Address::resolve(par(id, 7, 0), std::slice::from_ref(&group)).is_none(),
            "adjacent cutoff remains unsupported"
        );
        assert!(
            Address::resolve(par(id, 3, 3), std::slice::from_ref(&group)).is_none(),
            "target indices belong to their own source"
        );
    }
    let source = "on init
            declare $initial := get_engine_par($ENGINE_PAR_INTMOD_INTENSITY,0,7,1)
            make_persistent($initial)
            declare $after := 0
            make_persistent($after)
            end on
            on controller
            if (%CC[1] > 0)
                set_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY,250000,0,7,1)
                set_engine_par($ENGINE_PAR_INTMOD_INTENSITY,750000,0,7,1)
                set_engine_par($ENGINE_PAR_MOD_TARGET_MP_INTENSITY,775160,0,7,1)
                set_engine_par($ENGINE_PAR_INTMOD_INTENSITY,250000,0,7,3)
                set_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY,250000,0,3,1)
                $after := get_engine_par($ENGINE_PAR_INTMOD_INTENSITY,0,7,1)
            end if
            end on";
    let instrument = Instrument {
        groups: vec![group.clone()],
        scripts: vec![source.into()],
        ..Default::default()
    };
    let create = || {
        let sample = Sample {
            rate: 48_000,
            frames: (0..4096)
                .map(|n| [(n as f32 * 0.031).sin() * 0.2; 2])
                .collect(),
        };
        let mut e = Engine::default();
        e.attack = 0.;
        e.release = 0.001;
        e.set_transport(false, 120., 0., (4, 4));
        e.set_bank(Some(Box::new(
            Bank::from_samples(
                vec![group.clone()],
                vec![Zone::default()],
                vec![(std::path::PathBuf::new(), sample)],
            )
            .unwrap(),
        )));
        e
    };
    let (runtime, errors) = load_scripts(&instrument, vec![], 48_000.);
    assert!(errors.is_empty(), "{errors:?}");
    let (mut live, mut reference) = (create(), create());
    live.set_script(runtime);
    let mut snapshot = live.script().unwrap().native_state.snapshot();
    let (mut a, mut ar, mut b, mut br) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
    let equal = |a: &[f32], b: &[f32]| {
        assert!(
            a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-7),
            "live depth write must preserve elapsed source phase"
        )
    };
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            live.note_on(0, 60, 127);
            reference.note_on(0, 60, 127);
            live.render(&mut a[..13], &mut ar[..13]);
            reference.render(&mut b[..13], &mut br[..13]);
            equal(&a[..13], &b[..13]);
            live.cc(0, 1, 127);
            // Independent physical-depth reference, without setters or source reset.
            let lfos = &mut reference.state.settings[0].pitch_lfos;
            lfos[0].targets[0].1 = 0.166_665_58;
            lfos[0].targets[1].1 = -0.125;
            lfos[0].depth = 0.166_665_58 - 0.125;
            lfos[1].targets[0].1 = 0.0625;
            lfos[1].depth = 0.0625;
            for _ in 0..8 {
                live.render(&mut a, &mut ar);
                reference.render(&mut b, &mut br);
                equal(&a, &b);
                equal(&ar, &br);
            }
            let settings = live.settings().unwrap();
            assert_eq!(
                read(settings, address(id::INTMOD_INTENSITY, 7, 3)),
                Some(-0.125)
            );
            assert_eq!(
                read(settings, address(id::MOD_TARGET_INTENSITY, 3, 1)),
                Some(0.0625)
            );
            assert!((settings[0].pitch_lfos[0].depth - (0.166_665_58 - 0.125)).abs() < 1e-7);
            assert!(!write(
                &mut live.state.settings,
                address(id::INTMOD_INTENSITY, 7, 1),
                f32::NAN
            ));
            while !live
                .script()
                .unwrap()
                .native_state
                .refresh(&mut snapshot, 1)
            {}
            live.note_off(0, 60);
            reference.note_off(0, 60);
            for _ in 0..16 {
                live.render(&mut a, &mut ar);
                reference.render(&mut b, &mut br);
            }
            assert_eq!(live.active_voices(), 0);
            assert_eq!(reference.active_voices(), 0);
        }),
        0
    );
    let rt = live.script().unwrap();
    for (id, slot, target) in [
        (id::MOD_TARGET_INTENSITY, 7, 1),
        (id::INTMOD_INTENSITY, 7, 1),
        (id::MOD_TARGET_MP_INTENSITY, 7, 1),
        (id::INTMOD_INTENSITY, 7, 3),
        (id::MOD_TARGET_INTENSITY, 3, 1),
    ] {
        assert!(
            rt.env.engine_par(par(id, slot, target)).is_none(),
            "accepted aliases must not use the unsupported stored-value fallback"
        );
    }
    let Value::Int(after) = rt.persistence()[0]["$after"] else {
        panic!("missing readback");
    };
    assert!((after - 775160).abs() <= 1);
    let edits = snapshot.saved();
    assert_eq!(
        edits.len(),
        3,
        "aliases share a physical source/target record"
    );
    assert!(
        edits
            .iter()
            .any(|e| e.par == par(id::MOD_TARGET_MP_INTENSITY, 7, 1) && e.value == 775160)
    );
    assert!(
        edits
            .iter()
            .any(|e| e.par == par(id::INTMOD_INTENSITY, 7, 3) && e.value == 250000)
    );
    assert!(
        edits
            .iter()
            .any(|e| e.par == par(id::MOD_TARGET_INTENSITY, 3, 1) && e.value == 250000)
    );
    let (restored, errors) = load_scripts_with_state(&instrument, vec![], 48_000., &[], &edits);
    assert!(errors.is_empty(), "{errors:?}");
    let mut replay = create();
    replay.set_script(restored);
    assert_eq!(
        replay.settings().unwrap()[0].pitch_lfos[0].targets,
        live.settings().unwrap()[0].pitch_lfos[0].targets
    );
    assert_eq!(
        replay.settings().unwrap()[0].pitch_lfos[1].targets,
        live.settings().unwrap()[0].pitch_lfos[1].targets
    );
    let Value::Int(initial) = replay.script().unwrap().persistence()[0]["$initial"] else {
        panic!("missing seeded readback");
    };
    assert!(
        (initial - 775160).abs() <= 1,
        "restoration seeds init getters before callbacks"
    );
}

#[test]
fn saved_sine_volume_reaches_amplifier_and_retains_shared_bypass_without_heap() {
    for start_phase in [0., 0.25, 0.499_023_44, 0.5, 0.75, 1.] {
        saved_sine_volume_reaches_amplifier_and_retains_shared_bypass_without_heap_at_phase(
            start_phase,
        );
    }
}

fn saved_sine_volume_reaches_amplifier_and_retains_shared_bypass_without_heap_at_phase(
    start_phase: f32,
) {
    use crate::{
        audio::Sample,
        engine::{Bank, Engine, load_scripts, load_scripts_with_state},
        import::{Instrument, PitchLfo, VolumeLfo, Zone},
    };
    for mixed in [false, true] {
        let source = PitchLfo { extended_fade: false, fade_sync: None, shared: false,
            digital: None,
            start_phase,
            slot: 7,
            count: 16.,
            note_value: 1. / 24.,
            sine: 1.,
            fade_ms: 0.,
            depth: 0.25,
            targets: vec![(2, 0.25)],
            bypassed: false,
        };
        let mut group = Group {
            volume_lfos: vec![VolumeLfo {
                source: source.clone(),
                target: 4,
                intensity: 1.,
                negative: false,
                lag_ms: 15,
            }],
            ..Group::default()
        };
        if mixed {
            group.pitch_lfos.push(source);
        }
        group.modulators.resize_with(8, || Modulator {
            name: String::new(),
            targets: vec![],
            assignments: None,
            volume_env: false,
            bypassed: false,
            flex: false,
            primary_flex: false,
            envelope: None,
            kind: "lfo".into(),
        });
        group.modulators[7].targets = vec![
            "Other0".into(),
            "Other1".into(),
            "Pitch".into(),
            "Other3".into(),
            "Volume".into(),
        ];
        let par = EnginePar {
            id: id::INTMOD_BYPASS,
            group: 0,
            slot: 7,
            generic: -1,
        };
        let bypass = Address::resolve(par, std::slice::from_ref(&group)).unwrap();
        for id in [
            id::INTMOD_INTENSITY,
            id::MOD_TARGET_MP_INTENSITY,
            id::MOD_TARGET_INTENSITY,
        ] {
            assert!(
                Address::resolve(
                    EnginePar {
                        id,
                        generic: 4,
                        ..par
                    },
                    std::slice::from_ref(&group)
                )
                .is_some(),
                "native parameter rows admit the retained original volume target"
            );
        }
        if mixed {
            assert!(
                Address::resolve(
                    EnginePar {
                        id: id::MOD_TARGET_MP_INTENSITY,
                        generic: 2,
                        ..par
                    },
                    std::slice::from_ref(&group)
                )
                .is_some(),
                "the existing pitch sibling remains writable"
            );
        }
        let script = "on init\ndeclare ui_button $pause\ndeclare $seed := get_engine_par($ENGINE_PAR_INTMOD_BYPASS,0,7,-1)\nmake_persistent($seed)\nend on\non ui_control($pause)\nset_engine_par($ENGINE_PAR_INTMOD_BYPASS,$pause,0,7,-1)\nend on";
        let instrument = Instrument {
            groups: vec![group.clone()],
            scripts: vec![script.into()],
            ..Default::default()
        };
        let create = |group: Group| {
            let mut e = Engine::default();
            e.attack = 0.0001;
            e.release = 0.001;
            e.set_transport(false, 120., 0., (4, 4));
            e.set_bank(Some(Box::new(
                Bank::from_samples(
                    vec![group],
                    vec![Zone::default()],
                    vec![(
                        std::path::PathBuf::new(),
                        Sample {
                            rate: 48_000,
                            frames: vec![[0.25; 2]; 4096],
                        },
                    )],
                )
                .unwrap(),
            )));
            e
        };
        let mut full = create(group.clone());
        let mut split = create(group.clone());
        for engine in [&mut full, &mut split] {
            let (rt, errors) = load_scripts(&instrument, vec![], 48_000.);
            assert!(errors.is_empty(), "{errors:?}");
            engine.set_script(rt);
        }
        assert_eq!(
            full.settings().unwrap()[0].volume_lfos[0]
                .source
                .start_phase
                .to_bits(),
            start_phase.to_bits(),
            "the public imported source phase survives Bank preparation exactly"
        );
        let mut plain_group = group.clone();
        plain_group.volume_lfos.clear();
        let mut plain = create(plain_group);
        let mut snapshot = full.script().unwrap().native_state.snapshot();
        let (mut a, mut ar, mut b, mut br, mut c, mut cr) = (
            [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128],
        );
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                for engine in [&mut full, &mut split, &mut plain] {
                    engine.note_on(0, 60, 127);
                }
                full.render(&mut a, &mut ar);
                plain.render(&mut c, &mut cr);
                split.render(&mut b[..17], &mut br[..17]);
                split.render(&mut b[17..], &mut br[17..]);
                assert!(a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-7));
                let initial =
                    (1. - (std::f64::consts::TAU * f64::from(start_phase)).sin() as f32) * 0.5;
                let signal = -(std::f64::consts::TAU
                    * (f64::from(start_phase) + 3. * 32. / 48_000.))
                    .sin() as f32;
                let expected = initial + ((signal + 1.) * 0.5 - initial) * 0.18508725;
                assert!(c[64] > 0.01);
                assert!(
                    (a[64] / c[64] - expected).abs() < 1e-6,
                    "native control-point gain reaches actual resident sample output"
                );
                let note_start = a;
                // Mid-interval bypass and resume, including all-source bypass
                // for the shared pitch+volume source. No clock reset on either.
                for (n, paused) in [(13, true), (39, false), (128, true), (77, false)] {
                    for engine in [&mut full, &mut split] {
                        engine.ui_control(0, 0, i32::from(paused));
                    }
                    full.render(&mut a[..n], &mut ar[..n]);
                    let first = 17.min(n);
                    split.render(&mut b[..first], &mut br[..first]);
                    if n > first {
                        split.render(&mut b[first..n], &mut br[first..n]);
                    }
                    assert!(
                        a[..n]
                            .iter()
                            .zip(&b[..n])
                            .all(|(a, b)| (a - b).abs() < 1e-7)
                    );
                    // Script writes are accepted into the fixed command queue;
                    // the audio render applies them at their exact sample.
                    for engine in [&full, &split] {
                        assert_eq!(
                            read(engine.settings().unwrap(), bypass),
                            Some(f32::from(paused))
                        );
                        if mixed {
                            assert_eq!(
                                engine.settings().unwrap()[0].pitch_lfos[0].bypassed,
                                paused
                            );
                        }
                        assert_eq!(
                            engine.settings().unwrap()[0].volume_lfos[0].source.bypassed,
                            paused
                        );
                    }
                }
                full.ui_control(0, 0, 1);
                full.render(&mut a, &mut ar);
                while !full
                    .script()
                    .unwrap()
                    .native_state
                    .refresh(&mut snapshot, 1)
                {}
                for engine in [&mut full, &mut split, &mut plain] {
                    engine.note_off(0, 60);
                }
                for _ in 0..40 {
                    full.render(&mut a, &mut ar);
                    split.render(&mut b, &mut br);
                    plain.render(&mut c, &mut cr);
                }
                assert_eq!(
                    (
                        full.active_voices(),
                        split.active_voices(),
                        plain.active_voices()
                    ),
                    (0, 0, 0)
                );
                for engine in [&mut full, &mut split] {
                    engine.ui_control(0, 0, 0);
                    engine.note_on(0, 60, 127);
                }
                full.render(&mut a, &mut ar);
                split.render(&mut b[..17], &mut br[..17]);
                split.render(&mut b[17..], &mut br[17..]);
                assert!(
                    a.iter()
                        .zip(note_start)
                        .all(|(a, initial)| (a - initial).abs() < 1e-7),
                    "a new note retriggers the saved phase and target lag"
                );
                assert!(a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-7));
            }),
            0
        );
        let edits = snapshot.saved();
        assert!(edits.iter().any(|edit| edit.par == par && edit.value == 1));
        let (rt, errors) = load_scripts_with_state(&instrument, vec![], 48_000., &[], &edits);
        assert!(errors.is_empty(), "{errors:?}");
        let mut restored = create(group);
        restored.set_script(rt);
        assert_eq!(read(restored.settings().unwrap(), bypass), Some(1.));
        assert!(
            restored.settings().unwrap()[0].volume_lfos[0]
                .source
                .bypassed
        );
        assert_eq!(
            restored.script().unwrap().persistence()[0]["$seed"],
            crate::ksp::Value::Int(1),
            "restored native source bypass is visible to the authored init getter"
        );
        assert_eq!(full.dropped_commands(), 0);
        assert_eq!(split.dropped_commands(), 0);
    }
}

#[test]
fn saved_sine_volume_uses_decoded_nonlinear_amplifier_split_without_heap() {
    use crate::{
        audio::Sample,
        engine::{Bank, Engine},
        fx::{
            Chain, Effect, Kind, Params,
            params::{Field, Value},
        },
        import::{PitchLfo, VolumeLfo, Zone},
    };
    let source = PitchLfo { extended_fade: false, fade_sync: None, shared: false,
        digital: None,
        start_phase: 0.,
        slot: 7,
        count: 16.,
        note_value: 1. / 24.,
        sine: 0.,
        fade_ms: 0.,
        depth: 0.,
        targets: vec![],
        bypassed: false,
    };
    let effect = Effect {
        slot: 0,
        kind: Kind::SurroundPanner,
        version: 0,
        bypass: false,
        output_gain: 1.,
        dry_level: 0.,
        params: Params::Fields(
            crate::fx::params::layout_names(Kind::SurroundPanner)
                .unwrap()
                .iter()
                .zip([1., 0.])
                .map(|(&name, value)| Field {
                    name,
                    value: Value::Number(value),
                })
                .collect(),
        ),
    };
    let create = |group| {
        let mut engine = Engine::default();
        engine.attack = 0.0001;
        engine.set_transport(false, 120., 0., (4, 4));
        engine.set_bank(Some(Box::new(
            Bank::from_samples(
                vec![group],
                vec![Zone::default()],
                vec![(
                    std::path::PathBuf::new(),
                    Sample {
                        rate: 48_000,
                        frames: vec![[0.25; 2]; 4096],
                    },
                )],
            )
            .unwrap(),
        )));
        engine
    };
    let mut endpoints = [0.; 2];
    for split in [0, 1] {
        let group = Group {
            amp_split_slot: Some(split),
            fx: Chain {
                slots: vec![effect.clone()],
            },
            volume_lfos: vec![VolumeLfo {
                source: source.clone(),
                target: 0,
                intensity: 1.,
                negative: false,
                lag_ms: 0,
            }],
            ..Group::default()
        };
        let mut reference = group.clone();
        reference.volume_lfos.clear();
        reference.gain = 0.5;
        let mut actual = create(group);
        let mut expected = create(reference);
        let (mut a, mut ar, mut b, mut br) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                actual.note_on(0, 60, 127);
                expected.note_on(0, 60, 127);
                // Bipolar source0 at depth1 is exactly .5, so the independent
                // reference places group gain.5 at the decoded Amplifier slot.
                for _ in 0..4 {
                    actual.render(&mut a, &mut ar);
                    expected.render(&mut b, &mut br);
                    assert!(
                        a.iter()
                            .chain(&ar)
                            .zip(b.iter().chain(&br))
                            .all(|(a, b)| (a - b).abs() < 1e-7)
                    );
                    assert!(a.iter().chain(&ar).all(|x| x.is_finite()));
                }
            }),
            0
        );
        endpoints[usize::from(split)] = a[127];
    }
    assert!(
        (endpoints[0] - endpoints[1]).abs() > 0.02,
        "nonlinear pre/post-Amp paths must not accidentally collapse: {endpoints:?}"
    );
}

#[test]
fn live_saved_pitch_lfo_bypass_preserves_clock_fade_pcm_and_restore_without_heap() {
    use crate::{
        audio::Sample,
        engine::{Bank, Engine, load_scripts, load_scripts_with_state},
        import::{Instrument, PitchLfo, Zone},
        ksp::Value,
    };
    for mixed in [false, true] {
        let mut group = Group {
            pitch_lfos: vec![PitchLfo { extended_fade: false, fade_sync: None, shared: false,
                digital: None,
                start_phase: 0.,
                slot: 7,
                count: 1.,
                note_value: 1. / 24.,
                sine: 0.5,
                fade_ms: 6.,
                depth: 0.4,
                targets: vec![(0, 0.4)],
                bypassed: false,
            }],
            ..Group::default()
        };
        if mixed {
            group.pitch_lfos.push(PitchLfo { extended_fade: false, fade_sync: None, shared: false,
                digital: None,
                start_phase: 0.,
                slot: 3,
                count: 2.,
                note_value: 1. / 24.,
                sine: 0.3,
                fade_ms: 0.,
                depth: 0.13,
                targets: vec![(0, 0.13)],
                bypassed: false,
            });
        }
        group.modulators.resize_with(8, || Modulator {
            name: String::new(),
            targets: vec![],
            assignments: None,
            volume_env: false,
            bypassed: false,
            flex: false,
            primary_flex: false,
            envelope: None,
            kind: "lfo".into(),
        });
        let par = |slot| EnginePar {
            id: id::INTMOD_BYPASS,
            group: 0,
            slot,
            generic: -1,
        };
        assert!(
            Address::resolve(par(6), std::slice::from_ref(&group)).is_none(),
            "a free-running or otherwise unprepared LFO remains unsupported"
        );
        let mut other_kind = group.clone();
        other_kind.modulators[7].kind = "step".into();
        assert!(
            Address::resolve(par(7), std::slice::from_ref(&other_kind)).is_none(),
            "a same-slot non-LFO source must not claim prepared pitch-LFO storage"
        );
        let source = "on init
                declare $seed := get_engine_par($ENGINE_PAR_INTMOD_BYPASS,0,7,-1)
                make_persistent($seed)
                declare $after := 0
                make_persistent($after)
                end on
                on controller
                if ($CC_NUM = 16)
                    set_engine_par($ENGINE_PAR_INTMOD_BYPASS,%CC[16],0,7,-1)
                    $after := get_engine_par($ENGINE_PAR_INTMOD_BYPASS,0,7,-1)
                end if
                if ($CC_NUM = 17)
                    set_engine_par($ENGINE_PAR_INTMOD_BYPASS,%CC[17],0,3,-1)
                end if
                end on";
        let instrument = Instrument {
            groups: vec![group.clone()],
            scripts: vec![source.into()],
            ..Default::default()
        };
        let create = || {
            let sample = Sample {
                rate: 48_000,
                frames: (0..4096).map(|n| [n as f32 / 8192.; 2]).collect(),
            };
            let mut engine = Engine::default();
            engine.attack = 0.0001;
            engine.release = 0.001;
            engine.set_transport(false, 120., 0., (4, 4));
            engine.set_bank(Some(Box::new(
                Bank::from_samples(
                    vec![group.clone()],
                    vec![Zone::default()],
                    vec![(std::path::PathBuf::new(), sample)],
                )
                .unwrap(),
            )));
            engine
        };
        let mut captures = [[0.; 689]; 2];
        let mut saved = None;
        for (partition, capture) in captures.iter_mut().enumerate() {
            let (rt, errors) = load_scripts(&instrument, vec![], 48_000.);
            assert!(errors.is_empty(), "{errors:?}");
            let mut engine = create();
            engine.set_script(rt);
            let mut snapshot = engine.script().unwrap().native_state.snapshot();
            let (mut l, mut r) = ([0.; 128], [0.; 128]);
            assert_eq!(
                crate::plugin::tests::allocations(|| {
                    engine.note_on(0, 60, 127);
                    let mut at = 0;
                    let mut fragment = 0;
                    // Mid-control-tick removal, resumption, all-source bypass,
                    // then removal of only one source from the mixed sum.
                    for (end, bypass) in [
                        (141, None),
                        (205, Some(true)),
                        (333, Some(false)),
                        (401, Some(true)),
                        (561, Some(false)),
                        (689, Some(true)),
                    ] {
                        if let Some(bypass) = bypass {
                            engine.cc(0, 16, u8::from(bypass));
                            if mixed && matches!(at, 333 | 401) {
                                engine.cc(0, 17, u8::from(bypass));
                            }
                        }
                        while at < end {
                            let size = if partition == 0 {
                                128
                            } else {
                                [13, 19][fragment & 1]
                            };
                            let n = size.min(end - at);
                            engine.render(&mut l[..n], &mut r[..n]);
                            assert!(l[..n].iter().zip(&r[..n]).all(|(l, r)| l == r));
                            capture[at..at + n].copy_from_slice(&l[..n]);
                            at += n;
                            fragment += 1;
                        }
                    }
                    let bypass = Address::resolve(par(7), std::slice::from_ref(&group)).unwrap();
                    assert_eq!(read(engine.settings().unwrap(), bypass), Some(1.));
                    while !engine
                        .script()
                        .unwrap()
                        .native_state
                        .refresh(&mut snapshot, 1)
                    {}
                    engine.note_off(0, 60);
                    for _ in 0..16 {
                        engine.render(&mut l, &mut r);
                    }
                    assert_eq!(
                        engine.active_voices(),
                        0,
                        "bypass cannot extend the note lifetime"
                    );
                }),
                0
            );
            let rt = engine.script().unwrap();
            assert!(
                rt.env.engine_par(par(7)).is_none(),
                "accepted bypass has real prepared storage"
            );
            assert_eq!(rt.persistence()[0]["$after"], Value::Int(1));
            assert_eq!(rt.persistence()[0]["$seed"], Value::Int(0));
            saved = Some(snapshot.saved());
        }
        assert!(
            captures[0]
                .iter()
                .zip(&captures[1])
                .all(|(a, b)| (a - b).abs() < 1e-7),
            "commands and native source ticks must be invariant to 13/19 versus128 planner fragments"
        );
        // Independent physical reference uses elapsed ACTIVE audio frames
        // for phase, source-point count for fade, and the native32 grid for
        // the shared interpolator. It neither calls Clock nor setters.
        let (mut cursor, mut previous, mut current) = (0f64, 0f64, 0f64);
        let mut active_frames = [0usize; 2];
        let mut fade_points = 0;
        for (frame, &actual) in captures[0].iter().enumerate() {
            let main = frame < 141 || (205..333).contains(&frame) || (401..561).contains(&frame);
            let second = mixed && !(333..401).contains(&frame);
            let pitch = if main || second {
                if frame & 31 == 0 {
                    previous = current;
                    current = 0.;
                    if main {
                        let start = f64::from(0.3f32);
                        let fade = if fade_points < 9 {
                            (start * (1. + 1. / start).powf(fade_points as f64 / 9.)).min(1.)
                                - start
                        } else {
                            1.
                        };
                        current -= 2.4
                            * fade
                            * (active_frames[0] as f64 * 48. / 48000. * std::f64::consts::TAU)
                                .sin();
                        fade_points += 1;
                    }
                    if second {
                        current -= 0.468
                            * (active_frames[1] as f64 * 24. / 48000. * std::f64::consts::TAU)
                                .sin();
                    }
                }
                previous + (current - previous) * (frame & 31) as f64 / 32.
            } else {
                0.
            };
            if frame >= 64 {
                assert!(
                    (actual - cursor as f32 / 8192.).abs() < 2e-6,
                    "mixed={mixed}, frame{frame}: PCM{actual} versus source cursor{cursor}"
                );
            }
            cursor += 2f64.powf(pitch / 12.);
            active_frames[0] += usize::from(main);
            active_frames[1] += usize::from(second);
        }
        let edits = saved.unwrap();
        assert!(edits.iter().any(|e| e.par == par(7) && e.value == 1));
        let (restored, errors) = load_scripts_with_state(&instrument, vec![], 48_000., &[], &edits);
        assert!(errors.is_empty(), "{errors:?}");
        let mut replay = create();
        replay.set_script(restored);
        assert!(replay.settings().unwrap()[0].pitch_lfos[0].bypassed);
        assert_eq!(
            replay.script().unwrap().persistence()[0]["$seed"],
            Value::Int(1),
            "native restoration precedes init getters, without reviving a previous note clock"
        );
        let mut expected = create();
        expected.state.settings[0].pitch_lfos[0].bypassed = true;
        let (mut a, mut ar, mut b, mut br) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                replay.note_on(0, 60, 127);
                expected.note_on(0, 60, 127);
                replay.render(&mut a, &mut ar);
                expected.render(&mut b, &mut br);
                assert!(
                    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-7),
                    "restoration preserves controls, not an earlier note's phase/fade"
                );
            }),
            0
        );
        let mut fresh = create();
        let (rt, _) = load_scripts(&instrument, vec![], 48_000.);
        fresh.set_script(rt);
        assert!(
            !fresh.settings().unwrap()[0].pitch_lfos[0].bypassed,
            "fresh load must not inherit prior edits"
        );
    }
}

#[test]
fn signed_intensity_aliases_reach_external_pitch_and_cutoff_pcm_without_heap() {
    use crate::{
        audio::Sample,
        engine::{Bank, Engine, ScriptSetup},
        import::{Instrument, Zone},
        ksp::{Runtime, Value},
    };
    let assignment = |target, intensity| ModAssignment {
        cc_default: None,
        name: "Constant".into(),
        source: ModSource::Constant,
        target,
        intensity,
        invert: false,
        lag_ms: 0,
        shaper: None,
    };
    let group = Group {
        mods: vec![
            assignment(ModTarget::Volume, 1.),
            assignment(ModTarget::Pitch, 0.),
            assignment(
                ModTarget::Module {
                    param: "filterCutoff".into(),
                    slot: 0,
                },
                0.,
            ),
        ],
        modulators: vec![Modulator {
            name: "Constant".into(),
            targets: vec!["Volume".into(), "Pitch".into(), "Cutoff".into()],
            assignments: Some(0),
            volume_env: false,
            bypassed: false,
            flex: false,
            primary_flex: false,
            envelope: None,
            kind: "external".into(),
        }],
        fx: crate::fx::Chain {
            slots: vec![crate::fx::Effect {
                slot: 0,
                kind: crate::fx::Kind::Filter,
                version: 0,
                bypass: false,
                output_gain: 1.,
                dry_level: 1.,
                params: crate::fx::params::Params::Filter(crate::fx::params::Filter {
                    filter_type: 2,
                    cutoff: 0.3,
                    resonance: 0.,
                    extra: [0.; 3],
                    native_flag: None,
                }),
            }],
        },
        ..Group::default()
    };
    let render = |group: Group, source: &str, pitch_raw: i32, cutoff_raw: i32| {
        let instrument = Instrument {
            groups: vec![group.clone()],
            ..Default::default()
        };
        let mut setup = ScriptSetup::new(&instrument, 48_000.);
        let (rt, errors) = Runtime::with_scripts(&[source], &mut setup, 0, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        for (name, expected) in [("$p", pitch_raw), ("$c", cutoff_raw)] {
            let Value::Int(raw) = rt.persistence()[0][name] else {
                panic!("missing readback");
            };
            assert!((raw - expected).abs() <= 1, "{name}: {raw} != {expected}");
        }
        let sample = Sample {
            rate: 48_000,
            frames: (0..8192)
                .map(|n| [(std::f32::consts::TAU * 2000. * n as f32 / 48_000.).sin() * 0.25; 2])
                .collect(),
        };
        let bank = Bank::from_samples(
            vec![group],
            vec![Zone::default()],
            vec![(std::path::PathBuf::new(), sample)],
        )
        .unwrap();
        let mut engine = Engine::default();
        engine.set_bank(Some(Box::new(bank)));
        engine.set_script(Some(Box::new(rt)));
        let (mut l, mut r) = ([0.; 2048], [0.; 2048]);
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                engine.note_on(0, 60, 100);
                engine.render(&mut l, &mut r);
            }),
            0
        );
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
        l
    };
    let mut power = [0.; 4];
    for (i, (pitch_raw, cutoff_raw)) in [
        (224_840, 250_000),
        (224_840, 750_000),
        (775_160, 250_000),
        (775_160, 750_000),
    ]
    .into_iter()
    .enumerate()
    {
        // Independently saved Conflux 2 st target; signed midpoint probes
        // also exercise an audible cutoff change rather than readback alone.
        let readback = "declare $p := get_engine_par($ENGINE_PAR_MOD_TARGET_MP_INTENSITY,0,0,1)\nmake_persistent($p)\ndeclare $c := get_engine_par($ENGINE_PAR_INTMOD_INTENSITY,0,0,2)\nmake_persistent($c)\nend on";
        let source = |name| {
            format!(
                "on init\nset_engine_par({name},{pitch_raw},0,0,1)\nset_engine_par({name},{cutoff_raw},0,0,2)\n{readback}\non note\nset_engine_par({name},{pitch_raw},0,0,1)\nset_engine_par({name},{cutoff_raw},0,0,2)\nend on"
            )
        };
        let legacy = render(
            group.clone(),
            &source("$ENGINE_PAR_INTMOD_INTENSITY"),
            pitch_raw,
            cutoff_raw,
        );
        let modern = render(
            group.clone(),
            &source("$ENGINE_PAR_MOD_TARGET_MP_INTENSITY"),
            pitch_raw,
            cutoff_raw,
        );
        let mut reference = group.clone();
        reference.mods[1].intensity = if pitch_raw < 500_000 {
            -0.166_665_58
        } else {
            0.166_665_58
        };
        reference.mods[2].intensity = if cutoff_raw < 500_000 { -0.125 } else { 0.125 };
        let reference = render(
            reference,
            &format!("on init\n{readback}"),
            pitch_raw,
            cutoff_raw,
        );
        // Authored serialized target records exercise the import path too:
        // magnitudes are positive; the negative cutoff is target flag0x02,
        // independently of the serialized invert button (left false).
        use ni_file::kontakt::{Chunk, StructuredObject};
        let name = |out: &mut Vec<u8>, text: &str| {
            out.extend((text.len() as u32).to_le_bytes());
            out.extend(text.as_bytes());
        };
        let object = |id, version: u16, private: &[u8], public: &[u8]| {
            let mut data = vec![1];
            data.extend(version.to_le_bytes());
            for part in [private, public, &[]] {
                data.extend((part.len() as u32).to_le_bytes());
                data.extend(part);
            }
            Chunk { id, data }
        };
        let mut private = 3u32.to_le_bytes().to_vec();
        for (param, label, slot, depth, flags) in [
            ("volume", "Volume", None, 1.0f32, 0x10),
            (
                "pitch",
                "Pitch",
                None,
                0.166_665_58,
                if pitch_raw < 500_000 { 0x12 } else { 0x10 },
            ),
            (
                "filterCutoff",
                "Cutoff",
                Some(0),
                0.125,
                if cutoff_raw < 500_000 { 0x12 } else { 0x10 },
            ),
        ] {
            name(&mut private, param);
            private.extend(depth.to_le_bytes());
            private.extend((-1i16).to_le_bytes());
            private.push(flags);
            private.extend(0u16.to_le_bytes());
            name(&mut private, label);
            private.extend(slot);
            private.push(0);
        }
        private.extend([0; 3]);
        name(&mut private, "Constant");
        private.extend(1u32.to_le_bytes());
        private.extend(9u32.to_le_bytes());
        private.extend([0; 4]);
        private.extend(0u32.to_le_bytes());
        let mut public = vec![1];
        object(0x0c, 0x102, &private, &[])
            .write(&mut public)
            .unwrap();
        public.extend([0; 31]);
        let raw = ni_file::kontakt::objects::Group(StructuredObject {
            version: 0x95,
            public_data: vec![],
            private_data: vec![],
            children: vec![object(0x3c, 0x10, &[], &public)],
        });
        let imported = crate::modulation::read_group(&raw).unwrap();
        let mut saved = group.clone();
        saved.mods = imported.mods;
        saved.modulators = imported.modulators;
        let saved = render(
            saved,
            &format!("on init\n{readback}"),
            pitch_raw,
            cutoff_raw,
        );
        assert_eq!(legacy, modern, "aliases must produce identical PCM");
        assert_eq!(
            saved, reference,
            "saved signed targets must match the physical-depth reference"
        );
        assert!(
            legacy
                .iter()
                .zip(reference)
                .all(|(a, b)| (a - b).abs() < 1e-6),
            "signed setters must match independently supplied physical depths"
        );
        power[i] = legacy.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>();
    }
    assert!(
        power[1] > power[0] * 4. && power[3] > power[2] * 4.,
        "cutoff depth changes the real filter response at both pitch directions: {power:?}"
    );
}

#[test]
fn addresses_resolve_by_decoded_names() {
    let groups = [group()];
    assert_eq!(find_mod(&groups, 0, &|n| n == "CC_VOLUME"), Some(2));
    assert_eq!(
        find_target(&groups, 0, 0, &|n| n == "ENV_AHDSR_VOLUME"),
        Some(0)
    );
    let par = |id, slot, generic| EnginePar {
        id,
        group: 0,
        slot,
        generic,
    };
    assert_eq!(
        Address::resolve(par(id::MOD_TARGET_INTENSITY, 2, -1), &groups),
        Some(Address::Intensity {
            group: 0,
            index: 1,
            bipolar: false,
            cubic: None,
        })
    );
    assert_eq!(
        Address::resolve(par(id::ATTACK, 0, -1), &groups),
        Some(Address::Envelope(0, Stage::Attack))
    );
    assert_eq!(Address::resolve(par(id::ATTACK, 1, -1), &groups), None);
    // A flex envelope (slot 1 here) has no AHDSR stages, nor has a
    // missing slot: Kontakt ignores those; an external modulator's are
    // left unmapped.
    let mut flex = group();
    flex.modulators.insert(
        1,
        Modulator {
            name: "ENV_FLEX".into(),
            targets: Vec::new(),
            assignments: None,
            volume_env: false,
            bypassed: false,
            flex: true,
            primary_flex: false,
            envelope: None,
            kind: "flex".into(),
        },
    );
    let flex = [flex];
    assert!(Address::inert(par(id::ATTACK, 1, -1), &flex));
    assert!(Address::inert(par(id::RELEASE, 9, -1), &flex));
    assert!(!Address::inert(par(id::ATTACK, 2, -1), &flex));
    assert!(!Address::inert(par(id::VOLUME, 1, -1), &flex));
    // A filter envelope is addressed by its `Group::envelopes` index.
    let mut filter = group();
    filter.modulators[1].envelope = Some(3);
    assert_eq!(
        Address::resolve(par(id::DECAY, 1, -1), &[filter]),
        Some(Address::ModEnvelope(0, 3, Stage::Decay))
    );
    let bus = EnginePar {
        id: id::VOLUME,
        group: -1,
        slot: -1,
        generic: 1002,
    };
    assert_eq!(
        Address::resolve(bus, &groups),
        Some(Address::Fx(Rack::Bus(2), 0, FxParam::Volume))
    );
}

#[cfg(feature = "plugin")]
#[path = "volume_lfo_depth_tests.rs"]
mod volume_lfo_depth_tests;

#[cfg(feature = "plugin")]
#[path = "lfo_frequency_tests.rs"]
mod lfo_frequency_tests;
