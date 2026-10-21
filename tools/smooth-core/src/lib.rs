//! The engine.
//!
//! This is the Resonance Lab's clean-room reimplementation of soothe3's
//! spectral suppressor, ported from the research tree
//! (`Research/t3-research/…/soothe3-decoded-dsp-rea` and the audits that
//! followed it). Nothing here is a fresh design: the canonical log and
//! exponential programs, the projection, the spatial operators, the weighting
//! banks and the reduction arithmetic all come from that work, and the
//! comments in each file are the research's own.
//!
//! What this file adds is the bank: seven engines, one per plan, so the
//! quality and phase choices are prepared ahead of the switch rather than
//! built on the audio thread.
//!
//! The code is the research's, kept whole so it can be diffed against the
//! source it came from. That is why the style lints below are off: the
//! arithmetic is read against the decompiled original, not against a house
//! style, and rewriting it would change the numbers.

#![allow(
    dead_code,
    clippy::approx_constant,
    clippy::chunks_exact_to_as_chunks,
    clippy::manual_div_ceil,
    clippy::excessive_precision,
    clippy::needless_range_loop,
    clippy::collapsible_if,
    clippy::needless_return,
    clippy::neg_multiply,
    clippy::assign_op_pattern,
    clippy::manual_clamp,
    clippy::same_item_push,
    clippy::unnecessary_cast,
    clippy::field_reassign_with_default,
    clippy::too_many_arguments,
    clippy::manual_is_multiple_of,
    clippy::manual_saturating_arithmetic,
    reason = "the research arithmetic is kept whole and read against the decompiled original; see the module docs"
)]

pub mod conditioning;
pub mod dsp;
pub mod fractional_table;
pub mod numerics;
pub mod plot;
pub mod precursor;
pub mod projection;
pub mod projection_log2_table;
pub mod projection_math;
pub mod reduction;
pub mod reference_plan;
pub mod spatial;
pub mod weighting;
pub mod weighting_tables;

pub use dsp::{Engine, EngineConfig, Plan, Quality, Settings};
/// The plans, one engine each.
///
/// Plan zero is the short-window experiment; the other six are the
/// normal/high/ultra qualities with and without linear phase. Every one is
/// built when the bank is built, so a switch is a pointer swap.
pub struct EngineBank {
    engines: Vec<Engine>,
    selection: usize,
    mid_side: bool,
}

impl EngineBank {
    /// A bank for `rate`, with `selection` active.
    pub fn new(rate: f32, selection: usize, mid_side: bool) -> Result<Self, &'static str> {
        let mut engines = Vec::with_capacity(7);
        engines.push(Engine::new(
            rate,
            EngineConfig {
                reduced_latency: true,
                ..EngineConfig::default()
            },
        )?);
        for linear_phase in [false, true] {
            for quality in [Quality::Normal, Quality::High, Quality::Ultra] {
                engines.push(Engine::new(
                    rate,
                    EngineConfig {
                        quality,
                        linear_phase,
                        reduced_latency: false,
                    },
                )?);
            }
        }
        Ok(Self {
            engines,
            selection: selection.min(6),
            mid_side,
        })
    }

    /// Switch plans. A change deliberately resets the engine's history, and
    /// may briefly interrupt the audio.
    pub fn select(&mut self, selection: usize, mid_side: bool) -> bool {
        let selection = selection.min(6);
        if self.selection != selection || self.mid_side != mid_side {
            self.selection = selection;
            self.mid_side = mid_side;
            self.engines[self.selection].reset();
            true
        } else {
            false
        }
    }

    /// The active engine.
    pub fn active(&self) -> &Engine {
        &self.engines[self.selection]
    }

    /// The active engine, for the audio thread.
    pub fn active_mut(&mut self) -> &mut Engine {
        &mut self.engines[self.selection]
    }

    /// The plan the active engine is running.
    pub fn plan(&self) -> Plan {
        self.active().plan()
    }

    /// How many samples the active engine is behind.
    pub fn latency(&self) -> usize {
        self.active().latency()
    }

    /// Forget everything.
    pub fn reset(&mut self) {
        for engine in &mut self.engines {
            engine.reset();
        }
    }
}
