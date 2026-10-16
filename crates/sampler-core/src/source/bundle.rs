//! Initial three-pack source admission. Storage identity and topology are
//! checked together at the ordinary IR lowering boundary, before effects or
//! behavior binding. This first milestone deliberately refuses execution;
//! there is no fallback voice using only pack zero.
use crate::{AssetId, Pcm};
use crate::lower::{Feature, LowerError};
use sampler_ir as ir;
use std::collections::{HashMap, HashSet};

const MAX_BUNDLES: usize = 256;
const MAX_ASSETS: usize = 1024;
const MAX_CONTROLS: usize = 8192;
const MAX_BOUNDARIES: usize = 512;
const MAX_INITIAL_BYTES: usize = 512 << 20;

/// Honest prepublication refusal. None of these variants is a native-layout
/// proof, nor does an empty/saved bypass make an unsupported layout admissible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceBundleRefusal {
    BundleLimit,
    PhysicalAssetLimit,
    ControlLimit,
    BoundaryLimit,
    InitialMemoryLimit,
    Geometry,
    PhysicalIdentity,
    ConflictingPackRole,
    SharedScope,
    BoundaryPlacement,
    SourceMode,
    /// Shared traversal, all-pack onset/demand and typed DSP boundary are not
    /// installed by milestone one. EVERY valid recipe remains blocked here.
    ExecutionPending,
}

fn refused(owner: impl Into<String>, reason: SourceBundleRefusal) -> LowerError {
    LowerError::Unsupported { owner: owner.into(), feature: Feature::SourceBundle(reason) }
}

/// Checked immutable physical dependency set. No cursors, envelopes, voice
/// credits, resident pins or resource leases are duplicated by this record.
struct CheckedBundle {
    zone: usize,
    packs: [usize; 3],
    assets: [AssetId; 3],
    frames: usize,
    rate: u32,
}

/// Aggregate INITIAL preparation estimate only. Existing/retired page and head
/// quotas and decoder workspaces are separate obligations of later milestones;
/// this estimate must never be reported as total runtime memory.
#[derive(Default)]
struct InitialCharge {
    decoded: usize,
    packs: usize,
    resident: usize,
    recipes: usize,
}
impl InitialCharge {
    fn checked(&self) -> Option<usize> {
        self.decoded.checked_add(self.packs)?
            .checked_add(self.resident)?.checked_add(self.recipes)
    }
    fn admit(&self) -> Result<(), LowerError> {
        if self.checked().is_none_or(|bytes| bytes > MAX_INITIAL_BYTES) {
            Err(refused("source bundles", SourceBundleRefusal::InitialMemoryLimit))
        } else { Ok(()) }
    }
}

