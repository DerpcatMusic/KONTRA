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
    read_parity_reports_mode(path, false)
}

/// Source-only coverage for aggregate reports that discard token and lowered
/// operation maps. Construct statuses and resource diagnostics are preserved.
pub fn read_parity_reports_summary(
    path: &std::path::Path,
) -> Result<Vec<ScriptReport>, crate::LoadError> {
    read_parity_reports_mode(path, true)
}

fn read_parity_reports_mode(
    path: &std::path::Path,
    summary: bool,
) -> Result<Vec<ScriptReport>, crate::LoadError> {
    use ni_file::kontakt::objects::Program;
    let chunks = crate::read_chunks(path)?;
    if let Some(chunk) = chunks.find_first(0x28) {
        let program =
            Program::try_from(chunk).map_err(|e| crate::LoadError::decode(path, "program", e))?;
        return program_reports_mode(path, &program, "", summary);
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
            reports.extend(program_reports_mode(
                path,
                program,
                &format!("program{slot}:{index}:"),
                summary,
            )?);
        }
    }
    Ok(reports)
}
#[cfg(test)]
fn program_reports(
    path: &std::path::Path,
    program: &ni_file::kontakt::objects::Program,
    prefix: &str,
) -> Result<Vec<ScriptReport>, crate::LoadError> {
    program_reports_mode(path, program, prefix, false)
}

