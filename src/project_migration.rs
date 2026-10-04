//! Explicit, mapping-driven project migration helpers.

use anyhow::{Context, Result, ensure};
use moose::core::{PluginExport, PluginRuntime, state};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use crate::{
    MigrationPart as Part, MigrationSavedMulti as SavedMulti, MigrationSelection as Selection,
    Plugin,
};

const ROOT_FIELDS: &[&str] = &["format", "version", "name", "parts"];
const PART_FIELDS: &[&str] = &[
    "path",
    "group",
    "port",
    "output",
    "channel",
    "gain",
    "pan",
    "tune",
    "mute",
    "solo",
    "program",
    "script_state",
    "name",
    "collapsed",
    "height",
    "aux",
    "aux_gain",
    "articulate",
    "mpe",
    "streaming",
    "edits",
    "timing",
    "output_manual",
    "mic_buses",
    "mic_names",
    "view",
    "ir_settings",
    "snapshot",
    "engine_state",
    "delay_state",
    "uvi",
    "uvi_state",
];
const REQUIRED_ROUTE_FIELDS: &[&str] = &[
    "channel",
    "port",
    "output",
    "aux",
    "aux_gain",
    "output_manual",
    "mic_buses",
    "mic_names",
];

#[derive(Serialize)]
pub struct StateExportReport {
    pub status: &'static str,
    pub multi: String,
    pub state: String,
    pub parts: usize,
    pub state_bytes: usize,
    pub envelope_round_trip_verified: bool,
    pub sample_references: &'static str,
    pub imported_parts: Vec<ImportedPartReport>,
    pub limitations: Vec<&'static str>,
}

#[derive(Serialize)]
pub struct ImportedPartReport {
    pub part: usize,
    pub path: String,
    pub name: String,
    pub first_playable_group: u32,
    pub zones: usize,
    pub available_zones: usize,
    pub missing_samples: Vec<String>,
    pub import_warnings: Vec<String>,
    pub effect_warnings: Vec<String>,
    pub script_persistence: &'static str,
}

#[derive(Serialize)]
pub struct StateBlobComparisonReport {
    pub status: &'static str,
    pub expected: StateBlobSummary,
    pub readback: StateBlobSummary,
    pub matches: StateSectionMatches,
    pub selection_differences: Vec<String>,
}

#[derive(Serialize)]
pub struct StateBlobSummary {
    pub path: String,
    pub bytes: usize,
    pub parameter_count: usize,
    pub extra_bytes: usize,
    pub persist_bytes: usize,
    pub multi: String,
    pub parts: Vec<StatePartSummary>,
    pub order: Vec<u32>,
}

#[derive(Serialize)]
pub struct StatePartSummary {
    pub path: String,
    pub name: String,
    pub group: u32,
    pub channel: i16,
    pub port: u8,
    pub output: u8,
}

#[derive(Serialize)]
pub struct StateSectionMatches {
    pub parameter_values: bool,
    pub selection: bool,
    pub extra_bytes: bool,
    pub persistence_bytes: bool,
    pub whole_blob: bool,
}

/// Compare two KONTRA state envelopes through the same parser and restore path the plugin uses.
pub fn compare_state_blobs(
    expected_path: &Path,
    readback_path: &Path,
) -> Result<StateBlobComparisonReport> {
    let expected =
        fs::read(expected_path).with_context(|| format!("Reading {}", expected_path.display()))?;
    let readback =
        fs::read(readback_path).with_context(|| format!("Reading {}", readback_path.display()))?;
    let plugin_id = state::hash_plugin_id(Plugin::info().clap_id);
    let expected_sections = state::deserialize_state(&expected, plugin_id)
        .context("Expected blob is not a valid KONTRA state envelope")?;
    let readback_sections = state::deserialize_state(&readback, plugin_id)
        .context("REAPER readback is not a valid KONTRA state envelope")?;
    let expected_selection = selection_from_blob(&expected)?;
    let readback_selection = selection_from_blob(&readback)?;
    let selection_differences = selection_differences(&expected_selection, &readback_selection);
    let matches = StateSectionMatches {
        parameter_values: expected_sections.params == readback_sections.params,
        selection: expected_selection == readback_selection,
        extra_bytes: expected_sections.extra == readback_sections.extra,
        persistence_bytes: expected_sections.persist == readback_sections.persist,
        whole_blob: expected == readback,
    };
    let status = if matches.parameter_values
        && matches.selection
        && matches.extra_bytes
        && matches.persistence_bytes
    {
        "semantically_matching"
    } else {
        "state_mismatch"
    };
    Ok(StateBlobComparisonReport {
        status,
        expected: state_blob_summary(
            expected_path,
            expected.len(),
            &expected_sections,
            expected_selection,
        ),
        readback: state_blob_summary(
            readback_path,
            readback.len(),
            &readback_sections,
            readback_selection,
        ),
        matches,
        selection_differences,
    })
}

