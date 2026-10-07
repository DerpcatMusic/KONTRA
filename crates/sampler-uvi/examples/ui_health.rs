// Metadata-only UI survey; decoded programs, scripts and resources stay in memory.
use sampler_ui_ir::{AssetKind, Binding, Kind};
use sampler_uvi::{
    Bank,
    script::{Config, ScriptHost},
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, Write},
    path::Path,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    run(|_, _, _| serde_json::Value::Null)
}

fn run(
    mut render: impl FnMut(&sampler_ui_ir::Interface, &str, &str) -> serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let items = std::io::BufReader::new(std::fs::File::open(&args[1])?);
    // Resume from flushed per-item metadata, including failed items.
    let mut done = BTreeSet::new();
    if let Ok(cache) = std::fs::File::open(&args[2]) {
        for line in std::io::BufReader::new(cache).lines() {
            if let Ok(record) = serde_json::from_str::<serde_json::Value>(&line?)
                && let Some(id) = record["id"].as_str()
            {
                done.insert(id.to_owned());
            }
        }
    }
    let max_items: usize = args.get(3).map(|v| v.parse()).transpose()?.unwrap_or(40);
    let deadline = Instant::now() + Duration::from_secs(240);
    let mut completed = 0;
    let mut banks = BTreeMap::<String, Vec<String>>::new();
    for line in items.lines() {
        let line = line?;
        if let Some(path) = line.strip_prefix("uvi-program\t")
            && let Some((bank, program)) = path.split_once("::")
        {
            if !done.contains(path) {
                banks.entry(bank.into()).or_default().push(program.into());
            }
        }
    }
    let mut out = std::io::BufWriter::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&args[2])?,
    );
    for (path, programs) in banks {
        if completed >= max_items || Instant::now() >= deadline {
            break;
        }
        let bank = Bank::open(Path::new(&path));
        for program in programs {
            if completed >= max_items || Instant::now() >= deadline {
                break;
            }
            let id = format!("{path}::{program}");
            let record = match bank
                .as_ref()
                .ok()
                .and_then(|bank| bank.program(&program).ok().map(|(xml, _)| (bank, xml)))
            {
                None => json!({"id":id, "loaded":false, "failure":"bank/program read"}),
                Some((bank, xml)) => match ScriptHost::new(&xml, bank.scripts(), Config::default())
                {
                    Err(_) => json!({"id": format!("{path}::{program}"), "loaded":false}),
                    Ok(host) => {
                        let face = host.interface();
                        let rendered = render(&face, &path, &program);
                        let mut features = BTreeMap::<String, usize>::new();
                        for f in host.findings() {
                            *features.entry(f.feature).or_default() += f.count;
                        }
                        let (mut missing, mut fonts, mut missing_fonts) = (0, 0, 0);
                        for asset in &face.assets {
                            let is_font = matches!(asset.kind, AssetKind::TrueTypeFont);
                            fonts += usize::from(is_font);
                            if bank.ui_resource(&program, &asset.path).is_err() {
                                missing += 1;
                                missing_fonts += usize::from(is_font);
                            }
                        }
                        let interactive = |k: &Kind| {
                            matches!(
                                k,
                                Kind::Knob { .. }
                                    | Kind::Slider { .. }
                                    | Kind::ValueEdit { .. }
                                    | Kind::Button { .. }
                                    | Kind::Menu { .. }
                                    | Kind::Table { .. }
                                    | Kind::Xy { .. }
                            )
                        };
                        let controls = face.widgets.iter().filter(|w| interactive(&w.kind)).count();
                        let unbound = face
                            .widgets
                            .iter()
                            .filter(|w| {
                                interactive(&w.kind) && !matches!(w.binding, Binding::Control(_))
                            })
                            .count();
                        let mut xml_ui = BTreeMap::<String, usize>::new();
                        let doc = sampler_uvi::parse_program_xml(&xml)?;
                        for n in doc.descendants().filter(|n| n.is_element()) {
                            let tag = n.tag_name().name();
                            if tag.contains("UI") || tag.contains("Skin") || tag.contains("View") {
                                *xml_ui.entry(tag.into()).or_default() += 1;
                            }
                            for a in n.attributes() {
                                if a.name().contains("UI")
                                    || a.name().contains("Skin")
                                    || a.name().contains("View")
                                {
                                    *xml_ui.entry(format!("{tag}.{}", a.name())).or_default() += 1;
                                }
                            }
                        }
                        json!({"id":format!("{path}::{program}"), "loaded":true, "widgets":face.widgets.len(), "controls":controls,
                        "unbound":unbound, "assets":face.assets.len(), "missing_assets":missing, "fonts":fonts, "missing_fonts":missing_fonts,
                        "ui_loaded":!face.widgets.is_empty() && !features.contains_key("lua error"), "properties":host.ui_properties(),
                        "xml_ui":xml_ui, "rendered":rendered, "unsupported":face.unsupported.iter().map(|u| &u.feature).collect::<Vec<_>>(),
                        "validation_error":face.validate().err().map(|e| e.to_string()), "findings":features})
                    }
                },
            };
            writeln!(out, "{record}")?;
            out.flush()?;
            completed += 1;
        }
    }
    eprintln!("UI shard completed {completed} UVI programs; cache resumes on next invocation");
    Ok(())
}
