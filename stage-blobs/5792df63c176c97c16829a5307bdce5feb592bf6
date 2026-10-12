//! Why a note made no sound, in one sentence. Pure, so the plugin's load
//! report and the corpus scoreboard say the same thing for the same facts.

use crate::{Rejection, SelectionRecord};

/// A script callback that ended in a fault: where, and with what error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptFault {
    /// e.g. `"script 1 on note"`.
    pub callback: String,
    /// e.g. `"InvalidInput"`.
    pub error: String,
}

impl std::fmt::Display for ScriptFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} in {}", self.error, self.callback)
    }
}

fn reason(r: Rejection) -> &'static str {
    match r {
        Rejection::Trigger => "trigger phase (attack or release)",
        Rejection::Articulation => "articulation (keyswitch not active)",
        Rejection::Condition => "controller condition",
        Rejection::Velocity => "velocity range",
        Rejection::Group => "group selection (script)",
        Rejection::Take => "round-robin take",
    }
}

/// A selection that chose no region, as plain counts: fixed size, so the
/// audio thread can note it without allocating.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SilentNote {
    pub key: u8,
    /// The key's script suppressed the attack.
    pub suppressed: bool,
    /// Rejected regions per reason, in [`ORDER`].
    pub counts: [u32; 6],
}

const ORDER: [Rejection; 6] = [
    Rejection::Group,
    Rejection::Velocity,
    Rejection::Articulation,
    Rejection::Condition,
    Rejection::Trigger,
    Rejection::Take,
];

impl SilentNote {
    pub fn slot(r: Rejection) -> usize {
        ORDER.iter().position(|o| *o == r).unwrap_or(0)
    }

    /// `None` if a region was accepted (the silence is not the selection's).
    pub fn from_record(r: &SelectionRecord) -> Option<Self> {
        let mut note = Self {
            key: r.key,
            suppressed: r.suppressed,
            ..Self::default()
        };
        for c in &r.candidates {
            match c.rejected {
                Some(why) => note.counts[Self::slot(why)] += 1,
                None => return None,
            }
        }
        Some(note)
    }

    /// Three words for atomics; counts saturate at 65535.
    pub fn pack(&self) -> [u64; 3] {
        let c = |i: usize| u64::from(self.counts[i].min(0xffff));
        [
            u64::from(self.key) | u64::from(self.suppressed) << 8,
            c(0) | c(1) << 16 | c(2) << 32 | c(3) << 48,
            c(4) | c(5) << 16,
        ]
    }

    pub fn unpack(w: [u64; 3]) -> Self {
        let n = |word: u64, i: u32| ((word >> (16 * i)) & 0xffff) as u32;
        Self {
            key: w[0] as u8,
            suppressed: w[0] >> 8 & 1 == 1,
            counts: [
                n(w[1], 0),
                n(w[1], 1),
                n(w[1], 2),
                n(w[1], 3),
                n(w[2], 0),
                n(w[2], 1),
            ],
        }
    }

    /// One sentence, with the script faults seen, if any.
    pub fn message(&self, faults: &[ScriptFault]) -> String {
        let key = self.key;
        let faults_text = || {
            faults
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        if self.suppressed {
            return if faults.is_empty() {
                format!("key {key}: note suppressed by its script")
            } else {
                format!(
                    "key {key}: note suppressed by script fault {}",
                    faults_text()
                )
            };
        }
        let (top, n) = self
            .counts
            .iter()
            .enumerate()
            .max_by_key(|(_, n)| **n)
            .map(|(i, n)| (ORDER[i], *n))
            .unwrap_or((Rejection::Group, 0));
        if n == 0 {
            return format!("key {key}: no zone is mapped to this key");
        }
        let mut text = format!("key {key}: {n} zones rejected by {}", reason(top));
        let others: Vec<String> = ORDER
            .iter()
            .zip(self.counts)
            .filter(|(r, n)| **r != top && *n > 0)
            .map(|(r, n)| format!("{n} by {}", reason(*r)))
            .collect();
        if !others.is_empty() {
            text += &format!(", {}", others.join(", "));
        }
        if !faults.is_empty() {
            text += &format!("; script faults: {}", faults_text());
        }
        text
    }
}

/// The reason a selection of `key` produced no sound, from the selection
/// records of that note and the script faults seen. `None` if a region
/// was accepted (the silence is not the selection's).
pub fn why_silent(key: u8, records: &[SelectionRecord], faults: &[ScriptFault]) -> Option<String> {
    if let Some(r) = records.iter().find(|r| r.key == key && r.suppressed) {
        return SilentNote::from_record(r).map(|n| n.message(faults));
    }
    let Some(record) = records.iter().find(|r| r.key == key) else {
        return Some(format!("key {key}: no selection was recorded"));
    };
    SilentNote::from_record(record).map(|n| n.message(faults))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RegionVerdict, Trigger};

    fn record(key: u8, suppressed: bool, rejected: &[Option<Rejection>]) -> SelectionRecord {
        SelectionRecord {
            event: 0,
            parent_event: None,
            at: 0,
            key,
            velocity: 0.5,
            trigger: Trigger::Attack,
            suppressed,
            candidates: rejected
                .iter()
                .enumerate()
                .map(|(region, r)| RegionVerdict {
                    region,
                    group: None,
                    rejected: *r,
                    started: None,
                })
                .collect(),
        }
    }

    #[test]
    fn says_why_a_note_was_silent() {
        let group = Some(Rejection::Group);
        let fault = ScriptFault {
            callback: "script 1 on note".into(),
            error: "InvalidInput".into(),
        };
        assert_eq!(
            why_silent(
                60,
                &[record(
                    60,
                    false,
                    &[group, group, Some(Rejection::Velocity)]
                )],
                &[]
            )
            .unwrap(),
            "key 60: 2 zones rejected by group selection (script), 1 by velocity range"
        );
        assert_eq!(
            why_silent(60, &[record(60, true, &[])], &[fault]).unwrap(),
            "key 60: note suppressed by script fault InvalidInput in script 1 on note"
        );
        assert_eq!(
            why_silent(61, &[record(61, false, &[])], &[]).unwrap(),
            "key 61: no zone is mapped to this key"
        );
        assert_eq!(
            why_silent(60, &[record(60, false, &[None, group])], &[]),
            None
        );
        let note = SilentNote {
            key: 60,
            suppressed: false,
            counts: [1122, 3, 0, 0, 0, 7],
        };
        assert_eq!(SilentNote::unpack(note.pack()), note);
        assert_eq!(
            note.message(&[]),
            "key 60: 1122 zones rejected by group selection (script), 3 by velocity range, 7 by round-robin take"
        );
    }
}
