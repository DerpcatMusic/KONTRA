//! Kontakt "Mangling" effects (Beat Masher, Beat Slicer, Gater, Reverse Grain,
//! Transpose Stretch). None has a DSP mapping yet: the shared engine has no
//! gate, granular, slicer or stretch core. Each stays a reported
//! `NotModeled` slot with the reason below; the saved fields are not bound.
//!
//! Evidence (repo, not the Kontakt 8 decompile): serialization IDs and reader
//! VAs in `docs/re/kontakt-catalog/ser_classes.tsv`; field layouts in
//! `reader_schemas.tsv`; Gater (0x72) and Reverse Grain (0x74) decode to typed
//! `param_N` fields with no assigned law (`docs/architecture-v2/KONTAKT_FX_MOD_FORMAT.md`).
//! Details: `docs/re/engine/k-mangling.md`.

/// Why a mangling module without a mapping stays unsupported.
pub(super) fn unproved(module: u16) -> Option<&'static str> {
    Some(match module {
        0x71 => "Beat Masher: reader VA 0x140ac4ee0 (fields FFFBBBI) is catalogued, but its repeat/slice engine and parameter laws are not extracted",
        0x72 => "Gater: reader VA 0x140ac5980 (fields Fx4Bx4) decodes, but the gate envelope, step pattern and parameter laws are not extracted",
        0x73 => "Beat Slicer: reader VA 0x140ac5070 (fields FFBBBII) is catalogued, but the slice playback engine and parameter laws are not extracted",
        0x74 => "Reverse Grain: reader VA 0x140ac5fa0 (fields Fx4BBB) decodes, but its grain engine and parameter laws are not extracted; its SerType 0x74 is shared with Transpose Stretch in the catalogue (unresolved)",
        0x75 => "Transpose Stretch: reader VA 0x140ac5fa0 (fields Fx4BBB, shared with Reverse Grain) and its own vtable are catalogued, but the stretch engine and parameter laws are not extracted",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::unproved;

    #[test]
    fn every_mangling_module_has_a_reason_and_others_do_not() {
        for module in 0x71..=0x75 {
            assert!(unproved(module).is_some_and(|why| !why.is_empty()), "{module:#x}");
        }
        assert_eq!(unproved(0x70), None);
        assert_eq!(unproved(0x76), None);
    }
}
