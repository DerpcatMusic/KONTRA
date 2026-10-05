use super::*;
use crate::{audio::Sample, engine::{Engine, HostNote, HostPattern},
    import::{Group, Instrument, Wavetable, Zone}, ksp::engine::{ZoneEdit, ZonePar}};

// Real worker-head preparation against the original ordinary map, followed by
// the same remap/rebased preload sequence used by the plugin worker. The RAM
// oracle authors the resulting WT zones directly, without a resize or rebase.
fn performance(partitions: &[usize]) -> [Vec<f32>; 2] {
    const FRAMES: usize = 131072;
    const OFF: usize = 1801;
    const END: usize = 4096;
    let dir = tempfile::tempdir().unwrap();
    let paths = [dir.path().join("tables.wav"), dir.path().join("ordinary.wav")];
    let frames: Vec<_> = (0..FRAMES).map(|i| {
        let phase = std::f32::consts::TAU * (i % 2048) as f32 / 2048.;
        [0.08 * phase.sin() + 0.02 * (2. * phase).cos() * (i / 2048 + 1) as f32 / 64.,
            0.07 * phase.cos() - 0.01 * (3. * phase).sin()]
    }).collect();
    for path in &paths {
        let mut out = hound::WavWriter::create(path, hound::WavSpec {
            channels: 2, sample_rate: 48000, bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        }).unwrap();
        for frame in &frames { for &value in frame { out.write_sample(value).unwrap(); } }
        out.finalize().unwrap();
    }
    let groups = vec![Group { source_mode: Some(9), wavetable: Some(Wavetable::default()),
        ..Default::default() }, Group { source_mode: Some(0), ..Default::default() }];
    // Zone ends are signed offsets from EOF; each table window is 4096 frames.
    let zones = vec![
        Zone { sample: paths[0].clone(), group: 1, low_key: 60, high_key: 60,
            end: 4096 - FRAMES as i32, pan: -1., ..Default::default() },
        Zone { sample: paths[0].clone(), group: 1, low_key: 64, high_key: 64,
            start: 65536, end: 69632 - FRAMES as i32, pan: 1., ..Default::default() },
        Zone { sample: paths[1].clone(), group: 1, low_key: 65, high_key: 65,
            ..Default::default() },
    ];
    let instrument = Instrument { path: dir.path().join("heads.nki"),
        groups: groups.clone(), zones: zones.clone(), ..Default::default() };
    let budget = Streamer::BYTES + 3 * 4096 * 8;
    let mut initial = Bank::load_within(&instrument, budget).unwrap();
    let mut residency = initial.take_residency().expect("actual streamed head manager");
    assert_eq!(initial.streamed_samples(), 2);
    let table_sample = initial.plays[0].sample;
    let normal_sample = initial.plays[2].sample;
    let table_tracker = residency.samples.iter().position(|s| s.sample == table_sample).unwrap();
    let normal_tracker = residency.samples.iter().position(|s| s.sample == normal_sample).unwrap();
    let mut stale = residency.samples[table_tracker].read(
        residency.sources[table_sample as usize].as_ref().unwrap(), Tier::Cold, residency.cover).unwrap();
    let normal = residency.samples[normal_tracker].read(
        residency.sources[normal_sample as usize].as_ref().unwrap(), Tier::Cold, residency.cover).unwrap();
    assert!(stale.spans.iter().all(|s| s.end() < 69632));
    let make_engine = |bank| {
        let mut e = Engine::default(); e.reset(48000.); e.attack = 0.; e.release = 0.002;
        e.set_bank(Some(Box::new(bank))); e
    };
    let mut actual = make_engine(initial);
    let jobs: Vec<_> = [0, 1].map(|zone| actual.bank().unwrap().zone_job(ZoneEdit {
        zone, par: ZonePar::Group, value: 0, slot: 0, id: zone as i32 + 1,
    }).unwrap()).into();
    let context = jobs[0].context.clone();
    let base = jobs[0].base.clone();
    let mut prepared = crate::engine::zone::Prepared::build(jobs, base, &|| false);
    assert!(prepared.success, "{:?}", prepared.error);
    let mut install = || assert!(actual.install_zone_map(&mut prepared));
    #[cfg(feature = "plugin")]
    assert_eq!(crate::plugin::tests::allocations(&mut install), 0);
    #[cfg(not(feature = "plugin"))]
    install();
    drop(prepared); // Worker retirement, outside the callback allocation guard.
    let parent = actual.bank().unwrap().zone_state.as_ref().unwrap().clone();
    let mut incoming = Box::new(Bank::load_within(&instrument, budget).unwrap());
    incoming.rebase_zone_preload(&context, parent, &|| false).unwrap();
    let mut retired = None;
    let upgrade = || retired = actual.upgrade_bank(incoming);
    #[cfg(feature = "plugin")]
    assert_eq!(crate::plugin::tests::allocations(upgrade), 0);
    #[cfg(not(feature = "plugin"))]
    upgrade();
    assert!(retired.is_some()); drop(retired);
    assert_eq!(Arc::strong_count(actual.bank().unwrap().zone_state.as_ref().unwrap()), 1,
        "rebased snapshot is writable: rejection must come from the table dependency");
    // A head covering only the first table also cannot revoke the second one.
    residency.samples[table_tracker].plays = vec![(actual.bank().unwrap().plays[0], (0, 0))];
    let mut first_only = residency.samples[table_tracker].read(
        residency.sources[table_sample as usize].as_ref().unwrap(), Tier::Cold, residency.cover).unwrap();
    assert_eq!(first_only.spans.len(), 1); assert_eq!(first_only.spans[0].end(), 4096);
    residency.samples[table_tracker].plays = actual.bank().unwrap().plays.iter()
        .filter(|p| p.sample == table_sample).map(|p| (*p, (0, 0))).collect();
    let complete = residency.samples[table_tracker].read(
        residency.sources[table_sample as usize].as_ref().unwrap(), Tier::Cold, residency.cover).unwrap();
    let retained = actual.bank().unwrap().samples[table_sample as usize].spans.last().unwrap().data.clone();
    let stale_ptr = stale.spans.as_ptr(); let first_ptr = first_only.spans.as_ptr();
    let mut resize = || {
        actual.swap_heads(std::slice::from_mut(&mut stale));
        actual.swap_heads(std::slice::from_mut(&mut first_only));
    };
    #[cfg(feature = "plugin")]
    assert_eq!(crate::plugin::tests::allocations(&mut resize), 0);
    #[cfg(not(feature = "plugin"))]
    resize();
    assert!(!stale.applied && !first_only.applied);
    assert_eq!(stale_ptr, stale.spans.as_ptr()); assert_eq!(first_ptr, first_only.spans.as_ptr());
    assert!(actual.bank().unwrap().samples[table_sample as usize].spans.iter()
        .any(|s| Arc::ptr_eq(&retained, &s.data)), "rejected packets retain current PCM");
    let mut reference_zones = zones; reference_zones[0].group = 0; reference_zones[1].group = 0;
    let mut oracle = make_engine(Bank::from_samples(groups, reference_zones,
        paths.iter().map(|p| (p.clone(), Sample { rate: 48000, frames: frames.clone() })).collect()).unwrap());
    let notes = [60, 64].map(|key| HostNote { port: 0, channel: 0, key, id: key as i32, clap: true });
    let mut pcm = [vec![0.; END], vec![0.; END]];
    let mut expected = [vec![0.; END], vec![0.; END]];
    let mut frame = 0; let mut chunk = 0;
    let mut audio = || {
        for e in [&mut actual, &mut oracle] {
            for owner in notes { assert!(e.host_note_on(owner, 0, owner.key, 100, 0.)); }
        }
        for stop in [OFF, END] {
            while frame < stop {
                let n = partitions[chunk % partitions.len()].min(stop - frame);
                let [left, right] = &mut pcm; let [ol, or] = &mut expected;
                actual.render(&mut left[frame..frame+n], &mut right[frame..frame+n]);
                oracle.render(&mut ol[frame..frame+n], &mut or[frame..frame+n]);
                frame += n; chunk += 1;
            }
            if stop == OFF { for e in [&mut actual, &mut oracle] { for owner in notes {
                e.host_note_off(HostPattern { port: 0, channel: 0, key: owner.key as i32,
                    id: owner.id, clap: true });
            } } }
        }
        actual.mark_host_notes(); oracle.mark_host_notes();
        for owner in notes {
            assert!(!actual.host_note_pending(owner));
            assert_eq!(actual.host_note_end_at(owner), oracle.host_note_end_at(owner));
            assert!(actual.host_note_end_at(owner).is_some());
        }
        assert_eq!(actual.active_voices(), 0); assert_eq!(oracle.active_voices(), 0);
    };
    #[cfg(feature = "plugin")]
    assert_eq!(crate::plugin::tests::allocations(&mut audio), 0);
    #[cfg(not(feature = "plugin"))]
    audio();
    for channel in 0..2 {
        assert!(expected[channel][..OFF].iter().any(|x| x.abs() > 0.01));
        let error = pcm[channel].iter().zip(&expected[channel])
            .fold(0_f32, |err, (a, b)| err.max((a - b).abs()));
        assert!(error < 1e-6, "complete remapped WT PCM differs from RAM by {error}");
        assert!(pcm[channel][END-512..].iter().all(|&x| x == 0.));
    }
    // Harmless complete tables and the unrelated ordinary sample remain eligible.
    let mut heads = [complete, normal];
    let mut valid = || actual.swap_heads(&mut heads);
    #[cfg(feature = "plugin")]
    assert_eq!(crate::plugin::tests::allocations(&mut valid), 0);
    #[cfg(not(feature = "plugin"))]
    valid();
    assert!(heads.iter().all(|h| h.applied));
    for play in actual.bank().unwrap().plays.iter().filter(|p| p.wavetable.is_some()) {
        assert!(actual.bank().unwrap().samples[play.sample as usize].wavetable_span(play.wavetable.unwrap()).is_some());
    }
    // All returned/new/replaced packet data retires here, off audio.
    pcm
}

#[test]
fn rebased_preload_heads_preserve_all_remapped_wavetables_and_complete_pcm_without_heap() {
    let fixed = performance(&[128]);
    let fragmented = performance(&[1, 7, 31, 127]);
    let error = fixed.iter().flatten().zip(fragmented.iter().flatten())
        .fold(0_f32, |err, (a, b)| err.max((a - b).abs()));
    assert!(error < 2e-5, "host partitions changed complete WT PCM by {error}");
}
