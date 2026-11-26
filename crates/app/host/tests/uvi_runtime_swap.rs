//! End-to-end tests proving UVI runtime sound/layer replacement genuinely swaps
//! samples, mappings, loops, slice metadata, oscillator/keygroup parameters,
//! and live effects off the audio thread with zero audio-thread allocations.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kontra_host::rack::{
    Event, HostNote, HostPattern, Rack, RackAudio, RenderMode, Store, StoreConfig, ir,
};
use kontra_ui_model::{UiModifiers, Value as UiValue};

const RATE: u32 = 48_000;
const BLOCK: usize = 256;

/// Writes a 16-bit mono PCM WAV sine wave (with optional `cue ` chunk slice
/// offsets and `smpl` loop points) to `path`.
fn write_sine_wav(
    path: &Path,
    freq_hz: f64,
    amp: f64,
    frames: usize,
    cues: &[u32],
    loop_range: Option<(u32, u32)>,
) {
    let mut pcm = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let s = (2.0 * std::f64::consts::PI * freq_hz * i as f64 / f64::from(RATE)).sin() * amp;
        let q = (s.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
        pcm.extend_from_slice(&q.to_le_bytes());
    }
    let mut wav = b"RIFF\0\0\0\0WAVE".to_vec();
    // fmt chunk
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    // data chunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm);
    // Optional cue chunk
    if !cues.is_empty() {
        let chunk_size = 4 + cues.len() * 24;
        wav.extend_from_slice(b"cue ");
        wav.extend_from_slice(&(chunk_size as u32).to_le_bytes());
        wav.extend_from_slice(&(cues.len() as u32).to_le_bytes());
        for (idx, &offset) in cues.iter().enumerate() {
            wav.extend_from_slice(&((idx + 1) as u32).to_le_bytes()); // dwName
            wav.extend_from_slice(&offset.to_le_bytes()); // dwPosition
            wav.extend_from_slice(b"data"); // fccChunk
            wav.extend_from_slice(&0u32.to_le_bytes()); // dwChunkStart
            wav.extend_from_slice(&0u32.to_le_bytes()); // dwBlockStart
            wav.extend_from_slice(&offset.to_le_bytes()); // dwSampleOffset
        }
    }
    // Optional smpl chunk
    if let Some((lo, hi)) = loop_range {
        wav.extend_from_slice(b"smpl");
        wav.extend_from_slice(&(36u32 + 24).to_le_bytes());
        for _ in 0..7 {
            wav.extend_from_slice(&0u32.to_le_bytes());
        }
        wav.extend_from_slice(&1u32.to_le_bytes()); // cSampleLoops
        wav.extend_from_slice(&0u32.to_le_bytes()); // cbSamplerData
        wav.extend_from_slice(&0u32.to_le_bytes()); // dwIdentifier
        wav.extend_from_slice(&0u32.to_le_bytes()); // dwType (forward)
        wav.extend_from_slice(&lo.to_le_bytes());
        wav.extend_from_slice(&hi.to_le_bytes());
        wav.extend_from_slice(&0u32.to_le_bytes()); // dwFraction
        wav.extend_from_slice(&0u32.to_le_bytes()); // dwPlayCount (infinite)
    }
    let riff_size = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
    std::fs::write(path, &wav).expect("write wav");
}

fn new_rack() -> (Rack, RackAudio) {
    let store = Store::new(StoreConfig::DEFAULT).expect("store");
    let (rack, mut audio) = Rack::new(RATE, store, BLOCK);
    audio.set_render_mode(RenderMode::Offline);
    (rack, audio)
}

/// Runs `blocks` of audio + control ticks so commands, swaps, and Lua timers settle.
fn pump(rack: &mut Rack, audio: &mut RackAudio, blocks: usize) {
    for _ in 0..blocks {
        rack.tick();
        audio.process(BLOCK, &[]);
    }
    rack.tick();
}

