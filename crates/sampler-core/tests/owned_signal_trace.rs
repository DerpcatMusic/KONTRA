mod support;
use sampler_core::{Envelope, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime};

fn prepared() -> Prepared {
    Prepared::new(
        48000,
        vec![Pcm::new(48000, vec![[0.125; 2]; 1024].into()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::GATE,
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap()
}
fn play(runtime: &mut Runtime) -> Vec<[f32; 2]> {
    let mut output = vec![[0.; 2]; 256];
    support::without_heap(|| {
        runtime
            .trigger(
                Input {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: None,
                },
                60,
                1.,
            )
            .unwrap();
        runtime.render(&mut output).unwrap();
    });
    output
}

#[test]
fn external_reader_has_identical_pcm_and_does_not_allow_reconfiguration() {
    let plan = prepared();
    let limits = Limits::for_plan(&plan, 8, 16);
    let mut plain = Runtime::new(plan, limits).unwrap();
    let (plan, reader) = prepared().with_signal_trace_reader(4096).unwrap();
    assert!(plan.with_signal_trace(4096).is_err());
    let (plan, owned) = prepared().with_signal_trace_reader(4096).unwrap();
    let mut traced = Runtime::new(plan, limits).unwrap();
    assert_eq!(play(&mut plain), play(&mut traced));
    assert_eq!(owned.dropped(), 0);
    assert!(!owned.drain().is_empty());
    assert!(!owned.onset_reader().drain().unwrap().is_empty());
    assert!(reader.drain().is_empty());
    assert!(prepared().with_signal_trace_reader(0).is_err());
}

#[test]
fn external_reader_owns_all_rows_even_when_environment_reporting_is_enabled() {
    const CHILD: &str = "KONTRA_OWNED_TRACE_CHILD";
    if let Some(mode) = std::env::var_os(CHILD) {
        let root = std::path::PathBuf::from(std::env::var_os("KONTRA_REPORT_DIR").unwrap());
        let control = mode == "existing_worker";
        if control {
            let plan = prepared();
            let limits = Limits::for_plan(&plan, 8, 16);
            let mut environment = Runtime::new(plan, limits).unwrap();
            play(&mut environment);
            drop(environment);
            assert!(sampler_core::trace_report::flush(
                std::time::Duration::from_secs(2)
            ));
            assert_eq!(sampler_core::trace_report::reports().len(), 1);
        }
        let before = sampler_core::trace_report::reports().len();
        let (plan, reader) = if mode == "scoped" {
            prepared()
                .with_scoped_signal_trace_reader(
                    4096,
                    sampler_core::trace::SignalTraceScope::new(60, 60, 128).unwrap(),
                )
                .unwrap()
        } else {
            prepared().with_signal_trace_reader(4096).unwrap()
        };
        let limits = Limits::for_plan(&plan, 8, 16);
        let mut runtime = Runtime::new(plan, limits).unwrap();
        let output = play(&mut runtime);
        assert!(output.iter().any(|v| v[0] != 0.));
        drop(runtime);
        assert!(sampler_core::trace_report::flush(
            std::time::Duration::from_secs(2)
        ));
        let rows = reader.drain();
        let births = reader.onset_reader().drain().unwrap();
        assert!(
            rows.iter()
                .any(|row| row.contribution && row.output.onset.nonzero[0].is_some())
        );
        assert_eq!(
            births
                .iter()
                .filter(|row| matches!(row, sampler_core::trace::onset::Record::Birth(_)))
                .count(),
            1
        );
        assert_eq!(reader.dropped(), 0);
        assert_eq!(reader.onset_reader().dropped(), 0);
        println!("OWNED_ROWS {} {}", rows.len(), births.len());
        assert_eq!(sampler_core::trace_report::reports().len(), before);
        if control {
            assert_eq!(std::fs::read_dir(root).unwrap().count(), 2);
        } else {
            assert!(
                !root.exists(),
                "external ownership must never start a reporter or create its directory"
            );
        }
        return;
    }
    let scratch = std::env::var_os("KONTRA_TEST_SCRATCH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = scratch.join(format!(
        "kontra-owned-trace-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&base).unwrap();
    let mut expected = None;
    for mode in ["baseline", "fresh", "scoped", "existing_worker"] {
        let root = base.join(mode);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "external_reader_owns_all_rows_even_when_environment_reporting_is_enabled",
                "--nocapture",
            ])
            .env(CHILD, mode)
            .env(
                "KONTRA_SIGNAL_TRACE",
                if mode == "baseline" { "0" } else { "1" },
            )
            .env("KONTRA_REPORT_DIR", &root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let count = stdout
            .lines()
            .find_map(|line| line.strip_prefix("OWNED_ROWS "))
            .unwrap()
            .to_owned();
        if let Some(ref expected) = expected {
            assert_eq!(&count, expected, "environment worker stole diagnostic rows");
        } else {
            expected = Some(count);
        }
    }
    std::fs::remove_dir_all(base).unwrap();
}
