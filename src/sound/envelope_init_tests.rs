use super::*;

#[test]
fn dolce_authored_envelope_init_reaches_production_pcm() {
    let root = Path::new("/mnt/MAIN_STORAGE/Libraries/Kontakt/Audio Imperia Dolce/Instruments");
    let cases = [
        (
            "01 7 1st Violins/Dolce - 03 7 1st Violins - Sustained Con Sordino.nki",
            60,
        ),
        (
            "01 7 1st Violins/Dolce - 04 7 1st Violins - Harmonics.nki",
            73,
        ),
        (
            "02 5 2nd Violins/Dolce - 04 5 2nd Violins - Harmonics.nki",
            73,
        ),
        ("03 4 Violas/Dolce - 04 4 Violas - Harmonics.nki", 72),
        ("05 3 Basses/Dolce - 13 3 Basses - Bend FX.nki", 48),
        ("06 Ensemble Patches/Dolce - 11 Ensemble - Trill WT.nki", 60),
        ("01 7 1st Violins/Dolce - 01 7 1st Violins - Legato.nki", 60),
    ];
    let mut tested = 0;
    for (case, (relative, key)) in cases.into_iter().enumerate() {
        let path = root.join(relative);
        if !path.is_file() {
            continue;
        }
        let loaded = V2Loader
            .prepare(
                &LoadRequest {
                    path,
                    sample_rate: 48000.,
                    dynamics_start: Some(100),
                    threads: None,
                    ..Default::default()
                },
                &mut |_| {},
                &|| false,
            )
            .unwrap_or_else(|_| panic!("production load failed; authored diagnostics omitted"));
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, loaded.part);
        core.begin_block(&BlockInfo {
            frames: 128,
            ..Default::default()
        });
        if case == 4 {
            core.parts[0]
                .as_mut()
                .unwrap()
                .runtime
                .record_selections(true);
        }
        core.event(0, Event::midi1(0xb0, 1, 100));
        core.event(0, Event::midi1(0xb0, 11, 127));
        core.event(0, Event::midi1(0x90, key, 64));
        let mut peak = 0f64;
        let mut voices = 0;
        for _ in 0..180 {
            core.begin_block(&BlockInfo {
                frames: 128,
                ..Default::default()
            });
            let rendered = core.render(128);
            for bus in rendered.buses {
                for channel in bus {
                    for sample in &channel[..128] {
                        peak = peak.max(f64::from(*sample).abs());
                    }
                }
            }
            voices = voices.max(core.parts[0].as_ref().unwrap().runtime.voice_count());
            core.end_block(128, &mut |_| true);
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        if case == 4 {
            let selections = core.parts[0]
                .as_mut()
                .unwrap()
                .runtime
                .take_selection_records();
            assert!(selections.iter().any(|selection| selection.suppressed));
            assert!(selections.iter().any(|selection| {
                selection
                    .candidates
                    .iter()
                    .filter(|candidate| candidate.rejected.is_none())
                    .count()
                    == 6
            }));
        }
        assert!(
            peak > 1e-5,
            "case {case}: init must retain a nonzero authored envelope without manual restoration"
        );
        assert!(
            voices > 0,
            "case {case}: one-shot stages must not retire before source playback"
        );
        assert_eq!(
            core.problems(0).fault_program,
            0,
            "case {case}: callbacks must finish without a fault"
        );
        assert_eq!(
            core.parts[0]
                .as_ref()
                .unwrap()
                .runtime
                .stats()
                .stream_underruns,
            0
        );
        println!("DOLCE_PRODUCTION_PCM case={case} peak={peak:.8} voices={voices} underruns=0");
        tested += 1;
    }
    println!("DOLCE_PRODUCTION_PCM tested={tested}");
}
