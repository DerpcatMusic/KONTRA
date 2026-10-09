//! The load report: what an instrument load translated, what it could not and
//! why, and what has gone wrong while it plays. Written for a musician deciding
//! whether the instrument is usable, not as a log.
//!
//! ```text
//! ┌ Vista Cellos ───────────── Playing with 3 gaps · 1 runtime problem ┐
//! │ MISSING                                                            │
//! │ ● Script error  "Main" line 412:9  unknown variable $foo           │
//! │ ◦ Effect not translated  Convolution (group 3)  size 1.2 · mix 30% │
//! │ ● 340 samples missing  Samples/Cello_C3_pp.ncw, … (show all)       │
//! │ PLAYING NOW                                                        │
//! │ ● Script over budget  "Legato" 12 times, worst 2.4 ms              │
//! │ LOADED                                                             │
//! │ ✓ Mapping 3 412 zones · 48 groups   ✓ Scripts 3 of 4               │
//! └────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! Problems that make the instrument play wrong (missing samples, script
//! errors, access) are coral; translation gaps are neutral. Repeated problems
//! of one kind collapse to a single row with a count.

use super::theme::*;
use moose::mui::mui::prelude::*;
use std::collections::HashSet;

/// One instrument load's report: plain data, filled by whichever core loaded it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub instrument: String,
    pub loaded: Vec<Loaded>,
    pub missing: Vec<Missing>,
    pub runtime: Vec<Runtime>,
    /// Why the last silent note was silent, one sentence.
    pub why_silent: Option<String>,
    /// Script faults behind it, each "error in callback".
    pub faults: Vec<String>,
}

/// A part of the instrument that was translated and plays.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub area: Area,
    /// Counts in words: "3 412 zones · 48 groups".
    pub summary: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Area {
    Mapping,
    Samples,
    Effects,
    Modulation,
    Scripts,
    Interface,
}

impl Area {
    fn label(self) -> &'static str {
        match self {
            Self::Mapping => "Mapping",
            Self::Samples => "Samples",
            Self::Effects => "Effects",
            Self::Modulation => "Modulation",
            Self::Scripts => "Scripts",
            Self::Interface => "Interface",
        }
    }
}

/// Something in the source that does not play as authored, and why.
#[derive(Clone, Debug, PartialEq)]
pub enum Missing {
    /// An effect module with no translation; its authored parameters.
    Effect {
        module: String,
        location: String,
        params: Vec<(String, String)>,
    },
    /// A modulation source or law the runtime does not model.
    Modulation { law: String, location: String },
    /// A script that did not compile; it does not run.
    ScriptError {
        script: String,
        line: u32,
        column: u32,
        message: String,
    },
    /// A script that runs without one builtin; calls to it do nothing.
    ScriptBuiltin {
        script: String,
        name: String,
        line: u32,
        column: u32,
    },
    /// A sample file that is not on disk; its zones are silent.
    Sample { path: String },
    /// Library content that could not be opened.
    Access { what: String, reason: String },
    /// Anything else the translator recorded (`sampler_ir::Unsupported`).
    Other {
        location: String,
        feature: String,
        value: String,
    },
}

impl Missing {
    /// Whether the instrument plays wrong because of it, not merely plainer.
    pub fn severe(&self) -> bool {
        matches!(
            self,
            Self::ScriptError { .. } | Self::Sample { .. } | Self::Access { .. }
        )
    }

    /// What rows of the same kind collapse under.
    fn key(&self) -> (u8, &str) {
        match self {
            Self::ScriptError { script, .. } => (0, script),
            Self::Access { what, .. } => (1, what),
            Self::Sample { .. } => (2, ""),
            Self::ScriptBuiltin { name, .. } => (3, name),
            Self::Effect { module, .. } => (4, module),
            Self::Modulation { law, .. } => (5, law),
            Self::Other { feature, .. } => (6, feature),
        }
    }

    /// Heading of a row of `n` such problems.
    fn title(&self, n: usize) -> String {
        let s = if n == 1 { "" } else { "s" };
        match self {
            Self::ScriptError { script, .. } => {
                format!("Script \u{201c}{script}\u{201d} did not compile")
            }
            Self::ScriptBuiltin { name, .. } => format!("Script function {name} not available"),
            Self::Sample { .. } => format!("{n} sample{s} missing"),
            Self::Access { what, .. } => format!("Could not open {what}"),
            Self::Effect { module, .. } => format!("Effect not translated: {module}"),
            Self::Modulation { law, .. } => format!("Modulation not translated: {law}"),
            Self::Other { feature, .. } => format!("Not translated: {feature}"),
        }
    }

