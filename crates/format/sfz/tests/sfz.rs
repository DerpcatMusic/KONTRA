//! SFZ text to the instrument model. Sample files are silent WAVs written
//! into the target dir here, stand-ins for real ones.

use std::path::{Path, PathBuf};

use kontra_format_sfz::parse;
use kontra_ir::{self as ir, Impact, Loop, StartRule};

/// A fresh folder for one test, under the target dir.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    // Cargo keeps this folder between runs: start it empty. A folder that
    // cannot be made fails the test at its first write.
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// A silent mono WAV of `frames` frames at `path`, its folder made as needed.
fn wav(path: &Path, frames: u32) -> std::io::Result<()> {
    wav_with_smpl(path, frames, None)
}

/// A silent mono 16-bit 44.1 kHz WAV with an optional `smpl` forward loop `(start, end_inclusive)`.
fn wav_with_smpl(path: &Path, frames: u32, smpl_loop: Option<(u32, u32)>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let data_len = frames * 2;
    let mut out = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(1u16.to_le_bytes()); // mono
    out.extend(44_100u32.to_le_bytes());
    out.extend(88_200u32.to_le_bytes());
    out.extend(2u16.to_le_bytes()); // block align
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data_len.to_le_bytes());
    out.resize(out.len() + data_len as usize, 0);
    if let Some((start, end_inc)) = smpl_loop {
        let mut body = vec![0u8; 36];
        body.splice(28..32, 1u32.to_le_bytes());
        for word in [0u32, 0, start, end_inc, 0, 0] {
            body.extend(word.to_le_bytes());
        }
        out.extend(b"smpl");
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
    }
    let riff_size = (out.len() - 8) as u32;
    out.splice(4..8, riff_size.to_le_bytes());
    std::fs::write(path, out)
}

/// Writes a minimal FLAC header (STREAMINFO only) declaring `frames` at 48 kHz stereo.
fn flac(path: &Path, frames: u64) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let packed: u64 =
        (48_000u64 << 44) | (1u64 << 41) | (15u64 << 36) | (frames & ((1u64 << 36) - 1));
    let mut out = b"fLaC\x80\x00\x00\x22".to_vec();
    out.extend([0u8; 10]);
    out.extend(packed.to_be_bytes());
    out.extend([0u8; 16]);
    std::fs::write(path, out)
}

/// Writes a silent AIFF with `frames` at 44.1 kHz stereo.
fn aiff(path: &Path, frames: u32) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut body = b"AIFFCOMM".to_vec();
    body.extend(18u32.to_be_bytes());
    body.extend(2u16.to_be_bytes());
    body.extend(frames.to_be_bytes());
    body.extend(16u16.to_be_bytes());
    body.extend([0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]); // 44100 Hz IEEE extended
    let data_bytes = frames * 4;
    body.extend(b"SSND");
    body.extend((8 + data_bytes).to_be_bytes());
    body.resize(body.len() + 8 + data_bytes as usize, 0);
    let mut out = b"FORM".to_vec();
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    std::fs::write(path, out)
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn inheritance_and_override() {
    let dir = scratch("inherit");
    wav(&dir.join("a.wav"), 1000).expect("wav");
    wav(&dir.join("b.wav"), 1000).expect("wav");
    let text = "\
<global> volume=-6 ampeg_release=0.5
<group> group=2 ampeg_attack=0.2 pan=50
<region> sample=a.wav lokey=c4 hikey=c4
<region> sample=a.wav lokey=d4 hikey=d4 volume=-3 ampeg_attack=0.3
<group> pan=-50
<region> sample=b.wav key=60
";
    let inst = parse(text, &dir);
    assert_eq!(inst.groups.len(), 3);
    let first = inst.groups.first().expect("group 0");
    let second = inst.groups.get(1).expect("group 1");
    let third = inst.groups.get(2).expect("group 2");
    assert!(near(first.envelope.attack, 0.2));
    assert!(near(second.envelope.attack, 0.3));
    assert!(near(third.envelope.attack, 0.0));
    assert!(
        near(first.envelope.release, 0.5),
        "global reaches the group"
    );
    assert!(
        near(third.envelope.release, 0.5),
        "a new group keeps global"
    );
    let z0 = first.zones.first().expect("zone");
    assert!(near(z0.volume_db, -6.0) && near(z0.pan, 0.5));
    let z1 = second.zones.first().expect("zone");
    assert!(near(z1.volume_db, -3.0), "region overrides global");
    let z2 = third.zones.first().expect("zone");
    assert!(near(z2.pan, -0.5), "new group resets pan");
    assert_eq!(z2.sample, 1, "b.wav is the second sample");
}

