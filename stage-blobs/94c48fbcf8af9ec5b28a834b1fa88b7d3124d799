use super::*;
use crate::{audio::Sample, engine::{Bank, Engine, HostNote, HostPattern},
    import::{Group, Instrument, Zone}};

fn performance(partitions: &[usize]) -> [Vec<f32>; 2] {
    const WAIT: usize = 128;
    const OFF: usize = 1801;
    const END: usize = 4096;
    let dir = tempfile::tempdir().unwrap();
    let paths = [dir.path().join("old.wav"), dir.path().join("new.wav")];
    let frames: [Vec<_>; 2] = std::array::from_fn(|sample| (0..131072).map(|i| {
        if sample == 0 { [0.4, -0.3] }
        else { [0.12 * (i as f32 * 0.037).sin(), 0.09 * (i as f32 * 0.051).cos()] }
    }).collect());
    for (path, data) in paths.iter().zip(&frames) {
        let mut wav = hound::WavWriter::create(path, hound::WavSpec {
            channels: 2, sample_rate: 48000, bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        }).unwrap();
        for frame in data { for &value in frame { wav.write_sample(value).unwrap(); } }
        wav.finalize().unwrap();
    }
    let instrument = Instrument { groups: vec![Group::default()],
        zones: [60, 64].into_iter().zip(&paths).map(|(key, path)| Zone {
            sample: path.clone(), root: key, low_key: key, high_key: key,
            ..Default::default()
        }).collect(), ..Default::default() };
    let bank = Bank::load_bare(&instrument).unwrap();
    let new_sample = bank.plays[1].sample;
    let new_map = bank.plays[1].map;
    let mut worker = ManualWorker::paused(&bank);
    let ram = Bank::from_samples(instrument.groups.clone(), instrument.zones.clone(),
        paths.into_iter().zip(frames).map(|(path, frames)| (path, Sample { rate: 48000, frames })).collect()).unwrap();
    let (mut actual, mut oracle) = (Engine::default(), Engine::default());
    for e in [&mut actual, &mut oracle] { e.reset(48000.); e.attack = 0.007; e.release = 0.005; }
    actual.set_bank(Some(Box::new(bank))); oracle.set_bank(Some(Box::new(ram)));
    let old = HostNote { port: 0, channel: 0, key: 60, id: 901, clap: true };
    let new = HostNote { key: 64, id: 902, ..old };
    let pattern = |note: HostNote| HostPattern { port: 0, channel: 0,
        key: note.key as i32, id: note.id, clap: true };
    assert_eq!(crate::plugin::tests::allocations(|| {
        assert!(actual.host_note_on(old, 0, 60, 127, 0.));
        actual.render(&mut [0.; 1], &mut [0.; 1]);
    }), 0);
    let stream = actual.player.voices[0].stream.unwrap();
    let shared = worker.shared.clone();
    let slot = &shared.slots[stream.slot as usize];
    let old_seq = slot.seq.load(Ordering::Acquire);
    let gate = Arc::new(AtomicU32::new(0));
    worker.worker.before_dense_publish = Some(gate.clone());
    let mut pcm = [vec![0.; END], vec![0.; END]];
    let mut expected = [vec![0.; END], vec![0.; END]];
    std::thread::scope(|scope| {
        // Exactly one serve: it cannot start the new cursor before inspection.
        let producer = scope.spawn(|| worker.worker.serve(slot,
            &mut worker.cursors[stream.slot as usize], RING, &shared.rings, &worker.sources));
        let deadline = Instant::now() + Duration::from_secs(5);
        while gate.load(Ordering::Acquire) != 1 {
            assert!(Instant::now() < deadline, "old disk decode reaches pre-publication gate");
            std::thread::yield_now();
        }
        // Stop the real waiting old owner, then recycle the same actual slot.
        // Include stop and the final real onset in exactly65536 config writes.
        assert_eq!(crate::plugin::tests::allocations(|| {
            actual.host_note_off(pattern(old));
            actual.render(&mut [0.; 1], &mut [0.; 1]);
            assert_eq!(actual.active_voices(), 0);
            let changes = slot.seq.load(Ordering::Acquire).wrapping_sub(old_seq) / 2;
            assert!(changes < 65535);
            for _ in changes..65535 { slot.configure(new_sample, &new_map, 0, 0, 0); }
            assert!(actual.host_note_on(new, 0, 64, 127, 0.));
        }), 0);
        let new_stream = actual.player.voices[0].stream.unwrap();
        assert_eq!(new_stream.slot, stream.slot);
        assert_eq!(new_stream.tag, stream.tag, "the packed tag genuinely aliases");
        assert_eq!(slot.seq.load(Ordering::Acquire), old_seq.wrapping_add(65536 * 2));
        gate.store(2, Ordering::Release);
        assert!(producer.join().unwrap());
    });
    assert_eq!(slot.dense_published_seq.load(Ordering::Acquire), old_seq,
        "the actual old producer published after the new configure cleared written");
    assert_eq!(slot.published(stream.tag), None, "full generation rejects the aliased old PCM");
    assert_eq!(slot.ready(stream.tag, 132, false), Ok(false));
    assert_eq!(crate::plugin::tests::allocations(|| {
        let [left, right] = &mut pcm;
        actual.render(left.get_mut(..WAIT).unwrap(), right.get_mut(..WAIT).unwrap());
        assert!(actual.player.voices[0].onset == super::super::super::voice::Onset::Waiting);
        assert_eq!(actual.player.voices[0].pos, 0.);
        assert_eq!(actual.player.voices[0].env.level(), 0.);
        assert!(left[..WAIT].iter().chain(&right[..WAIT]).all(|&x| x == 0.));
        // Independent RAM owner starts only when actual new disk data is ready.
        oracle.render(&mut [0.; 128], &mut [0.; 128]);
        oracle.render(&mut [0.; 2], &mut [0.; 2]);
        assert_eq!(oracle.player.now, actual.player.now);
    }), 0);
    worker.serve(stream.slot); // Real fresh generation decodes the second WAV.
    assert_eq!(slot.dense_published_seq.load(Ordering::Acquire), slot.seq.load(Ordering::Acquire));
    assert!(slot.published(stream.tag).unwrap() >= 4096);
    let mut frame = WAIT; let mut chunk = 0;
    assert_eq!(crate::plugin::tests::allocations(|| {
        assert!(oracle.host_note_on(new, 0, 64, 127, 0.));
        for stop in [OFF, END] {
            while frame < stop {
                let n = partitions[chunk % partitions.len()].min(stop - frame);
                let [left, right] = &mut pcm; let [ol, or] = &mut expected;
                actual.render_checked(&mut left[frame..frame+n], &mut right[frame..frame+n]).unwrap();
                oracle.render_checked(&mut ol[frame..frame+n], &mut or[frame..frame+n]).unwrap();
                frame += n; chunk += 1;
            }
            if stop == OFF { actual.host_note_off(pattern(new)); oracle.host_note_off(pattern(new)); }
        }
        actual.mark_host_notes(); oracle.mark_host_notes();
        assert!(!actual.host_note_pending(old) && !actual.host_note_pending(new));
        assert_eq!(actual.host_note_end_at(old), Some(1));
        assert_eq!(actual.host_note_end_at(new), oracle.host_note_end_at(new));
        assert!(actual.host_note_end_at(new).is_some());
        assert_eq!(actual.active_voices(), 0); assert_eq!(oracle.active_voices(), 0);
    }), 0);
    assert_eq!(actual.underruns(), 0);
    for channel in 0..2 {
        let error = pcm[channel].iter().zip(&expected[channel])
            .fold(0_f32, |err, (a, b)| err.max((a - b).abs()));
        assert!(error < 1e-6, "aliased old producer changed full new-owner PCM by {error}");
        assert!(expected[channel][WAIT..OFF].iter().any(|x| x.abs() > 0.01));
        assert!(pcm[channel][END-512..].iter().all(|&x| x == 0.));
    }
    pcm
}

#[test]
fn old_dense_producer_after_u16_tag_wrap_cannot_start_or_corrupt_new_owner_pcm() {
    let fixed = performance(&[128]);
    let fragmented = performance(&[1, 7, 31, 127]);
    let error = fixed.iter().flatten().zip(fragmented.iter().flatten())
        .fold(0_f32, |err, (a, b)| err.max((a - b).abs()));
    assert!(error < 2e-5, "host partitions changed fresh-generation PCM by {error}");
}
