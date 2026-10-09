use sampler_core::{Input, Limits, Prepared, Protocol, Runtime};
mod support;

#[test]
fn sparse_retirement_preserves_parent_cleanup_backpressure_and_slot_reuse() {
    let plan = Prepared::new(48000, vec![], vec![], 0).unwrap();
    let limits = Limits::for_plan(&plan, 16384, 8);
    let mut rt = Runtime::new(plan, limits).unwrap();
    support::without_heap(|| {
        for cycle in 0..16 {
            for key in 0..8 {
                let note = rt
                    .note_on(
                        Input {
                            protocol: Protocol::Native,
                            port: 0,
                            group: 0,
                            channel: 0,
                            key,
                            external_id: Some(cycle * 8 + i32::from(key)),
                        },
                        key,
                        1.,
                    )
                    .unwrap();
                rt.release(note).unwrap();
            }
            rt.flush_ended(|_| false);
            assert_eq!(rt.note_count(), 8);
            let mut ended = 0;
            rt.flush_ended(|_| {
                ended += 1;
                true
            });
            assert_eq!(ended, 8);
            assert_eq!(rt.note_count(), 0);
            rt.flush_ended(|_| panic!("empty arena"));
        }
    });
}
