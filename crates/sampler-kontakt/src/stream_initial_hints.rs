//! One-shot preload predictions for the bound initial controller state.
//!
//! These windows supplement the authored base starts. They never select a
//! musical cursor or certify a later input: live CCs, scripts, listeners and
//! source/plan revisions keep the ordinary strict stream-demand path. The
//! borrowed recipe exists only while startup constructs its physical ranges.
use super::{
    ConstantStartHints, Pcm, Range, StreamPolicy, canceled_head, constant_start_window,
    merge_start_ranges, zone_step,
};
use sampler_core::Prepared;
use sampler_ir as ir;
use std::io;

/// Reachable sample-start offsets of one zone, in source frames past `Playback::start`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct StartRange {
    /// Furthest offset a source reaches: the zone's S.Mod range, or 0 when no route moves the start.
    /// Script `play_note` offsets raise it too, converted at the zone's asset rate.
    pub(super) max_frames: u64,
    /// The first script `play_note` offset with no static bound, named by script and line.
    /// Such a zone would need its whole source; nothing widens the preload from this yet.
    pub(super) unbounded: Option<String>,
    /// The zone's sample-start routes, in zone order.
    pub(super) sources: Vec<ir::RouteRef>,
}

impl StartRange {
    pub(super) fn of(instrument: &ir::Instrument, zone: &ir::Zone) -> Self {
        let sources: Vec<ir::RouteRef> = zone
            .routes
            .iter()
            .copied()
            .filter(|r| {
                instrument
                    .routes
                    .get(r.0)
                    .is_some_and(|r| r.target == ir::Target::SampleStart)
            })
            .collect();
        let max_frames = if sources.is_empty() {
            0
        } else {
            zone.playback.start_range
        };
        Self {
            max_frames,
            unbounded: None,
            sources,
        }
    }

    /// Raises the range to a script's largest constant `play_note` offset, rounded
    /// up to frames at `asset_rate` so the window never ends early.
    pub(super) fn widen(&mut self, max_micros: u32, asset_rate: u32, unbounded: Option<&str>) {
        let frames = (u64::from(max_micros) * u64::from(asset_rate)).div_ceil(1_000_000);
        self.max_frames = self.max_frames.max(frames);
        self.unbounded = self
            .unbounded
            .take()
            .or_else(|| unbounded.map(str::to_owned));
    }
}

pub(super) struct InitialStartHints<'plan> {
    // Retain the immutable preparation boundary, not a runtime or engine epoch.
    _prepared: &'plan Prepared,
    instrument: &'plan ir::Instrument,
    assets: &'plan [Pcm],
    rate: u32,
    policy: StreamPolicy,
    constant: ConstantStartHints,
    start_ranges: Vec<StartRange>,
    controllers: Vec<(usize, u32)>,
    pub(super) eligible: usize,
    pub(super) unsupported: usize,
    /// Scripts with a `play_note` offset that has no static bound, one per script.
    pub(super) unbounded_scripts: usize,
    pub(super) controller_loops: usize,
}

impl<'plan> InitialStartHints<'plan> {
    /// Widens every zone's start range by the scripts' `play_note` offsets.
    /// A script's offset may start any zone's group, so every zone takes the largest bound.
    pub(super) fn with_script_offsets(mut self, scripts: &[sampler_ksp::ScriptView]) -> Self {
        let max_micros = scripts
            .iter()
            .map(|script| script.start_offsets.max_micros)
            .max()
            .unwrap_or(0);
        let unbounded = scripts.iter().enumerate().find_map(|(index, script)| {
            script
                .unbounded_line
                .map(|line| format!("script {index} line {line}"))
        });
        for (range, zone) in self.start_ranges.iter_mut().zip(&self.instrument.zones) {
            if let Some(pcm) = zone.asset.and_then(|asset| self.assets.get(asset.0)) {
                range.widen(max_micros, pcm.sample_rate(), unbounded.as_deref());
            }
        }
        self.unbounded_scripts = scripts
            .iter()
            .filter(|script| script.unbounded_line.is_some())
            .count();
        self
    }

