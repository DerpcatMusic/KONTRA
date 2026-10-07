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

/// The reason a selection of `key` produced no sound, from the selection
/// records of that note and the script faults seen. `None` if a region
/// was accepted (the silence is not the selection's).
pub fn why_silent(key: u8, records: &[SelectionRecord], faults: &[ScriptFault]) -> Option<String> {
    let faults_text = || faults.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
    let attack = records.iter().find(|r| r.key == key && !r.suppressed);
    if records.iter().any(|r| r.key == key && r.suppressed) {
        return Some(if faults.is_empty() {
            format!("key {key}: note suppressed by its script")
        } else {
            format!("key {key}: note suppressed by script fault {}", faults_text())
        });
    }
    let Some(record) = attack else {
        return Some(format!("key {key}: no selection was recorded"));
    };
    if record.candidates.is_empty() {
        return Some(format!("key {key}: no zone is mapped to this key"));
    }
    let accepted = record.candidates.iter().filter(|c| c.rejected.is_none()).count();
    if accepted > 0 {
        return None;
    }
    let mut counts = [0usize; 6];
    let order = [
        Rejection::Group,
        Rejection::Velocity,
        Rejection::Articulation,
        Rejection::Condition,
        Rejection::Trigger,
        Rejection::Take,
    ];
    for c in &record.candidates {
        if let Some(r) = c.rejected {
            counts[order.iter().position(|o| *o == r).unwrap_or(0)] += 1;
        }
    }
    let (top, n) = counts.iter().enumerate().max_by_key(|(_, n)| **n).map(|(i, n)| (order[i], *n)).unwrap_or((Rejection::Group, 0));
    let mut text = format!("key {key}: {n} zones rejected by {}", reason(top));
    let others: Vec<String> = order
        .iter()
        .zip(counts)
        .filter(|(r, n)| **r != top && *n > 0)
        .map(|(r, n)| format!("{n} by {}", reason(*r)))
        .collect();
    if !others.is_empty() {
        text += &format!(", {}", others.join(", "));
    }
    if !faults.is_empty() {
        text += &format!("; script faults: {}", faults_text());
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RegionVerdict, Trigger};

    fn record(key: u8, suppressed: bool, rejected: &[Option<Rejection>]) -> SelectionRecord {
        SelectionRecord {
            at: 0,
            key,
            velocity: 0.5,
            trigger: Trigger::Attack,
            suppressed,
            candidates: rejected
                .iter()
                .enumerate()
                .map(|(region, r)| RegionVerdict { region, group: None, rejected: *r })
                .collect(),
        }
    }

    #[test]
    fn says_why_a_note_was_silent() {
        let group = Some(Rejection::Group);
        let fault = ScriptFault { callback: "script 1 on note".into(), error: "InvalidInput".into() };
        assert_eq!(
            why_silent(60, &[record(60, false, &[group, group, Some(Rejection::Velocity)])], &[]).unwrap(),
            "key 60: 2 zones rejected by group selection (script), 1 by velocity range"
        );
        assert_eq!(
            why_silent(60, &[record(60, true, &[])], &[fault]).unwrap(),
            "key 60: note suppressed by script fault InvalidInput in script 1 on note"
        );
        assert_eq!(why_silent(61, &[record(61, false, &[])], &[]).unwrap(), "key 61: no zone is mapped to this key");
        assert_eq!(why_silent(60, &[record(60, false, &[None, group])], &[]), None);
    }
}
