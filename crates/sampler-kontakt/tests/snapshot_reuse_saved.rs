//! Source-free reader/schema oracles. These do not execute installed scripts,
//! initialize a Runtime, decode PCM, create UI, or mutate an installed library.
#[path = "support/snapshot_reuse.rs"]
mod fixture;
use sampler_ir::Saved;
use sampler_kontakt::{
    read_snapshot, read_snapshot_for_base, snapshot_base_name, snapshot_identity,
    validate_snapshot_base,
};
use sampler_ksp::setup::{
    SnapshotSavedError, SnapshotSavedLimits, SnapshotSavedRowError, SnapshotSavedSlot,
    snapshot_policy, snapshot_saved_state,
};

const NAME: &str = "Authored Snapshot Base";
const FIRST: &str = "declare $integer make_persistent($integer) declare ~real make_persistent(~real) declare @text make_persistent(@text) declare %ints[2] make_persistent(%ints) declare ?reals[2] make_persistent(?reals) declare !texts[3] make_persistent(!texts)";
const LAST: &str = "on init declare $last make_persistent($last) end on";
const ROWS: &[&str] = &[
    "$integer -7",
    "~real 1.25",
    "@text  two words\nnext ",
    "%ints 1 -2",
    "?reals 0.25 -0.0",
    "!texts first line\n\nlast\n",
];
fn limits() -> SnapshotSavedLimits {
    SnapshotSavedLimits {
        value_entries: 8192,
        text_bytes: 8 << 20,
    }
}
fn incoming(state: &sampler_kontakt::SnapshotState) -> Vec<SnapshotSavedSlot> {
    state
        .saved_values()
        .unwrap()
        .into_iter()
        .enumerate()
        .map(|(slot, values)| SnapshotSavedSlot {
            slot: slot as u8,
            values,
        })
        .collect()
}

#[test]
fn authored_script_only_file_preserves_all_typed_rows_and_sparse_physical_slots() {
    let dir = fixture::directory();
    let path = dir.path().join("saved.nksn");
    std::fs::write(
        &path,
        fixture::script_only_snapshot(NAME, "", &[ROWS, &[], &[], &[], &["$last 42"]]),
    )
    .unwrap();
    let snapshot = read_snapshot(&path).unwrap();
    assert_eq!(snapshot.instrument, NAME);
    assert!(snapshot.groups.is_empty());
    assert!(snapshot.effects.is_empty());
    assert!(snapshot.other_files.is_empty());
    assert_eq!(snapshot.persistent.len(), 5);
    assert!(snapshot.persistent[1..4].iter().all(Vec::is_empty));
    let rows = snapshot.saved_values().unwrap();
    assert_eq!(
        rows[0],
        vec![
            ("$integer".into(), Saved::Int(-7)),
            ("~real".into(), Saved::Real(1.25)),
            ("@text".into(), Saved::Text(" two words\nnext ".into())),
            ("%ints".into(), Saved::Ints(vec![1, -2])),
            ("?reals".into(), Saved::Reals(vec![0.25, -0.0])),
            (
                "!texts".into(),
                Saved::Texts(vec!["first line".into(), "".into(), "last".into()])
            ),
        ]
    );
    let Saved::Reals(real) = &rows[0][4].1 else {
        panic!()
    };
    assert_eq!(real[1].to_bits(), (-0.0f64).to_bits());
    assert_eq!(rows[4], vec![("$last".into(), Saved::Int(42))]);
    assert_eq!(snapshot_identity(&path).unwrap(), NAME);
}

