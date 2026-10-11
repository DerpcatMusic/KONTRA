fn warnings(source: &str) -> Vec<String> {
    sampler_ksp::compile(
        source,
        48000,
        sampler_ksp::Limits {
            source_bytes: 65536,
            instructions: 4096,
            variables: 16,
            array_cells: 16,
        },
        &[],
    )
    .unwrap()
    .warnings()
    .iter()
    .map(|w| w.message.clone())
    .collect()
}

#[test]
fn catalog_vendor_constants_are_silent_and_unknown_ones_say_so() {
    let known = warnings(
        "on init\n declare $x\n $x := $NI_NKS_STYLE_KNOB\n $x := $INST_LIB_COPYRIGHT_ID\n $x := $FILTER_TYPE_SV_LP1\nend on",
    );
    assert!(known.iter().all(|m| !m.contains("opaque")), "{known:?}");
    let unknown = warnings("on init\n declare $x\n $x := $NOT_A_KONTAKT_NAME\nend on");
    assert!(unknown.iter().any(|m| m.contains("not a known Kontakt constant")), "{unknown:?}");
}

#[test]
fn groups_selected_reads_zero_without_a_warning() {
    let w = warnings("on init\n declare $x\n $x := %GROUPS_SELECTED[0]\nend on");
    assert!(w.is_empty(), "{w:?}");
}
