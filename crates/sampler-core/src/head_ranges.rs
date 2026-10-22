//! Inert, page-bounded head packing and generation-owned budget admission.
use super::{
    AssetId, Error, Frame, ResidencyRanges,
    head_budget::{HeadBudget, Lease},
};
use crate::{PAGE_FRAMES, Packed};

/// Immutable resident source ranges. Audio only borrows; budget ownership stays
/// with this physical generation until control reclaims the packed storage.
#[derive(Debug, Default)]
pub struct Ranges {
    data: Box<[(usize, Packed)]>,
    lease: std::sync::OnceLock<Lease>,
}
impl Ranges {
    pub(super) fn charge(&self, budget: &HeadBudget) -> Result<(), Error> {
        if let Some(lease) = self.lease.get() {
            return if lease.belongs_to(budget) {
                Ok(())
            } else {
                Err(Error::RevisionConflict)
            };
        }
        let bytes = self
            .data
            .iter()
            .try_fold(0usize, |n, (_, p)| n.checked_add(p.bytes()))
            .ok_or(Error::Capacity)?;
        self.lease
            .set(budget.reserve(bytes)?)
            .map_err(|_| Error::RevisionConflict)
    }
}
impl std::ops::Deref for Ranges {
    type Target = [(usize, Packed)];
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl<'a> IntoIterator for &'a Ranges {
    type Item = &'a (usize, Packed);
    type IntoIter = std::slice::Iter<'a, (usize, Packed)>;
    fn into_iter(self) -> Self::IntoIter {
        self.data.iter()
    }
}

pub struct PreparedRanges {
    pub(super) asset: AssetId,
    pub(super) ranges: Ranges,
    bytes: usize,
}
impl PreparedRanges {
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    /// The packed chunks, ascending and disjoint.
    pub fn ranges(&self) -> &Ranges {
        &self.ranges
    }
    pub(super) fn charged_to(&self, budget: &HeadBudget) -> bool {
        self.bytes == 0
            || self
                .ranges
                .lease
                .get()
                .is_some_and(|l| l.belongs_to(budget))
    }
}

/// Immutable control recipe, without a strong Pcm reference or callback cycle.
#[derive(Clone)]
pub struct HeadPreparer {
    asset: AssetId,
    rate: u32,
    length: usize,
    streamed: bool,
    budget: Option<HeadBudget>,
}
impl HeadPreparer {
    pub(super) fn new(
        asset: AssetId,
        rate: u32,
        length: usize,
        streamed: bool,
        budget: Option<HeadBudget>,
    ) -> Self {
        Self {
            asset,
            rate,
            length,
            streamed,
            budget,
        }
    }
    pub fn sample_rate(&self) -> u32 {
        self.rate
    }
    pub fn frame_count(&self) -> usize {
        self.length
    }
    pub fn budget_status(&self) -> Option<super::HeadBudgetStatus> {
        self.budget.as_ref().map(HeadBudget::status)
    }
    pub fn with_budget(mut self, budget: HeadBudget) -> Result<Self, Error> {
        if self.budget.as_ref().is_some_and(|old| !old.same(&budget)) {
            return Err(Error::RevisionConflict);
        }
        self.budget = Some(budget);
        Ok(self)
    }
    pub fn builder(&self) -> HeadRangeBuilder {
        HeadRangeBuilder {
            recipe: self.clone(),
            data: Vec::new(),
            lease: None,
            bytes: 0,
        }
    }
    pub fn prepare(&self, ranges: ResidencyRanges) -> Result<PreparedRanges, Error> {
        if !self.streamed {
            return Err(Error::InvalidInput);
        }
        let mut builder = self.builder();
        for (start, frames) in ranges {
            builder.push(start, &frames)?;
        }
        Ok(builder.finish())
    }
}

/// The decoder supplies at most one source page at a time. Exact lossless width
/// is analyzed before any packed payload allocation and charged before encoding.
pub struct HeadRangeBuilder {
    recipe: HeadPreparer,
    data: Vec<(usize, Packed)>,
    lease: Option<Lease>,
    bytes: usize,
}
impl HeadRangeBuilder {
    fn admit_next(&self, start: usize, frames: usize) -> Result<(), Error> {
        if !self.recipe.streamed
            || frames == 0
            || start
                .checked_add(frames)
                .is_none_or(|end| end > self.recipe.length)
            || self.data.last().is_some_and(|(s, p)| s + p.len() > start)
        {
            return Err(Error::InvalidInput);
        }
        Ok(())
    }
    /// Charges `bytes` to this generation's lease before the payload is kept;
    /// returns the new total.
    fn charge(&mut self, bytes: usize) -> Result<usize, Error> {
        let total = self.bytes.checked_add(bytes).ok_or(Error::Capacity)?;
        if let Some(budget) = &self.recipe.budget {
            match &mut self.lease {
                Some(lease) => lease.extend(bytes)?,
                None => self.lease = Some(budget.reserve(bytes)?),
            }
        }
        Ok(total)
    }
    pub fn push(&mut self, start: usize, frames: &[Frame]) -> Result<(), Error> {
        self.admit_next(start, frames.len())?;
        for (index, chunk) in frames.chunks(PAGE_FRAMES).enumerate() {
            if chunk.iter().flatten().any(|v| !v.is_finite()) {
                return Err(Error::InvalidInput);
            }
            let packing = Packed::prepare(chunk);
            self.bytes = self.charge(packing.bytes())?;
            self.data.push((start + index * PAGE_FRAMES, packing.finish()));
        }
        Ok(())
    }
    /// Admits a chunk already packed from the same immutable frames (the head
    /// cache). Geometry and budget checks match `push`.
    pub fn push_packed(&mut self, start: usize, packed: Packed) -> Result<(), Error> {
        self.admit_next(start, packed.len())?;
        if packed.len() > PAGE_FRAMES {
            return Err(Error::InvalidInput);
        }
        self.bytes = self.charge(packed.bytes())?;
        self.data.push((start, packed));
        Ok(())
    }
    /// The chunk admitted last; callers that push one page at a time read their own chunk here.
    pub fn last_packed(&self) -> Option<&Packed> {
        self.data.last().map(|(_, packed)| packed)
    }
    pub fn finish(self) -> PreparedRanges {
        let lease = std::sync::OnceLock::new();
        if let Some(charge) = self.lease {
            lease.set(charge).expect("new generation lease is empty");
        }
        PreparedRanges {
            asset: self.recipe.asset,
            ranges: Ranges {
                data: self.data.into_boxed_slice(),
                lease,
            },
            bytes: self.bytes,
        }
    }
}