#[test]
fn minimal_authored_base_owns_resources_and_maps_saved_rows_for_modes_one_and_three() {
    let dir = fixture::directory();
    let base = dir.path().join(format!("{NAME}.nki"));
    let path = dir.path().join("saved.nksn");
    std::fs::write(
        &path,
        fixture::script_only_snapshot(NAME, "", &[ROWS, &[], &[], &[], &["$last 42"]]),
    )
    .unwrap();
    for mode in [1, 3] {
        let first = format!("on init set_snapshot_type({mode}) {FIRST} end on");
        std::fs::write(
            &base,
            fixture::base_instrument(NAME, &[Some(&first), None, None, None, Some(LAST)]),
        )
        .unwrap();
        assert_eq!(snapshot_base_name(&base).unwrap(), NAME);
        let resources = sampler_kontakt::Resources::for_program(&base, 0).unwrap();
        assert!(resources.namespace_complete());
        assert_eq!(resources.root(), dir.path());
        assert!(resources.locations().is_empty());
        let parsed = sampler_kontakt::read(&base).unwrap();
        assert!(parsed.instrument.zones.is_empty());
        assert_eq!(
            parsed
                .instrument
                .behaviors
                .iter()
                .map(|behavior| behavior.slot)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(4)]
        );
        validate_snapshot_base(&path, &base).unwrap();
        let snapshot = read_snapshot_for_base(&path, &base).unwrap();
        let scripts: Vec<_> = parsed
            .instrument
            .behaviors
            .iter()
            .map(|behavior| {
                sampler_ksp::compile_with(
                    &behavior.source,
                    48_000,
                    sampler_ksp::Limits::LIBRARY,
                    &[],
                    &sampler_ksp::Environment {
                        slot: behavior.slot.unwrap(),
                        ..Default::default()
                    },
                )
                .unwrap()
            })
            .collect();
        let views: Vec<_> = scripts.iter().map(sampler_ksp::Script::view).collect();
        let policy = snapshot_policy(&views).unwrap();
        assert!(!policy.mode.reruns_init());
        assert_eq!(policy.writer.unwrap().0, 0);
        assert_eq!(
            policy.mode,
            sampler_ksp::model::SnapshotMode::from_native(mode).unwrap()
        );
        let state = snapshot_saved_state(&views, &incoming(&snapshot), limits()).unwrap();
        assert_eq!(state.values.len(), 11);
        assert_eq!(state.texts.len(), 4);
    }
}

