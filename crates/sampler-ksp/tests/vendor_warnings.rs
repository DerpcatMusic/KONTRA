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

#[test]
fn output_channel_name_and_get_folder_evaluate_in_init() {
    let ok = warnings("on init\n declare @a\n @a := output_channel_name(1) & get_folder($GET_FOLDER_PATCH_DIR)\nend on");
    assert!(ok.is_empty(), "{ok:?}");
    let bad = warnings("on init\n declare @a\n @a := output_channel_name($NUM_OUTPUT_CHANNELS)\nend on");
    assert!(bad.iter().any(|m| m.contains("out of range")), "{bad:?}");
}

#[test]
fn bounds_warnings_retain_actual_operand_domain_and_recovery() {
    let messages = warnings("on init declare %a[2] declare $x := %a[-1] %a[2] := 7 set_key_color(128,0) declare ui_menu $m set_menu_item_value($m,13,0) end on");
    assert!(messages.iter().any(|m| m.contains("index -1") && m.contains("%a (length 2)") && m.contains("read returns")), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("index 2") && m.contains("write ignored")), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("key 128") && m.contains("0..=127") && m.contains("set_key_color ignored")), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("menu item 13") && m.contains("item count 0") && m.contains("ignored")), "{messages:?}");
}
