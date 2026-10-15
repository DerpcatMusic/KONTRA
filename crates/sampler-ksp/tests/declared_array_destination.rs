//! Authored scripts only; no installed library or native-host fixtures.
use sampler_ksp::{Environment, Kind, Limits};

const LIMITS: Limits = Limits {
    source_bytes: 4096,
    instructions: 4096,
    variables: 16,
    array_cells: 32,
};

#[test]
fn get_scale_rejects_runtime_owned_destinations_before_init_or_callback_lowering() {
    for array in [
        "%KEY_DOWN",
        "%CC",
        "%CC_TOUCHED",
        "%POLY_AT",
        "%GROUPS_SELECTED",
        "%GROUPS_AFFECTED",
        "%KEY_DOWN_OCT",
        "%EVENT_PAR",
        "!NI_DND_ITEMS_AUDIO",
        "!NI_DND_ITEMS_MIDI",
        "!NI_DND_ITEMS_ARRAY",
    ] {
        for callback in ["init", "persistence_changed", "note"] {
            let source = if callback == "init" {
                format!("on init\n get_scale(0, {array})\nend on")
            } else {
                format!("on init\nend on\non {callback}\n get_scale(0, {array})\nend on")
            };
            let initialized = sampler_ksp::initialize(&source, LIMITS, &Environment::default())
                .err()
                .expect("initialization must return an error without panicking");
            let compiled = sampler_ksp::compile(&source, 48000, LIMITS, &[])
                .err()
                .expect("compilation must reject the unowned destination");
            for error in [initialized, compiled] {
                assert_eq!(error.kind, Kind::Error, "{source}");
                assert_eq!(error.builtin, Some("get_scale"), "{source}");
                assert_eq!(
                    error.message, "get_scale requires a declared destination array",
                    "{source}"
                );
                assert_eq!(error.offset, source.find(array).unwrap(), "{source}");
                assert_eq!(error.column, 15, "{source}: {error:?}");
            }
        }
    }
}

#[test]
fn declared_destinations_and_readable_system_arrays_still_compile() {
    // Existing arrays.rs::get_scale_fills_semitones_at_init_and_in_callbacks
    // verifies the exact init and callback values for these declared homes.
    let source = "on init
 declare %notes[8]
 declare $count := num_elements(%CC)
 declare $first := search(%KEY_DOWN, 0)
 get_scale(5, %notes)
end on
on note
 get_scale(2, %notes)
end on";
    let script = sampler_ksp::compile(source, 48000, LIMITS, &[]).unwrap();
    assert!(script.warnings().is_empty(), "{:?}", script.warnings());
    assert!(script.coverage().iter().any(|&(name, coverage, _)| {
        name == "get_scale" && coverage == sampler_ksp::Coverage::Native
    }));
}