/// Renders one note for `blocks` audio blocks (note-on at block 0, note-off at end)
/// and returns the left-channel samples.
fn render_note(
    rack: &mut Rack,
    audio: &mut RackAudio,
    key: u8,
    vel: u8,
    blocks: usize,
) -> Vec<f32> {
    let mut out = Vec::with_capacity(blocks * BLOCK);
    let on = [(
        0u32,
        Event::NoteOn {
            note: HostNote {
                port: 0,
                channel: 0,
                key,
                id: 1,
                clap: false,
            },
            velocity: f64::from(vel) / 127.0,
            tune: 0.0,
        },
    )];
    let off = [(
        0u32,
        Event::NoteOff(HostPattern {
            port: -1,
            channel: -1,
            key: i32::from(key),
            id: -1,
            clap: false,
        }),
    )];
    for b in 0..blocks {
        rack.tick();
        let evs: &[(u32, Event)] = if b == 0 { &on } else { &[] };
        audio.process(BLOCK, evs);
        out.extend(audio.output(0, BLOCK).iter().map(|s| s[0]));
    }
    audio.process(BLOCK, &off);
    for _ in 0..4 {
        rack.tick();
        audio.process(BLOCK, &[]);
    }
    out
}

fn rms(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum / samples.len() as f64).sqrt()
}

/// Estimates fundamental frequency of a pure sine render via positive zero crossings.
fn estimate_hz(samples: &[f32]) -> f64 {
    let mut crossings = 0usize;
    for w in samples.windows(2) {
        if w[0] <= 0.0 && w[1] > 0.0 {
            crossings += 1;
        }
    }
    crossings as f64 * f64::from(RATE) / samples.len().max(1) as f64
}

fn widget_index(rack: &Rack, name: &str) -> usize {
    let view = rack.parts()[0]
        .as_ref()
        .and_then(|p| p.view.as_ref())
        .expect("part view");
    view.tree()
        .widgets
        .iter()
        .position(|w| w.name == name)
        .unwrap_or_else(|| panic!("missing widget {name}"))
}

fn make_temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kontra-uvi-swap-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Samples")).expect("mkdir");
    dir
}