fn program_reports_mode(
    path: &std::path::Path,
    program: &ni_file::kontakt::objects::Program,
    prefix: &str,
    summary: bool,
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
                    None => missing_link = source.trim().is_empty(),
                }
            }
            let mut performance_view = Default::default();
            let mut view_gaps = Vec::new();
            if let Some(name) = sampler_ksp::nckp::view_name(&source) {
                let result = resources
                    .get_or_insert_with(|| crate::Resources::of(path))
                    .read_result(&format!("Resources/performance_view/{name}.nckp"));
                // Resource/parser errors may contain authored paths or values. Keep
                // the failure stage and source position, never those private bytes.
                match result {
                    Ok(Some(bytes)) => match sampler_ksp::nckp::parse(&bytes) {
                        Ok((view, skipped)) => {
                            performance_view = view;
                            let mut fields = std::collections::BTreeSet::new();
                            let mut control_types = 0;
                            for warning in &skipped {
                                match sampler_ksp::nckp::omitted_field(warning) {
                                    Some(field) => { fields.insert(field); }
                                    None => control_types += 1,
                                }
                            }
                            for field in fields {
                                view_gaps.push((format!("@performance-view-field:{field}"), format!(
                                    "Requested performance view source field is not imported: {field}"
                                )));
                            }
                            if control_types != 0 {
                                view_gaps.push(("@performance-view".into(), format!(
                                    "{} performance view control type(s) are unsupported and were omitted",
                                    control_types
                                )));
                            }
                        }
                        Err(_) => view_gaps.push(("@performance-view".into(), "Requested performance view resource could not be parsed".into())),
                    },
                    Ok(None) => view_gaps.push(("@performance-view".into(), "Requested performance view resource was not found in the library".into())),
                    Err(_) => view_gaps.push(("@performance-view".into(), "Requested performance view resource could not be read or resolved".into())),
                }
            }
            let id = format!("ksp:{prefix}slot{slot}");
            let title = format!(
                "Script slot {slot}{}",
                if script.bypass { " (bypassed)" } else { "" }
            );
            let environment = sampler_ksp::Environment {
                performance_view,
                slot: slot.min(u8::MAX as usize) as u8,
                ..Default::default()
            };
            let mut report = if summary {
                sampler_ksp::parity_report_summary_with(
                    &source,
                    &id,
                    &title,
                    sampler_ksp::Limits::LIBRARY,
                    &environment,
                )
            } else {
                sampler_ksp::parity_report_with(
                    &source,
                    &id,
                    &title,
                    sampler_ksp::Limits::LIBRARY,
                    &environment,
                )
            };
            for (name, detail) in view_gaps {
                let span = report.constructs.iter()
                    .find(|c| c.name == "load_performance_view")
                    .map(|c| c.span)
                    .unwrap_or(sampler_parity::Span { start: 0, end: 0, line: 1 });
                report.constructs.push(sampler_parity::Construct {
                    span,
                    kind: sampler_parity::ConstructKind::Syntax,
                    name,
                    status: sampler_parity::Status::Unsupported,
                    detail,
                    owner: Some("crates/sampler-ksp/src/nckp.rs".into()),
                    spec: Some("docs/architecture-v2/KSP_PARITY.md".into()),
                });
            }
            if missing_link {
                report.constructs.push(sampler_parity::Construct {
                    span: sampler_parity::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                    },
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

#[cfg(test)]
mod tests {
    fn program(source: &str, link: &str) -> ni_file::kontakt::objects::Program {
        use ni_file::kontakt::{Chunk, StructuredObject, objects::Program};
        let mut data = vec![0, 0x50, 0];
        data.extend((source.len() as u32).to_le_bytes());
        data.extend(source.as_bytes());
        data.extend([0; 3]);
        data.extend(0u32.to_le_bytes());
        data.extend(u32::MAX.to_le_bytes());
        data.extend((link.len() as u32).to_le_bytes());
        data.extend(link.as_bytes());
        Program(StructuredObject {
            version: 0xaf,
            public_data: vec![0; 70],
            private_data: Vec::new(),
            children: vec![Chunk { id: 6, data }],
        })
    }

    #[test]
    fn performance_view_resource_failures_are_retained_without_authored_values() {
        let root =
            std::env::temp_dir().join(format!("parity-view-resources-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let views = root.join("Resources/performance_view");
        std::fs::create_dir_all(&views).unwrap();
        let path = root.join("Fixture.nki");
        std::fs::write(&path, []).unwrap();
        let source = "on init\n load_performance_view(\"PRIVATE_FIXTURE_NAME\")\nend on";
        let program = program(source, "");
        for (bytes, expected) in [
            (None, "not found"),
            (Some(b"PRIVATE_INVALID_RESOURCE_BYTES".as_slice()), "could not be parsed"),
            (Some(br#"{"value":{"performanceView":{"controls":[{"index":999,"value":{"common":{"id":"PRIVATE_CONTROL_NAME"}}}]}}}"#.as_slice()), "control type"),
            (Some(br#"{"value":{"performanceView":{"controls":[]}}}"#.as_slice()), ""),
        ] {
            let resource = views.join("PRIVATE_FIXTURE_NAME.nckp");
            if let Some(bytes) = bytes {
                std::fs::write(&resource, bytes).unwrap();
            }
            let report = super::program_reports(&path, &program, "").unwrap().pop().unwrap();
            let summary = super::program_reports_mode(&path, &program, "", true)
                .unwrap()
                .pop()
                .unwrap();
            assert_eq!(summary.constructs, report.constructs);
            assert!(summary.tokens.is_empty());
            assert!(summary.lowered.is_empty());
            let gaps: Vec<_> = report.constructs.iter().filter(|c| c.name.starts_with("@performance-view")).collect();
            if expected.is_empty() {
                assert!(gaps.is_empty(), "a valid resource must not acquire a resource failure");
            } else {
                assert_eq!(gaps.len(), 1, "the requested resource failure must remain observable");
                assert_eq!(gaps[0].status, sampler_parity::Status::Unsupported);
                assert_eq!(gaps[0].span.line, 2);
                assert_eq!(gaps[0].span.start as usize, source.find("load_performance_view").unwrap());
                assert!(gaps[0].detail.contains(expected));
                assert!(!gaps[0].detail.contains("PRIVATE_"));
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn performance_view_field_losses_retain_each_canonical_key_and_source_span() {
        let root = std::env::temp_dir().join(format!("parity-view-fields-{}", std::process::id()));
        let views = root.join("Resources/performance_view");
        std::fs::create_dir_all(&views).unwrap();
        let path = root.join("Fixture.nki");
        std::fs::write(&path, []).unwrap();
        std::fs::write(views.join("PRIVATE_VIEW.nckp"), br#"{"value":{"performanceView":{"controls":[
            {"index":3,"value":{"common":{"id":"PRIVATE_CONTROL"},"value":{"min":0,"max":100},"text":{"font":{"type":3,"custom":"PRIVATE_FONT"},"fonts":{"on":{"type":4}},"shiftOnPress":false}}},
            {"index":9,"value":{"common":{"id":"PRIVATE_STEPS"},"steps":{"total":32,"visible":8}}}
        ]}}}"#).unwrap();
        let source = "on init\n load_performance_view(\"PRIVATE_VIEW\")\nend on";
        let report = super::program_reports(&path, &program(source, ""), "")
            .unwrap()
            .pop()
            .unwrap();
        let fields: Vec<_> = report
            .constructs
            .iter()
            .filter(|c| c.name.starts_with("@performance-view-field:"))
            .collect();
        assert_eq!(
            fields.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            [
                "@performance-view-field:steps.visible",
                "@performance-view-field:text.font.custom",
                "@performance-view-field:text.fonts.on",
                "@performance-view-field:text.shiftOnPress"
            ]
        );
        for field in fields {
            assert_eq!(field.status, sampler_parity::Status::Unsupported);
            assert_eq!(field.span.line, 2);
            assert_eq!(
                field.span.start as usize,
                source.find("load_performance_view").unwrap()
            );
            assert!(!field.detail.contains("PRIVATE_"));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_link_uses_embedded_fallback_and_only_reports_absent_source() {
        let path = std::env::temp_dir().join(format!(
            "parity-missing-link-{}/Piano.nki",
            std::process::id()
        ));
        for source in ["", "on init message(1) end on"] {
            let program = program(source, "Source.txt");
            let report = super::program_reports(&path, &program, "")
                .unwrap()
                .pop()
                .unwrap();
            assert_eq!(report.id, "ksp:slot0");
            assert_eq!(
                report.constructs.iter().any(|c| c.name == "@linked-source"),
                source.is_empty()
            );
            if !source.is_empty() {
                assert!(report.constructs.iter().any(|c| c.name == "message"));
            }
        }
    }
}
