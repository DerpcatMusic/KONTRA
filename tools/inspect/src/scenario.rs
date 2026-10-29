//! Timed note and controller input for the offline player.
use anyhow::{Context, Result, bail, ensure};
use kontra_engine::{Event, HostNote, HostPattern};
use serde_json::{Value, json};
use std::path::Path;

use super::RATE;

#[derive(Clone, Debug)]
pub struct Input {
    pub frame: u64,
    pub event: Event,
    pub description: Value,
}

fn note(frame: u64, key: u8, on: bool) -> Input {
    let event = if on {
        Event::NoteOn {
            note: HostNote {
                port: 0,
                channel: 0,
                key,
                id: -1,
                clap: true,
            },
            velocity: 100.0 / 127.0,
            tune: 0.0,
        }
    } else {
        Event::NoteOff(HostPattern {
            port: 0,
            channel: 0,
            key: i32::from(key),
            id: -1,
            clap: true,
        })
    };
    Input {
        frame,
        event,
        description: json!({"type":if on {"note_on"} else {"note_off"},"key":key}),
    }
}
fn cc(frame: u64, cc: u8, value: u8) -> Input {
    Input {
        frame,
        event: Event::midi1(0xB0, cc, value),
        description: json!({"type":"cc","cc":cc,"value":value}),
    }
}

/// Legato, chords and repeats, each with slow and fast CC1/CC11 sweeps.
pub fn user() -> Vec<Input> {
    let rate = u64::from(RATE);
    let mut out = Vec::new();
    for part in 0..6u64 {
        let start = part * 2 * rate;
        match part / 2 {
            0 => {
                for (n, key) in [60, 62, 64, 65, 67].into_iter().enumerate() {
                    let t = start + n as u64 * rate / 4;
                    out.push(note(t, key, true));
                    out.push(note(t + rate * 3 / 10, key, false));
                }
            }
            1 => {
                for key in [48, 52, 55, 60, 64, 67] {
                    out.push(note(start, key, true));
                    out.push(note(start + rate * 3 / 2, key, false));
                }
            }
            _ => {
                for n in 0..16u64 {
                    let t = start + n * rate / 12;
                    out.push(note(t, 62, true));
                    out.push(note(t + rate / 24, 62, false));
                }
            }
        }
        let (from, duration) = if part % 2 == 0 {
            (start, rate * 3 / 2)
        } else {
            (start + rate / 2, rate / 4)
        };
        for i in 0..=32u64 {
            let t = from + duration * i / 32;
            let v = if i <= 16 {
                127 - i * 127 / 16
            } else {
                (i - 16) * 127 / 16
            };
            let v = u8::try_from(v).unwrap_or(0);
            out.push(cc(t, 1, v));
            out.push(cc(t, 11, v));
        }
    }
    out.sort_by_key(|e| e.frame);
    out
}

/// Read `{ "events": [{"frame": 0, "type": "note_on", "key": 60}, ...] }`.
pub fn file(path: &Path) -> Result<Vec<Input>> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let value: Value = serde_json::from_slice(&data).context("parsing scenario JSON")?;
    let entries = value
        .get("events")
        .and_then(Value::as_array)
        .context("scenario needs an events array")?;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let frame = entry
            .get("frame")
            .and_then(Value::as_u64)
            .context("event needs a frame")?;
        let byte = |name: &str| -> Result<u8> {
            let n = entry
                .get(name)
                .and_then(Value::as_u64)
                .with_context(|| format!("event needs {name}"))?;
            ensure!(n <= 127, "{name} must be 0..127");
            Ok(u8::try_from(n)?)
        };
        let kind = entry
            .get("type")
            .and_then(Value::as_str)
            .context("event needs a type")?;
        out.push(match kind {
            "note_on" => note(frame, byte("key")?, true),
            "note_off" => note(frame, byte("key")?, false),
            "cc" => cc(frame, byte("cc")?, byte("value")?),
            _ => bail!("unknown event type {kind}"),
        });
    }
    out.sort_by_key(|e| e.frame);
    Ok(out)
}