fn selection_differences(expected: &Selection, readback: &Selection) -> Vec<String> {
    let mut differences = Vec::new();
    macro_rules! compare_fields {
        ($left:ident, $right:ident; $($field:ident),+ $(,)?) => {
            $(if $left.$field != $right.$field {
                differences.push(stringify!($field).to_owned());
            })+
        };
    }
    compare_fields!(expected, readback;
        root, order, midi_thru, multi, favorites, recent, qwerty, browser_width,
        browser_split, appearance, sharp_artwork, sticky_off, buses, streaming,
        auto_align, align_transport_only, outputs,
        uvi_favorites, uvi_recent, uvi_requested,
    );
    if expected.parts.len() != readback.parts.len() {
        differences.push("parts.length".to_owned());
    }
    for (index, (left, right)) in expected.parts.iter().zip(&readback.parts).enumerate() {
        let start = differences.len();
        compare_fields!(left, right;
            path, group, port, output, channel, gain, pan, tune, mute, solo, program,
            script_state, ir_settings, name, collapsed, height, aux, aux_gain,
            articulate, mpe, streaming, edits, timing, output_manual, mic_buses,
            mic_names, view, snapshot, engine_state, delay_state, uvi, uvi_state,
        );
        for field in &mut differences[start..] {
            *field = format!("parts[{index}].{field}");
        }
    }
    differences
}

fn selection_from_blob(bytes: &[u8]) -> Result<Selection> {
    let mut plugin = Plugin::create();
    state::restore_plugin(&mut plugin, bytes).context("Restoring KONTRA state envelope")?;
    Ok(plugin
        .params()
        .selection
        .read()
        .map_err(|_| anyhow::anyhow!("KONTRA selection lock was poisoned"))?
        .clone())
}

fn state_blob_summary(
    path: &Path,
    bytes: usize,
    sections: &state::DeserializedState,
    selection: Selection,
) -> StateBlobSummary {
    StateBlobSummary {
        path: path.to_string_lossy().into_owned(),
        bytes,
        parameter_count: sections.params.len(),
        extra_bytes: sections.extra.as_ref().map_or(0, Vec::len),
        persist_bytes: sections.persist.len(),
        multi: selection.multi,
        parts: selection
            .parts
            .into_iter()
            .map(|part| StatePartSummary {
                path: part.path,
                name: part.name,
                group: part.group,
                channel: part.channel,
                port: part.port,
                output: part.output,
            })
            .collect(),
        order: selection.order,
    }
}

