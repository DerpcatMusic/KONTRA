//! Fixtures shared by the family tests (test builds only).

use super::*;
use ni_file::kontakt::Chunk;

/// The saved Solid G-EQ record v0x11 with every knob flat and only the low
/// pass enabled (eq.rs `read` order).
pub(crate) fn solid_payload() -> Vec<u8> {
    let mut b = Vec::new();
    for v in [0.5f32, 0.5] {
        b.extend(v.to_le_bytes()); // LF gain, freq
    }
    b.push(0); // LF shelf
    for _ in 0..6 {
        b.extend(0.5f32.to_le_bytes()); // LMF gain, freq, Q; HMF gain, freq, Q
    }
    for _ in 0..2 {
        b.extend(0.5f32.to_le_bytes()); // HF gain, freq
    }
    b.push(0); // HF shelf
    for _ in 0..2 {
        b.extend(0.5f32.to_le_bytes()); // HP freq, LP freq
    }
    b.extend([1u8, 0]); // LP on, HP off
    b
}

pub(crate) fn legacy_eq_slot(kind: i32) -> super::Slot {
    let mut public = [kind.to_le_bytes(); 2].concat();
    for values in [[300f32, 1., 0.], [1000., 1.5, 0.], [6000., 2., 0.]]
        .into_iter()
        .take((kind - 21) as usize)
    {
        for value in values {
            public.extend(value.to_le_bytes());
        }
    }
    let mut fx = slot(0x18, public, 1.);
    fx.slot = 5;
    fx
}

/// A configured-but-unproved filter is reported as an unknown law and nothing else.
pub(crate) fn only_law_notes(notes: &[(usize, String, String, sampler_ir::Reason)]) -> bool {
    notes.iter().all(|n| n.3 == sampler_ir::Reason::UnknownLaw)
}

pub(crate) fn slot(module: u16, public: Vec<u8>, gain: f32) -> Slot {
    Slot {
        slot: 0,
        module,
        version: 0x50,
        bypass: false,
        output_gain: gain,
        dry_level: 1.0,
        output_set: true,
        public,
    }
}

// Owned wire fixture: the wrapper's internal enum deliberately differs from the child ID.
pub(crate) fn effect_fixture(module: u16, version: u16, public: &[u8]) -> BParamArrayBParFX8 {
    mod wire {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/chunks.rs"
        ));
    }
    let mut state = 20u32.to_le_bytes().to_vec();
    state.extend([0; 6]);
    state.extend(1f32.to_le_bytes());
    state.extend(0f32.to_le_bytes());
    state.extend((-1i32).to_le_bytes());
    let child = wire::chunk(module, &wire::object(version, &[], public, &[]));
    let wrapper = wire::object(0x50, &state, &[], &child);
    let mut array = vec![0];
    array.extend(0x13u16.to_le_bytes());
    array.extend(8u32.to_le_bytes());
    for index in 0..8 {
        array.push(u8::from(index == 5));
        if index == 5 {
            array.extend(wire::chunk(0x25, &wrapper));
        }
    }
    BParamArrayBParFX8::try_from(&Chunk {
        id: RACK,
        data: array,
    })
    .unwrap()
}

// Authored independently of the decoder schema: native Raum reader
// 0x140ac5cc0, v0x10, serializes I + 12F + 2B + 3F + B (67 bytes).
pub(crate) fn raum_payload() -> Vec<u8> {
    // Kontakt 8.13.1 FUN_140ac5cc0 validates the leading u32 as an enum 0..=2.
    let mut bytes = 2i32.to_le_bytes().to_vec();
    bytes.extend(
        [
            0.125f32, 0.25, 0.5, 1., 2., 4., 8., 16., 32., 64., 128., 256.,
        ]
        .into_iter()
        .flat_map(f32::to_le_bytes),
    );
    bytes.extend([0xa5, 0x5a]);
    bytes.extend([-0.5f32, -1., -2.].into_iter().flat_map(f32::to_le_bytes));
    bytes.push(0xff);
    bytes
}

pub(crate) fn fx_runtime(processors: Vec<sampler_ir::Processor>, tempo: f64, input: &[[f32; 2]]) -> sampler_core::Runtime {
    let mut instrument = sampler_ir::Instrument::default();
    instrument.assets.push(sampler_ir::Asset { residency: Default::default(), location: sampler_ir::AssetLocation::Path("probe".into()), encoding: sampler_ir::Encoding::Wav, root_key: None, loops: vec![] });
    instrument.chains.push(sampler_ir::Chain { scope: sampler_ir::Scope::Voice, pre_amplitude: processors, post_amplitude: vec![] });
    let mut zone = sampler_ir::Zone::new(sampler_ir::AssetRef(0));
    zone.keys = sampler_ir::KeyRange { low: 60, high: 60 };
    zone.chain = Some(sampler_ir::ChainRef(0)); zone.velocity = sampler_ir::VelocityResponse::None;
    instrument.zones.push(zone);
    let mut samples = vec![[0.0; 2]; 64]; samples[..input.len()].copy_from_slice(input);
    let pcm = sampler_core::Pcm::new(48000, samples.into_boxed_slice()).unwrap();
    let plan = sampler_core::lower::lower(&instrument, 48000, vec![pcm], |_, p| Ok(p)).unwrap();
    let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
    let mut rt = sampler_core::Runtime::new(plan, limits).unwrap();
    rt.trigger(sampler_core::Input { protocol: sampler_core::Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: None }, 60, 1.0).unwrap();
    rt.set_tempo(tempo).unwrap();
    rt
}

pub(crate) fn fx_probe(processors: Vec<sampler_ir::Processor>, tempo: f64, input: &[[f32; 2]]) -> [[f32; 2]; 4] {
    let mut rt = fx_runtime(processors, tempo, input);
    let mut output = [[0.0; 2]; 4]; rt.render(&mut output).unwrap();
    output
}