#[test]
fn malformed_rows_fail_typed_decode_and_duplicate_names_survive_for_schema_rejection() {
    let dir = fixture::directory();
    let path = dir.path().join("malformed.nksn");
    for row in [
        "$integer invalid",
        "%ints 1 invalid",
        "$integer 2147483648",
        "$bad-name 1",
        "#unknown 1",
    ] {
        std::fs::write(
            &path,
            fixture::script_only_snapshot(NAME, "", &[&[row], &[], &[], &[], &[]]),
        )
        .unwrap();
        let snapshot = read_snapshot(&path).unwrap();
        assert!(
            snapshot.saved_values().is_err(),
            "malformed authored row was admitted"
        );
    }
    // Lexically valid nonfinite numbers retain their type at decode, then the
    // incoming setup domain refuses them before any live transaction exists.
    for (source, row, name) in [
        (
            "on init declare ~real make_persistent(~real) end on",
            "~real NaN",
            "~real",
        ),
        (
            "on init declare ?reals[2] make_persistent(?reals) end on",
            "?reals 0.5 inf",
            "?reals",
        ),
    ] {
        std::fs::write(
            &path,
            fixture::script_only_snapshot(NAME, "", &[&[row], &[], &[], &[], &[]]),
        )
        .unwrap();
        let snapshot = read_snapshot(&path).unwrap();
        snapshot.saved_values().unwrap();
        let view = sampler_ksp::compile(source, 48_000, sampler_ksp::Limits::LIBRARY, &[])
            .unwrap()
            .view();
        assert_eq!(
            snapshot_saved_state(&[view], &incoming(&snapshot), limits()).unwrap_err(),
            SnapshotSavedError::Row {
                slot: 0,
                name: name.into(),
                reason: SnapshotSavedRowError::Domain
            }
        );
    }
    std::fs::write(
        &path,
        fixture::script_only_snapshot(
            NAME,
            "",
            &[&["$integer 1", "$integer 2"], &[], &[], &[], &[]],
        ),
    )
    .unwrap();
    let snapshot = read_snapshot(&path).unwrap();
    assert_eq!(
        snapshot.saved_values().unwrap()[0],
        vec![
            ("$integer".into(), Saved::Int(1)),
            ("$integer".into(), Saved::Int(2))
        ]
    );
    let script = sampler_ksp::compile(
        "on init declare $integer make_persistent($integer) end on",
        48_000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    assert_eq!(
        snapshot_saved_state(&[script.view()], &incoming(&snapshot), limits()).unwrap_err(),
        SnapshotSavedError::Row {
            slot: 0,
            name: "$integer".into(),
            reason: SnapshotSavedRowError::Duplicate
        }
    );
}

#[test]
fn explicit_base_selection_validates_embedded_program_and_generic_filename_independently() {
    let dir = fixture::directory();
    let good = dir.path().join(format!("{NAME}.nki"));
    let wrong_name = dir.path().join("Wrong.nki");
    let renamed = dir.path().join("Renamed.nki");
    let path = dir.path().join("saved.nksn");
    std::fs::write(&good, fixture::base_instrument(NAME, &[None; 5])).unwrap();
    std::fs::write(
        &wrong_name,
        fixture::base_instrument("Other Program", &[None; 5]),
    )
    .unwrap();
    std::fs::write(&renamed, fixture::base_instrument(NAME, &[None; 5])).unwrap();
    std::fs::write(
        &path,
        fixture::script_only_snapshot(NAME, "Other Content", &[&[]; 5]),
    )
    .unwrap();
    validate_snapshot_base(&path, &good).unwrap();
    assert!(read_snapshot_for_base(&path, &wrong_name).is_err());
    // An ordinary authored identity binds the program, regardless of filename.
    validate_snapshot_base(&path, &renamed).unwrap();
    std::fs::write(
        &path,
        fixture::script_only_snapshot("Kontakt", NAME, &[&[]; 5]),
    )
    .unwrap();
    assert_eq!(snapshot_identity(&path).unwrap(), NAME);
    assert_eq!(
        read_snapshot_for_base(&path, &good).unwrap().instrument,
        "Kontakt"
    );
    assert!(validate_snapshot_base(&path, &renamed).is_err());
    assert!(read_snapshot_for_base(&path, &wrong_name).is_err());
    // A generic template program is also valid when the selected filename is
    // exactly the embedded content identity.
    std::fs::write(&good, fixture::base_instrument("Kontakt", &[None; 5])).unwrap();
    read_snapshot_for_base(&path, &good).unwrap();
    // Correct filename alone cannot authorize a different embedded program.
    std::fs::write(&good, fixture::base_instrument("Other Program", &[None; 5])).unwrap();
    assert!(read_snapshot_for_base(&path, &good).is_err());

    // A parseable state without metadata cannot acquire an explicit base.
    std::fs::write(&good, fixture::base_instrument(NAME, &[None; 5])).unwrap();
    let mut body = vec![0, 3, 0]; // Unstructured snapshot v3.
    body.extend(3u32.to_le_bytes());
    body.extend([0; 20]); // All five physical slots empty.
    std::fs::write(&path, fixture::container(&fixture::chunk(0x4f, &body))).unwrap();
    assert_eq!(read_snapshot(&path).unwrap().instrument, "");
    assert!(read_snapshot_for_base(&path, &good).is_err());
    let mut sixth = body.clone();
    sixth.extend(0u32.to_le_bytes());
    let mut flags = body.clone();
    flags[3..7].copy_from_slice(&1u32.to_le_bytes());
    let short = body[..body.len() - 1].to_vec();
    for bad in [sixth, flags, short] {
        std::fs::write(&path, fixture::container(&fixture::chunk(0x4f, &bad))).unwrap();
        assert!(read_snapshot(&path).is_err());
    }
}