#[test]
fn define_and_include_expand() {
    let dir = scratch("include");
    wav(&dir.join("sub/x.wav"), 10).expect("wav");
    std::fs::write(dir.join("inc.sfz"), "<region> sample=$DIR/x.wav key=10\n").expect("inc");
    let text = "#define $DIR sub\n#include \"inc.sfz\"\n<region> sample=$DIR/x.wav key=20\n";
    let inst = parse(text, &dir);
    assert_eq!(inst.samples.len(), 1, "both regions name one file");
    assert_eq!(
        inst.samples.first().map(|s| s.file.clone()),
        Some(dir.join("sub/x.wav"))
    );
    let zones: Vec<&ir::Zone> = inst.groups.iter().flat_map(|g| &g.zones).collect();
    assert_eq!(
        zones.iter().map(|z| *z.keys.start()).collect::<Vec<_>>(),
        [10, 20]
    );
}

#[test]
fn includes_drop_a_bom_and_stay_in_the_folder() {
    let dir = scratch("include-bom");
    wav(&dir.join("x.wav"), 10).expect("wav");
    std::fs::write(
        dir.join("bom.sfz"),
        "\u{FEFF}<region> sample=x.wav key=10\n",
    )
    .expect("bom");
    let inst = parse("#include \"bom.sfz\"\n", &dir);
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
    assert_eq!(inst.groups.iter().map(|g| g.zones.len()).sum::<usize>(), 1);

    std::fs::write(dir.join("self.sfz"), "#include \"self.sfz\"\n").expect("self");
    let text = "#include \"../outside.sfz\"\n#include \"/etc/hosts\"\n#include \"self.sfz\"\n";
    let inst = parse(text, &dir);
    let report = format!("{:?}", inst.diagnostics);
    assert!(
        report.contains("outside the folder\", detail: \"2×"),
        "{report}"
    );
    assert!(report.contains("#include depth"), "{report}");
}

#[test]
fn sample_path_with_spaces_and_backslashes() {
    let dir = scratch("spaces");
    wav(&dir.join("Piano Set/C4 soft.wav"), 100).expect("wav");
    let text =
        "<control> default_path=Piano Set\\\n<region> sample=C4 soft.wav lokey=60 hikey=60\n";
    let inst = parse(text, &dir);
    let sample = inst.samples.first().expect("sample");
    assert_eq!(sample.file, dir.join("Piano Set/C4 soft.wav"));
    assert_eq!(sample.frames, 100, "header read from the file");
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
}

#[test]
fn key_velocity_ranges_and_keycenter() {
    let dir = scratch("ranges");
    wav(&dir.join("a.wav"), 10).expect("wav");
    let text = "\
<region> sample=a.wav key=c4
<region> sample=a.wav lokey=c3 hikey=c3 pitch_keycenter=c2 lovel=1 hivel=64 xfin_lovel=0 xfout_hivel=100
";
    let inst = parse(text, &dir);
    let zones: Vec<&ir::Zone> = inst.groups.iter().flat_map(|g| &g.zones).collect();
    let (keyed, ranged) = (
        zones.first().expect("zone 0"),
        zones.get(1).expect("zone 1"),
    );
    assert_eq!((keyed.keys.clone(), keyed.root), (60..=60, 60));
    assert_eq!(keyed.velocities, 0..=127);
    assert_eq!(
        (ranged.keys.clone(), ranged.root),
        (48..=48, 36),
        "c2 is 36"
    );
    assert_eq!(
        ranged.velocities,
        0..=100,
        "xfin_lovel=0 widens the zone down"
    );
    assert_eq!(ranged.velocity_fade, (1, 36), "fade in from 0 to lovel 1");
}

#[test]
fn ampeg_maps_to_the_envelope() {
    let dir = scratch("ampeg");
    wav(&dir.join("a.wav"), 10).expect("wav");
    let text = "\
<region> sample=a.wav ampeg_attack=0.1 ampeg_hold=0.2 ampeg_decay=0.3 ampeg_sustain=50 ampeg_release=0.4
<region> sample=a.wav
";
    let inst = parse(text, &dir);
    let shaped = inst.groups.first().expect("group 0").envelope;
    assert!(near(shaped.attack, 0.1) && near(shaped.hold, 0.2));
    assert!(near(shaped.decay, 0.3) && near(shaped.release, 0.4));
    assert!(near(shaped.sustain, 0.5), "sustain is a percentage");
    let plain = inst.groups.get(1).expect("group 1").envelope;
    assert!(
        near(plain.sustain, 1.0) && near(plain.release, 0.001),
        "spec defaults"
    );
}

