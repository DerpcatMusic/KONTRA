//! What every format reads into the instrument model: diagnostics folded by
//! feature, so a format reports each gap once with a count (06-FORMATS.md).

use std::collections::BTreeMap;

use kontra_ir as ir;

/// Diagnostics folded by feature: one entry with a count and the first detail.
#[derive(Default)]
pub struct Notes(BTreeMap<(ir::Impact, String), (usize, String)>);

impl Notes {
    pub fn add(&mut self, impact: ir::Impact, feature: &str, detail: impl ToString) {
        let entry = self
            .0
            .entry((impact, feature.into()))
            .or_insert((0, detail.to_string()));
        entry.0 += 1;
    }

    #[must_use]
    pub fn finish(self) -> Vec<ir::Diagnostic> {
        self.0
            .into_iter()
            .rev()
            .map(|((impact, feature), (count, first))| ir::Diagnostic {
                impact,
                detail: format!("{count}× (first: {first})"),
                feature,
            })
            .collect()
    }
}
