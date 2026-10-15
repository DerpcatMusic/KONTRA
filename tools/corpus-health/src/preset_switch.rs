//! Explicit runtime diagnostic; no plugin entry point or host is launched.

struct Arguments<'a> {
    plan: &'a str,
    artifacts: Option<(&'a std::path::Path, sampler_ui_ir::Source, u32)>,
}

fn arguments(args: &[String]) -> Result<Arguments<'_>, &'static str> {
    let Some(plan) = args.first().filter(|plan| !plan.starts_with('-')) else {
        return Err("preset-switch requires a plan path");
    };
    let (mut directory, mut slot, mut page, mut source) = (None, None, None, None);
    let mut options = args[1..].chunks_exact(2);
    for pair in &mut options {
        match pair[0].as_str() {
            "--ui-artifacts" if directory.is_none() => {
                directory = Some(std::path::Path::new(&pair[1]));
            }
            "--ui-source" if source.is_none() => {
                if !matches!(pair[1].as_str(), "ksp" | "uvi" | "performance-view") {
                    return Err("UI source must be ksp, uvi or performance-view");
                }
                source = Some(pair[1].as_str());
            }
            "--ui-slot" if slot.is_none() => {
                slot = Some(
                    pair[1]
                        .parse::<u8>()
                        .map_err(|_| "UI slot must be 0..255")?,
                );
            }
            "--ui-page" if page.is_none() => {
                page = Some(
                    pair[1]
                        .parse::<u32>()
                        .map_err(|_| "UI page must be a nonnegative u32")?,
                );
            }
            _ => return Err("Unknown or duplicate preset-switch option"),
        }
    }
    if !options.remainder().is_empty() {
        return Err("Every preset-switch capture option requires a value");
    }
    let artifacts = match (directory, slot, page, source) {
        (None, None, None, None) => None,
        (Some(directory), slot, Some(page), source) if directory.is_absolute() => {
            let source = match (source.unwrap_or("ksp"), slot) {
                ("ksp", Some(slot)) => sampler_ui_ir::Source::Ksp { slot },
                ("uvi", None) => sampler_ui_ir::Source::FalconLua,
                ("performance-view", None) => sampler_ui_ir::Source::PerformanceView,
                _ => return Err("Only KSP capture requires --ui-slot; other sources refuse it"),
            };
            Some((directory, source, page))
        }
        _ => {
            return Err("Capture requires --ui-artifacts NEW_ABSOLUTE_DIR --ui-page N and explicit source selection (KSP also requires --ui-slot N)");
        }
    };
    Ok(Arguments { plan, artifacts })
}

pub(super) fn run(args: &[String]) -> i32 {
    let args = match arguments(args) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("preset-switch: {error}");
            eprintln!(
                "preset-switch PLAN.json [--ui-artifacts NEW_ABSOLUTE_DIR --ui-page N --ui-slot N | --ui-source uvi|performance-view] (requires ui-render; see tools/corpus-health/PRESET_SWITCH.md)"
            );
            return 2;
        }
    };
    #[cfg(not(feature = "ui-render"))]
    {
        let _ = (args.plan, args.artifacts);
        eprintln!("preset-switch requires the ui-render diagnostic helper feature");
        2
    }
    #[cfg(feature = "ui-render")]
    {
        use std::io::Read;
        let mut bytes = Vec::new();
        let read = std::fs::File::open(args.plan)
            .and_then(|file| file.take((1 << 20) + 1).read_to_end(&mut bytes));
        if read.is_err() || bytes.len() > 1 << 20 {
            eprintln!("preset-switch plan could not be read within the 1 MiB limit");
            return 2;
        }
        let plan: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(plan) => plan,
            Err(_) => {
                eprintln!("preset-switch plan is not valid JSON");
                return 2;
            }
        };
        let result = match args.artifacts {
            Some((directory, source, page)) => {
                kontakto::OfflineUiLifecycle::preset_switch_probe_with_source_ui_artifacts(
                    &plan, directory, source, page,
                )
            }
            None => kontakto::OfflineUiLifecycle::preset_switch_probe(&plan),
        };
        match result {
            Ok(report) => {
                println!("{report}");
                0
            }
            Err(error) => {
                eprintln!("preset-switch: {error}");
                1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(
        values: &[&str],
    ) -> Result<(String, Option<(std::path::PathBuf, sampler_ui_ir::Source, u32)>), &'static str> {
        let args: Vec<_> = values.iter().map(|value| value.to_string()).collect();
        let parsed = arguments(&args)?;
        Ok((
            parsed.plan.to_string(),
            parsed
                .artifacts
                .map(|(path, slot, page)| (path.to_path_buf(), slot, page)),
        ))
    }

    #[test]
    fn capture_arguments_preserve_default_and_require_explicit_selection() {
        assert_eq!(parse(&["plan.json"]).unwrap(), ("plan.json".into(), None));
        let directory = std::env::current_dir()
            .unwrap()
            .join("unused-private-capture");
        let directory = directory.to_str().unwrap();
        let selected = parse(&[
            "plan.json",
            "--ui-page",
            "3",
            "--ui-slot",
            "2",
            "--ui-artifacts",
            directory,
        ])
        .unwrap();
        assert_eq!(selected.1, Some((directory.into(), sampler_ui_ir::Source::Ksp { slot: 2 }, 3)));
        for (name, source) in [("uvi", sampler_ui_ir::Source::FalconLua), ("performance-view", sampler_ui_ir::Source::PerformanceView)] {
            assert_eq!(parse(&["plan.json", "--ui-artifacts", directory, "--ui-page", "0", "--ui-source", name]).unwrap().1,
                Some((directory.into(), source, 0)));
            assert!(parse(&["plan.json", "--ui-artifacts", directory, "--ui-page", "0", "--ui-source", name, "--ui-slot", "0"]).is_err());
        }
        assert!(parse(&["plan.json", "--ui-source", "unknown"]).is_err());
        assert!(parse(&["plan.json", "--ui-source", "uvi", "--ui-source", "uvi"]).is_err());
        assert!(parse(&["plan.json", "--ui-artifacts", directory, "--ui-page", "0", "--ui-source", "ksp"]).is_err());
        for args in [
            vec!["plan.json", "--ui-artifacts", directory],
            vec!["plan.json", "--ui-artifacts", directory, "--ui-slot", "2"],
            vec![
                "plan.json",
                "--ui-artifacts",
                "relative",
                "--ui-slot",
                "2",
                "--ui-page",
                "0",
            ],
            vec![
                "plan.json",
                "--ui-artifacts",
                directory,
                "--ui-slot",
                "256",
                "--ui-page",
                "0",
            ],
            vec![
                "plan.json",
                "--ui-artifacts",
                directory,
                "--ui-slot",
                "2",
                "--ui-page",
                "-1",
            ],
            vec!["plan.json", "--ui-slot", "2", "--ui-slot", "3"],
            vec!["plan.json", "--ui-page"],
        ] {
            assert!(parse(&args).is_err());
        }
    }
}