/// Export an OAST blob from a KONTRA-owned SavedMulti, never from a Kontakt chunk.
pub fn export_multi_state(multi_path: &Path, state_path: &Path) -> Result<StateExportReport> {
    ensure!(
        crate::import::is_saved_multi(multi_path),
        "input must be a .kontra-multi mapping"
    );
    let multi_path = multi_path
        .canonicalize()
        .context("Resolving SavedMulti path")?;
    let source =
        fs::read(&multi_path).with_context(|| format!("Reading {}", multi_path.display()))?;
    let json: serde_json::Value =
        serde_json::from_slice(&source).context("Parsing SavedMulti JSON")?;
    validate_shape(&json)?;
    let multi = SavedMulti::read(&multi_path).context("Reading KONTRA SavedMulti")?;
    ensure!(
        multi.version == 1,
        "only KONTRA SavedMulti version 1 is supported"
    );
    ensure!(
        !multi.parts.is_empty(),
        "SavedMulti contains no instruments"
    );

    let mut imported_parts = Vec::with_capacity(multi.parts.len());
    for (index, part) in multi.parts.iter().enumerate() {
        imported_parts.push(validate_part(part, index)?);
    }

    let mut parts = multi.parts;
    for (part, imported) in parts.iter_mut().zip(&imported_parts) {
        // Match the worker's visible mapping-selection default. Every NKI
        // group still plays; this only avoids a host-load state rewrite.
        part.group = canonical_group(part.group, imported.first_playable_group);
    }
    let selection = Selection {
        order: (0..parts.len()).map(|index| index as u32).collect(),
        parts,
        multi: multi_path.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let plugin = Plugin::create();
    *plugin
        .params()
        .selection
        .write()
        .map_err(|_| anyhow::anyhow!("KONTRA selection lock was poisoned"))? = selection.clone();
    let blob = state::snapshot_plugin(&plugin);

    // Use the plugin's own state parser and persistence restore path before writing anything.
    let mut restored = Plugin::create();
    state::restore_plugin(&mut restored, &blob)
        .map_err(|error| anyhow::anyhow!("KONTRA rejected its generated state: {error}"))?;
    let restored_selection = restored
        .params()
        .selection
        .read()
        .map_err(|_| anyhow::anyhow!("KONTRA selection lock was poisoned"))?
        .clone();
    ensure!(
        restored_selection == selection,
        "KONTRA state round-trip changed the mapped multi"
    );
    ensure!(
        state::snapshot_plugin(&restored) == blob,
        "KONTRA state envelope was not stable after restore"
    );

    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(state_path)
        .with_context(|| {
            format!(
                "Creating state file {} without overwriting",
                state_path.display()
            )
        })?;
    if let Err(error) = out.write_all(&blob) {
        drop(out);
        let _ = fs::remove_file(state_path);
        return Err(error).with_context(|| format!("Writing state file {}", state_path.display()));
    }

    Ok(StateExportReport {
        status: "experimental_state_exported",
        multi: multi_path.to_string_lossy().into_owned(),
        state: state_path.to_string_lossy().into_owned(),
        parts: selection.parts.len(),
        state_bytes: blob.len(),
        envelope_round_trip_verified: true,
        sample_references: "NKI/NKM parsed; missing samples, importer warnings and available zones are reported per part; audio decoding and Kontakt parity are not verified",
        imported_parts,
        limitations: vec![
            "Kontakt opaque plugin state and exposed parameter values are not translated.",
            "Explicit SavedMulti script persistence is kept unchanged; opaque Kontakt KSP state and unmapped controls cannot be recovered.",
            "SavedMulti does not contain rack-level output bus settings; KONTRA rack defaults are used.",
            "Kontakt output pin assignments and unrecognized routing remain unmapped.",
        ],
    })
}

fn validate_shape(json: &serde_json::Value) -> Result<()> {
    let root = json
        .as_object()
        .context("SavedMulti root must be an object")?;
    for field in root.keys() {
        ensure!(
            ROOT_FIELDS.contains(&field.as_str()),
            "SavedMulti has unsupported top-level field {field:?}"
        );
    }
    let parts = root
        .get("parts")
        .and_then(serde_json::Value::as_array)
        .context("SavedMulti must contain a parts array")?;
    for (index, value) in parts.iter().enumerate() {
        let part = value
            .as_object()
            .with_context(|| format!("part {index} must be an object"))?;
        for field in part.keys() {
            ensure!(
                PART_FIELDS.contains(&field.as_str()),
                "part {index} has unsupported field {field:?}"
            );
        }
        for field in REQUIRED_ROUTE_FIELDS {
            ensure!(
                part.contains_key(*field),
                "part {index} omits explicit routing field {field:?}"
            );
        }
    }
    Ok(())
}

fn validate_part(part: &Part, index: usize) -> Result<ImportedPartReport> {
    let label = if part.name.is_empty() {
        part.path.as_str()
    } else {
        part.name.as_str()
    };
    ensure!(
        part.uvi.is_none() && part.uvi_state.is_empty(),
        "part {index} ({label}) contains native UVI source or state; Kontakt mapping export cannot migrate it"
    );
    ensure!(
        !part.path.is_empty(),
        "part {index} ({label}) has no instrument path"
    );
    ensure!(
        part.channel == -1 || (0..=15).contains(&part.channel),
        "part {index} has an invalid MIDI channel"
    );
    ensure!(part.port <= 3, "part {index} has an invalid MIDI port");
    ensure!(part.output <= 7, "part {index} has an invalid output bus");
    ensure!(
        (-1..=15).contains(&part.aux),
        "part {index} has an invalid auxiliary bus"
    );
    ensure!(
        part.mic_buses.len() <= 16 && part.mic_buses.iter().all(|bus| (-1..=15).contains(bus)),
        "part {index} has invalid mic-bus routing"
    );
    ensure!(
        part.gain.is_finite()
            && (-60.0..=6.0).contains(&part.gain)
            && part.pan.is_finite()
            && (-1.0..=1.0).contains(&part.pan)
            && part.tune.is_finite()
            && (-36.0..=36.0).contains(&part.tune)
            && part.aux_gain.is_finite()
            && (-60.0..=6.0).contains(&part.aux_gain),
        "part {index} has out-of-range gain, pan, tuning, or send level"
    );
    validate_script_state(&part.script_state, index)?;

    let path = Path::new(&part.path);
    ensure!(
        path.is_file(),
        "part {index} instrument is missing: {}",
        path.display()
    );
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    ensure!(
        extension.eq_ignore_ascii_case("nki") || extension.eq_ignore_ascii_case("nkm"),
        "part {index} must reference an NKI or NKM file"
    );
    let instrument = crate::import::read_program(path, part.program)
        .with_context(|| format!("Parsing part {index} {}", path.display()))?;
    let first_playable_group = instrument
        .first_playable_group()
        .with_context(|| format!("part {index} has no playable sample zones"))?;
    let first_playable_group = u32::try_from(first_playable_group)
        .with_context(|| format!("part {index} playable group index is out of range"))?;
    Ok(ImportedPartReport {
        part: index,
        path: instrument.path.to_string_lossy().into_owned(),
        name: if instrument.name.is_empty() {
            label.to_owned()
        } else {
            instrument.name
        },
        first_playable_group,
        zones: instrument.zones.len(),
        available_zones: instrument
            .zones
            .iter()
            .filter(|zone| zone.available)
            .count(),
        missing_samples: instrument.missing_samples,
        import_warnings: instrument.warnings,
        effect_warnings: instrument.fx.warnings(),
        script_persistence: if part.script_state.trim().is_empty() {
            "not supplied"
        } else {
            "supplied and preserved"
        },
    })
}

fn canonical_group(group: u32, first_playable_group: u32) -> u32 {
    if group == u32::MAX {
        first_playable_group
    } else {
        group
    }
}

fn validate_script_state(saved: &str, index: usize) -> Result<()> {
    if !saved.trim().is_empty() {
        let _: Vec<crate::ksp::Persisted> = serde_json::from_str(saved)
            .with_context(|| format!("part {index} has invalid script persistence JSON"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use moose::core::PluginExport;

    #[test]
    fn generated_envelope_round_trips_selection_through_plugin_parser() {
        let part = Part {
            path: "/fixture/Piano.nki".into(),
            channel: 3,
            port: 1,
            output: 2,
            gain: -6.5,
            pan: -0.5,
            tune: 7.02,
            name: "Left hand".into(),
            script_state: serde_json::to_string(&vec![
                [("$legato".to_owned(), crate::ksp::Value::Int(1))]
                    .into_iter()
                    .collect::<crate::ksp::Persisted>(),
            ])
            .unwrap(),
            ..Default::default()
        };
        let expected = Selection {
            parts: vec![part],
            order: vec![0],
            multi: "/fixture/Evening.kontra-multi".into(),
            ..Default::default()
        };
        assert!(validate_script_state(&expected.parts[0].script_state, 0).is_ok());
        assert!(validate_script_state(r#"[{"$legato":{"value":1}}]"#, 0).is_err());
        let plugin = Plugin::create();
        *plugin.params().selection.write().unwrap() = expected.clone();
        let blob = state::snapshot_plugin(&plugin);
        let mut restored = Plugin::create();
        state::restore_plugin(&mut restored, &blob).unwrap();
        assert!(*restored.params().selection.read().unwrap() == expected);
        assert_eq!(state::snapshot_plugin(&restored), blob);
    }

    #[test]
    fn selection_difference_report_identifies_changed_part_fields() {
        let mut expected = Selection {
            parts: vec![Part {
                path: "Piano.nki".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut readback = expected.clone();
        readback.parts[0].script_state = "[]".into();
        assert_eq!(
            selection_differences(&expected, &readback),
            ["parts[0].script_state"]
        );
        expected.parts[0].script_state = "[]".into();
        assert!(selection_differences(&expected, &readback).is_empty());
    }

    #[test]
    fn compare_state_accepts_only_section_equal_host_padding() {
        let plugin = Plugin::create();
        *plugin.params().selection.write().unwrap() = Selection {
            parts: vec![Part {
                path: "/fixture/Piano.nki".into(),
                ..Default::default()
            }],
            order: vec![0],
            ..Default::default()
        };
        let expected = state::snapshot_plugin(&plugin);
        let mut host_readback = expected.clone();
        host_readback.extend_from_slice(&[0; 8]);
        let root = std::env::temp_dir().join(format!(
            "kontakto-reaper-state-compare-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let expected_path = root.join("expected.state");
        let readback_path = root.join("readback.state");
        std::fs::write(&expected_path, expected).unwrap();
        std::fs::write(&readback_path, host_readback).unwrap();

        let report = compare_state_blobs(&expected_path, &readback_path).unwrap();
        assert_eq!(report.status, "semantically_matching");
        assert!(report.matches.parameter_values);
        assert!(report.matches.selection);
        assert!(report.matches.extra_bytes);
        assert!(report.matches.persistence_bytes);
        assert!(!report.matches.whole_blob);
        assert!(report.selection_differences.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn current_saved_multi_keeps_inert_native_fields_and_legacy_defaults() {
        let selection = Selection {
            parts: vec![Part {
                path: "Piano.nki".into(),
                ..Default::default()
            }],
            order: vec![0],
            ..Default::default()
        };
        let saved = SavedMulti::of("Compatibility", &selection);
        let json = serde_json::to_value(&saved).unwrap();
        assert_eq!(json["parts"][0]["uvi"], serde_json::Value::Null);
        assert_eq!(json["parts"][0]["uvi_state"], serde_json::json!([]));
        validate_shape(&json).unwrap();
        let round_trip: SavedMulti = serde_json::from_value(json.clone()).unwrap();
        assert!(round_trip.parts == saved.parts);
        let path = std::env::temp_dir().join(format!(
            "kontra-migration-native-fields-{}.kontra-multi",
            std::process::id()
        ));
        saved.save(&path).unwrap();
        let from_disk = SavedMulti::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(from_disk.parts == saved.parts);
        validate_shape(&serde_json::to_value(&from_disk).unwrap()).unwrap();
        let mut legacy = json.clone();
        let part = legacy["parts"][0].as_object_mut().unwrap();
        part.remove("uvi");
        part.remove("uvi_state");
        validate_shape(&legacy).unwrap();
        let legacy: SavedMulti = serde_json::from_value(legacy).unwrap();
        assert!(legacy.parts == saved.parts);
        let mut unknown = json;
        unknown["parts"][0]["unknown_backend"] = serde_json::json!({});
        assert!(validate_shape(&unknown).is_err());
    }

    #[test]
    fn kontakt_export_rejects_native_identity_or_state_before_import() {
        let source = crate::library::UviSource {
            bank: "unused.ufs".into(),
            bank_uuid: [7; 16],
            member: "Piano.uvip".into(),
        };
        for (uvi, uvi_state) in [(Some(source), Vec::new()), (None, vec![1, 2, 3])] {
            let part = Part {
                path: "unused.nki".into(),
                uvi,
                uvi_state: uvi_state.into(),
                ..Default::default()
            };
            let error = validate_part(&part, 0).err().unwrap().to_string();
            assert!(error.contains("native UVI source or state"), "{error}");
        }
    }

    #[test]
    fn selection_difference_report_identifies_native_persistence_and_cursor() {
        let expected = Selection {
            parts: vec![Part::default()],
            ..Default::default()
        };
        let source = crate::library::UviSource {
            bank: "unused.ufs".into(),
            bank_uuid: [7; 16],
            member: "Piano.uvip".into(),
        };
        let mut readback = expected.clone();
        readback.parts[0].uvi = Some(source.clone());
        readback.parts[0].uvi_state = vec![1, 2, 3].into();
        readback.uvi_favorites.push(source.clone());
        readback.uvi_recent.push(source.clone());
        readback.uvi_requested = Some(crate::library::UviRequest {
            source,
            slot: Some(0),
            new: false,
        });
        assert_eq!(
            selection_differences(&expected, &readback),
            [
                "uvi_favorites",
                "uvi_recent",
                "uvi_requested",
                "parts[0].uvi",
                "parts[0].uvi_state",
            ]
        );
    }

    #[test]
    fn export_canonicalizes_only_default_group_selection() {
        assert_eq!(canonical_group(u32::MAX, 3), 3);
        assert_eq!(canonical_group(2, 3), 2);
    }
}