#[test]
fn velocity_tracking_defaults_to_full_on_the_squared_velocity() {
    let dir = scratch("veltrack");
    wav(&dir.join("a.wav"), 10).expect("wav");
    let inst = parse("<region> sample=a.wav seq_length=3 seq_position=2\n", &dir);
    let group = inst.groups.first().expect("group");
    assert_eq!(group.mods.len(), 1, "amp_veltrack defaults to 100");
    let ir::ModKind::Volume {
        intensity, curve, ..
    } = group.mods[0].kind
    else {
        panic!("{:?}", group.mods[0].kind);
    };
    // sfizz: half velocity plays at a quarter.
    assert_eq!(curve.gain(intensity, 0.5), 0.25);
    assert_eq!(
        group.start,
        vec![(
            ir::Join::And,
            StartRule::RoundRobin {
                position: 1,
                length: 3,
                random: false
            }
        )]
    );
}

#[test]
fn cutoff_and_fil_type_become_a_group_filter() {
    let dir = scratch("filter");
    wav(&dir.join("a.wav"), 10).expect("wav");
    let inst = parse(
        "<region> sample=a.wav cutoff=1200 resonance=6 fil_type=hpf_2p\n",
        &dir,
    );
    assert!(inst.diagnostics.iter().all(|d| d.feature != "filter"));
    let Some(Some(effect)) = inst.groups[0].inserts.first() else {
        panic!("a filter insert");
    };
    let ir::EffectKind::Filter(f) = effect.kind else {
        panic!("{:?}", effect.kind);
    };
    assert_eq!((f.pass, f.hz, f.stages), (ir::Pass::High, 1200.0, 1));
    assert!((f.damping[0] - 0.5).abs() < 0.01, "{:?}", f.damping);
}

#[test]
fn end_minus_one_is_a_region_that_plays_nothing() {
    let dir = scratch("silent_end");
    wav(&dir.join("a.wav"), 10).expect("wav");
    let inst = parse("<region> sample=a.wav end=-1\n", &dir);
    let zone = &inst.groups[0].zones[0];
    assert_eq!(zone.end, 0);
    assert_eq!(zone.start, 0);
}

#[test]
fn offset_end_and_loop_use_the_sample_frames() {
    let dir = scratch("frames");
    wav(&dir.join("a.wav"), 1000).expect("wav");
    let text = "<region> sample=a.wav offset=10 end=99 loop_mode=loop_continuous loop_start=20 loop_end=49\n";
    let inst = parse(text, &dir);
    let zone = inst
        .groups
        .first()
        .and_then(|g| g.zones.first())
        .expect("zone");
    assert_eq!((zone.start, zone.end), (10, 100), "SFZ end is inclusive");
    assert_eq!(
        zone.loops,
        vec![Loop {
            start: 20,
            end: 50,
            ..Loop::default()
        }]
    );
}

#[test]
fn loop_sustain_loops_until_release() {
    let dir = scratch("sustain");
    wav(&dir.join("a.wav"), 1000).expect("wav");
    let text = "<region> sample=a.wav loop_mode=loop_sustain loop_start=20 loop_end=49\n";
    let inst = parse(text, &dir);
    let zone = inst
        .groups
        .first()
        .and_then(|g| g.zones.first())
        .expect("zone");
    assert_eq!(
        zone.loops,
        vec![Loop {
            start: 20,
            end: 50,
            until_release: true,
            ..Loop::default()
        }]
    );
}

#[test]
fn unknown_opcodes_are_collected_not_fatal() {
    let dir = scratch("unknown");
    wav(&dir.join("a.wav"), 10).expect("wav");
    let inst = parse("<region> sample=a.wav lorand=0 hirand=0.5\n", &dir);
    let lorand = inst
        .diagnostics
        .iter()
        .find(|d| d.feature == "opcode lorand");
    assert_eq!(lorand.map(|d| d.impact), Some(Impact::Audible));
    assert_eq!(
        inst.groups.first().map(|g| g.zones.len()),
        Some(1),
        "the zone still plays"
    );
}

