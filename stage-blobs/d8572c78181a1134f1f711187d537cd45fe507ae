//! Opt-in owned-bank check; no bank data or access material is written.
#[cfg(feature = "library-access")]
#[test]
fn installed_protected_program_loads_without_a_workstation_reader() {
    let Some(path) = std::env::var_os("KONTRA_UVI_TEST_BANK")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
    else {
        eprintln!("skipped: KONTRA_UVI_TEST_BANK is absent");
        return;
    };
    if std::env::var_os("KONTRA_UVI_NATIVE_TEST_CHILD").is_some() {
        render_native_witness(&path);
        return;
    }

    let case = std::env::var("KONTRA_UVI_NATIVE_CASE").unwrap_or_else(|_| "unset".into());
    assert!(matches!(case.as_str(), "unset" | "invalid"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!(
        "kontra-uvi-native-home-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir(&home).unwrap();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "installed_protected_program_loads_without_a_workstation_reader",
            "--nocapture",
        ])
        .env("KONTRA_UVI_TEST_BANK", path)
        .env("KONTRA_UVI_NATIVE_TEST_CHILD", "1")
        .env("HOME", &home)
        .env_remove("KONTRA_UVI_REFERENCE_READER")
        .env_remove("WINEPREFIX")
        .env_remove("PROGRAMFILES")
        .env_remove("ProgramW6432");
    if case == "invalid" {
        command.env(
            "KONTRA_UVI_READER",
            home.join("missing/UVIWorkstationx64.exe"),
        );
    } else {
        command.env_remove("KONTRA_UVI_READER");
    }
    let output = command.output().unwrap();
    let _ = std::fs::remove_dir_all(&home);
    assert!(
        output.status.success(),
        "native UVI child failed: {}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let witness = stderr
        .lines()
        .chain(stdout.lines())
        .find(|line| line.starts_with("NATIVE_UVI "))
        .expect("numeric native witness missing");
    eprintln!("{witness}");
}

#[cfg(feature = "library-access")]
fn render_native_witness(path: &std::path::Path) {
    assert!(
        std::env::var_os("KONTRA_UVI_REFERENCE_READER").is_none(),
        "native witness must not use a reference executable"
    );
    for variable in ["WINEPREFIX", "PROGRAMFILES", "ProgramW6432"] {
        assert!(
            std::env::var_os(variable).is_none(),
            "native witness must not use Windows or Wine paths"
        );
    }
    let home = std::env::var_os("HOME").expect("isolated HOME");
    let workstation = std::path::PathBuf::from(home)
        .join(".wine/drive_c/Program Files/UVI Workstation/UVIWorkstationx64.exe");
    assert!(
        !workstation.exists(),
        "isolated HOME must not contain a Workstation executable"
    );
    let case = if let Some(reader) = std::env::var_os("KONTRA_UVI_READER") {
        assert!(
            !std::path::Path::new(&reader).exists(),
            "invalid-reader witness path must not exist"
        );
        "invalid-override"
    } else {
        "unset-override"
    };
    let bank = sampler_uvi::Bank::open(path).unwrap();
    let program = std::env::var("KONTRA_UVI_TEST_PROGRAM")
        .ok()
        .or_else(|| bank.programs().into_iter().next())
        .unwrap();
    let options = sampler_kontakt::Options {
        keys: 60..=60,
        ..Default::default()
    };
    let loaded = sampler_uvi::load_program_with_options(&bank, &program, &options).unwrap();
    assert!(!loaded.instrument.zones.is_empty());
    assert!(
        loaded
            .instrument
            .zones
            .iter()
            .all(|z| z.keys.low <= 60 && z.keys.high >= 60)
    );
    let limits = sampler_core::Limits {
        notes: 4,
        channels: 16,
        performances: 1,
        expressions: 4,
        families: 4,
        decisions: 256,
        voices: 512,
        commands: 256,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    };
    let zone_count = loaded.instrument.zones.len();
    let mut runtime = sampler_core::Runtime::new(loaded.plan, limits).unwrap();
    runtime
        .trigger(
            sampler_core::Input {
                protocol: sampler_core::Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: None,
            },
            60,
            0.8,
        )
        .unwrap();
    let mut audio = vec![[0.0; 2]; 4800];
    runtime.render(&mut audio).unwrap();
    assert!(audio.iter().flatten().all(|v| v.is_finite()));
    assert!(audio.iter().flatten().any(|v| v.abs() > 0.0001));
    let mut peak = 0.0f32;
    let mut energy = 0.0f64;
    let mut audible_samples = 0usize;
    for sample in audio.iter().flatten() {
        let magnitude = sample.abs();
        peak = peak.max(magnitude);
        energy += f64::from(*sample) * f64::from(*sample);
        audible_samples += usize::from(magnitude > 0.0001);
    }
    let rms = (energy / (audio.len() * 2) as f64).sqrt();
    eprintln!(
        "NATIVE_UVI case={case} zones={zone_count} frames={} audible_samples={audible_samples} peak={peak:.6} rms={rms:.6}",
        audio.len()
    );
}