    /// One line of detail for this occurrence.
    fn detail(&self) -> String {
        match self {
            Self::ScriptError {
                line,
                column,
                message,
                ..
            } => format!("Line: {line}:{column}\nFull error: {message}"),
            Self::ScriptBuiltin {
                script,
                line,
                column: 0,
                ..
            } => format!("\u{201c}{script}\u{201d} line {line}"),
            Self::ScriptBuiltin {
                script,
                line,
                column,
                ..
            } => format!("\u{201c}{script}\u{201d} line {line}:{column}"),
            Self::Sample { path } => format!("File: {path}"),
            Self::Access { reason, .. } => format!("Reason: {reason}"),
            Self::Effect {
                location, params, ..
            } => {
                let params: Vec<String> = params.iter().map(|(k, v)| format!("{k} {v}")).collect();
                if params.is_empty() {
                    format!("Location: {location}")
                } else {
                    format!("Location: {location}\nParameters: {}", params.join(" · "))
                }
            }
            Self::Modulation { location, .. } => format!("Location: {location}"),
            Self::Other {
                location, value, ..
            } => {
                if value.is_empty() {
                    format!("Location: {location}")
                } else {
                    format!("Location: {location}\nValue: {value}")
                }
            }
        }
    }

    /// What it means for the sound, in one sentence.
    fn impact(&self) -> &'static str {
        match self {
            Self::ScriptError { .. } => {
                "The script does not run: its controls, keyswitches and note handling are missing."
            }
            Self::ScriptBuiltin { .. } => "Calls to it do nothing; the script runs otherwise.",
            Self::Sample { .. } => "Zones that use these samples are silent.",
            Self::Access { .. } => "Content behind it does not load.",
            Self::Effect { .. } => "The signal passes through unprocessed.",
            Self::Modulation { .. } => "The target keeps its static value.",
            Self::Other { .. } => "Plays without this setting.",
        }
    }
}

/// A problem while playing; counters are cumulative since the load.
#[derive(Clone, Debug, PartialEq)]
pub enum Runtime {
    ScriptBudget {
        overruns: u64,
    },
    VoicesDropped {
        count: u64,
    },
    StreamUnderruns {
        count: u64,
    },
    NonFinite {
        count: u64,
    },
    /// MIDI 2.0 values played at MIDI 1.0 precision.
    InputNarrowed {
        count: u64,
    },
    /// Messages the instrument does not play (per-note controllers, program changes).
    InputIgnored {
        count: u64,
    },
    /// Voices faded out to make room at full polyphony.
    VoicesStolen {
        count: u64,
    },
}

impl Runtime {
    fn text(&self) -> (String, String) {
        match self {
            Self::ScriptBudget { overruns } => (
                format!("Scripts over their time budget {overruns} times"),
                "Their work was deferred, so some notes may be late.".into(),
            ),
            Self::VoicesDropped { count } => (
                format!("{count} notes dropped"),
                "The part ran out of voices.".into(),
            ),
            Self::StreamUnderruns { count } => (
                format!("{count} streaming underruns"),
                "The disk did not keep up; those notes played silent for a moment.".into(),
            ),
            Self::NonFinite { count } => (
                "Invalid audio silenced".into(),
                format!("{count} frames were replaced with silence to protect your speakers."),
            ),
            Self::InputNarrowed { count } => (
                format!("{count} MIDI 2.0 messages played at MIDI 1.0 precision"),
                "The instrument reads 7-bit values; finer steps were rounded.".into(),
            ),
            Self::InputIgnored { count } => (
                format!("{count} MIDI messages ignored"),
                "This instrument does not respond to them yet.".into(),
            ),
            Self::VoicesStolen { count } => (
                format!("{count} voices stolen"),
                "Polyphony was full: released, then the quietest voices faded out to make room."
                    .into(),
            ),
        }
    }
}

/// Missing rows, grouped: severe first, then the biggest non-severe groups, then by kind; each group in source order.
pub fn groups(missing: &[Missing]) -> Vec<Vec<&Missing>> {
    let mut groups: Vec<Vec<&Missing>> = Vec::new();
    for m in missing {
        match groups.iter_mut().find(|g| g[0].key() == m.key()) {
            Some(g) => g.push(m),
            None => groups.push(vec![m]),
        }
    }
    groups.sort_by_key(|g| {
        (
            !g[0].severe(),
            if g[0].severe() {
                0
            } else {
                usize::MAX - g.len()
            },
            g[0].key().0,
        )
    });
    groups
}

/// The one-line verdict in the header.
pub fn verdict(r: &Report) -> String {
    let gaps = groups(&r.missing).len();
    let severe = r.missing.iter().any(Missing::severe);
    let mut out = match (gaps, severe) {
        (0, _) => "Plays as authored".to_owned(),
        (_, true) => format!(
            "Plays incompletely · {gaps} problem{}",
            if gaps == 1 { "" } else { "s" }
        ),
        _ => format!(
            "Playing with {gaps} translation gap{}",
            if gaps == 1 { "" } else { "s" }
        ),
    };
    if !r.runtime.is_empty() {
        let n = r.runtime.len();
        out += &format!(" · {n} runtime problem{}", if n == 1 { "" } else { "s" });
    }
    out
}

/// A concise report row with its complete, labelled detail.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Entry {
    pub title: String,
    pub detail: String,
    pub severe: bool,
}