#[test]
fn missing_sample_is_reported_and_kept() {
    let dir = scratch("missing");
    let inst = parse("<region> sample=gone.wav\n", &dir);
    let report = inst
        .diagnostics
        .iter()
        .find(|d| d.feature == "samples not found");
    assert_eq!(report.map(|d| d.impact), Some(Impact::Broken));
    assert_eq!(inst.samples.first().map(|s| s.frames), Some(0));
    assert_eq!(inst.groups.first().map(|g| g.zones.len()), Some(1));
}

#[test]
fn group_numbers_become_voice_groups_with_their_limits_and_off_modes() {
    let dir = scratch("voice_groups");
    wav(&dir.join("a.wav"), 1000).expect("wav");
    // An open hat (group 1, choked by 2, released when choked), a closed hat
    // (group 2, two voices, one per key, selfmask off, 50 ms off), and an
    // ungrouped region whose polyphony stays its own.
    let text = "\
<region> sample=a.wav key=46 group=1 off_by=2 off_mode=normal
<region> sample=a.wav key=42 group=2 polyphony=2 note_polyphony=1 note_selfmask=off off_time=0.05
<region> sample=a.wav key=60 polyphony=3
";
    let inst = parse(text, &dir);
    let [open, closed] = inst.voice_groups.as_slice() else {
        panic!("{:?}", inst.voice_groups)
    };
    assert!(open.release && open.voices == u16::MAX);
    assert_eq!(
        (closed.voices, closed.per_key, closed.quieter_only),
        (2, Some(1), false)
    );
    assert!(!closed.release && near(closed.fade_ms, 50.0));
    let groups: Vec<_> = (inst.groups.iter())
        .map(|g| (g.voice_group, g.off_by, g.polyphony))
        .collect();
    assert_eq!(
        groups,
        [
            (Some(0), Some(1), None),
            (Some(1), None, None),
            (None, None, Some(3))
        ]
    );
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
}

#[test]
fn probes_flac_and_aiff_samples_without_hound() {
    let dir = scratch("flac_aiff");
    flac(&dir.join("pad.flac"), 48_000).expect("flac");
    aiff(&dir.join("bell.aiff"), 22_050).expect("aiff");
    let text = "\
<region> sample=pad.flac key=c4
<region> sample=bell.aiff key=d4
";
    let inst = parse(text, &dir);
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
    assert_eq!(inst.samples.len(), 2);
    assert_eq!(
        (
            inst.samples[0].channels,
            inst.samples[0].rate,
            inst.samples[0].frames
        ),
        (2, 48_000, 48_000)
    );
    assert_eq!(
        (
            inst.samples[1].channels,
            inst.samples[1].rate,
            inst.samples[1].frames
        ),
        (2, 44_100, 22_050)
    );
}

#[test]
fn embedded_wav_smpl_loop_is_used_when_loop_mode_omits_points() {
    let dir = scratch("smpl_loop");
    wav_with_smpl(&dir.join("looped.wav"), 1_000, Some((120, 619))).expect("wav");
    let inst = parse("<region> sample=looped.wav loop_mode=loop_sustain\n", &dir);
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
    let zone = &inst.groups[0].zones[0];
    assert_eq!(
        zone.loops,
        vec![Loop {
            start: 120,
            end: 620,
            until_release: true,
            ..Loop::default()
        }]
    );
}

#[test]
fn large_region_group_dedup_is_linear_and_exact() {
    let dir = scratch("large_dedup");
    wav(&dir.join("a.wav"), 100).expect("wav");
    // 4,000 regions across 2,000 unique attack settings (each repeated twice).
    let mut text = String::with_capacity(200_000);
    for i in 0..4_000u32 {
        let bucket = i % 2_000;
        let key = (i % 128) as u8;
        text.push_str(&format!(
            "<region> sample=a.wav key={key} ampeg_attack={:.4}\n",
            f64::from(bucket) * 0.0005
        ));
    }
    let inst = parse(&text, &dir);
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
    assert_eq!(inst.samples.len(), 1);
    assert_eq!(inst.groups.len(), 2_000);
    assert_eq!(
        inst.groups.iter().map(|g| g.zones.len()).sum::<usize>(),
        4_000
    );
    assert!(inst.groups.iter().all(|g| g.zones.len() == 2));
}
