//! The saved values of module controls no processor plays
//! ([`ir::EnginePar::Held`]), so a script's first read answers what the
//! instrument saved, as Kontakt's module does. Where each control sits in a
//! module's saved record is `data/held_fields.tsv` (tools/re/held_fields.py:
//! the measured bindings over the readers' layouts).

use kontra_ir as ir;

const FIELDS: &str = include_str!("../data/held_fields.tsv");

/// Module `module`'s saved controls in record `public` of `version`, as
/// (held key, raw value): a float 0..1 reads as value × 10⁶, an index or a
/// switch as itself, a tempo unit as the `$NI_SYNC_UNIT_*` its beats name.
pub(crate) fn saved(module: u16, version: u16, public: &[u8]) -> Vec<(u32, i32)> {
    let word = |at: usize| <[u8; 4]>::try_from(public.get(at..at + 4)?).ok();
    FIELDS
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|line| {
            let mut f = line.split('\t');
            let hex = |s: Option<&str>| u16::from_str_radix(s?.trim_start_matches("0x"), 16).ok();
            let (m, v) = (hex(f.next())?, hex(f.next())?);
            if (m, v) != (module, version) {
                return None;
            }
            let name = f.next()?;
            let at: usize = f.next()?.parse().ok()?;
            let raw = match f.next()? {
                "F" => {
                    let x = f32::from_le_bytes(word(at)?);
                    (x.is_finite()).then(|| (f64::from(x.clamp(0.0, 1.0)) * 1e6).round() as i32)?
                }
                "I" => i32::from_le_bytes(word(at)?),
                "U" => unit(f32::from_le_bytes(word(at)?)),
                _ => i32::from(*public.get(at)? != 0),
            };
            Some((ir::held_key(name), raw))
        })
        .collect()
}

/// The `$NI_SYNC_UNIT_*` whose note value is `beats` quarter notes; 0
/// (`ABS`) when the time runs free (−1).
// ponytail: units 1..4 save −1 as a free time does, so they read back 0.
fn unit(beats: f32) -> i32 {
    let units = crate::engine_pars::lfo_units();
    let same = |r: &Option<ir::Rate>| matches!(r, Some(ir::Rate::Steps { beats: b, .. }) if (b - beats).abs() < 1e-4);
    (units.iter().position(same)).map_or(0, |n| i32::try_from(n).unwrap_or(0))
}

/// The held controls a script-loaded type starts with, by effect type id:
/// a fresh unit's, as measured (Kontakt 8.13.1 INIT records and
/// read-backs, script_knobs.rs and the effect laws). A control not listed
/// reads nothing until a script sets it.
pub(crate) fn fresh() -> Vec<(i32, u32, i32)> {
    let key = ir::held_key;
    let x = |v: f32| (f64::from(v.clamp(0.0, 1.0)) * 1e6).round() as i32;
    let mut out = Vec::new();
    for (ty, module, version, record) in crate::script_knobs::init_records() {
        out.extend(
            saved(module, version, record)
                .into_iter()
                .map(|(k, v)| (ty, k, v)),
        );
    }
    let r = crate::replika::INIT;
    let replika = [
        ("$ENGINE_PAR_RDL_TYPE", r.mode),
        ("$ENGINE_PAR_RDL_TAPEAGE", x(r.tape_age)),
        ("$ENGINE_PAR_RDL_FLUTTER", x(r.flutter)),
        ("$ENGINE_PAR_RDL_QUALITY", x(r.quality)),
        ("$ENGINE_PAR_RDL_DEPTH", x(r.depth)),
        ("$ENGINE_PAR_RDL_RATE", x(r.rate)),
        ("$ENGINE_PAR_RDL_STEREO", i32::from(r.stereo)),
        ("$ENGINE_PAR_RDL_PINGPONG", i32::from(r.pingpong)),
        ("$ENGINE_PAR_RDL_PINGPONG_FLIP", i32::from(r.flip)),
        ("$ENGINE_PAR_RDL_PAN", x(r.pan)),
        ("$ENGINE_PAR_RDL_WIDTH", x(r.width)),
        ("$ENGINE_PAR_RDL_LR_OFFSET", x(r.offset)),
        ("$ENGINE_PAR_RDL_TIME_UNIT", r.beats.map_or(0, unit)),
    ];
    out.extend(replika.map(|(n, v)| (40, key(n), v)));
    // A fresh Freak runs its Oscillator mode; Psyche Delay's time is free.
    out.push((54, key("$ENGINE_PAR_FREAK_MODE"), crate::freak::OSCILLATOR));
    let psyche = crate::psyche::INIT.beats.map_or(0, unit);
    out.push((49, key("$ENGINE_PAR_PSYDL_TIME_UNIT"), psyche));
    out
}

#[cfg(test)]
mod tests {
    /// Bite's record (IFFFFFFFFFFB): HPF 2, BITS 0.25 at byte 8, DC_QUANT on.
    #[test]
    fn a_saved_bite_reads_back_as_scripts_see_it() {
        let mut public = vec![0u8; 45];
        public[..4].copy_from_slice(&2i32.to_le_bytes());
        public[8..12].copy_from_slice(&0.25f32.to_le_bytes());
        public[44] = 1;
        let got = super::saved(0x66, 0x10, &public);
        let key = kontra_ir::held_key;
        assert!(got.contains(&(key("$ENGINE_PAR_BITE_HPF"), 2)), "{got:?}");
        assert!(got.contains(&(key("$engine_par_bite_bits"), 250_000)));
        assert!(got.contains(&(key("$ENGINE_PAR_BITE_DC_QUANT"), 1)));
        assert!(
            super::saved(0x66, 0x11, &public).is_empty(),
            "another version"
        );
    }

    /// A synced time reads back as its unit: 1/8 is unit 12 (half a beat),
    /// a free time 0.
    #[test]
    fn a_tempo_unit_reads_as_its_sync_unit() {
        assert_eq!(super::unit(0.5), 12);
        assert_eq!(super::unit(4.0), 5);
        assert_eq!(super::unit(-1.0), 0);
    }
}
