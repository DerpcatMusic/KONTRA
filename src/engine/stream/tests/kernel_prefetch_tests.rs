use super::*;
use crate::{audio::Sample, engine::{Bank, Engine, HostNote, HostPattern},
    import::{Group, Instrument, Zone}};

fn performance(quality: i32, partitions: &[usize]) -> [Vec<f32>; 2] {
    const WAIT: usize = 128;
    const STARTED: usize = WAIT + 128;
    const RECOVER: usize = STARTED + 1;
    const AUDIO_END: usize = WAIT + 131072 / 64;
    const OFF: usize = 2301;
    const END: usize = 3072;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kernel-prefetch.wav");
    // Source bandwidth and unequal channels make the wide convolution audible.
    let frames: Vec<_> = (0..131072).map(|i| {
        [0.16 * (i as f32 * 0.017).sin() + 0.025 * (i as f32 * 0.79).cos(),
         0.13 * (i as f32 * 0.021).cos() - 0.031 * (i as f32 * 0.63).sin()]
    }).collect();
    let mut wav = hound::WavWriter::create(&path, hound::WavSpec {
        channels: 2, sample_rate: 48000, bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    }).unwrap();
    for frame in &frames { for &value in frame { wav.write_sample(value).unwrap(); } }
    wav.finalize().unwrap();
    let instrument = Instrument {
        sampling_profile: crate::import::SamplingProfile::Kontakt,
        groups: vec![Group { source_mode: Some(0), interp_quality: quality, ..Default::default() }],
        // Zone tuning stores a linear ratio:64 is six octaves at equal rates.
        zones: vec![Zone { sample: path.clone(), tune: 64., ..Default::default() }],
        ..Default::default()
    };
    let mut bank = Bank::load_bare(&instrument).unwrap();
    let mut worker = ManualWorker::paused(&bank);
    let mut resident = Bank::from_samples_with_profile(instrument.groups.clone(), instrument.zones.clone(),
        vec![(path, Sample { rate: 48000, frames })],crate::import::SamplingProfile::Kontakt).unwrap();
    // Exercise native Sampler HQ, with a resident oracle independently loaded
    // from the authored full sample rather than copied stream packet results.
    for b in [&mut bank, &mut resident] {
        b.initial_state.as_mut().unwrap().controller_policy = crate::engine::instrument::ControllerPolicy::Native;
    }
    let (mut actual, mut oracle) = (Engine::default(), Engine::default());
    for e in [&mut actual, &mut oracle] { e.reset(48000.); e.attack = 0.; e.release = 0.005; }
    actual.set_bank(Some(Box::new(bank))); oracle.set_bank(Some(Box::new(resident)));
    let note = HostNote { port: 0, channel: 0, key: 60, id: 917, clap: true };
    let pattern = HostPattern { port: 0, channel: 0, key: 60, id: 917, clap: true };
    let mut pcm = [vec![0.; END], vec![0.; END]];
    let mut expected = [vec![0.; END], vec![0.; END]];
    let (mut left, mut right, mut reference_l, mut reference_r) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
    assert_eq!(crate::plugin::tests::allocations(|| {
        assert!(actual.host_note_on(note, 0, 60, 127, 0.));
        actual.render_checked(&mut left, &mut right).unwrap();
        oracle.render_checked(&mut reference_l, &mut reference_r).unwrap();
    }), 0);
    assert_eq!(left, [0.; 128]); assert_eq!(right, [0.; 128]);
    let stream = actual.player.voices[0].stream.unwrap();
    assert_eq!(actual.player.voices[0].pos, 0.);
    assert!(actual.player.voices[0].onset == crate::engine::voice::Onset::Waiting);
    worker.serve(stream.slot);
    assert_eq!(crate::plugin::tests::allocations(|| {
        assert!(oracle.host_note_on(note, 0, 60, 127, 0.));
        actual.render_checked(&mut left[..1], &mut right[..1]).unwrap();
        oracle.render_checked(&mut reference_l[..1], &mut reference_r[..1]).unwrap();
    }), 0);
    pcm[0][WAIT] = left[0]; pcm[1][WAIT] = right[0];
    expected[0][WAIT] = reference_l[0]; expected[1][WAIT] = reference_r[0];
    // Leave the next 256-point lookahead unfinished while consuming the
    // remaining127 ready points from the initial128-point onset packet.
    assert_eq!(crate::plugin::tests::allocations(|| {
        let mut at = WAIT + 1;
        for n in [127] {
            actual.render_checked(&mut left[..n], &mut right[..n]).unwrap();
            oracle.render_checked(&mut reference_l[..n], &mut reference_r[..n]).unwrap();
            assert_eq!(&left[..n], &reference_l[..n]); assert_eq!(&right[..n], &reference_r[..n]);
            pcm[0][at..at + n].copy_from_slice(&left[..n]); pcm[1][at..at + n].copy_from_slice(&right[..n]);
            expected[0][at..at + n].copy_from_slice(&reference_l[..n]); expected[1][at..at + n].copy_from_slice(&reference_r[..n]);
            at += n;
        }
    }), 0);
    assert_eq!(actual.underruns(), 0);
    assert!(pcm[0][WAIT..STARTED].iter().any(|x| x.abs() > 0.001));
    let shared = worker.shared.clone();
    let slot = &shared.slots[stream.slot as usize];
    let mut words = [0; SPARSE_TAPS];
    let (epoch, len, bank, kind) = slot.sparse_request(&mut words).unwrap();
    assert_eq!(kind, PacketKind::kernel(quality).unwrap());
    assert_eq!(len, SPARSE_TAPS, "full 256-point lookahead is actually queued");
    let key = u64::from(slot.seq.load(Ordering::Acquire)) << 32 | u64::from(epoch);
    let gate = Arc::new(AtomicU32::new(0));
    worker.worker.before_sparse_publish = Some(gate.clone());
    std::thread::scope(|scope| {
        struct Resume<'a>(&'a AtomicU32);
        impl Drop for Resume<'_> {
            fn drop(&mut self) { self.0.store(2, Ordering::Release); }
        }
        // Sole real producer: pause only after decoding the actual disk packet.
        let producer = scope.spawn(|| worker.worker.serve(slot,
            &mut worker.cursors[stream.slot as usize], RING, &shared.rings, &worker.sources));
        // Release before the scope joins its producer if any assertion fails.
        let _resume = Resume(&gate);
        let deadline = Instant::now() + Duration::from_secs(5);
        while gate.load(Ordering::Acquire) != 1 {
            assert!(Instant::now() < deadline, "disk kernel reaches publication gate");
            std::thread::yield_now();
        }
        let before = actual.player.voices[0].pos;
        assert_eq!(crate::plugin::tests::allocations(|| {
            let mut at = STARTED;
            for n in [1] {
                actual.render_checked(&mut left[..n], &mut right[..n]).unwrap();
                oracle.render_checked(&mut reference_l[..n], &mut reference_r[..n]).unwrap();
                assert_eq!(&left[..n], &[0.; 128][..n]);
                assert_eq!(&right[..n], &[0.; 128][..n]);
                pcm[0][at..at + n].copy_from_slice(&left[..n]); pcm[1][at..at + n].copy_from_slice(&right[..n]);
                at += n;
            }
        }), 0);
        assert_eq!(actual.player.voices[0].pos, before + 64., "Started underruns advance the actual source clock");
        assert_eq!(actual.player.voices[0].pos, oracle.player.voices[0].pos);
        assert_eq!(slot.sparse_seq.load(Ordering::Acquire), epoch,
            "short callbacks preserve the matching selected unfinished job");
        gate.store(2, Ordering::Release);
        assert!(producer.join().unwrap());
    });
    worker.worker.before_sparse_publish = None;
    assert_eq!(slot.sparse_packets[bank].ready.load(Ordering::Acquire), key,
        "the original decoded worker result really publishes after callbacks");
    let mut at = RECOVER;
    let mut block = 0;
    while at < END {
        worker.serve(stream.slot);
        let stop = [AUDIO_END - 1, AUDIO_END, OFF, END].into_iter().find(|&stop| stop > at).unwrap();
        let n = partitions[block % partitions.len()].min(stop - at);
        assert_eq!(crate::plugin::tests::allocations(|| {
            if at == OFF { actual.host_note_off(pattern); oracle.host_note_off(pattern); }
            actual.render_checked(&mut left[..n], &mut right[..n]).unwrap();
            oracle.render_checked(&mut reference_l[..n], &mut reference_r[..n]).unwrap();
            if at + n == AUDIO_END - 1 {
                assert_eq!(actual.active_voices(), 1); assert_eq!(oracle.active_voices(), 1);
            }
            if at + n == AUDIO_END {
                assert_eq!(actual.active_voices(), 0); assert_eq!(oracle.active_voices(), 0);
            }
        }), 0);
        pcm[0][at..at + n].copy_from_slice(&left[..n]); pcm[1][at..at + n].copy_from_slice(&right[..n]);
        expected[0][at..at + n].copy_from_slice(&reference_l[..n]); expected[1][at..at + n].copy_from_slice(&reference_r[..n]);
        at += n; block += 1;
    }
    assert!(pcm[0][RECOVER..OFF].iter().any(|x| x.abs() > 0.001), "real recovered disk PCM is audible");
    assert_eq!(pcm, expected, "complete PCM differs only at the explicit cold/late readiness intervals");
    actual.mark_host_notes(); oracle.mark_host_notes();
    assert!(!actual.host_note_pending(note) && !oracle.host_note_pending(note));
    assert_eq!(actual.host_note_end_at(note), oracle.host_note_end_at(note));
    assert_eq!(actual.host_note_end_at(note), Some(OFF as u64), "physical key-up closes the held host owner after its audio EOF");
    assert_eq!(actual.active_voices(), 0); assert_eq!(oracle.active_voices(), 0);
    assert!(pcm.iter().all(|channel| channel[AUDIO_END..].iter().all(|&x| x == 0.)));
    assert!(!actual.blocking_streams);
    pcm
}

#[test]
fn decoded_kernel_survives_selected_pending_prefetch_and_recovers_at_the_live_cursor() {
    for quality in [1, 2] {
        let fixed = performance(quality, &[16]);
        let fragmented = performance(quality, &[1, 7, 15]);
        let error = fixed.iter().flatten().zip(fragmented.iter().flatten())
            .fold(0_f32, |error, (a, b)| error.max((a - b).abs()));
        assert!(error < 2e-5, "host partitions changed complete kernel PCM by {error}");
    }
}
