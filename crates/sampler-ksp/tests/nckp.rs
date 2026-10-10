use sampler_ksp::nckp::view_name;

#[test]
fn resource_name_comes_from_a_complete_literal_init_command() {
    for (source, expected) in [
        (r#"on init load_performance_view("real") end on"#, "real"),
        (
            r#"{ load_performance_view("decoy") }
               on init load_performance_view("real") end on"#,
            "real",
        ),
        (
            r#"on init message("load_performance_view")
               load_performance_view("real") end on"#,
            "real",
        ),
        (
            r#"on init message("load_performance_view(decoy)")
               load_performance_view { comment } ...
               ("réal view") end on"#,
            "réal view",
        ),
        (r#"on init load_performance_view(("real")) end on"#, "real"),
        // The current KSP lexer preserves backslashes; do not unescape them here.
        (
            r#"on init load_performance_view("folder\real") end on"#,
            r"folder\real",
        ),
        (
            r#"USE_CODE_IF(OFF)
               on init load_performance_view("decoy") end on
               END_USE_CODE
               on init load_performance_view("real") end on"#,
            "real",
        ),
        (
            r#"SET_CONDITION(ON)
               USE_CODE_IF(ON)
               on init load_performance_view("real") end on
               END_USE_CODE"#,
            "real",
        ),
        (
            r#"on init if (1 = 1) load_performance_view("real") end if end on"#,
            "real",
        ),
        (
            r#"on init while (0) load_performance_view("real") end while end on"#,
            "real",
        ),
        (
            r#"on init select (1) case 1 load_performance_view("real") end select end on"#,
            "real",
        ),
    ] {
        assert_eq!(view_name(source), Some(expected), "{source}");
    }
}

#[test]
fn resource_name_never_guesses_from_nonliteral_or_ambiguous_source() {
    for source in [
        "on init end on",
        r#"{ load_performance_view("decoy") } on init end on"#,
        r#"on init message("load_performance_view(decoy)") end on"#,
        r#"on init load_performance_view_extra("decoy") end on"#,
        r#"on init declare @view := "real" load_performance_view(@view)
           message("unrelated") end on"#,
        r#"on init load_performance_view("base" & "suffix") end on"#,
        r#"on init declare @suffix := "suffix"
           load_performance_view("base" & @suffix) end on"#,
        r#"on init load_performance_view("real", "extra") end on"#,
        r#"on init load_performance_view() message("unrelated") end on"#,
        r#"on init load_performance_view("first") load_performance_view("second") end on"#,
        r#"on init if (1 = 1) load_performance_view("first")
           else load_performance_view("second") end if end on"#,
        r#"on init load_performance_view("first") end on
           on init load_performance_view("second") end on"#,
        r#"on note load_performance_view("runtime") end on"#,
        r#"function view load_performance_view("indirect") end function
           on init call view end on"#,
        r#"on init make_perfview load_performance_view("real") end on"#,
        r#"on init load_performance_view("real") make_perfview end on"#,
        r#"on init load_performance_view("real") end"#,
        r#"on init load_performance_view("real" end on"#,
        r#"on init load_performance_view("unterminated) end on"#,
        r#"{ unterminated load_performance_view("real")"#,
        // Embedded escaped quotes are not supported by the compiler's lexer.
        r#"on init load_performance_view("escaped\"quote") end on"#,
        r#"USE_CODE_IF(OFF) on init load_performance_view("real") end on"#,
    ] {
        assert_eq!(view_name(source), None, "{source}");
    }
}