    pub(super) fn new(
        instrument: &'plan ir::Instrument,
        prepared: &'plan Prepared,
        assets: &'plan [Pcm],
        rate: u32,
        head: usize,
        policy: &StreamPolicy,
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> io::Result<Self> {
        if canceled() {
            return Err(canceled_head());
        }
        if rate != prepared.sample_rate() || instrument.zones.len() != prepared.region_count() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "initial onset plan geometry mismatch",
            ));
        }
        // Check before constructing either kind of window. A physical source
        // replacement must not inherit a previous source's predicted residency.
        for (region, zone) in instrument.zones.iter().enumerate() {
            if canceled() {
                return Err(canceled_head());
            }
            let Some(asset) = zone.asset else { continue };
            let matches = assets
                .get(asset.0)
                .zip(prepared.region_asset(region))
                .is_some_and(|(pcm, source)| pcm.asset_id() == source.asset_id());
            if !matches {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "initial onset source identity mismatch",
                ));
            }
        }
        let constant =
            ConstantStartHints::new(instrument, prepared, assets, rate, head, policy, canceled)?;
        let start_ranges: Vec<StartRange> = instrument
            .zones
            .iter()
            .map(|zone| StartRange::of(instrument, zone))
            .collect();
        let mut this = Self {
            _prepared: prepared,
            instrument,
            assets,
            rate,
            policy: *policy,
            eligible: constant.eligible,
            constant,
            start_ranges,
            controllers: Vec::new(),
            unsupported: 0,
            unbounded_scripts: 0,
            controller_loops: 0,
        };
        for (region, zone) in instrument.zones.iter().enumerate() {
            if canceled() {
                return Err(canceled_head());
            }
            if this.start_ranges[region].sources.is_empty()
                || prepared.constant_start_offset(region).is_some()
            {
                continue;
            }
            let Some(offset) = prepared.initialized_start_offset(region) else {
                this.unsupported += 1;
                continue;
            };
            if offset == 0 {
                continue;
            }
            let Some(asset) = zone.asset else { continue };
            if zone
                .group
                .is_some_and(|g| instrument.groups[g.0].wavetable.is_some())
            {
                continue;
            }
            // Do not extrapolate the nonloop window across a cursor transition.
            // Existing Constant prefixes retain their exact prepared proof.
            if !matches!(
                zone.playback.looping,
                ir::Looping::None | ir::Looping::OneShot
            ) {
                this.controller_loops += 1;
                continue;
            }
            let pcm = &assets[asset.0];
            let step = zone_step(
                zone,
                &instrument.groups,
                f64::from(pcm.sample_rate()) / f64::from(rate),
                policy,
            );
            if constant_start_window(&zone.playback, pcm.frame_count(), offset, step, head)
                .is_some()
            {
                this.controllers.push((region, offset));
                this.eligible += 1;
            }
        }
        if canceled() {
            return Err(canceled_head());
        }
        Ok(this)
    }

    pub(super) fn controller_zones(&self) -> usize {
        self.controllers.len()
    }

    pub(super) fn ranges(
        &self,
        head: usize,
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> io::Result<Vec<Vec<Range<usize>>>> {
        // The Constant recipe enforces the original requested horizon, even
        // when it contains no eligible Constant routes.
        let mut ranges = self.constant.ranges(
            self.instrument,
            self.assets,
            self.rate,
            head,
            &self.policy,
            canceled,
        )?;
        for &(region, offset) in &self.controllers {
            if canceled() {
                return Err(canceled_head());
            }
            let zone = &self.instrument.zones[region];
            let asset = zone
                .asset
                .expect("a prepared onset prediction owns a source")
                .0;
            let pcm = &self.assets[asset];
            let step = zone_step(
                zone,
                &self.instrument.groups,
                f64::from(pcm.sample_rate()) / f64::from(self.rate),
                &self.policy,
            );
            if let Some(window) =
                constant_start_window(&zone.playback, pcm.frame_count(), offset, step, head)
            {
                ranges[asset].push(window);
            }
        }
        for ranges in &mut ranges {
            merge_start_ranges(ranges);
        }
        if canceled() {
            return Err(canceled_head());
        }
        Ok(ranges)
    }
}

#[cfg(test)]
#[path = "stream_initial_hints_tests.rs"]
mod tests;