fn check_initial(ir: &ir::Instrument, pcm: &[Pcm]) -> Result<Vec<CheckedBundle>, LowerError> {
    use SourceBundleRefusal as R;
    if pcm.len() != ir.assets.len() {
        return Err(LowerError::AssetCount { assets: ir.assets.len(), supplied: pcm.len() });
    }
    let recipes = &ir.source_indices.source_bundles;
    if recipes.len() > MAX_BUNDLES { return Err(refused("source bundles", R::BundleLimit)); }
    if ir.assets.len() > MAX_ASSETS { return Err(refused("source bundles", R::PhysicalAssetLimit)); }
    // Callers run structural validation first. All new public-input access is
    // still checked here, so this admission helper does not acquire an indexing
    // precondition from mutable Translated.instrument or an unresolved impulse.
    let mut checked = Vec::with_capacity(recipes.len());
    let mut roles = HashMap::new();
    let mut identities = HashMap::new();
    let mut controls = HashSet::new();
    let mut boundaries = HashSet::new();
    let mut origins = HashMap::new();
    let mut resident_ids = HashSet::new();
    let mut charge = InitialCharge::default();
    for asset in pcm {
        if resident_ids.insert(asset.asset_id()) {
            charge.resident = charge.resident.checked_add(asset.resident_bytes())
                .ok_or_else(|| refused("source bundles", R::InitialMemoryLimit))?;
        }
    }
    for bundle in recipes {
        let owner = format!("source bundle zone {}", bundle.zone.0);
        let bad = |reason| refused(owner.clone(), reason);
        let zone = ir.zones.get(bundle.zone.0).ok_or_else(|| bad(R::Geometry))?;
        let chain = ir.chains.get(bundle.boundary.chain.0).ok_or_else(|| bad(R::BoundaryPlacement))?;
        // This is a semantic one-oscillator/per-voice recipe. It does NOT prove
        // UVI keygroup oscillator grouping, and no saved source-count shortcut
        // is used. Group-scope matrices cannot be executed by a per-voice path.
        if chain.scope != ir::Scope::Voice { return Err(bad(R::SharedScope)); }
        if bundle.boundary.section != ir::SourceBoundarySection::BeforeAmplitude
            || bundle.boundary.index != 0
            || !matches!(chain.pre_amplitude.first(), Some(ir::Processor::StereoMatrix(_))) {
            return Err(bad(R::BoundaryPlacement));
        }
        let group = zone.group.and_then(|g| ir.groups.get(g.0));
        if matches!(zone.playback.looping, ir::Looping::Slots(_))
            || group.is_some_and(|g| g.wavetable.is_some() || g.time_stretch.is_some()) {
            return Err(bad(R::SourceMode));
        }
        let frames = usize::try_from(bundle.geometry.frames).map_err(|_| bad(R::Geometry))?;
        if frames == 0 || bundle.geometry.rate == 0 { return Err(bad(R::Geometry)); }
        let mut pack_ids = Vec::with_capacity(3);
        for (ordinal, pack) in bundle.packs.iter().enumerate() {
            let asset = pcm.get(pack.0).ok_or_else(|| bad(R::Geometry))?;
            if asset.frame_count() != frames || asset.sample_rate() != bundle.geometry.rate {
                return Err(bad(R::Geometry));
            }
            if pack_ids.contains(&asset.asset_id()) { return Err(bad(R::PhysicalIdentity)); }
            if roles.insert(pack.0, ordinal).is_some_and(|old| old != ordinal) {
                return Err(bad(R::ConflictingPackRole));
            }
            // Different indices cannot hide the same physical immutable asset
            // under different channel roles. A shared recipe may share the SAME
            // ordered physical identities; it never creates per-pack voices.
            if identities.insert(asset.asset_id(), ordinal).is_some_and(|old| old != ordinal) {
                return Err(bad(R::ConflictingPackRole));
            }
            pack_ids.push(asset.asset_id());
        }
        let origin = (bundle.origin, bundle.logical_asset);
        let geometry = (bundle.geometry, bundle.packs);
        if let Some(previous) = origins.insert(origin, geometry) {
            if previous != geometry { return Err(bad(R::PhysicalIdentity)); }
        } else {
            let bytes = frames.checked_mul(6).and_then(|n| n.checked_mul(size_of::<f32>()))
                .ok_or_else(|| bad(R::InitialMemoryLimit))?;
            charge.decoded = charge.decoded.checked_add(bytes).ok_or_else(|| bad(R::InitialMemoryLimit))?;
            charge.packs = charge.packs.checked_add(bytes).ok_or_else(|| bad(R::InitialMemoryLimit))?;
        }
        charge.recipes = charge.recipes.checked_add(size_of::<CheckedBundle>())
            .ok_or_else(|| bad(R::InitialMemoryLimit))?;
        charge.admit()?;
        for cell in bundle.boundary.cells {
            if let Some(control) = cell.control { controls.insert(control.0); }
        }
        if controls.len() > MAX_CONTROLS { return Err(bad(R::ControlLimit)); }
        // Amplitude section is fixed above, so a typed key needs no guessed
        // flattened index or lossy source-node-to-native-slot conversion.
        boundaries.insert((bundle.boundary.chain.0, bundle.boundary.index, bundle.boundary.node));
        if boundaries.len() > MAX_BOUNDARIES { return Err(bad(R::BoundaryLimit)); }
        // TryFrom has a finite exact length contract; no fallback pack or panic.
        let assets: [AssetId; 3] = pack_ids.try_into().map_err(|_| bad(R::PhysicalIdentity))?;
        checked.push(CheckedBundle {
            zone: bundle.zone.0, packs: bundle.packs.map(|a| a.0), assets,
            frames, rate: bundle.geometry.rate,
        });
    }
    Ok(checked)
}

pub(crate) fn admit_initial(ir: &ir::Instrument, pcm: &[Pcm]) -> Result<(), LowerError> {
    if ir.source_indices.source_bundles.is_empty() { return Ok(()); }
    let checked = check_initial(ir, pcm)?;
    // Consume every checked field explicitly in this staging assertion rather
    // than keeping an apparently renderable Prepared recipe with no lifecycle.
    // Activation requires the real renderer/all-pack ownership milestone.
    let first = checked.first().ok_or_else(|| refused("source bundles", SourceBundleRefusal::Geometry))?;
    let owner = format!("source bundle zone {}: packs {:?}, assets {:?}, {} frames at {} Hz",
        first.zone, first.packs, first.assets, first.frames, first.rate);
    Err(refused(owner, SourceBundleRefusal::ExecutionPending))
}
