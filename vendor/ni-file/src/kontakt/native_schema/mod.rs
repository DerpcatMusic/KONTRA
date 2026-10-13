//! Kontakt 8.13.1's serializable-object readers, as data.
//!
//! `programs.rs` is generated from the native readers themselves (tools/re/gen_native_schemas.py,
//! catalog in docs/re/kontakt-catalog/): for every SerType the stored versions Kontakt accepts
//! and the control flow that reads them. [`decode`] runs that flow over a stream, so an object
//! either decodes with every byte accounted for exactly as Kontakt reads it, or the error says
//! what stops it. Nothing here guesses a layout.

mod programs;
mod vm;

pub use vm::{accepts, decode, decode_prefix, readers, synthesize, Decoded, Field, ReaderInfo, Undecoded};

#[cfg(test)]
mod tests {
    use super::*;

    /// Every class Kontakt can read, at every stored version it accepts, round-trips:
    /// a stream its reader accepts decodes with all bytes consumed, and (for readers
    /// whose layout is fixed by the stream alone) every proper prefix is rejected.
    #[test]
    fn every_reader_round_trips_at_every_version() {
        let mut checked = 0;
        for r in readers() {
            if matches!(r.status, "stub" | "not-a-reader" | "no-version-accepted") {
                continue;
            }
            for &(lo, hi) in r.versions {
                for version in lo..=hi {
                    let t0 = std::time::Instant::now();
                    let bytes = match synthesize(r.sertype, version) {
                        Ok(b) => b,
                        // Readers that only read through host objects have no synthesizable stream.
                        Err(Undecoded::Unresolved(_) | Undecoded::Budget | Undecoded::Rejected) => continue,
                        Err(e) => panic!("{} 0x{:X}: {e:?}", r.class, version),
                    };
                    let d = decode(r.sertype, version, &bytes)
                        .unwrap_or_else(|e| panic!("{} 0x{:X} {bytes:02x?}: {e:?}", r.class, version));
                    assert_eq!(d.consumed, bytes.len(), "{} 0x{:X}", r.class, version);
                    if r.status == "typed" {
                        for end in 0..bytes.len() {
                            assert!(
                                decode(r.sertype, version, &bytes[..end]).is_err(),
                                "{} 0x{:X}: a {end}-byte prefix of {} decoded",
                                r.class,
                                version,
                                bytes.len()
                            );
                        }
                        let mut longer = bytes.clone();
                        longer.push(0);
                        assert!(decode(r.sertype, version, &longer).is_err(), "{} 0x{:X}", r.class, version);
                    }
                    checked += 1;
                    if t0.elapsed().as_secs_f32() > 2.0 {
                        eprintln!("slow: {} 0x{:X} {:?} ({} bytes)", r.class, version, t0.elapsed(), bytes.len());
                    }
                }
            }
        }
        assert!(checked > 150, "only {checked} reader versions exercised");
    }

    /// Streams accepted by the reference interpreter (tools/re/ir_sim.py) decode here with the
    /// same field sequence: two independent executors of the same native control flow.
    #[test]
    fn executor_agrees_with_reference_interpreter() {
        let mut n = 0;
        for line in include_str!("../../../tests/native_schema_samples.txt").lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let mut it = line.split(' ');
            let sertype: u16 = it.next().unwrap().parse().unwrap();
            let version: u16 = it.next().unwrap().parse().unwrap();
            let kinds = it.next().unwrap();
            let hex = it.next().unwrap();
            let bytes: Vec<u8> = if hex == "-" {
                Vec::new()
            } else {
                (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect()
            };
            let d = decode(sertype, version, &bytes)
                .unwrap_or_else(|e| panic!("sertype 0x{sertype:X} v0x{version:X} {hex}: {e:?}"));
            let got: String = d.fields.iter().filter(|f| f.depth == 0).map(|f| f.kind).collect();
            assert_eq!(got, kinds, "sertype 0x{sertype:X} v0x{version:X} {}", d.class);
            n += 1;
        }
        assert!(n > 200, "only {n} samples");
    }

    #[test]
    fn unknown_versions_are_not_guessed() {
        assert_eq!(decode(0x08, 0x74, &[0; 64]).unwrap_err(), Undecoded::NoReader);
        assert!(!accepts(0x08, 0x6F));
        assert!(accepts(0x08, 0x73));
    }

    /// Kontakt's enum validation is part of the reader: an out-of-range filter type throws.
    #[test]
    fn native_validation_rejects_unknown_enum() {
        // BParLFO 0x73 starts with a validated u32 waveform (0..=6).
        let mut ok = synthesize(0x08, 0x73).unwrap();
        assert!(decode(0x08, 0x73, &ok).is_ok());
        ok[..4].copy_from_slice(&99u32.to_le_bytes());
        assert_eq!(decode(0x08, 0x73, &ok).unwrap_err(), Undecoded::Rejected);
    }
}
