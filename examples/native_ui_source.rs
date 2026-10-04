fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(
        args.len() >= 3,
        "Usage: native_ui_source INSTRUMENT MODULE|--list|--render|--bindings|--dsp|--scripts DIRECTORY|--extract DIRECTORY|--shots DIRECTORY"
    );
    #[cfg(feature = "plugin")]
    if args[2] == "--shots" {
        let directory = args
            .get(3)
            .ok_or_else(|| anyhow::anyhow!("--shots requires a directory"))?;
        let mut audit = vec![args[1].clone(), "--shots".into(), directory.clone()];
        audit.extend_from_slice(&args[4..]);
        return kontakto::audit_ui(&audit);
    }
    if args[2] == "--dsp" || args[2] == "--scripts" {
        let instrument = kontakto::import::read(std::path::Path::new(&args[1]))?;
        if args[2] == "--scripts" {
            let dir = std::path::Path::new(
                args.get(3)
                    .ok_or_else(|| anyhow::anyhow!("--scripts requires a directory"))?,
            );
            std::fs::create_dir_all(dir)?;
            for (slot, source) in instrument.scripts.iter().enumerate() {
                std::fs::write(dir.join(format!("script-{slot}.ksp")), source)?;
            }
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "groups": instrument.groups, "fx": instrument.fx, "warnings": instrument.warnings,
                    "missing_samples": instrument.missing_samples.len()
                }))?
            );
        }
        return Ok(());
    }
    if args[2] == "--list" {
        for name in kontakto::native_ui::package_members(std::path::Path::new(&args[1])) {
            println!("{name}");
        }
        return Ok(());
    }
    if args[2] == "--extract" {
        anyhow::ensure!(args.len() > 3, "--extract requires a directory");
        let package = kontakto::native_ui::Package::load(std::path::Path::new(&args[1]))?;
        for (name, bytes) in package.members {
            if name.ends_with(".nui") {
                let path = std::path::Path::new(&args[3]).join(name);
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(path, &bytes)?;
            }
        }
        return Ok(());
    }
    if args[2] == "--render" || args[2] == "--bindings" {
        let instrument = kontakto::import::read(std::path::Path::new(&args[1]))?;
        let (rt, errors) = kontakto::engine::load_scripts(&instrument, Vec::new(), 48000.);
        for error in errors {
            eprintln!("{error}");
        }
        let rt = rt.ok_or_else(|| anyhow::anyhow!("No KSP runtime"))?;
        if args[2] == "--bindings" {
            println!("{}", serde_json::to_string(&rt.exposed_controls())?);
            return Ok(());
        }
        let package = std::sync::Arc::new(kontakto::native_ui::Package::load(
            std::path::Path::new(&args[1]),
        )?);
        let session = kontakto::native_ui::Session::new(
            package,
            rt.native_ui_entry().unwrap_or("main"),
            rt.exposed_controls().into(),
        )?;
        let graph = session.render()?;
        println!("NativeUI root: {}", graph.get::<String>("kind")?);
        return Ok(());
    }
    let source = kontakto::native_ui::module_source(std::path::Path::new(&args[1]), &args[2])?;
    print!("{source}");
    Ok(())
}
