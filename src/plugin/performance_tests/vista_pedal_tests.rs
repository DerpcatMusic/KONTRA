//! Licensed Vista functional guard through actual Sampler ingress and native clocks.
//! Offline blocking streams establish source availability, not realtime throughput.
use super::*;
use crate::{
    engine::{Streaming, effects, load_scripts},
    import::Instrument,
};
use std::sync::atomic::AtomicU32;

fn setup(instrument: &Instrument, cc1: u8, partition: &'static [usize]) -> Performance {
    let mut p = Performance::new(false, partition);
    p.process_mode = moose::core::ProcessMode::Offline;
    let bank = Bank::load_counting(
        instrument,
        1 << 30,
        Streaming::Auto,
        &[(1, cc1), (100, cc1)],
        &AtomicU32::new(0),
    )
    .unwrap();
    assert!(bank.bytes <= 1 << 30, "bank exceeded bounded source budget");
    p.stages.push(
        json!({"load": {"bytes": bank.bytes, "preload": bank.preload,
        "streamed_samples": bank.streamed_samples(), "skipped_zones": bank.skipped_zones,
        "issues": bank.issues, "warning": bank.warning}}),
    );
    let (runtime, errors) = load_scripts(instrument, instrument.script_state.clone(), RATE as f64);
    assert!(errors.is_empty(), "{}: {errors:?}", instrument.name);
    assert!(runtime.is_some(), "authored scripts must run");
    let fx = effects(instrument, runtime.as_deref(), RATE as f32);
    let engine = p.dsp.rack.parts[0].instrument_mut().unwrap();
    engine.set_bank(Some(Box::new(bank)));
    engine.set_fx(fx);
    engine.set_script(runtime);
    engine.blocking_streams = true;
    p
}

fn snapshot(
    p: &mut Performance,
    instrument: &Instrument,
    label: &str,
) -> Vec<(u32, u32, u8, u8, u32, bool)> {
    // Sampler drains the Engine source latch into these sticky journal owners.
    // Read/report outside the callback; a drained Engine cell is not success.
    let journal=p.params.diagnostic_report();
    p.stages.push(json!({"source_checkpoint":label,"journal":journal,
        "sticky_stream_failure":format!("{:?}",p.dsp.stream_failure),
        "sticky_part_stream_errors":format!("{:?}",p.dsp.stream_errors)}));
    if p.dsp.stream_failure.is_some() {
        save_pcm(p,"vista-failed-source");
        let dir=PathBuf::from(std::env::var_os("KONTRA_PERFORMANCE_ARTIFACT_DIR").unwrap());
        std::fs::write(dir.join("vista-failed-source.json"),serde_json::to_vec_pretty(&json!({
            "instrument":instrument.path,"input":p.input,"stages":p.stages,
        })).unwrap()).unwrap();
    }
    assert!(p.dsp.stream_failure.is_none() && p.dsp.stream_errors.iter().all(Option::is_none),
        "{label}: sticky source failure {:?}; part errors {:?}",p.dsp.stream_failure,p.dsp.stream_errors);
    let engine = p.dsp.rack.parts[0].instrument().unwrap();
    assert!(
        engine.take_stream_error().is_none(),
        "{label}: streaming failure"
    );
    assert_eq!(
        engine.underruns(),
        0,
        "{label}: offline sources unavailable"
    );
    assert_eq!(
        engine.dropped_commands(),
        0,
        "{label}: authored commands dropped"
    );
    let rows = crate::engine::vista_pedal_probe::zones(engine);
    let selected: Vec<_> = rows.iter().map(|&(zone_id,group,key,velocity,event,released)| {
        let z = &instrument.zones[zone_id as usize];
        assert_eq!(z.group as u32, group, "zone identity/group drift");
        assert!((z.low_key..=z.high_key).contains(&key));
        assert!((z.low_velocity..=z.high_velocity).contains(&velocity));
        json!({"zone_id":zone_id,"group":group,"group_name":instrument.groups[group as usize].name,
            "sample":z.sample,"key":key,"velocity":velocity,"event":event,"released":released})
    }).collect();
    eprintln!(
        "{} {label} frame={} selected={selected:?} diagnostics={:?}",
        instrument.name,
        p.pcm.len(),
        engine.script().unwrap().diagnostics()
    );
    p.stages.push(
        json!({"checkpoint":label,"frame":p.pcm.len(),"voices":selected,
        "diagnostics":engine.script().unwrap().diagnostics()}),
    );
    rows
}

