//! KSP slot reports from the source already retained by the instrument.
use sampler_parity::ScriptReport;

pub fn parity_reports(instrument: &sampler_ir::Instrument) -> Vec<ScriptReport> {
    instrument
        .behaviors
        .iter()
        .enumerate()
        .filter(|(_, b)| b.language == sampler_ir::Language::Ksp)
        .map(|(i, b)| {
            let slot = b.slot.unwrap_or(i.min(u8::MAX.into()) as u8);
            let environment = crate::load::script_environment(
                b,
                i,
                instrument.groups.iter().map(|g| g.name.clone()).collect(),
                &instrument.source_indices,
                &instrument.zones,
                Default::default(),
            );
            sampler_ksp::parity_report_with(
                &b.source,
                &format!("ksp:slot{slot}"),
                &b.name,
                sampler_ksp::Limits::LIBRARY,
                &environment,
            )
        })
        .collect()
}
impl crate::Kontakt {
    pub fn parity_reports(&self) -> Vec<ScriptReport> {
        parity_reports(&self.instrument)
    }
}
impl crate::Loaded {
    pub fn parity_reports(&self) -> Vec<ScriptReport> {
        parity_reports(&self.instrument)
    }
}

/// Inspect every physical script slot, including empty/bypassed slots, without
/// resolving samples or executing on init. Linked script bytes stay in memory.
pub fn read_parity_reports(path: &std::path::Path) -> Result<Vec<ScriptReport>, crate::LoadError> {
    use ni_file::kontakt::objects::Program;
    let chunks = crate::read_chunks(path)?;
    if let Some(chunk) = chunks.find_first(0x28) {
        let program =
            Program::try_from(chunk).map_err(|e| crate::LoadError::decode(path, "program", e))?;
        return program_reports(path, &program, "");
    }
    let chunk = chunks
        .find_first(3)
        .ok_or_else(|| crate::LoadError::Invalid {
            path: path.into(),
            reason: "parity requires an instrument or multi bank".into(),
        })?;
    let bank = ni_file::kontakt::objects::Bank::try_from(chunk)
        .map_err(|e| crate::LoadError::decode(path, "multi", e))?;
    let mut slots: Vec<_> = bank
        .slot_list()
        .map_err(|e| crate::LoadError::decode(path, "multi slots", e))?
        .slots
        .into_iter()
        .collect();
    slots.sort_by_key(|(slot, _)| *slot);
    let mut reports = Vec::new();
    for (slot, container) in slots {
        for (index, program) in container
            .program_list()
            .map_err(|e| crate::LoadError::decode(path, "multi programs", e))?
            .programs
            .iter()
            .enumerate()
        {
            reports.extend(program_reports(
                path,
                program,
                &format!("program{slot}:{index}:"),
            )?);
        }
    }
    Ok(reports)
}
fn program_reports(
    path: &std::path::Path,
    program: &ni_file::kontakt::objects::Program,
    prefix: &str,
) -> Result<Vec<ScriptReport>, crate::LoadError> {
    use ni_file::kontakt::objects::BParScript;
    let mut resources = None;
    program
        .0
        .children
        .iter()
        .filter(|c| c.id == 6)
        .enumerate()
        .map(|(slot, chunk)| {
            let script = BParScript::try_from(chunk)
                .and_then(|s| s.params())
                .map_err(|e| crate::LoadError::decode(path, "script", e))?;
            let mut source = script.text.unwrap_or_default();
            let mut missing_link = false;
            if let Some(link) = script
                .textfile_name
                .as_deref()
                .filter(|s| !s.trim().is_empty())
            {
                match resources
                    .get_or_insert_with(|| crate::Resources::of(path))
                    .script(link)
                {
                    Some(text) => source = text,
                    None => missing_link = true,
                }
            }
            let performance_view = sampler_ksp::nckp::view_name(&source)
                .and_then(|name| {
                    resources
                        .get_or_insert_with(|| crate::Resources::of(path))
                        .read(&format!("Resources/performance_view/{name}.nckp"))
                })
                .and_then(|bytes| sampler_ksp::nckp::parse(&bytes).ok())
                .map(|v| v.0)
                .unwrap_or_default();
            let mut report = sampler_ksp::parity_report_with(
                &source,
                &format!("ksp:{prefix}slot{slot}"),
                &format!(
                    "Script slot {slot}{}",
                    if script.bypass { " (bypassed)" } else { "" }
                ),
                sampler_ksp::Limits::LIBRARY,
                &sampler_ksp::Environment {
                    performance_view,
                    slot: slot.min(u8::MAX as usize) as u8,
                    ..Default::default()
                },
            );
            if missing_link {
                report.constructs.push(sampler_parity::Construct {
                    span: Default::default(),
                    kind: sampler_parity::ConstructKind::Syntax,
                    name: "@linked-source".into(),
                    status: sampler_parity::Status::Unsupported,
                    detail: "Linked script resource could not be resolved; no source text emitted"
                        .into(),
                    owner: Some("crates/sampler-kontakt/src/resources.rs:179".into()),
                    spec: Some("docs/architecture-v2/KSP_PARITY.md".into()),
                });
            }
            Ok(report)
        })
        .collect()
}
