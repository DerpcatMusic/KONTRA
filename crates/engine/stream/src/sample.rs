//! One loaded sample: pinned pages (the preload law) plus a table of streamed pages.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::cache::tag;
use crate::fmt::{Mix, SampleFmt};
use crate::source::Source;
use crate::store::{Request, StreamClient};

/// Frames per page. 1024 frames of 24-bit stereo is 6 KiB, Kontakt's DFD
/// granularity. A sample with frames wider than the store's cache slots
/// allow halves its pages until one fits a slot.
pub const PAGE_FRAMES: u64 = 1024;
const PAGE_SHIFT: u32 = PAGE_FRAMES.trailing_zeros();

/// log2 of the frames per page for frames of `frame_bytes` in slots of
/// `PAGE_FRAMES · max_frame_bytes` bytes.
pub(crate) fn page_shift(frame_bytes: usize, max_frame_bytes: usize) -> u32 {
    let mut shift = PAGE_SHIFT;
    while shift > 0 && (frame_bytes << shift) > (max_frame_bytes << PAGE_SHIFT) {
        shift -= 1;
    }
    shift
}

/// Store-assigned identity of a loaded sample, used in requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SampleId(pub(crate) u32);

// Streamed page table entry states; a resident page stores `slot + RESIDENT`.
pub(crate) const ABSENT: u32 = 0;
pub(crate) const REQUESTED: u32 = 1;
pub(crate) const RESIDENT: u32 = 2;

pub struct Sample {
    pub(crate) id: SampleId,
    pub(crate) fmt: SampleFmt,
    pub(crate) frames: u64,
    /// log2 of this sample's frames per page.
    pub(crate) shift: u32,
    pub(crate) source: Arc<dyn Source>,
    /// Preloaded pages, shared with every other sample of the same blob.
    pub(crate) pinned: Box<[Option<Arc<[u8]>>]>,
    pub(crate) streamed: Box<[AtomicU32]>,
}

/// What one [`Sample::read`] could not deliver.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Read {
    /// Frames inside the sample whose data was not in memory; they play as silence.
    pub missing: u32,
}

impl Sample {
    #[must_use]
    pub fn frames(&self) -> u64 {
        self.frames
    }

    #[must_use]
    pub fn fmt(&self) -> SampleFmt {
        self.fmt
    }

    /// Whether frame `frame` is preloaded, so a voice starting there plays at once.
    #[must_use]
    pub fn is_pinned(&self, frame: u64) -> bool {
        self.pinned
            .get(page_of(frame, self.shift))
            .is_some_and(Option::is_some)
    }

    /// Frames in each of this sample's pages.
    pub(crate) fn page_frames(&self) -> u64 {
        1 << self.shift
    }

    /// Audio thread: decodes frames `first..first + out.len()` as stereo. Never
    /// blocks or allocates. Frames past the end are silence; frames not in memory
    /// are silence and counted in [`Read::missing`]. The caller keeps its timeline.
    pub fn read(&self, client: &StreamClient, first: u64, out: &mut [[f32; 2]]) -> Read {
        self.read_mixed(client, first, out, None)
    }

    /// As [`Self::read`], each channel reaching the outputs by its row of
    /// `mix`; `None` plays mono on both sides and two or more channels as
    /// left and right.
    pub fn read_mixed(
        &self,
        client: &StreamClient,
        first: u64,
        out: &mut [[f32; 2]],
        mix: Option<&Mix>,
    ) -> Read {
        let size = self.page_frames();
        let mut read = Read::default();
        let mut pos = first;
        let mut rest = out;
        while !rest.is_empty() {
            if pos >= self.frames {
                rest.fill([0.0; 2]);
                break;
            }
            let page = page_of(pos, self.shift);
            let offset = (pos % size) as usize;
            let len = (size - pos % size)
                .min(self.frames - pos)
                .min(rest.len() as u64) as usize;
            let (chunk, tail) = rest.split_at_mut(len);
            if !self.read_page(client, page, offset, chunk, mix) {
                read.missing += len as u32;
            }
            rest = tail;
            pos += len as u64;
        }
        read
    }

    fn read_page(
        &self,
        client: &StreamClient,
        page: usize,
        offset: usize,
        out: &mut [[f32; 2]],
        mix: Option<&Mix>,
    ) -> bool {
        if let Some(Some(bytes)) = self.pinned.get(page) {
            self.fmt
                .decode(|i| bytes.get(i).copied().unwrap_or(0), offset, out, mix);
            return true;
        }
        let entry = self
            .streamed
            .get(page)
            .map_or(ABSENT, |e| e.load(Ordering::Acquire));
        if entry >= RESIDENT {
            return client.cache().read(
                entry - RESIDENT,
                tag(self.id.0, page as u32),
                self.fmt,
                offset,
                out,
                mix,
            );
        }
        out.fill([0.0; 2]);
        false
    }

    /// Audio thread: asks the store for the streamed pages covering
    /// `first..first + frames`, needed by sample time `due`. Pages already pinned,
    /// resident or requested cost one load each. A full request ring is counted
    /// and the page is asked for again on the next call.
    pub fn prefetch(&self, client: &StreamClient, first: u64, frames: u64, due: u64) {
        let end = first.saturating_add(frames).min(self.frames);
        if first >= end {
            return;
        }
        for page in page_of(first, self.shift)..=page_of(end - 1, self.shift) {
            if self.pinned.get(page).is_some_and(Option::is_some) {
                continue;
            }
            let Some(entry) = self.streamed.get(page) else {
                continue;
            };
            if entry
                .compare_exchange(ABSENT, REQUESTED, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }
            if !client.push(Request {
                sample: self.id,
                page: page as u32,
                due,
            }) {
                entry.store(ABSENT, Ordering::Relaxed);
            }
        }
    }
}

pub(crate) fn page_of(frame: u64, shift: u32) -> usize {
    (frame >> shift) as usize
}

pub(crate) fn page_count(frames: u64, shift: u32) -> usize {
    frames.div_ceil(1 << shift) as usize
}