fn family(group: u32, base: u32) -> bool {
    (base..base + 4).contains(&group) || (base + 32..base + 36).contains(&group)
}

fn audible(p: &mut Performance, name: &str, wire: &EventList, frames: usize, settle: usize) {
    let level = p.stage_for(
        name,
        "authored sustained PCM remains audible",
        wire,
        frames,
        settle,
    );
    if !level.into_iter().all(|x| x.is_finite() && x > 1e-7) {
        save_pcm(p, &format!("vista-failed-{name}"));
    }
    assert!(
        level.into_iter().all(|x| x.is_finite() && x > 1e-7),
        "{name}: silent stereo {level:?}"
    );
}

fn performance(
    instrument: &Instrument,
    cc1: u8,
    key: i16,
    partition: &'static [usize],
) -> Performance {
    let mut p = setup(instrument, cc1, partition);
    let mut wire = EventList::with_capacity(8);
    cc(&mut wire, 0, 1, cc1);
    cc(&mut wire, 0, 64, 127);
    note(&mut wire, ExactNoteKind::On, 100, 0, key);
    p.stage_for(
        "pedal-before-on",
        "attack and sustain children",
        &wire,
        512,
        0,
    );
    let first = snapshot(&mut p, instrument, "first-on");
    let mut groups:Vec<_>=first.iter().map(|r|r.1).collect();groups.sort_unstable();
    assert_eq!(groups,(0..8).chain(32..40).collect::<Vec<_>>(),
        "all authored first-on attack+sustain dynamic layers must instantiate");
    assert!(
        first.iter().any(|r| family(r.1, 0)),
        "missing authored attack"
    );
    assert!(
        first.iter().any(|r| family(r.1, 4)),
        "missing authored sustain"
    );
    assert!(
        first
            .iter()
            .all(|r| (family(r.1, 0) || family(r.1, 4)) && r.2 == key as u8)
    );
    wire.clear();
    audible(&mut p, "late-first-sustain", &wire, 2 * RATE - 512, RATE);

    // Independent benign-control reference: pedal before the FIRST held On
    // does not change this authored attack+sustain branch or its complete PCM.
    let mut reference = setup(instrument, cc1, partition);
    cc(&mut wire, 0, 1, cc1);
    note(&mut wire, ExactNoteKind::On, 100, 0, key);
    reference.stage_for(
        "first-on-no-pedal",
        "same authored first-on branch",
        &wire,
        512,
        0,
    );
    wire.clear();
    reference.stage_for(
        "first-on-reference-tail",
        "held first-on",
        &wire,
        2 * RATE - 512,
        RATE,
    );
    assert_eq!(p.pcm.len(), reference.pcm.len());
    let error = p
        .pcm
        .iter()
        .zip(&reference.pcm)
        .flat_map(|(a, b)| (0..2).map(move |c| (a[c] - b[c]).abs()))
        .fold(0_f32, f32::max);
    assert!(
        error <= 2e-6,
        "CC64 before first On changed complete authored PCM: {error}"
    );
    p.stages
        .push(json!({"first_on_no_pedal_complete_pcm_max_error":error}));
    drop(reference);

    note(&mut wire, ExactNoteKind::Off, 100, 0, key);
    audible(&mut p, "first-off-pedal-held", &wire, RATE, RATE / 2);
    let held = snapshot(&mut p, instrument, "first-off-held");
    assert!(
        held.iter().any(|r| family(r.1, 4) && !r.5),
        "pedal-held sustain retired"
    );
    wire.clear();
    note(&mut wire, ExactNoteKind::On, 101, 0, key + 2);
    p.stage_for(
        "nearby-on-under-pedal",
        "authored rising interval2",
        &wire,
        512,
        0,
    );
    let nearby = snapshot(&mut p, instrument, "nearby-on");
    let new: Vec<_> = nearby
        .iter()
        .filter(|r| !first.iter().any(|f| f.4 == r.4))
        .collect();
    assert!(
        new.iter().any(|r| family(r.1, 16)),
        "missing authored legato child"
    );
    assert!(
        new.iter().any(|r| family(r.1, 20)),
        "missing authored piSustain child"
    );
    assert!(
        new.iter()
            .all(|r| (family(r.1, 16) || family(r.1, 20)) && r.2 == key as u8 && r.3 == 18),
        "rising interval2 must select old-note/velocity18 legato zones: {new:?}"
    );
    wire.clear();
    audible(&mut p, "late-nearby-sustain", &wire, 2 * RATE - 512, RATE);
    note(&mut wire, ExactNoteKind::Off, 101, 0, key + 2);
    audible(&mut p, "nearby-off-pedal-held", &wire, RATE, RATE / 2);
    let pre_up = snapshot(&mut p, instrument, "nearby-off-held");
    let old: Vec<_> = pre_up.iter().map(|r| r.4).collect();
    wire.clear();
    cc(&mut wire, 0, 64, 0);
    p.stage_for(
        "pedal-up",
        "authored release mask and150ms fade",
        &wire,
        512,
        0,
    );
    let up = snapshot(&mut p, instrument, "pedal-up");
    assert!(
        up.iter().any(|r| family(r.1, 28) && !old.contains(&r.4)),
        "missing authored pedal-up release"
    );
    wire.clear();
    p.stage_for(
        "authored-fade-complete",
        "150ms source children retire",
        &wire,
        RATE / 2 - 512,
        0,
    );
    let faded = snapshot(&mut p, instrument, "fade-complete");
    assert!(
        faded.iter().all(|r| !old.contains(&r.4)),
        "old pedal-held child survived authored stop fade"
    );
    p.stage_for(
        "natural-release-tail",
        "release samples end without panic",
        &wire,
        20 * RATE,
        19 * RATE,
    );
    assert!(
        snapshot(&mut p, instrument, "natural-end").is_empty(),
        "stuck authored release voice"
    );
    assert!(
        p.pcm.iter().flatten().all(|x| x.is_finite()),
        "nonfinite complete PCM"
    );
    assert!(
        p.ended.contains(&address(100, 0, key)) && p.ended.contains(&address(101, 0, key + 2)),
        "physical input roots did not complete"
    );
    p
}

