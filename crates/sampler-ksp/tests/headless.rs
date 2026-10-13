use sampler_core::{Limits, Prepared, Runtime};

#[test]
fn headless_external_jobs_stay_in_the_outbox_until_a_real_host_accepts_them() {
    for (source, service) in [
        (
            "on init declare $job end on on persistence_changed $job:=mf_reset() wait_async($job) end on",
            sampler_core::MIDI_SERVICE,
        ),
        (
            "on init declare %data[1] declare $job end on on persistence_changed $job:=load_array_str(%data,\"/not-opened.nka\") wait_async($job) end on",
            sampler_core::ARRAY_FILE_SERVICE,
        ),
    ] {
        let script =
            sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        let mut views = vec![script.view()];
        let plan = script
            .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
            .unwrap();
        let limits = Limits::for_plan(&plan, 4, 4);
        let mut runtime = Runtime::new(plan, limits).unwrap();
        assert_eq!(
            sampler_ksp::service_ui_effects(&mut runtime, &mut views)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::Unsupported
        );
        let mut retained = 0;
        runtime.drain_effects(|effect| {
            assert_eq!(effect.service, service);
            retained += 1;
            true
        });
        assert_eq!(
            retained, 1,
            "unsupported jobs retain their owner and request"
        );
    }
}
