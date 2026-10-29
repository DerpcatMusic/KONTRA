//! One loaded sample: pinned pages (the preload law) plus a table of streamed pages.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::cache::tag;
use crate::fmt::{Mix, SampleFmt, emit};
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

/// A pinned page: packed PCM, or the codec's blocks as stored, decoded on
/// read (`[count: u32][start: u32; count + 1][blocks]`).
#[derive(Clone)]
pub(crate) enum Page {
    Pcm(Arc<[u8]>),
    Blocks(Arc<[u8]>),
}

impl Page {
    pub(crate) fn bytes(&self) -> &Arc<[u8]> {
        match self {
            Self::Pcm(b) | Self::Blocks(b) => b,
        }
    }

    /// Lays out `blocks` (bytes and starts, one more start for the end) as a
    /// [`Page::Blocks`].
    pub(crate) fn blocks(bytes: &[u8], starts: &[u32]) -> Self {
        let table = 4 * (starts.len() + 1);
        let mut out = Vec::with_capacity(table + bytes.len());
        let count = starts.len().saturating_sub(1) as u32;
        out.extend(count.to_le_bytes());
        out.extend(starts.iter().flat_map(|s| s.to_le_bytes()));
        out.extend_from_slice(bytes);
        Self::Blocks(out.into())
    }
}

/// Block `n` of a [`Page::Blocks`] page.
fn block(page: &[u8], n: usize) -> Option<&[u8]> {
    let word = |i: usize| {
        let b = page.get(4 * i..4 * i + 4)?;
        Some(u32::from_le_bytes(b.try_into().ok()?) as usize)
    };
    let count = word(0)?;
    if n >= count {
        return None;
    }
    let table = 4 * (count + 2);
    page.get(table + word(1 + n)?..table + word(2 + n)?)
}

pub struct Sample {
    pub(crate) id: SampleId,
    pub(crate) fmt: SampleFmt,
    pub(crate) frames: u64,
    /// log2 of this sample's frames per page.
    pub(crate) shift: u32,
    pub(crate) source: Arc<dyn Source>,
    /// Preloaded pages, shared with every other sample of the same blob.
    pub(crate) pinned: Box<[Option<Page>]>,
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
        match self.pinned.get(page) {
            Some(Some(Page::Pcm(bytes))) => {
                self.fmt
                    .decode(|i| bytes.get(i).copied().unwrap_or(0), offset, out, mix);
                return true;
            }
            Some(Some(Page::Blocks(bytes))) => {
                return self.read_blocks(bytes, page, offset, out, mix);
            }
            _ => {}
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

    /// [`Self::read_page`] from a page of stored blocks: decodes each block
    /// the read touches on the stack. Never allocates.
    fn read_blocks(
        &self,
        bytes: &[u8],
        page: usize,
        offset: usize,
        out: &mut [[f32; 2]],
        mix: Option<&Mix>,
    ) -> bool {
        const BLOCK: usize = ncw::SAMPLES_PER_BLOCK;
        let Some(packing) = self.source.packing() else {
            out.fill([0.0; 2]);
            return false;
        };
        let page_first = (page as u64) << self.shift;
        let mut values = [[0i32; BLOCK]; 2];
        let mut done = 0;
        while done < out.len() {
            let at = offset + done;
            let index = ((page_first + at as u64) / BLOCK as u64) as usize;
            let frames = block(bytes, at / BLOCK).and_then(|group| {
                ncw::decode_group_fixed(group, &packing.header, packing.format, index, &mut values)
                    .ok()
            });
            let skip = at % BLOCK;
            let n = (BLOCK - skip).min(out.len() - done);
            let Some(chunk) = out.get_mut(done..done + n) else {
                break;
            };
            let Some(frames) = frames else {
                chunk.fill([0.0; 2]);
                return false;
            };
            let value = |f: usize, c: usize| {
                let v = values.get(c).and_then(|ch| ch.get(skip + f)).copied();
                if skip + f < frames {
                    v.map_or(0.0, |v| packing.value(v))
                } else {
                    0.0
                }
            };
            emit(usize::from(self.fmt.channels), value, chunk, mix);
            done += n;
        }
        true
    }

    /// Audio thread: asks the store for the streamed pages covering
    /// `first..first + frames`. The voice reaches the run's start (its end
    /// when `descending`) at sample time `due` and moves `step` source frames
    /// per output frame, so each page is due when the voice gets to it: the
    /// store reads the most urgent first. Pages already pinned, resident or
    /// requested cost one load each. A full request ring is counted and the
    /// page is asked for again on the next call.
    pub fn prefetch(
        &self,
        client: &StreamClient,
        first: u64,
        frames: u64,
        due: u64,
        step: f64,
        descending: bool,
    ) {
        let end = first.saturating_add(frames).min(self.frames);
        if first >= end {
            return;
        }
        for page in page_of(first, self.shift)..=page_of(end - 1, self.shift) {
            let due = page_due(page, self.shift, first..end, due, step, descending);
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

/// When a voice reading `run` (from its start, or its end when
/// `descending`, at sample time `due`, `step` source frames per output frame)
/// first needs `page`.
fn page_due(
    page: usize,
    shift: u32,
    run: Range<u64>,
    due: u64,
    step: f64,
    descending: bool,
) -> u64 {
    let lo = ((page as u64) << shift).max(run.start);
    let hi = (((page as u64 + 1) << shift) - 1).min(run.end.saturating_sub(1));
    let lead = if descending {
        run.end.saturating_sub(1).saturating_sub(hi)
    } else {
        lo.saturating_sub(run.start)
    };
    due.saturating_add((lead as f64 / step.max(1e-3)) as u64)
}

pub(crate) fn page_of(frame: u64, shift: u32) -> usize {
    (frame >> shift) as usize
}

pub(crate) fn page_count(frames: u64, shift: u32) -> usize {
    frames.div_ceil(1 << shift) as usize
}

#[cfg(test)]
mod tests {
    use super::page_due;

    #[test]
    fn a_page_is_due_when_the_voice_reaches_it() {
        // 1024-frame pages; a run over frames 1000..5000 reached at time 100.
        let due = |page, step, descending| page_due(page, 10, 1000..5000, 100, step, descending);
        assert_eq!(due(0, 1.0, false), 100);
        assert_eq!(due(1, 1.0, false), 100 + 24);
        assert_eq!(due(4, 1.0, false), 100 + 3096);
        // Twice the pitch: there twice as soon.
        assert_eq!(due(4, 2.0, false), 100 + 1548);
        // Backwards from frame 4999: the last page first.
        assert_eq!(due(4, 1.0, true), 100);
        assert_eq!(due(3, 1.0, true), 100 + 904);
        assert_eq!(due(0, 1.0, true), 100 + 3976);
    }
}
