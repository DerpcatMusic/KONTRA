use super::*;

#[test]
fn superseded_sparse_publication_keeps_reselected_disk_pcm_without_heap() {
    let dir = tempfile::tempdir().unwrap();
    let instrument = instrument(&dir.path().join("sparse-publication.wav"), false);
    let pcm = crate::audio::decode(&instrument.zones[0].sample, 131072).unwrap();
    let mut source_reader = crate::audio::Sources::default();
    let source = Arc::new(source_reader.source(&instrument.zones[0].sample).unwrap());
    let sources = [Some(source)];
    let ring: Vec<_> = (0..RING).map(|_| AtomicU64::new(0)).collect();
    let mut wake = Wake::default();
    let slot = Slot::new(NonNull::from(&ring[0]), NonNull::from(&mut wake));
    let map = PlayMap {
        start: 0,
        end: 131072,
        reverse: false,
        looped: None,
    };
    let generation = slot.configure(0, &map, 0, 0, 0);
    let (mut worker, mut cursor, rings) = (Worker::new(), Cursor::default(), RingUse::default());
    let current = [0, 1, 2, 3];
    let predicted = [4096, 4097, 4098, 4099];
    let replacement = [8192, 8193, 8194, 8195];
    let expected = predicted.map(|tap| pcm.frames[tap as usize]);
    assert_ne!(expected, replacement.map(|tap| pcm.frames[tap as usize]));
    let mut output = [[0.; 2]; 4];
    let current_request = slot.request_sparse(generation, &current).unwrap();
    assert!(worker.serve(&slot, &mut cursor, URGENT, &rings, &sources));
    assert_eq!(
        crate::test_support::allocations(|| {
            assert!(slot.copy_sparse(generation, current_request, &mut output));
            assert_eq!(output, current.map(|tap| pcm.frames[tap as usize]));
            slot.prefetch_sparse(generation, &predicted).unwrap();
        }),
        0
    );

    // A live trajectory edit can replace unfinished lookahead. Pause the sole
    // disk producer after validation to expose its remaining publication stores.
    let gate = Arc::new(AtomicU32::new(0));
    worker.after_sparse_validation = Some(gate.clone());
    std::thread::scope(|scope| {
        struct Resume<'a>(&'a AtomicU32);
        impl Drop for Resume<'_> {
            fn drop(&mut self) {
                self.0.store(2, Ordering::Release);
            }
        }
        let producer = scope.spawn(|| worker.serve(&slot, &mut cursor, URGENT, &rings, &sources));
        let _resume = Resume(&gate);
        let deadline = Instant::now() + Duration::from_secs(5);
        while gate.load(Ordering::Acquire) != 1 {
            assert!(
                Instant::now() < deadline,
                "real disk producer reaches the validated publication gate"
            );
            std::thread::yield_now();
        }
        assert_eq!(
            crate::test_support::allocations(|| {
                slot.prefetch_sparse(generation, &replacement).unwrap();
            }),
            0
        );
        gate.store(2, Ordering::Release);
        assert!(producer.join().unwrap());
    });
    worker.after_sparse_validation = None;

    // Restore the original trajectory before its next actual read. A selected
    // cached packet must survive the still-pending replacement producer.
    let mut selected = None;
    assert_eq!(
        crate::test_support::allocations(|| {
            selected = Some(slot.request_sparse(generation, &predicted).unwrap());
        }),
        0
    );
    assert!(worker.serve(&slot, &mut cursor, URGENT, &rings, &sources));
    assert_eq!(
        crate::test_support::allocations(|| {
            let request = selected.unwrap();
            assert!(
                slot.copy_sparse(generation, request, &mut output),
                "a selected actual window must retain its published disk PCM"
            );
            assert_eq!(
                output, expected,
                "complete stereo taps must match the independent WAV decode"
            );
            assert_eq!(slot.ready_sparse(generation, request, false), Ok(true));
        }),
        0
    );
}
