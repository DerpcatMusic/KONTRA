//! The saved values of module controls no processor plays
//! ([`ir::EnginePar::Held`]), so a script's first read answers what the
//! instrument saved, as Kontakt's module does. Where each control sits in a
//! module's saved record is `data/held_fields.tsv` (tools/re/held_fields.py:
//! the measured bindings over the readers' layouts).

use kontra_ir as ir;

const FIELDS: &str = include_str!("../data/held_fields.tsv");

/// Module `module`'s saved controls in record `public` of `version`, as
/// (held key, raw value): a float 0..1 reads as value × 10⁶, an index or a
/// switch as itself.
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
                _ => i32::from(*public.get(at)? != 0),
            };
            Some((ir::held_key(name), raw))
        })
        .collect()
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
}
