//! Off-audio grouping. Keys contain typed identifiers, locations contain paths and indices.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const LOCATION_CAP: usize = 16;
pub const SCHEMA: &str = "grouped-diagnostics-v1";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Key {
    pub kind: String,
    pub reason: String,
    pub subject: String,
}
impl Key {
    pub fn new(kind: &str, reason: &str, subject: &str) -> Self {
        Self {
            kind: identifier(kind),
            reason: identifier(reason),
            subject: identifier(subject),
        }
    }
}
fn identifier(s: &str) -> String {
    let native = s
        .strip_prefix("@MIDI CC ")
        .is_some_and(|cc| cc.parse::<u8>().is_ok_and(|cc| cc < 128))
        || matches!(s, "@PitchBend" | "@VoiceParam Key" | "@VoiceParam Velocity");
    if native
        || (!s.is_empty()
            && s.len() <= 128
            && (s.as_bytes()[0].is_ascii_alphabetic() || s.as_bytes()[0] == b'_')
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_:().-".contains(&c)))
    {
        s.into()
    } else {
        "UnknownSubject".into()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Location {
    pub path: String,
    pub program: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_slot: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rack: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bus: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_slot: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_slot: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}
impl Location {
    pub fn new(path: &str, program: u32, authored: &str) -> Self {
        let text = authored.to_ascii_lowercase();
        let index = |word: &str| {
            text.split_once(word).and_then(|(_, t)| {
                t.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .ok()
            })
        };
        Self {
            path: path.split("::").next().unwrap_or(path).into(),
            program,
            group: index("group "),
            slot: index("slot "),
            zone: index("zone "),
            script_slot: index("script slot "),
            line: index("line "),
            bus: index("bus "),
            rack: text.split_whitespace().rev().find_map(|word| match word {
                "insert" => Some(0),
                "send" => Some(1),
                "main" => Some(2),
                "internal" => Some(3),
                "external" => Some(4),
                _ => None,
            }),
            node: text.split_once(':').and_then(|(tag, id)| {
                matches!(tag, "program" | "layer" | "keygroup" | "connection")
                    .then(|| id.parse().ok())
                    .flatten()
            }),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub enabled: u64,
    pub bypassed: u64,
    pub unknown: u64,
}
impl Counts {
    fn add(&mut self, enabled: Option<bool>, count: u64) {
        let n = match enabled {
            Some(true) => &mut self.enabled,
            Some(false) => &mut self.bypassed,
            None => &mut self.unknown,
        };
        *n = n.saturating_add(count);
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub schema: String,
    pub key: Key,
    #[serde(flatten)]
    pub counts: Counts,
    pub impact_rank: u8,
    pub libraries: usize,
    pub locations: Vec<Location>,
    pub locations_total: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Located {
    pub key: Key,
    pub location: Location,
    #[serde(flatten)]
    pub counts: Counts,
}
#[derive(Clone, Debug, Default)]
pub struct Collector {
    rows: BTreeMap<Key, BTreeMap<Location, Counts>>,
}
impl Collector {
    /// Returns true once for each typed group, including repeated identical locations.
    pub fn add(&mut self, key: Key, location: Location, enabled: Option<bool>, count: u64) -> bool {
        let first = !self.rows.contains_key(&key);
        self.rows
            .entry(key)
            .or_default()
            .entry(location)
            .or_default()
            .add(enabled, count);
        first
    }
    pub fn groups(&self) -> Vec<Group> {
        let mut groups: Vec<_> = self
            .rows
            .iter()
            .map(|(key, rows)| {
                let mut counts = Counts::default();
                let mut libraries = BTreeSet::new();
                for (loc, c) in rows {
                    counts.enabled = counts.enabled.saturating_add(c.enabled);
                    counts.bypassed = counts.bypassed.saturating_add(c.bypassed);
                    counts.unknown = counts.unknown.saturating_add(c.unknown);
                    libraries.insert(library(&loc.path));
                }
                Group {
                    schema: SCHEMA.into(),
                    key: key.clone(),
                    impact_rank: if counts.enabled > 0 {
                        1
                    } else if counts.bypassed > 0 {
                        2
                    } else {
                        3
                    },
                    counts,
                    libraries: libraries.len(),
                    locations: rows.keys().take(LOCATION_CAP).cloned().collect(),
                    locations_total: rows.len(),
                }
            })
            .collect();
        groups.sort_by(|a, b| {
            a.impact_rank
                .cmp(&b.impact_rank)
                .then(b.counts.enabled.cmp(&a.counts.enabled))
                .then(b.libraries.cmp(&a.libraries))
                .then(b.counts.bypassed.cmp(&a.counts.bypassed))
                .then(a.key.cmp(&b.key))
        });
        groups
    }
    pub fn sidecar(&self) -> Vec<Located> {
        self.rows
            .iter()
            .flat_map(|(key, rows)| {
                rows.iter().map(|(location, counts)| Located {
                    key: key.clone(),
                    location: location.clone(),
                    counts: counts.clone(),
                })
            })
            .collect()
    }
}
fn library(path: &str) -> String {
    for marker in ["/Libraries/Kontakt/", "/Libraries/UVI/"] {
        if let Some((_, suffix)) = path.split_once(marker) {
            return suffix.split('/').next().unwrap_or("").into();
        }
    }
    std::path::Path::new(path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new(""))
        .display()
        .to_string()
}

/// Map translator metadata, without admitting its authored values/messages into keys.
pub fn missing(feature: &str, reason: &str, value: &str) -> Key {
    if let Some((status, builtin)) = feature
        .strip_prefix("script ")
        .and_then(|s| s.split_once(": "))
    {
        return Key::new(
            "KspBuiltin",
            if status == "Unsupported" {
                "UnknownCommand"
            } else {
                "NativeLawUnverified"
            },
            builtin,
        );
    }
    if feature == "modulation target" {
        return Key::new(
            "ModTarget",
            reason,
            value.split_whitespace().next().unwrap_or("UnknownSubject"),
        );
    }
    Key::new("Unsupported", reason, &feature.replace(' ', "_"))
}

/// Session-owned files follow the journal's lock, retention and support export.
pub(super) fn sidecar_directory(journal: &std::path::Path) -> std::path::PathBuf {
    journal.with_extension("diagnostic-locations")
}
pub(super) fn write_sidecar(
    collector: &Collector,
    journal: &std::path::Path,
    id: &str,
) -> std::io::Result<std::path::PathBuf> {
    if !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
        return Err(std::io::Error::other("invalid diagnostic load identifier"));
    }
    let directory = sidecar_directory(journal);
    std::fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{id}.json"));
    let bytes =
        serde_json::to_vec(&serde_json::json!({"schema":SCHEMA,"locations":collector.sidecar()}))?;
    buffr_durable_file::publish_private(&path, &bytes)?;
    Ok(path)
}

#[cfg(feature = "plugin")]
#[derive(Clone, Debug)]
pub(crate) struct RuntimeLog {
    load_id: String,
    path: String,
    program: u32,
    collector: Collector,
    observed_counts: BTreeMap<(String, String), u64>,
}
#[cfg(feature = "plugin")]
impl RuntimeLog {
    pub(super) fn new(load_id: &str, path: &str, program: u32) -> Self {
        Self {
            load_id: load_id.into(),
            path: path.split("::").next().unwrap_or(path).into(),
            program,
            collector: Collector::default(),
            observed_counts: BTreeMap::new(),
        }
    }
    pub(crate) fn fault(&mut self, callback: usize, outcome: sampler_core::Outcome) {
        let reason = match outcome {
            sampler_core::Outcome::Fault(error) => format!("{error:?}"),
            sampler_core::Outcome::FuelExhausted => "FuelExhausted".into(),
            _ => return,
        };
        self.add("KspFault", &reason, "Callback", Some(callback), 1);
    }
    pub(crate) fn lost(&mut self, count: u64) {
        self.add(
            "Diagnostics",
            "TransportOverflow",
            "FaultInbox",
            None,
            count,
        );
    }
    pub(crate) fn lua(&mut self, faults: &sampler_uvi::script::FaultCounts) -> bool {
        let mut changed = false;
        for (kind, entries) in [
            ("LuaInitFault", &faults.init),
            ("LuaRuntimeFault", &faults.runtime),
        ] {
            for (category, count) in entries {
                let reason = format!("{category:?}");
                let old = self
                    .observed_counts
                    .entry((kind.into(), reason.clone()))
                    .or_default();
                let delta = count.saturating_sub(*old);
                *old = *count;
                if delta > 0 {
                    self.add(kind, &reason, "Callback", None, delta);
                    changed = true;
                }
            }
        }
        for finding in &faults.setter_type_mismatches {
            let subject = format!("{:?}To{:?}", finding.types.expected, finding.types.actual);
            let old = self
                .observed_counts
                .entry(("LuaSetter".into(), subject.clone()))
                .or_default();
            let delta = finding.count.saturating_sub(*old);
            *old = finding.count;
            if delta > 0 {
                self.add("LuaSetter", "SetterTypeMismatch", &subject, None, delta);
                changed = true;
            }
        }
        changed
    }
    pub(crate) fn counters(&mut self, p: crate::sound::report::RuntimeProblems) -> bool {
        let mut changed = false;
        for (reason, subject, count) in [
            ("CapacityDrops", "VoicePool", p.capacity_drops),
            ("Underruns", "Stream", p.underruns),
            ("StreamCapacity", "PagePool", p.stream_capacity),
            ("StreamDisconnected", "Reader", p.stream_disconnected),
            ("StreamFailed", "Reader", p.stream_failed),
            ("OfflineFailures", "Render", p.offline_failures),
            ("Nonfinite", "Render", p.nonfinite),
            ("ScriptOverruns", "Callback", p.script_overruns),
            ("RefusedStarts", "SampleStart", p.refused_starts),
            ("IgnoredInput", "Midi", p.ignored_input),
            ("NarrowedInput", "Midi", p.narrowed_input),
        ] {
            let old = self
                .observed_counts
                .entry(("RuntimeCounter".into(), reason.into()))
                .or_default();
            let delta = count.saturating_sub(*old);
            *old = count;
            if delta > 0 {
                self.add("RuntimeCounter", reason, subject, None, delta);
                changed = true;
            }
        }
        changed
    }
    fn add(
        &mut self,
        kind: &str,
        reason: &str,
        subject: &str,
        callback: Option<usize>,
        count: u64,
    ) {
        if count == 0 {
            return;
        }
        let key = Key::new(kind, reason, subject);
        let mut location = Location::new(&self.path, self.program, "");
        location.callback_index = callback;
        if self
            .collector
            .add(key.clone(), location.clone(), Some(true), count)
        {
            super::event(
                super::LogLevel::Error,
                "runtime",
                "runtime_issue",
                serde_json::json!({"load_id":self.load_id,"path":self.path,"program":self.program,"diagnostic_key":key,"location":location,"group":self.collector.groups().into_iter().find(|g|g.key==key)}),
            );
        }
    }
    pub(crate) fn summary(&self) -> serde_json::Value {
        let journal = super::log_path().expect("diagnostic session path");
        let sidecar = write_sidecar(
            &self.collector,
            &journal,
            &format!("{}-runtime", self.load_id),
        );
        let value = serde_json::json!({"schema":SCHEMA,"load_id":self.load_id,"path":self.path,"program":self.program,
            "groups":self.collector.groups(),"locations_sidecar":sidecar.as_ref().ok(),"locations_error":sidecar.err().map(|e|e.to_string())});
        // Per-group summaries remain below the journal's event byte limit.
        for group in self.collector.groups() {
            super::event(
                super::LogLevel::Info,
                "runtime",
                "runtime_diagnostics_summary",
                serde_json::json!({"load_id":self.load_id,"path":self.path,"program":self.program,"group":group,"locations_sidecar":value["locations_sidecar"],"locations_error":value["locations_error"]}),
            );
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_scanner_runtime_contract() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/kontra-scan/grouped-contract.json"
        ))
        .unwrap();
        let mut c = Collector::default();
        for event in fixture["events"].as_array().unwrap() {
            c.add(
                Key::new(
                    event["kind"].as_str().unwrap(),
                    event["reason"].as_str().unwrap(),
                    event["subject"].as_str().unwrap(),
                ),
                serde_json::from_value(event["location"].clone()).unwrap(),
                event["enabled"].as_bool(),
                event["count"].as_u64().unwrap(),
            );
        }
        assert_eq!(serde_json::to_value(c.groups()).unwrap(), fixture["groups"]);
    }

    #[test]
    #[cfg(feature = "plugin")]
    fn runtime_faults_count_repeats_and_do_not_recount_snapshots() {
        let mut log = RuntimeLog::new("test-runtime", "/Libraries/Kontakt/A/a.nki", 0);
        for callback in 0..40 {
            log.fault(
                callback,
                sampler_core::Outcome::Fault(sampler_core::Error::InvalidInput),
            );
        }
        assert_eq!(log.collector.groups()[0].counts.enabled, 40);
        assert_eq!(log.collector.groups()[0].locations.len(), LOCATION_CAP);
        assert_eq!(log.collector.sidecar().len(), 40);
        let p = crate::sound::report::RuntimeProblems {
            capacity_drops: 2,
            ..Default::default()
        };
        assert!(log.counters(p));
        assert!(!log.counters(p));
        let mut lua = sampler_uvi::script::FaultCounts::default();
        lua.runtime
            .insert(sampler_uvi::script::FaultCategory::Lua, 3);
        assert!(log.lua(&lua));
        assert!(!log.lua(&lua));
        log.fault(
            0,
            sampler_core::Outcome::Fault(sampler_core::Error::InvalidInput),
        );
        assert_eq!(log.collector.groups()[0].counts.enabled, 41);
    }

    #[test]
    fn counts_locations_privacy_and_library_ranking() {
        let mut c = Collector::default();
        for n in 0..40 {
            assert_eq!(
                c.add(
                    Key::new("ModTarget", "TargetsDropped", "pan"),
                    Location::new(
                        "/Libraries/Kontakt/A/a.nki",
                        0,
                        &format!("Group {n} Secret name slot 2")
                    ),
                    Some(true),
                    1
                ),
                n == 0
            );
        }
        c.add(
            Key::new("ModTarget", "TargetsDropped", "pan"),
            Location::new("/Libraries/Kontakt/B/b.nki", 1, "group 9 slot 2"),
            Some(false),
            1,
        );
        let groups = c.groups();
        assert_eq!(
            groups[0].counts,
            Counts {
                enabled: 40,
                bypassed: 1,
                unknown: 0
            }
        );
        assert_eq!(
            (
                groups[0].libraries,
                groups[0].locations.len(),
                groups[0].locations_total
            ),
            (2, 16, 41)
        );
        assert_eq!(c.sidecar().len(), 41);
        assert_ne!(
            Location::new("a.nki", 0, "group 0 internal slot 0"),
            Location::new("a.nki", 0, "group 0 external slot 0")
        );
        assert_eq!(Location::new("a.nki", 0, "Keygroup:93").node, Some(93));
        assert!(
            !serde_json::to_string(&groups)
                .unwrap()
                .contains("Secret name")
        );
        assert_eq!(
            missing(
                "script Unsupported: mf_get_command",
                "NotModeled",
                "secret text"
            )
            .subject,
            "mf_get_command"
        );
        assert_eq!(
            Key::new("ModTarget", "TargetsDropped", "secret text").subject,
            "UnknownSubject"
        );
        c.add(
            Key::new("ModTarget", "TargetsDropped", "pitch"),
            Location::new("/Libraries/Kontakt/C/c.nki", 0, ""),
            Some(true),
            40,
        );
        assert_eq!(
            c.groups()[0].key.subject,
            "pan",
            "library breadth breaks equal sounding counts"
        );
    }
}
