//! MPE timbre (CC74) has no native Kontakt meaning, so the expression layer
//! picks its target per instrument: its dynamics controller, else its filter
//! cutoff, else a tone low-pass. One test per branch.
use sampler_core::lower::{MpeDefaults, Options, TimbreTarget, lower_with};
use sampler_core::{Limits, Pcm, Runtime};
use sampler_ir as ir;
use sampler_midi::{Mpe, Packets, Zone};

fn packet(status: u8, channel: u8, a: u8, b: u8) -> u32 {
    0x2300_0000 | (u32::from(status | channel) << 16) | (u32::from(a) << 8) | u32::from(b)
}

fn instrument() -> ir::Instrument {
    ir::Instrument {
        assets: vec![ir::Asset {
            location: ir::AssetLocation::Path("a".into()),
            encoding: ir::Encoding::Wav,
            root_key: None,
            loops: Vec::new(),
        }],
        zones: vec![ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        ..Default::default()
    }
}

fn with_filter(mut ir: ir::Instrument) -> ir::Instrument {
    ir.chains.push(ir::Chain {
        scope: ir::Scope::Voice,
        pre_amplitude: vec![ir::Processor::Filter(ir::Filter {
            kind: ir::FilterKind::LowPass { poles: 2 },
            cutoff: ir::Frequency::Hertz(2000.0),
            resonance: ir::Resonance::Decibels(0.0),
        })],
        post_amplitude: vec![],
    });
    ir.zones[0].chain = Some(ir::ChainRef(0));
    ir
}

fn with_crossfade(mut ir: ir::Instrument, cc: u8) -> ir::Instrument {
    ir.modulators.push(ir::Modulator {
        scope: ir::Scope::Voice,
        source: ir::ModulationSource::Controller(cc),
    });
    ir.routes.push(ir::Route {
        source: ir::ModulatorRef(0),
        target: ir::Target::Amplitude,
        depth: ir::Depth::Normalized(1.0),
        invert: false,
        shape: None,
        smoothing: ir::Time::Milliseconds(0.0),
        scale: None,
    });
    ir.zones[0].routes.push(ir::RouteRef(0));
    ir
}

/// Peak level of key 60 on member channel 1 after it sends CC74 = `y`, with
/// the target `for_instrument` chose.
fn level(ir: &ir::Instrument, y: u8) -> f32 {
    level_of(ir, y, 1)
}

/// [`level`] with a square wave of `run` frames per half period.
fn level_of(ir: &ir::Instrument, y: u8, run: usize) -> f32 {
    let defaults = MpeDefaults::for_instrument(ir);
    let nyquist = (0..4800).map(|i| [if (i / run) % 2 == 0 { 0.25 } else { -0.25 }; 2]);
    let pcm = vec![Pcm::new(48000, nyquist.collect()).unwrap()];
    let options = Options {
        mpe: Some(defaults),
    };
    let plan = lower_with(ir, 48000, pcm, &options, |_, _| unreachable!()).unwrap();
    let limits = Limits {
        notes: 8,
        channels: 4,
        performances: 1,
        families: 8,
        expressions: 8,
        voices: 8,
        decisions: 0,
        commands: 8,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    };
    let mut rt = Runtime::new(plan, limits).unwrap();
    let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 2, 8).unwrap();
    if let TimbreTarget::Controller(cc) = defaults.timbre {
        mpe.set_timbre_controller(Some(cc));
    }
    for word in [packet(0xb0, 1, 74, y), packet(0x90, 1, 60, 127)] {
        mpe.apply(&mut rt, Packets::new(&[word]).next().unwrap().unwrap())
            .unwrap();
    }
    let mut out = [[0.0; 2]; 512];
    rt.render(&mut out).unwrap();
    out[256..].iter().map(|f| f[0].abs()).fold(0.0, f32::max)
}

#[test]
fn a_dynamics_controller_takes_timbre_as_its_per_note_position() {
    let ir = with_crossfade(with_filter(instrument()), 1);
    assert_eq!(
        MpeDefaults::for_instrument(&ir).timbre,
        TimbreTarget::Controller(1)
    );
    let (low, high) = (level_of(&ir, 20, 3), level_of(&ir, 127, 3));
    assert!(high > 0.01 && low < 0.5 * high, "{low} {high}");
}

#[test]
fn a_filter_without_a_dynamics_controller_takes_timbre_as_its_cutoff() {
    let ir = with_filter(instrument());
    assert_eq!(
        MpeDefaults::for_instrument(&ir).timbre,
        TimbreTarget::Cutoff
    );
    let (dark, centre, bright) = (
        level_of(&ir, 0, 3),
        level_of(&ir, 64, 3),
        level_of(&ir, 127, 3),
    );
    assert!(
        dark < centre && centre * 4.0 < bright,
        "{dark} {centre} {bright}"
    );
}

#[test]
fn an_instrument_with_neither_keeps_the_tone_low_pass() {
    let ir = instrument();
    assert_eq!(MpeDefaults::for_instrument(&ir).timbre, TimbreTarget::Tone);
    assert!(level(&ir, 0) < 0.001, "darkens below centre");
    assert!((level(&ir, 127) - 0.25).abs() < 1e-5, "never brightens");
}