pub(super) fn entries(r: &Report) -> Vec<Entry> {
    let mut rows = Vec::new();
    for group in groups(&r.missing) {
        rows.push(Entry {
            title: group[0].title(group.len()),
            severe: group[0].severe(),
            detail: format!(
                "Instrument: {}\nImpact: {}\n\nOccurrences ({}):\n{}",
                r.instrument,
                group[0].impact(),
                group.len(),
                group
                    .iter()
                    .map(|m| m.detail())
                    .collect::<Vec<_>>()
                    .join("\n\n")
            ),
        });
    }
    if let Some(why) = &r.why_silent {
        rows.push(Entry {
            title: "Last note was silent".into(),
            severe: true,
            detail: format!("Instrument: {}\nReason: {why}", r.instrument),
        });
    }
    for fault in &r.faults {
        rows.push(Entry {
            title: "Script fault".into(),
            severe: true,
            detail: format!("Instrument: {}\nFull error: {fault}", r.instrument),
        });
    }
    for runtime in &r.runtime {
        let (title, impact) = runtime.text();
        rows.push(Entry {
            title,
            severe: true,
            detail: format!("Instrument: {}\nImpact: {impact}", r.instrument),
        });
    }
    if !r.loaded.is_empty() {
        rows.push(Entry {
            title: verdict(r),
            severe: false,
            detail: format!(
                "Instrument: {}\n\nLoaded:\n{}",
                r.instrument,
                r.loaded
                    .iter()
                    .map(|l| format!("{}: {}", l.area.label(), l.summary))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        });
    }
    rows
}

#[derive(Default)]
pub struct State {
    open: HashSet<String>,
}

pub fn view(ui: &mut Ui, state: &mut State, r: &Report) -> El {
    let mut rows = vec![
        body(r.instrument.clone())
            .text_weight(Weight::SEMIBOLD)
            .w(Len::Pct(100.)),
    ];
    for (n, entry) in entries(r).iter().enumerate() {
        let open = state.open.contains(&entry.title);
        let (hit, heading) = action(ui, format!("report-entry-{n}"), &entry.title, open);
        if hit && !state.open.remove(&entry.title) {
            state.open.insert(entry.title.clone());
        }
        let mut row = vec![heading.when(entry.severe, |e| e.fill(Role::Warning))];
        if state.open.contains(&entry.title) {
            row.push(
                body(entry.detail.clone())
                    .text_size(TEXT)
                    .w(Len::Pct(100.))
                    .id(format!("report-detail-{n}")),
            );
        }
        rows.push(
            col(row)
                .gap(SPACE)
                .align(Align::Stretch)
                .w(Len::Pct(100.))
                .shrink(0),
        );
        rows.push(rule());
    }
    col(rows)
        .gap(SPACE)
        .align(Align::Stretch)
        .pad(INSET)
        .scroll()
        .flex(1)
        .min_h(0)
        .min_w(0)
        .a11y(A11y::Group)
        .named("Load report")
        .id("load-report")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_load_problem_expands_to_every_labelled_detail() {
        let report = Report {
            missing: vec![Missing::ScriptError {
                script: "Main".into(),
                line: 42,
                column: 9,
                message: "Long error detail ".repeat(80),
            }],
            ..Default::default()
        };
        let mut state = State::default();
        let mut ui = super::super::theme::ui();
        for _ in 0..3 {
            let root = view(&mut ui, &mut state, &report).w(500.).h(400.);
            ui.frame(
                root,
                Some(Size::new(500., 400.)),
                Input::default(),
                1. / 60.,
            )
            .unwrap();
        }
        assert!(
            ui.scene().unwrap().surface("report-entry-0").is_some(),
            "every problem is an expandable entry, including a single error"
        );
        assert!(ui.scene().unwrap().surface("report-detail-0").is_none());
        ui.focus("report-entry-0");
        for input in [
            Input {
                keys: vec![KeyPress {
                    key: Key::Enter,
                    mods: Mods::default(),
                }],
                ..Default::default()
            },
            Input::default(),
            Input::default(),
        ] {
            let root = view(&mut ui, &mut state, &report).w(500.).h(400.);
            ui.frame(root, Some(Size::new(500., 400.)), input, 1. / 60.)
                .unwrap();
        }
        assert!(ui.scene().unwrap().surface("report-detail-0").is_some());
    }

    #[test]
    fn severe_groups_first_and_samples_collapse() {
        let missing = vec![
            Missing::Effect {
                module: "Convolution".into(),
                location: "group 3".into(),
                params: vec![],
            },
            Missing::Sample {
                path: "a.ncw".into(),
            },
            Missing::ScriptError {
                script: "Main".into(),
                line: 4,
                column: 2,
                message: "x".into(),
            },
            Missing::Sample {
                path: "b.ncw".into(),
            },
        ];
        let g = groups(&missing);
        assert_eq!(g.len(), 3);
        assert!(matches!(g[0][0], Missing::ScriptError { .. }));
        assert_eq!(g[1].len(), 2);
        assert_eq!(g[1][0].title(2), "2 samples missing");
        let r = Report {
            missing,
            ..Report::default()
        };
        assert_eq!(verdict(&r), "Plays incompletely · 3 problems");
    }
}
