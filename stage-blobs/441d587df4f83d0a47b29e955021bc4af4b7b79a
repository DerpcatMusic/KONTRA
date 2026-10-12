#[cfg(test)]
mod v2_baseline_probe {
    use super::*;
    #[test]
    fn unmatched_terminal_retries_after_host_refusal() {
        let params = SamplerParams::new();
        let mut dsp = Dsp::default();
        dsp.until_poll = usize::MAX;
        let address = ExactNoteAddress::from_raw_signed(7, 4, 60, 42);
        let mut incoming = EventList::with_capacity(1);
        incoming
            .try_push_exact(ExactEvent::new(
                0,
                ExactEventBody::Note {
                    kind: ExactNoteKind::On,
                    address,
                    velocity: 0.8,
                },
            ))
            .unwrap();
        let mut output = EventList::with_capacity(1);
        output
            .try_push_exact(ExactEvent::new(
                0,
                ExactEventBody::Note {
                    kind: ExactNoteKind::End,
                    address: ExactNoteAddress::from_raw_signed(0, 0, 60, 1),
                    velocity: 0.0,
                },
            ))
            .unwrap();
        let transport = TransportInfo::default();
        let mut cx = ProcessContext::new(&transport, 48000., 128, &mut output);
        let (mut left, mut right) = ([0.; 128], [0.; 128]);
        let mut channels = [&mut left[..], &mut right[..]];
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, 128);
        Sampler::process(&mut dsp, &params, &mut buffer, &incoming, &mut cx);
        assert_eq!(dsp.host_note_end_rejections, 1);
        cx.output_events.clear();
        incoming.clear();
        Sampler::process(&mut dsp, &params, &mut buffer, &incoming, &mut cx);
        assert!(cx.output_events.lossless_iter().any(|event| matches!(event,LosslessEventRef::Exact(e) if matches!(e.body(),ExactEventBody::Note {kind:ExactNoteKind::End,address:a,..} if *a==address))), "missing retry for rejected no-owner NOTE_END");
    }
    #[test]
    fn persistence_capture_is_a_coherent_version() {
        let source = "on init\ndeclare %data[128]\nmake_persistent(%data)\ndeclare $i\ndeclare ui_knob $k(0,10,1)\nend on\non ui_control($k)\n$i := 0\nwhile ($i < 128)\n%data[$i] := $k\ninc($i)\nend while\nend on";
        let mut engine = crate::ksp::LogEngine::new(vec!["fixture".into()], 48000.);
        let (mut rt, errors) = Runtime::with_scripts(&[source], &mut engine, 2, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        let mut saved = rt.persistence();
        rt.ui_control(&mut engine, 0, 0, 1);
        let mut cursor = Refresh::default();
        assert!(!rt.refresh_persistence_within(&mut saved, &mut cursor, 64));
        rt.ui_control(&mut engine, 0, 0, 2);
        while !rt.refresh_persistence_within(&mut saved, &mut cursor, 64) {}
        let crate::ksp::Value::IntArray(values) = &saved[0]["%data"] else {
            panic!("array")
        };
        assert!(
            values.iter().all(|v| *v == values[0]),
            "torn persistent array: first={}, last={}",
            values[0],
            values[127]
        );
    }
}