fn save_pcm(p: &Performance, stem: &str) {
    let dir = PathBuf::from(
        std::env::var_os("KONTRA_PERFORMANCE_ARTIFACT_DIR").expect("set artifact directory"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let mut wav = hound::WavWriter::create(
        dir.join(format!("{stem}.wav")),
        hound::WavSpec {
            channels: 2,
            sample_rate: RATE as u32,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for frame in &p.pcm {
        for sample in frame {
            wav.write_sample(*sample).unwrap();
        }
    }
    wav.finalize().unwrap();
}

#[test]
#[ignore = "requires licensed Vista Cellos/Basses NKI paths and sample library"]
fn real_vista_pedal_before_on_and_held_off_nearby_on_keep_authored_sustain_pcm_and_release() {
    for (label, env, key) in [
        ("cellos", "KONTAKTO_VISTA_CELLOS_NKI", 60),
        ("basses", "KONTAKTO_VISTA_BASSES_NKI", 58),
    ] {
        let path = PathBuf::from(std::env::var_os(env).unwrap_or_else(|| panic!("set {env}")));
        let instrument = crate::import::read(&path).unwrap();
        assert!(
            instrument.missing_samples.is_empty(),
            "missing actual samples: {:?}",
            instrument.missing_samples
        );
        assert_eq!(
            instrument.groups.len(),
            64,
            "fixture needs actual Vista group layout"
        );
        for cc1 in [32, 64, 96, 127] {
            for (block, partition) in [("128", &[128][..]), ("fragmented", &[7, 31, 113, 64][..])] {
                eprintln!("VISTA functional case: {label} cc1={cc1} partition={block} key={key}");
                let p = performance(&instrument, cc1, key, partition);
                let dir = PathBuf::from(
                    std::env::var_os("KONTRA_PERFORMANCE_ARTIFACT_DIR")
                        .expect("set artifact directory"),
                );
                std::fs::create_dir_all(&dir).unwrap();
                let stem = format!("vista-{label}-cc1-{cc1}-{block}");
                save_pcm(&p, &stem);
                std::fs::write(dir.join(format!("{stem}.json")),serde_json::to_vec_pretty(&json!({
                    "instrument":path,"mode":"offline blocking Auto<=1Gi; functional guard, not native-host parity or realtime throughput",
                    "sample_rate":RATE,"partition":partition,"input":p.input,"stages":p.stages,
                    "complete_frames":p.pcm.len(),"note_ends":p.ended.iter().zip(&p.ended_frames).map(|(a,f)|json!({"address":format!("{a:?}"),"frame":f})).collect::<Vec<_>>(),
                })).unwrap()).unwrap();
            }
        }
    }
}
