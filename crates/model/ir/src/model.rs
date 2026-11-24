//! Registered effect models addressed by serializable [`ModelId`].
//!
//! Instead of storing raw function pointers inside the IR, every coupled effect
//! carries a compact [`ModelId`] that resolves through a lock-free static table
//! to a pure [`EffectModel`]. Both load-time format translation and audio-thread
//! script updates evaluate the exact same [`EffectModel`] implementation.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU16, Ordering};

use crate::{Effect, EffectKind};

/// Stable, serializable identifier for an effect parameter model registered
/// by a format crate.
///
/// ID bands:
/// - `0..64`: Kontakt effect modules
/// - `64..128`: UVI effect modules
/// - `128..192`: reserved for future formats
/// - `192..256`: tests and synthetic fixtures
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelId(pub u16);

impl ModelId {
    /// Total number of slots in the wait-free model table.
    pub const MAX_MODELS: usize = 256;
    /// Base ID for Kontakt effect modules (`0..64`).
    pub const KONTAKT_BASE: u16 = 0;
    /// Base ID for UVI effect modules (`64..128`).
    pub const UVI_BASE: u16 = 64;
    /// Base ID for test/synthetic models (`192..256`).
    pub const TEST_BASE: u16 = 192;
}

/// Pure stateless effect builder for a [`ModelId`].
///
/// Implementations must be deterministic and allocation-free so the audio
/// thread can evaluate them directly on script knob moves.
pub trait EffectModel: Send + Sync {
    /// Rebuilds the slot's [`EffectKind`] from its live parameter positions.
    fn build(&self, params: &[f32]) -> Option<EffectKind>;

    /// Rebuilds the full [`Effect`] from live parameter positions, preserving
    /// `current.bypass` (and by default `current.output` and `current.dry`).
    fn build_effect(&self, params: &[f32], current: &Effect) -> Option<Effect> {
        let kind = self.build(params)?;
        Some(Effect {
            kind,
            bypass: current.bypass,
            output: current.output,
            dry: current.dry,
        })
    }
}

static MODELS: [OnceLock<&'static dyn EffectModel>; ModelId::MAX_MODELS] =
    [const { OnceLock::new() }; ModelId::MAX_MODELS];

static NEXT_TEST_ID: AtomicU16 = AtomicU16::new(ModelId::TEST_BASE);

/// Registers `model` under `id`. Idempotent if `id` is already registered.
pub fn register_model(id: ModelId, model: &'static dyn EffectModel) {
    if let Some(slot) = MODELS.get(usize::from(id.0)) {
        let _ = slot.set(model);
    }
}

/// Registers a test/synthetic [`EffectModel`] under a fresh ID in `192..256`.
/// Returns `None` if the test ID band is exhausted.
pub fn register_test_model(model: &'static dyn EffectModel) -> Option<ModelId> {
    let raw = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
    if usize::from(raw) >= ModelId::MAX_MODELS {
        return None;
    }
    let id = ModelId(raw);
    register_model(id, model);
    Some(id)
}

/// Resolves `id` to its registered [`EffectModel`]. Wait-free O(1).
#[inline]
#[must_use]
pub fn model(id: ModelId) -> Option<&'static dyn EffectModel> {
    MODELS.get(usize::from(id.0))?.get().copied()
}

/// Evaluates [`EffectModel::build`] for `id`. Wait-free, zero allocations.
#[inline]
#[must_use]
pub fn eval_model(id: ModelId, params: &[f32]) -> Option<EffectKind> {
    model(id)?.build(params)
}

/// Evaluates [`EffectModel::build_effect`] for `id`. Wait-free, zero allocations.
#[inline]
#[must_use]
pub fn eval_model_effect(id: ModelId, params: &[f32], current: &Effect) -> Option<Effect> {
    model(id)?.build_effect(params, current)
}
