//! Kontakt's AHDSR, golden: each case is a probe instrument saved by Kontakt
//! 8.13 (a 1 kHz sine group, its envelope set by KSP, script cleared), and
//! Kontakt's own render of it, as dB per 1 ms re full level where Kontakt is
//! above -60 dB (scratch/laneDSP/b22/env_law.lua, golden.py). The case file
//! goes through the translator, prepare and the engine's envelope; each 1 ms
//! must be within 0.6 dB (1.5 dB below -40 dB) and the first sounding frame
//! exact. Kontakt's
//! script note-offs land 9-37 frames late, so the key-up is placed where it
//! fits best within one 48-frame block of the note's length.
#![cfg(test)]

use std::path::Path;

use kontra_engine::Envelope;

const RATE: u32 = 48_000;
const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/kontakt-envelope");

struct Case {
    n: u32,
    onset: usize,
    note_off_ms: usize,
    levels: Vec<(usize, f32)>,
}

fn golden() -> Vec<Case> {
    let text = std::fs::read_to_string(Path::new(DIR).join("golden.txt")).unwrap();
    text.lines()
        .map(|line| {
            let w: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| w[i].parse::<usize>().unwrap();
            Case {
                n: w[1].parse().unwrap(),
                onset: num(3),
                note_off_ms: num(5),
                levels: w[6..]
                    .iter()
                    .map(|p| {
                        let (ms, db) = p.split_once(':').unwrap();
                        (ms.parse().unwrap(), db.parse().unwrap())
                    })
                    .collect(),
            }
        })
        .collect()
}

/// The case's envelope times a 1 kHz sine (one cycle per ms, as the probe
/// played it) as dB per 1 ms re full level, and the first sounding frame.
fn render(case: &Case, late: usize) -> (Vec<f32>, usize) {
    let path = Path::new(DIR).join(format!("case{}.nki", case.n));
    let instrument = kontra_format_kontakt::load(&path).unwrap();
    // Group 0 is the case; group 1 holds the timing click.
    let group = &instrument.groups[0];
    let mut env = Envelope::new(kontra_prepare::envelope(group.envelope, RATE));
    let frames = 1_700 * 48;
    let mut out = Vec::with_capacity(frames);
    for i in 0..frames {
        if i == case.note_off_ms * 48 + late {
            env.release();
        }
        let sine = (std::f32::consts::TAU * 1_000.0 * i as f32 / RATE as f32).sin();
        out.push(env.tick() * sine);
    }
    // As in golden.py: the first frame that is not 0 (the sine's frame 0 is).
    let onset = out.iter().position(|x| *x != 0.0).unwrap_or(usize::MAX);
    let db = out
        .chunks(48)
        .map(|ms| {
            10.0 * (ms.iter().map(|x| x * x).sum::<f32>() / 48.0 / 0.5)
                .max(1e-20)
                .log10()
        })
        .collect();
    (db, onset)
}

#[test]
fn kontakt_envelopes_match_kontakt_per_millisecond() {
    let mut failures = Vec::new();
    for case in golden() {
        let misses = |late| {
            let (db, onset) = render(&case, late);
            let mut out = Vec::new();
            if onset != case.onset {
                out.push(format!(
                    "case {}: first frame {onset}, Kontakt {}",
                    case.n, case.onset
                ));
            }
            for &(ms, want) in &case.levels {
                let got = db[ms];
                // 0.6 dB: two points sit at 0.53 and 0.57 (the -100 % attack
                // fits k 10.05 against the shared law's 9.90, and one release
                // tail). Below -40 dB a dB is under 0.0003 of full scale: the
                // last frames of a release.
                let tolerance = if want >= -40.0 { 0.6 } else { 1.5 };
                if (got - want).abs() > tolerance {
                    out.push(format!(
                        "case {} at {ms} ms: {got:.2} dB, Kontakt {want:.1}",
                        case.n
                    ));
                }
            }
            out
        };
        let best = (0..48).map(misses).min_by_key(Vec::len).unwrap_or_default();
        failures.extend(best);
    }
    assert!(
        failures.is_empty(),
        "{} misses:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
