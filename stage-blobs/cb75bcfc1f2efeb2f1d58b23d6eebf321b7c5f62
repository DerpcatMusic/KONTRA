// Shared, sample-exact schedules for the v1 and v2 CPU adapters at 48 kHz.
struct AuditSchedule {
    events: Vec<(usize, u8, u8, u8)>,
    frames: usize,
    idle_blocks: usize,
    steady: std::ops::Range<usize>,
}

fn audit_schedule(name: &str) -> AuditSchedule {
    let mut events = vec![(0, 0xb0, 1, 110), (0, 0xb0, 11, 127)];
    let note = |events: &mut Vec<_>, at, key, velocity, length| {
        events.push((at, 0x90, key, velocity));
        events.push((at + length, 0x80, key, 0));
    };
    let (frames, idle_blocks, steady) = match name {
        "fast-repeat" => {
            // Sixteenths at quarter=140 BPM; half-length gates, no pedal.
            let mut ordinal = 0;
            for phase in 0..2 {
                let mut n = 0;
                loop {
                    let at = n * 48000 * 60 / (140 * 4);
                    if at >= 8 * 48000 {
                        break;
                    }
                    let key = if phase == 0 {
                        60
                    } else {
                        [36, 48, 60, 72][n % 4]
                    };
                    let velocity = if ordinal % 2 == 0 { 64 } else { 127 };
                    let next = (n + 1) * 48000 * 60 / (140 * 4);
                    note(
                        &mut events,
                        phase * 8 * 48000 + at,
                        key,
                        velocity,
                        ((next - at) / 2).min(8 * 48000 - at),
                    );
                    n += 1;
                    ordinal += 1;
                }
            }
            (17 * 48000, 1000, 0..16 * 48000)
        }
        "legato" => {
            // Authored eighth=180 BPM: one note every third second, 30 ms overlap.
            let step = 48000 * 60 / 180;
            let overlap = 48000 * 30 / 1000;
            for n in 0..24 {
                note(&mut events, n * step, 48 + n as u8, 100, step + overlap);
            }
            let end = 24 * step + overlap;
            (end + 48000, 1000, 0..end)
        }
        "cold-jump" => {
            for n in 0..8 {
                note(&mut events, 0, (n * 127 / 7) as u8, 100, 48000);
            }
            (2 * 48000, 0, 0..48000)
        }
        "piano" | "strings" | "fx" => {
            // Preserve the frozen original-instrument sequence exactly.
            events.push((0, 0xb0, 64, 127));
            let keys: Vec<u8> = if name == "piano" {
                vec![48, 52, 55, 60, 64, 67, 72, 76]
            } else {
                (48..60).collect()
            };
            for key in keys {
                note(&mut events, 0, key, 100, 48000);
            }
            events.push((144000, 0xb0, 64, 0));
            (192000, 1000, 12000..48000)
        }
        _ => panic!("unknown CPU audit schedule: {name}"),
    };
    events.sort_by_key(|e| e.0);
    AuditSchedule {
        events,
        frames,
        idle_blocks,
        steady,
    }
}

#[cfg(test)]
mod schedule_tests {
    use super::*;
    #[test]
    fn repeats_use_exact_tempo_alternating_velocities_and_octaves() {
        let s = audit_schedule("fast-repeat");
        let on: Vec<_> = s.events.iter().filter(|e| e.1 == 0x90).collect();
        assert_eq!(on.len(), 150);
        for (i, e) in on.iter().enumerate() {
            let n = i % 75;
            assert_eq!(e.0, (i / 75) * 384000 + n * 48000 * 60 / 560);
            assert_eq!(e.2, if i < 75 { 60 } else { [36, 48, 60, 72][n % 4] });
            assert_eq!(e.3, if i % 2 == 0 { 64 } else { 127 });
            let off = s
                .events
                .iter()
                .find(|off| off.1 == 0x80 && off.2 == e.2 && off.0 > e.0)
                .unwrap();
            assert!(off.0 <= e.0 + 2572);
        }
    }
    #[test]
    fn chromatic_legato_overlaps_by_thirty_milliseconds() {
        let s = audit_schedule("legato");
        for n in 0..24 {
            let key = 48 + n as u8;
            assert!(s.events.contains(&(n * 16000, 0x90, key, 100)));
            assert!(s.events.contains(&(n * 16000 + 17440, 0x80, key, 0)));
        }
        assert_eq!(s.events.iter().filter(|e| e.1 == 0x90).count(), 24);
    }
    #[test]
    fn cold_jump_has_no_idle_and_spans_the_full_midi_key_range() {
        let s = audit_schedule("cold-jump");
        assert_eq!(s.idle_blocks, 0);
        let on: Vec<_> = s.events.iter().filter(|e| e.1 == 0x90).collect();
        assert_eq!(on.len(), 8);
        assert!(on.iter().all(|e| e.0 == 0));
        assert_eq!(
            on.iter().map(|e| e.2).collect::<Vec<_>>(),
            [0, 18, 36, 54, 72, 90, 108, 127]
        );
    }
    #[test]
    fn original_schedules_keep_the_frozen_audit_event_order() {
        for name in ["piano", "strings", "fx"] {
            let s = audit_schedule(name);
            let keys: Vec<u8> = if name == "piano" {
                vec![48, 52, 55, 60, 64, 67, 72, 76]
            } else {
                (48..60).collect()
            };
            let mut expected = vec![(0, 0xb0, 1, 110), (0, 0xb0, 11, 127), (0, 0xb0, 64, 127)];
            for key in keys {
                expected.extend([(0, 0x90, key, 100), (48000, 0x80, key, 0)]);
            }
            expected.push((144000, 0xb0, 64, 0));
            expected.sort_by_key(|e| e.0);
            assert_eq!(s.events, expected);
            assert_eq!(s.frames, 192000);
            assert_eq!(s.idle_blocks, 1000);
            assert_eq!(s.steady, 12000..48000);
        }
    }
}