#[test]
fn runtime_load_sample_replaces_rendered_audio_slices_and_fires_done() {
    let root = make_temp_dir("load-sample");
    write_sine_wav(
        &root.join("Samples/low_220.wav"),
        220.0,
        0.8,
        12_000,
        &[],
        None,
    );
    write_sine_wav(
        &root.join("Samples/high_880.wav"),
        880.0,
        0.8,
        12_000,
        &[],
        None,
    );
    write_sine_wav(
        &root.join("Samples/silence.wav"),
        0.0,
        0.0,
        4_096,
        &[],
        None,
    );
    write_sine_wav(
        &root.join("Samples/sliced.wav"),
        440.0,
        0.8,
        8_000,
        &[0, 2000, 4000, 6000],
        None,
    );

    let uvip = root.join("SwapTest.uvip");
    let xml = r#"<UVI4><Program Name="SwapTest">
      <Scripts><ScriptProcessor Name="Main" Bypass="0"><script><![CDATA[
        local osc = Program.layers[1].keygroups[1].oscillators[1]
        local status = Label("Status")
        status.text = "init"
        local b_high = Button("LoadHigh")
        b_high.changed = function()
            loadSample(osc, "Samples/high_880.wav", function(t)
                status.text = t.success and "high_ok" or "high_fail"
            end)
        end
        local b_sil = Button("LoadSilence")
        b_sil.changed = function()
            loadSample(osc, "Samples/silence.wav", function(t)
                status.text = t.success and "sil_ok" or "sil_fail"
            end)
        end
        local b_slice = Button("LoadSliced")
        b_slice.changed = function()
            loadSample(osc, "Samples/sliced.wav", function(t)
                local sl = osc:getSliceInfo()
                status.text = string.format("slices:%d:%d:%d", #sl, sl[2] and sl[2].start or -1, sl[2] and sl[2].length or -1)
            end)
        end
      ]]></script></ScriptProcessor></Scripts>
      <Layers><Layer Name="L1"><Keygroups>
        <Keygroup Name="KG1" LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer SamplePath="Samples/low_220.wav" BaseNote="60"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program></UVI4>"#;
    std::fs::write(&uvip, xml).expect("write uvip");

    let (mut rack, mut audio) = new_rack();
    rack.load(0, &uvip).expect("load uvip");
    pump(&mut rack, &mut audio, 4);

    // 1. Baseline plays ~220 Hz.
    let base = render_note(&mut rack, &mut audio, 60, 100, 24);
    assert!(
        rms(&base) > 0.2,
        "baseline should be audible: {}",
        rms(&base)
    );
    let hz_base = estimate_hz(&base);
    assert!(
        (hz_base - 220.0).abs() < 25.0,
        "expected ~220 Hz, got {hz_base}"
    );

    // 2. Replace with 880 Hz at runtime and verify zero Luau heap spills on audio thread.
    let spills_before = kontra_lua::alloc::spills();
    let w_high = widget_index(&rack, "LoadHigh");
    rack.control(0, w_high, None, 1.0, UiModifiers::default());
    pump(&mut rack, &mut audio, 10);
    let spills_after = kontra_lua::alloc::spills();
    assert_eq!(
        spills_after, spills_before,
        "runtime loadSample spilled off the preallocated Luau arena on the audio thread"
    );

    let after_high = render_note(&mut rack, &mut audio, 60, 100, 24);
    assert!(rms(&after_high) > 0.2, "replaced sample should be audible");
    let hz_high = estimate_hz(&after_high);
    assert!(
        (hz_high - 880.0).abs() < 40.0,
        "expected ~880 Hz after loadSample, got {hz_high}"
    );
    let label = &rack.parts()[0]
        .as_ref()
        .unwrap()
        .view
        .as_ref()
        .unwrap()
        .tree()
        .widgets[widget_index(&rack, "Status")];
    assert_eq!(label.text, "high_ok");

    // 3. Replace with Silence.wav -> must truly silence playback.
    let w_sil = widget_index(&rack, "LoadSilence");
    rack.control(0, w_sil, None, 1.0, UiModifiers::default());
    pump(&mut rack, &mut audio, 10);
    let after_sil = render_note(&mut rack, &mut audio, 60, 100, 16);
    assert!(
        rms(&after_sil) < 1e-5,
        "expected silence after swapping to Silence.wav, got rms {}",
        rms(&after_sil)
    );

    // 4. Replace with cue-chunk WAV -> getSliceInfo returns real cue offsets & lengths.
    let w_slice = widget_index(&rack, "LoadSliced");
    rack.control(0, w_slice, None, 1.0, UiModifiers::default());
    pump(&mut rack, &mut audio, 10);
    let label = &rack.parts()[0]
        .as_ref()
        .unwrap()
        .view
        .as_ref()
        .unwrap()
        .tree()
        .widgets[widget_index(&rack, "Status")];
    assert_eq!(label.text, "slices:4:2000:2000");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn runtime_load_mapping_replaces_zones_and_key_splits() {
    let root = make_temp_dir("load-mapping");
    write_sine_wav(
        &root.join("Samples/low_220.wav"),
        220.0,
        0.8,
        12_000,
        &[],
        None,
    );
    write_sine_wav(
        &root.join("Samples/high_880.wav"),
        880.0,
        0.8,
        12_000,
        &[],
        None,
    );
    let map_xml = r#"<Program><Layers><Layer><Keygroups>
      <Keygroup LowKey="48" HighKey="59"><Oscillators>
        <SamplePlayer SamplePath="Samples/low_220.wav" BaseNote="55"/>
      </Oscillators></Keygroup>
      <Keygroup LowKey="60" HighKey="72"><Oscillators>
        <SamplePlayer SamplePath="Samples/high_880.wav" BaseNote="65"/>
      </Oscillators></Keygroup>
    </Keygroups></Layer></Layers></Program>"#;
    std::fs::write(root.join("split.xml"), map_xml).expect("write mapping");

    let uvip = root.join("MapTest.uvip");
    let xml = r#"<UVI4><Program Name="MapTest">
      <Scripts><ScriptProcessor Name="Main" Bypass="0"><script><![CDATA[
        local osc = Program.layers[1].keygroups[1].oscillators[1]
        local status = Label("Status")
        local btn = Button("LoadMap")
        btn.changed = function()
            osc:loadMapping("split.xml", function(t)
                status.text = string.format("%s:%d", tostring(t.success), osc:getNumZones())
            end)
        end
      ]]></script></ScriptProcessor></Scripts>
      <Layers><Layer Name="L1"><Keygroups>
        <Keygroup Name="KG1" LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer SamplePath="Samples/low_220.wav" BaseNote="60"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program></UVI4>"#;
    std::fs::write(&uvip, xml).expect("write uvip");

    let (mut rack, mut audio) = new_rack();
    rack.load(0, &uvip).expect("load uvip");
    pump(&mut rack, &mut audio, 4);

    let w_btn = widget_index(&rack, "LoadMap");
    rack.control(0, w_btn, None, 1.0, UiModifiers::default());
    pump(&mut rack, &mut audio, 10);

    let label = &rack.parts()[0]
        .as_ref()
        .unwrap()
        .view
        .as_ref()
        .unwrap()
        .tree()
        .widgets[widget_index(&rack, "Status")];
    assert_eq!(label.text, "true:2");

    let low_note = render_note(&mut rack, &mut audio, 55, 100, 20);
    let high_note = render_note(&mut rack, &mut audio, 65, 100, 20);
    assert!((estimate_hz(&low_note) - 220.0).abs() < 25.0);
    assert!((estimate_hz(&high_note) - 880.0).abs() < 40.0);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn runtime_osc_params_playback_options_and_dynamic_effects_affect_audio() {
    let root = make_temp_dir("params-fx");
    write_sine_wav(
        &root.join("Samples/tone_220.wav"),
        220.0,
        0.8,
        4_000,
        &[],
        None,
    );

    let uvip = root.join("ParamFx.uvip");
    let xml = r#"<UVI4><Program Name="ParamFx">
      <Scripts><ScriptProcessor Name="Main" Bypass="0"><script><![CDATA[
        local kg = Program.layers[1].keygroups[1]
        local osc = kg.oscillators[1]
        local b_root = Button("ShiftBaseNote")
        b_root.changed = function()
            osc:setParameter("BaseNote", 48)
        end
        local b_loop = Button("EnableLoop")
        b_loop.changed = function()
            setPlaybackOptions(osc, { LoopMode = 1, LoopStart = 200, LoopEnd = 1800 })
        end
        local b_fx = Button("SwapFx")
        b_fx.changed = function()
            kg:insertEffect("WaveShaper", 1)
            kg:removeEffect(2)
        end
      ]]></script></ScriptProcessor></Scripts>
      <Layers><Layer Name="L1"><Keygroups>
        <Keygroup Name="KG1" LowKey="0" HighKey="127">
          <Inserts><OnePole Freq="20000" Mode="0" Bypass="0"/></Inserts>
          <Oscillators><SamplePlayer SamplePath="Samples/tone_220.wav" BaseNote="60"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program></UVI4>"#;
    std::fs::write(&uvip, xml).expect("write uvip");

    let (mut rack, mut audio) = new_rack();
    rack.load(0, &uvip).expect("load uvip");
    pump(&mut rack, &mut audio, 4);

    // Lowering BaseNote 60 -> 48 pitches MIDI 60 up one octave (~440 Hz).
    // At 2x playback speed the 4,000-frame sample lasts 2,000 frames (~7.8 blocks),
    // so measure frequency over 6 blocks (1,536 frames) before the sample ends.
    rack.control(
        0,
        widget_index(&rack, "ShiftBaseNote"),
        None,
        1.0,
        UiModifiers::default(),
    );
    pump(&mut rack, &mut audio, 6);
    let pitched = render_note(&mut rack, &mut audio, 60, 100, 6);
    let hz = estimate_hz(&pitched);
    assert!(
        (hz - 440.0).abs() < 35.0,
        "expected ~440 Hz after BaseNote=48, got {hz}"
    );

    // Enabling LoopMode=1 sustains audio well past the 4,000-frame sample length (~83ms).
    rack.control(
        0,
        widget_index(&rack, "EnableLoop"),
        None,
        1.0,
        UiModifiers::default(),
    );
    pump(&mut rack, &mut audio, 6);
    let looped = render_note(&mut rack, &mut audio, 60, 100, 40);
    let tail = &looped[looped.len() - 1024..];
    assert!(
        rms(tail) > 0.15,
        "looped sample should still sound past 4000 frames; tail rms = {}",
        rms(tail)
    );

    // Dynamic insertEffect updates live plan & group inserts cleanly.
    rack.control(
        0,
        widget_index(&rack, "SwapFx"),
        None,
        1.0,
        UiModifiers::default(),
    );
    pump(&mut rack, &mut audio, 6);
    let plan = rack.parts()[0].as_ref().unwrap().plan.as_ref().unwrap();
    assert!(matches!(
        plan.groups[0]
            .inserts
            .first()
            .and_then(Option::as_ref)
            .map(|e| &e.kind),
        Some(ir::EffectKind::Shaper(_))
    ));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn four_sound_independent_replacement_keeps_other_layers_and_saves_state() {
    let root = make_temp_dir("four-sounds");
    let freqs = [200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0, 900.0];
    for (i, &f) in freqs.iter().enumerate() {
        write_sine_wav(
            &root.join(format!("Samples/s{i}.wav")),
            f,
            0.5,
            12_000,
            &[],
            None,
        );
    }

    let uvip = root.join("FourSounds.uvip");
    let xml = r#"<UVI4><Program Name="FourSounds">
      <Scripts><ScriptProcessor Name="Main" Bypass="0"><script><![CDATA[
        loaded = { "s0", "s1", "s2", "s3" }
        function onSave()
            return { loaded = loaded }
        end
        function onLoad(s)
            if s and s.loaded then loaded = s.loaded end
        end
        for i = 1, 4 do
            local btn = Button("Replace_" .. i)
            btn.changed = function()
                local next_name = "s" .. tostring(i + 3)
                local osc = Program.layers[i].keygroups[1].oscillators[1]
                loadSample(osc, "Samples/" .. next_name .. ".wav", function(t)
                    if t.success then loaded[i] = next_name end
                end)
            end
        end
      ]]></script></ScriptProcessor></Scripts>
      <Layers>
        <Layer Name="L1"><Keygroups><Keygroup Name="K1" LowKey="48" HighKey="48"><Oscillators><SamplePlayer SamplePath="Samples/s0.wav" BaseNote="48"/></Oscillators></Keygroup></Keygroups></Layer>
        <Layer Name="L2"><Keygroups><Keygroup Name="K2" LowKey="52" HighKey="52"><Oscillators><SamplePlayer SamplePath="Samples/s1.wav" BaseNote="52"/></Oscillators></Keygroup></Keygroups></Layer>
        <Layer Name="L3"><Keygroups><Keygroup Name="K3" LowKey="55" HighKey="55"><Oscillators><SamplePlayer SamplePath="Samples/s2.wav" BaseNote="55"/></Oscillators></Keygroup></Keygroups></Layer>
        <Layer Name="L4"><Keygroups><Keygroup Name="K4" LowKey="60" HighKey="60"><Oscillators><SamplePlayer SamplePath="Samples/s3.wav" BaseNote="60"/></Oscillators></Keygroup></Keygroups></Layer>
      </Layers>
    </Program></UVI4>"#;
    std::fs::write(&uvip, xml).expect("write uvip");

    let (mut rack, mut audio) = new_rack();
    rack.load(0, &uvip).expect("load uvip");
    pump(&mut rack, &mut audio, 4);

    let keys = [48u8, 52, 55, 60];
    let sample_ptrs = |rack: &Rack| -> Vec<usize> {
        let plan = rack.parts()[0].as_ref().unwrap().plan.as_ref().unwrap();
        (0..4u32)
            .map(|g| {
                let z = plan.zones.iter().find(|z| z.group == g).expect("zone");
                Arc::as_ptr(&z.region.sample) as usize
            })
            .collect()
    };

    let initial_ptrs = sample_ptrs(&rack);
    let mut current_ptrs = initial_ptrs.clone();

    // Replace each of the 4 sounds one by one and verify isolation.
    for slot in 0..4usize {
        let btn = widget_index(&rack, &format!("Replace_{}", slot + 1));
        rack.control(0, btn, None, 1.0, UiModifiers::default());
        pump(&mut rack, &mut audio, 10);

        let next_ptrs = sample_ptrs(&rack);
        assert_ne!(
            next_ptrs[slot],
            current_ptrs[slot],
            "sound {} sample pointer did not change",
            slot + 1
        );
        for other in 0..4usize {
            if other != slot {
                assert_eq!(
                    next_ptrs[other],
                    current_ptrs[other],
                    "replacing sound {} unexpectedly mutated sound {}",
                    slot + 1,
                    other + 1
                );
            }
        }
        current_ptrs = next_ptrs;

        let expected_hz = freqs[slot + 4];
        let rendered = render_note(&mut rack, &mut audio, keys[slot], 100, 20);
        let got_hz = estimate_hz(&rendered);
        assert!(
            (got_hz - expected_hz).abs() < 35.0,
            "sound {} expected ~{expected_hz} Hz, got {got_hz}",
            slot + 1
        );
    }

    // Saved state reflects all 4 replacements.
    pump(&mut rack, &mut audio, 8);
    let saved = rack.saved_scripts(0);
    let joined = saved
        .iter()
        .flat_map(|s| s.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    for expected in ["s4", "s5", "s6", "s7"] {
        assert!(
            joined.contains(expected),
            "saved state missing {expected}: {joined}"
        );
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn augmented_orchestra_real_four_sound_replacement_when_installed() {
    let path = Path::new(
        "/data/main/Libraries/UVI/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs/Presets/00 Orchestra/01 Strings/V Strings Bartok.uvip",
    );
    if !path.ancestors().any(Path::is_file) {
        eprintln!("skipped: no {}", path.display());
        return;
    }

    let (mut rack, mut audio) = new_rack();
    rack.load(0, path).expect("load Augmented Orchestra");
    // Advance > 100ms of audio so startup onLoad wait(50) echo completes cleanly.
    pump(&mut rack, &mut audio, 40);

    // Helper: snapshot Arc<Sample> pointers per layer (97 keygroups per layer in AO).
    let layer_ptrs = |rack: &Rack| -> [Vec<usize>; 4] {
        let plan = rack.parts()[0].as_ref().unwrap().plan.as_ref().unwrap();
        let groups_per_layer = (plan.groups.len() / 4).max(1) as u32;
        std::array::from_fn(|layer| {
            let lo = layer as u32 * groups_per_layer;
            let hi = lo + groups_per_layer;
            plan.zones
                .iter()
                .filter(|z| z.group >= lo && z.group < hi)
                .map(|z| Arc::as_ptr(&z.region.sample) as usize)
                .collect()
        })
    };

    let base_ptrs = layer_ptrs(&rack);
    let base_audio = render_note(&mut rack, &mut audio, 60, 100, 40);
    let base_rms = rms(&base_audio);
    assert!(
        base_rms > 1e-4,
        "Augmented Orchestra baseline should be audible (rms={base_rms})"
    );

    // Locate a real replacement LayerButton_<id>_<cat>_<inst> for each of the 4 layers.
    let pick_button = |rack: &Rack, layer_id: usize| -> Option<(usize, String)> {
        let view = rack.parts()[0].as_ref()?.view.as_ref()?;
        let prefix = format!("LayerButton_{layer_id}_");
        // Pick a button that is NOT currently selected (value == 0) in a different category/articulation.
        view.tree()
            .widgets
            .iter()
            .enumerate()
            .find(|(_, w)| {
                w.name.starts_with(&prefix)
                    && !matches!(w.value, Some(UiValue::Integer(v)) if v != 0)
                    && !matches!(w.value, Some(UiValue::Real(v)) if v >= 0.5)
            })
            .map(|(i, w)| (i, w.name.clone()))
    };

    let mut prev_ptrs = base_ptrs.clone();
    let mut swapped_layers = 0usize;
    for layer_idx in 0..4usize {
        let Some((w_idx, w_name)) = pick_button(&rack, layer_idx + 1) else {
            continue;
        };
        rack.control(0, w_idx, None, 1.0, UiModifiers::default());
        // AO Layer:load runs wait(50) (~10 blocks at 256/48k) then loads ~1,746 oscillators.
        pump(&mut rack, &mut audio, 60);

        let next_ptrs = layer_ptrs(&rack);
        let changed = next_ptrs[layer_idx]
            .iter()
            .zip(&prev_ptrs[layer_idx])
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            changed > 0,
            "layer {} ({w_name}) had 0 replaced zone sample pointers after runtime swap!",
            layer_idx + 1
        );
        for other in 0..4usize {
            if other != layer_idx {
                assert_eq!(
                    next_ptrs[other],
                    prev_ptrs[other],
                    "replacing AO layer {} unexpectedly altered layer {}",
                    layer_idx + 1,
                    other + 1
                );
            }
        }
        prev_ptrs = next_ptrs;
        swapped_layers += 1;
    }
    assert_eq!(
        swapped_layers, 4,
        "expected to replace all 4 Augmented Orchestra sound layers"
    );

    // Render note 60 again after replacing all 4 layers and verify audible audio genuinely changed!
    let new_audio = render_note(&mut rack, &mut audio, 60, 100, 40);
    let new_rms = rms(&new_audio);
    assert!(
        new_rms > 1e-4,
        "Augmented Orchestra after 4-sound replacement should still be audible (rms={new_rms})"
    );
    let diff_rms = {
        let n = base_audio.len().min(new_audio.len());
        let sum: f64 = base_audio[..n]
            .iter()
            .zip(&new_audio[..n])
            .map(|(&a, &b)| {
                let d = f64::from(a - b);
                d * d
            })
            .sum();
        (sum / n.max(1) as f64).sqrt()
    };
    assert!(
        diff_rms > 1e-4,
        "rendered audio must genuinely differ after replacing all 4 AO sounds (diff_rms={diff_rms})"
    );

    // Saved script state reflects the replaced layers (no longer all Bartok).
    pump(&mut rack, &mut audio, 12);
    let saved = rack.saved_scripts(0);
    let state_blob = saved
        .iter()
        .flat_map(|s| s.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        state_blob.contains("loadedLayer"),
        "expected saved AO script state to contain loadedLayer entries"
    );
    assert!(
        !state_blob.contains("Bartok"),
        "expected saved AO script state to reflect all 4 replaced layers (no Bartok remaining)"
    );
}

/// Goertzel DFT magnitude at `target_hz` over `samples` (used to verify
/// distortion harmonics and filter attenuation audibly).
fn dft_mag(samples: &[f32], target_hz: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let w = 2.0 * std::f64::consts::PI * target_hz / f64::from(RATE);
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, &s) in samples.iter().enumerate() {
        let phase = w * i as f64;
        re += f64::from(s) * phase.cos();
        im -= f64::from(s) * phase.sin();
    }
    2.0 * (re * re + im * im).sqrt() / samples.len() as f64
}

fn verify_empty_chain_live_effect_growth(tag: &str, target_expr: &str) {
    let root = make_temp_dir(tag);
    write_sine_wav(
        &root.join("Samples/tone_2000.wav"),
        2000.0,
        0.75,
        24_000,
        &[],
        None,
    );

    let uvip = root.join("EmptyChain.uvip");
    let xml = format!(
        r#"<UVI4><Program Name="EmptyChain">
      <Scripts><ScriptProcessor Name="Main" Bypass="0"><script><![CDATA[
        local target = {target_expr}
        local ws, lp
        local b_add_ws = Button("AddWS")
        b_add_ws.changed = function()
            ws = target:insertEffect("WaveShaper", 1)
            ws:setParameter("Mode", 2)
            ws:setParameter("Amount", 0.5)
            ws:setParameter("Mix", 1.0)
        end
        local b_add_lp = Button("AddLP")
        b_add_lp.changed = function()
            lp = target:insertEffect("OnePole", 2)
            lp:setParameter("Freq", 300)
            lp:setParameter("Mode", 0)
        end
        local b_move = Button("MoveLPFirst")
        b_move.changed = function()
            target:moveEffect(2, 1)
        end
        local b_clear = Button("ClearFx")
        b_clear.changed = function()
            target:removeEffect(1)
            target:removeEffect(1)
        end
      ]]></script></ScriptProcessor></Scripts>
      <Layers><Layer Name="L1"><Keygroups>
        <Keygroup Name="KG1" LowKey="0" HighKey="127">
          <Oscillators><SamplePlayer SamplePath="Samples/tone_2000.wav" BaseNote="60"/></Oscillators>
        </Keygroup>
      </Keygroups></Layer></Layers>
    </Program></UVI4>"#
    );
    std::fs::write(&uvip, xml).expect("write uvip");

    let (mut rack, mut audio) = new_rack();
    rack.load(0, &uvip).expect("load uvip");
    pump(&mut rack, &mut audio, 4);

    // 1. Baseline: initially 0 inserts everywhere -> clean 2000 Hz sine, negligible 6000 Hz 3rd harmonic.
    let base = render_note(&mut rack, &mut audio, 60, 100, 16);
    let base_win = &base[1024..3072];
    let base_rms = rms(base_win);
    let base_f1 = dft_mag(base_win, 2000.0);
    let base_f3 = dft_mag(base_win, 6000.0);
    assert!(base_rms > 1e-3, "{tag}: baseline silent ({base_rms})");
    assert!(
        base_f3 / base_f1.max(1e-9) < 0.01,
        "{tag}: baseline should be clean sine, f3/f1 = {}",
        base_f3 / base_f1
    );

    // 2. insertEffect("WaveShaper") on initially empty chain -> audible odd-harmonic distortion.
    let spills_before = kontra_lua::alloc::spills();
    rack.control(
        0,
        widget_index(&rack, "AddWS"),
        None,
        1.0,
        UiModifiers::default(),
    );
    pump(&mut rack, &mut audio, 8);
    let ws_audio = render_note(&mut rack, &mut audio, 60, 100, 16);
    let ws_win = &ws_audio[1024..3072];
    let ws_f1 = dft_mag(ws_win, 2000.0);
    let ws_f3 = dft_mag(ws_win, 6000.0);
    let ws_thd = ws_f3 / ws_f1.max(1e-9);
    assert!(
        ws_thd > 0.10,
        "{tag}: WaveShaper inserted into empty chain must audibly distort (f3/f1={ws_thd:.4}, f1={ws_f1:.4}, f3={ws_f3:.4})"
    );

    // 3. insertEffect("OnePole", 2) with Freq=300 AFTER WaveShaper -> strong lowpass roll-off.
    rack.control(
        0,
        widget_index(&rack, "AddLP"),
        None,
        1.0,
        UiModifiers::default(),
    );
    pump(&mut rack, &mut audio, 8);
    let ws_then_lp = render_note(&mut rack, &mut audio, 60, 100, 16);
    let ws_lp_win = &ws_then_lp[1024..3072];
    let ws_lp_rms = rms(ws_lp_win);
    let ws_lp_f1 = dft_mag(ws_lp_win, 2000.0);
    let ws_lp_f3 = dft_mag(ws_lp_win, 6000.0);
    let ws_lp_ratio = ws_lp_f3 / ws_lp_f1.max(1e-9);
    assert!(
        ws_lp_rms < rms(ws_win) * 0.35,
        "{tag}: OnePole(300Hz) must strongly roll off 2kHz signal (before={}, after={ws_lp_rms})",
        rms(ws_win)
    );

    // 4. moveEffect(2, 1): OnePole(300Hz) BEFORE WaveShaper -> order changes nonlinear output!
    // Putting OnePole before WaveShaper attenuates input before tanh, leaving post-shaper
    // harmonics unfiltered (higher f3/f1 ratio or higher overall level than post-filtered tanh).
    rack.control(
        0,
        widget_index(&rack, "MoveLPFirst"),
        None,
        1.0,
        UiModifiers::default(),
    );
    pump(&mut rack, &mut audio, 8);
    let lp_then_ws = render_note(&mut rack, &mut audio, 60, 100, 16);
    let lp_ws_win = &lp_then_ws[1024..3072];
    let lp_ws_f1 = dft_mag(lp_ws_win, 2000.0);
    let lp_ws_f3 = dft_mag(lp_ws_win, 6000.0);
    let lp_ws_ratio = lp_ws_f3 / lp_ws_f1.max(1e-9);
    let order_diff = (rms(lp_ws_win) - ws_lp_rms).abs() + (lp_ws_ratio - ws_lp_ratio).abs();
    assert!(
        order_diff > 0.03,
        "{tag}: moveEffect(2, 1) must audibly change WaveShaper/OnePole order (ws->lp rms={ws_lp_rms:.4} f3/f1={ws_lp_ratio:.4} vs lp->ws rms={:.4} f3/f1={lp_ws_ratio:.4})",
        rms(lp_ws_win)
    );

    // 5. removeEffect(1) twice -> clean baseline audio restored!
    rack.control(
        0,
        widget_index(&rack, "ClearFx"),
        None,
        1.0,
        UiModifiers::default(),
    );
    pump(&mut rack, &mut audio, 8);
    let restored = render_note(&mut rack, &mut audio, 60, 100, 16);
    let rest_win = &restored[1024..3072];
    let rest_rms = rms(rest_win);
    let rest_f1 = dft_mag(rest_win, 2000.0);
    let rest_f3 = dft_mag(rest_win, 6000.0);
    assert!(
        (rest_rms - base_rms).abs() < base_rms * 0.05,
        "{tag}: removeEffect must restore baseline level (base={base_rms:.4}, restored={rest_rms:.4})"
    );
    assert!(
        rest_f3 / rest_f1.max(1e-9) < 0.01,
        "{tag}: removeEffect must remove distortion harmonics (f3/f1={:.4})",
        rest_f3 / rest_f1
    );

    assert_eq!(
        kontra_lua::alloc::spills(),
        spills_before,
        "{tag}: dynamic insertEffect/moveEffect/removeEffect spilled Luau audio arena"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_empty_chain_effect_growth_remove_and_reorder_on_keygroup_bus_and_program() {
    verify_empty_chain_live_effect_growth("empty-kg", "Program.layers[1].keygroups[1]");
    verify_empty_chain_live_effect_growth("empty-bus", "Program.layers[1]");
    verify_empty_chain_live_effect_growth("empty-prog", "Program");
}
