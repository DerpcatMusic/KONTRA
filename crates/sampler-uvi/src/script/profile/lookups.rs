//! Bounded opt-in observations of existing linear parameter searches.
use serde_json::{Value, json};
use std::collections::BTreeMap;

const KIND_LIMIT: usize = 256;
const UPPER: [usize; 12] = [0, 1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024];

#[derive(Default)]
struct Search {
    calls: u64,
    hits: u64,
    comparisons: u64,
    length_sum: u64,
    max_length: usize,
    lengths: [u64; 13],
    hit_positions: [u64; 13],
}

fn bucket(n: usize) -> usize {
    UPPER
        .iter()
        .position(|upper| n <= *upper)
        .unwrap_or(UPPER.len())
}

impl Search {
    fn observe(&mut self, len: usize, hit: Option<usize>) {
        self.calls += 1;
        self.hits += u64::from(hit.is_some());
        self.comparisons += hit.map_or(len, |index| index + 1) as u64;
        self.length_sum += len as u64;
        self.max_length = self.max_length.max(len);
        self.lengths[bucket(len)] += 1;
        if let Some(index) = hit {
            self.hit_positions[bucket(index + 1)] += 1;
        }
    }

    fn report(&self) -> Value {
        json!({"calls":self.calls,"hits":self.hits,"misses":self.calls-self.hits,
            "comparisons":self.comparisons,"length_sum":self.length_sum,"max_length":self.max_length,
            "length_histogram":self.lengths,"hit_position_histogram":self.hit_positions})
    }
}

#[derive(Default)]
struct Kind {
    schema: Search,
    retained: Search,
    value_classes: BTreeMap<&'static str, u64>,
    retained_clone_bytes: u64,
    allocation_failures: u64,
}

#[derive(Default)]
pub(super) struct Lookups {
    kinds: BTreeMap<String, Kind>,
    dropped: u64,
    step_envelope_levels: u64,
}

pub(super) struct Observation<'a> {
    /// Must be a public catalog kind, or the fixed uncatalogued bucket.
    pub kind: &'a str,
    pub schema_len: usize,
    pub schema_hit: Option<usize>,
    pub retained_len: usize,
    pub retained_hit: Option<usize>,
    pub value_class: &'static str,
    pub clone_bytes: usize,
    pub allocation_failed: bool,
}

impl Lookups {
    pub(super) fn observe(&mut self, observation: Observation<'_>) {
        let Observation {
            kind,
            schema_len,
            schema_hit,
            retained_len,
            retained_hit,
            value_class,
            clone_bytes,
            allocation_failed,
        } = observation;
        if !self.kinds.contains_key(kind) {
            if self.kinds.len() >= KIND_LIMIT {
                self.dropped += 1;
                return;
            }
            // Allocate the public kind name only on its first observation,
            // rather than creating a temporary String for every API call.
            self.kinds.insert(kind.into(), Kind::default());
        }
        let record = self.kinds.get_mut(kind).expect("inserted observation kind");
        record.schema.observe(schema_len, schema_hit);
        record.retained.observe(retained_len, retained_hit);
        *record.value_classes.entry(value_class).or_default() += 1;
        record.retained_clone_bytes += clone_bytes as u64;
        record.allocation_failures += u64::from(allocation_failed);
    }

    pub(super) fn step_envelope_level(&mut self) {
        self.step_envelope_levels += 1;
    }

    pub(super) fn report(&self, collection_ns: u128) -> Value {
        let records: Vec<_> = self
            .kinds
            .iter()
            .map(|(kind, record)| {
                json!({
            "kind":kind,"schema":record.schema.report(),"retained_fields":record.retained.report(),
            "value_classes":record.value_classes,"retained_clone_bytes":record.retained_clone_bytes,
            "allocation_failures":record.allocation_failures})
            })
            .collect();
        json!({"kinds":records,"dropped_calls":self.dropped,
            "step_envelope_level_calls":self.step_envelope_levels,
            "collection_overhead_ms":collection_ns as f64/1_000_000.,
            "histogram_upper_bounds":[0,1,2,4,8,16,32,64,128,256,512,1024,null],
            "scope":"Exact native.param lookup counts while profiling is enabled. Hit positions are one-based; comparison counts derive from the first hit or list length on a miss. Length/hit histograms use inclusive upper bounds; null is overflow. Only public catalog kind names are retained; unknown kinds share a fixed bucket. Value classes and clone byte lengths contain no authored names, values or element identities. The collection clock measures record updates, not the complete opt-in branch or all profiler overhead"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distributions_account_misses_positions_bounds_and_values() {
        let mut lookups = Lookups::default();
        for hit in [Some(0), Some(656), None] {
            lookups.observe(Observation {
                kind: "Arpeggiator",
                schema_len: 657,
                schema_hit: hit,
                retained_len: 0,
                retained_hit: None,
                value_class: "missing",
                clone_bytes: 0,
                allocation_failed: false,
            });
        }
        let report = lookups.report(1000);
        let record = &report["kinds"][0];
        assert_eq!(record["schema"]["comparisons"], 1315);
        assert_eq!(record["schema"]["hits"], 2);
        assert_eq!(record["schema"]["misses"], 1);
        assert_eq!(record["schema"]["length_histogram"][11], 3);
        assert_eq!(record["schema"]["hit_position_histogram"][1], 1);
        assert_eq!(record["schema"]["hit_position_histogram"][11], 1);
        assert_eq!(record["retained_fields"]["length_histogram"][0], 3);
        assert_eq!(record["value_classes"]["missing"], 3);
        for index in 0..KIND_LIMIT {
            lookups.observe(Observation {
                kind: &format!("fixture{index}"),
                schema_len: 0,
                schema_hit: None,
                retained_len: 0,
                retained_hit: None,
                value_class: "missing",
                clone_bytes: 0,
                allocation_failed: false,
            });
        }
        assert_eq!(
            lookups.report(0)["kinds"].as_array().unwrap().len(),
            KIND_LIMIT
        );
        assert_eq!(lookups.report(0)["dropped_calls"], 1);
    }
}
