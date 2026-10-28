//! SFZ text to the instrument model. Sample files are silent WAVs written
//! into the target dir here, stand-ins for real ones.

use std::path::{Path, PathBuf};

use kontra_format_sfz::parse;
use kontra_ir::{self as ir, Impact, Loop, StartRule};

/// A fresh folder for one test, under the target dir.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    // A folder that cannot be made fails the test at its first write.
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// A silent mono WAV of `frames` frames at `path`, its folder made as needed.
fn wav(path: &Path, frames: u32) -> Result<(), hound::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for _ in 0..frames {
        writer.write_sample(0_i16)?;
    }
    writer.finalize()
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
        zone.looped,
        Some(Loop {
            start: 20,
            end: 50,
            crossfade: 0
        })
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
    let report = inst.diagnostics.iter().find(|d| d.feature == "sample file");
    assert_eq!(report.map(|d| d.impact), Some(Impact::Broken));
    assert_eq!(inst.samples.first().map(|s| s.frames), Some(0));
    assert_eq!(inst.groups.first().map(|g| g.zones.len()), Some(1));
}
