//! Streamed instruments: random-access sample reads, resident heads sized from
//! measured read latency, and the decode worker thread behind the core's
//! page cache. Authored RAM-only sources, heads and a bounded page pool are resident.

use crate::{
    LoadError, head_cache,
    samples::{FileAt, Source},
};
use sampler_core::{AssetId, DecodeFailure, HeadBudget, HeadBudgetStatus, HeadPreparer, PAGE_FRAMES, Pcm, ResidentBuffer, StreamCache, StreamWorker, reservation_reach_pages};
use sampler_ir as ir;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    ops::Range,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

type Frame = [f32; 2];
#[path = "head_prepare.rs"]
mod head_prepare;
#[path = "stream_initial_hints.rs"]
mod initial_hints;
use crate::dfd_window;
use initial_hints::{InitialStartHints, zone_start_ranges};
#[cfg(test)]
use initial_hints::StartRange;

/// A sample's bytes as a seekable stream, decrypted on the fly. Reads go
/// through one buffer: decoders read a few bytes at a time.
/// Sample bytes streamed from disk so far (loading reads are not counted).
/// Port from v1 0cb7a8a0:src/audio.rs.
pub static DISK_READ: AtomicU64 = AtomicU64::new(0);

struct Bytes {
    counted: bool,
    file: Arc<File>,
    source: Source,
    position: u64,
    /// Decrypted bytes from member offset `at`.
    buffer: Vec<u8>,
    at: u64,
}

/// Bytes per file read.
const BUFFER: usize = 16 << 10;

impl Bytes {
    fn open(source: &Source, counted: bool) -> io::Result<Self> {
        let file = match &source.handle {
            Some(handle) => handle.clone(),
            None => Arc::new(File::open(&source.path)?),
        };
        source.check_snapshot(&file)?;
        Ok(Self {
            counted,
            file,
            source: source.clone(),
            position: 0,
            buffer: Vec::with_capacity(BUFFER),
            at: 0,
        })
    }
}

/// Control-side complete encoded resident read through the ordinary immutable
/// Source recipe. This does not create a new decryption or path resolver.
pub(crate) fn read_encoded_resident(source: &Source, byte_limit: usize) -> io::Result<Vec<u8>> {
    let len=usize::try_from(source.size).map_err(|_|io::Error::new(io::ErrorKind::InvalidData,"encoded sample length overflow"))?;
    if len==0 || len>byte_limit {return Err(io::Error::new(io::ErrorKind::InvalidData,"encoded resident sample limit exceeded"));}
    let mut input=Bytes::open(source,false)?;
    let mut bytes=Vec::new();bytes.try_reserve_exact(len).map_err(|e|io::Error::new(io::ErrorKind::OutOfMemory,e))?;
    bytes.resize(len,0);input.read_exact(&mut bytes)?;
    source.check_snapshot(&input.file)?;
    Ok(bytes)
}

impl Read for Bytes {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let end = self.at + self.buffer.len() as u64;
        if self.position < self.at || self.position >= end {
            let left = self.source.size.saturating_sub(self.position);
            let len = BUFFER.min(usize::try_from(left).unwrap_or(usize::MAX));
            let mut file = FileAt {
                file: &self.file,
                pos: self.source.offset + self.position,
            };
            self.buffer.resize(len, 0);
            let mut filled = 0;
            while filled < len {
                match file.read(&mut self.buffer[filled..])? {
                    0 => break,
                    n => {
                        filled += n;
                        if self.counted {
                            DISK_READ.fetch_add(n as u64, Ordering::Relaxed);
                        }
                    }
                }
            }
            self.buffer.truncate(filled);
            self.source.check_snapshot(&self.file)?;
            if filled < len {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            if let Some(key) = &self.source.key {
                key.apply_at(self.position, &mut self.buffer);
            }
            self.at = self.position;
        }
        let offset = (self.position - self.at) as usize;
        let n = buf.len().min(self.buffer.len() - offset);
        buf[..n].copy_from_slice(&self.buffer[offset..offset + n]);
        self.position += n as u64;
        Ok(n)
    }
}

impl Seek for Bytes {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.position = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(delta) => self.source.size.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        }
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        Ok(self.position)
    }
}

impl symphonia::core::io::MediaSource for Bytes {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.source.size)
    }
}

/// Fills frames from a start.
type Reader = Box<dyn FnMut(usize, &mut [Frame]) -> io::Result<()> + Send>;

enum Codec {
    Wav(crate::samples::Wav),
    Ncw(Box<ncw::NcwReader<Bytes>>),
    /// A reader supplied by another loader: fills frames from a start.
    Custom(Reader),
    Encoded(Box<crate::EncodedChannelReader>),
}

/// Where an asset's frames are read from, reopened by each decode thread.
pub trait AssetSource: Send + Sync {
    fn open(&self) -> io::Result<SampleReader>;
    /// Original-channel reader admission is explicit. Opaque custom closures
    /// retain their legacy stereo domain and cannot acquire wide authority.
    fn open_channels(&self, _workspace_limit:usize) -> io::Result<Option<SampleReader>> { Ok(None) }
    fn open_stream_channels(&self, workspace_limit:usize) -> io::Result<Option<SampleReader>> {self.open_channels(workspace_limit)}
    /// Physical page source whose immutable recipe remains one logical reader.
    fn channel_pack(&self) -> Option<(&LogicalStreamRecipe, usize)> { None }
    /// A validated persistent numeric header, when available.
    fn header(&self) -> Option<(u32, usize)> {
        None
    }
    /// Channel count and sample width, read from the file header without
    /// counting as an open.
    fn header_layout(&self) -> Option<FrameLayout> {
        None
    }
    /// Playback/reload IO, counted separately from initial loading.
    fn open_stream(&self) -> io::Result<SampleReader> {
        self.open()
    }
    /// Equal keys decode to equal frames, so decoded head chunks may be shared
    /// in process. `None` (the default) opts out of the head cache.
    fn content_key(&self) -> Option<Arc<str>> {
        None
    }
}

impl AssetSource for Source {
    fn content_key(&self) -> Option<Arc<str>> {
        Some(self.source_key())
    }
    fn open_stream_channels(&self,workspace_limit:usize)->io::Result<Option<SampleReader>> {
        open_native_channel_reader(self,true,workspace_limit)
    }
    fn open_channels(&self,workspace_limit:usize)->io::Result<Option<SampleReader>> {
        open_native_channel_reader(self,false,workspace_limit)
    }
    fn open(&self) -> io::Result<SampleReader> {
        SampleReader::open(self)
    }
    fn header_layout(&self) -> Option<FrameLayout> {
        let mut bytes = Bytes::open(self, false).ok()?;
        let mut head = Vec::new();
        (&mut bytes).take(1 << 16).read_to_end(&mut head).ok()?;
        layout_from_head(&head)
    }
    fn header(&self) -> Option<(u32, usize)> {
        self.header
    }
    fn open_stream(&self) -> io::Result<SampleReader> {
        SampleReader::open_counted(self, true)
    }
}

/// Original width discovery and new-wide native workspace admission. Legacy
/// mono/stereo takes its original opener, without imposing this new wide cap.
/// The layout of a RIFF, FORM (AIFF) or NCW header's leading bytes. FORM goes
/// through `aiff_reader`, the same branch `open_native_channel_reader` uses, so
/// a mono 24-bit AIFF is not sized as the 16-bit stereo fallback.
fn layout_from_head(head: &[u8]) -> Option<FrameLayout> {
    let wav = if head.starts_with(b"FORM") {
        crate::samples::aiff_reader(&mut std::io::Cursor::new(head)).ok()?
    } else if head.starts_with(b"RIFF") {
        crate::samples::wav_layout(head).ok()?
    } else {
        let header = ncw::NcwHeader::read(&mut std::io::Cursor::new(head)).ok()?;
        return Some(FrameLayout {
            channels: usize::from(header.channels),
            bits: usize::from(header.bits_per_sample),
        });
    };
    Some(FrameLayout { channels: wav.channels, bits: wav.width * 8 })
}

fn open_native_channel_reader(source:&Source,counted:bool,workspace_limit:usize)->io::Result<Option<SampleReader>> {
    let mut bytes=Bytes::open(source,counted)?;let mut head=Vec::new();
    (&mut bytes).take(1<<12).read_to_end(&mut head)?;
    let channels=if head.starts_with(b"RIFF") || head.starts_with(b"FORM") {
        let wav=if head.starts_with(b"FORM") {
            bytes.seek(SeekFrom::Start(0))?;crate::samples::aiff_reader(&mut bytes)
        } else {
            if crate::samples::wav_layout(&head).is_err() {bytes.seek(SeekFrom::Start(0))?;head.clear();(&mut bytes).take(1<<16).read_to_end(&mut head)?;}
            crate::samples::wav_layout(&head)
        }.map_err(|e|io::Error::new(io::ErrorKind::InvalidData,e))?;
        if wav.channels>2 {
            let cost=PAGE_FRAMES.checked_mul(wav.channels).and_then(|n|n.checked_mul(wav.width)).and_then(|n|n.checked_add(BUFFER+source.path.capacity()+size_of::<SampleReader>()))
                .ok_or_else(||io::Error::new(io::ErrorKind::OutOfMemory,"native PCM workspace overflow"))?;
            if cost>workspace_limit {return Err(io::Error::new(io::ErrorKind::OutOfMemory,"native PCM workspace admission"));}
        }
        wav.channels
    } else {
        let header=ncw::NcwHeader::read(&mut std::io::Cursor::new(&head)).map_err(|e|io::Error::new(io::ErrorKind::InvalidData,e.to_string()))?;
        let channels=usize::from(header.channels);
        if channels>2 {
            let blocks=usize::try_from(header.num_samples.div_ceil(ncw::NcwReader::<Bytes>::FRAMES_PER_BLOCK as u32)).map_err(io::Error::other)?;
            let entries=header.data_offset.checked_sub(header.blocks_offset).ok_or_else(||io::Error::new(io::ErrorKind::InvalidData,"NCW index geometry"))?;
            if u64::from(header.data_offset)+u64::from(header.data_size)>source.size || u64::from(entries)!=(blocks as u64+1)*4 {
                return Err(io::Error::new(io::ErrorKind::InvalidData,"NCW declared index extent"));
            }
            let index=blocks.max(4).checked_next_power_of_two().and_then(|n|n.checked_mul(size_of::<u32>()))
                .ok_or_else(||io::Error::new(io::ErrorKind::OutOfMemory,"NCW index capacity overflow"))?;
            let outer=channels.checked_next_power_of_two().and_then(|n|n.checked_mul(size_of::<Vec<i32>>()))
                .ok_or_else(||io::Error::new(io::ErrorKind::OutOfMemory,"NCW channel capacity overflow"))?;
            let decoded=channels.checked_mul(ncw::NcwReader::<Bytes>::FRAMES_PER_BLOCK).and_then(|n|n.checked_mul(size_of::<i32>()))
                .ok_or_else(||io::Error::new(io::ErrorKind::OutOfMemory,"NCW block capacity overflow"))?;
            let cost=index.checked_add(outer).and_then(|n|n.checked_add(decoded)).and_then(|n|n.checked_add(ncw::NcwReader::<Bytes>::FRAMES_PER_BLOCK.checked_mul(size_of::<i32>())?))
                .and_then(|n|n.checked_add(BUFFER+source.path.capacity()+size_of::<ncw::NcwReader<Bytes>>()+size_of::<SampleReader>()))
                .ok_or_else(||io::Error::new(io::ErrorKind::OutOfMemory,"NCW aggregate workspace overflow"))?;
            if cost>workspace_limit {return Err(io::Error::new(io::ErrorKind::OutOfMemory,"NCW workspace admission before seek-table decode"));}
        }
        channels
    };
    drop(bytes);drop(head);
    if channels<=2 {return Ok(None);}
    let reader=SampleReader::open_counted(source,counted)?;
    if reader.original_channels()!=Some(channels) || reader.native_workspace_bound().is_none_or(|n|n>workspace_limit) {
        return Err(io::Error::new(io::ErrorKind::InvalidData,"original native channel admission changed"));
    }
    Ok(Some(reader))
}

/// Checked geometry and frozen normal origin for one paged logical source.
/// Contains real physical IDs but NO Pcm/SourceStorage strong references: loader
/// closures may retain this recipe without forming a residency-owner cycle.
pub struct LogicalStreamRecipe {
    id: sampler_core::SourceStorageId,
    source: Arc<dyn AssetSource>,
    channels: usize,
    rate: u32,
    frames: usize,
    packs: Box<[AssetId]>,
    workspace_limit: usize,
}
impl LogicalStreamRecipe {
    fn open(&self) -> io::Result<SampleReader> {self.open_counted(true)}
    fn open_initial(&self) -> io::Result<SampleReader> {self.open_counted(false)}
    fn reader_workspace_limit(&self) -> io::Result<usize> {
        // Persistent all-pack scratch and temporary destination references
        // coexist with the owned native reader. Reserve their logical
        // capacities BEFORE opening a decoder or allocating that scratch.
        let scratch=self.packs.len().checked_mul(PAGE_FRAMES)
            .and_then(|n|n.checked_mul(size_of::<Frame>()))
            .and_then(|n|self.packs.len().checked_mul(size_of::<Vec<Frame>>()).and_then(|meta|n.checked_add(meta)))
            .and_then(|n|self.packs.len().checked_mul(size_of::<&mut [Frame]>()).and_then(|refs|n.checked_add(refs)))
            .ok_or_else(||io::Error::new(io::ErrorKind::OutOfMemory,"logical coexisting workspace overflow"))?;
        self.workspace_limit.checked_sub(scratch)
            .ok_or_else(||io::Error::new(io::ErrorKind::OutOfMemory,"logical scratch exhausted native reader admission"))
    }
    fn open_counted(&self,counted:bool) -> io::Result<SampleReader> {
        let reader_limit=self.reader_workspace_limit()?;
        let reader = (if counted {self.source.open_stream_channels(reader_limit)} else {self.source.open_channels(reader_limit)})?.ok_or_else(||io::Error::new(io::ErrorKind::Unsupported,"logical source has no typed reader"))?;
        if reader.original_channels()!=Some(self.channels) || reader.rate()!=self.rate || reader.frames()!=self.frames {
            return Err(io::Error::new(io::ErrorKind::InvalidData,"logical source geometry changed"));
        }
        if reader.native_workspace_bound().is_none_or(|n|n>reader_limit) {
            return Err(io::Error::new(io::ErrorKind::OutOfMemory,"logical reader workspace exceeds admission"));
        }
        Ok(reader)
    }
    fn scratch(&self) -> io::Result<Vec<Vec<Frame>>> {
        self.reader_workspace_limit()?;
        let bytes=self.packs.len().checked_mul(PAGE_FRAMES).and_then(|n|n.checked_mul(size_of::<Frame>()))
            .and_then(|n|n.checked_add(self.packs.len()*size_of::<Vec<Frame>>()))
            .ok_or_else(||io::Error::new(io::ErrorKind::OutOfMemory,"logical page workspace overflow"))?;
        if bytes>self.workspace_limit {return Err(io::Error::new(io::ErrorKind::OutOfMemory,"logical page workspace limit"));}
        let mut packs=Vec::new();packs.try_reserve_exact(self.packs.len()).map_err(io::Error::other)?;
        for _ in &self.packs {let mut page=Vec::new();page.try_reserve_exact(PAGE_FRAMES).map_err(io::Error::other)?;page.resize(PAGE_FRAMES,[0.;2]);packs.push(page);}
        Ok(packs)
    }
    fn read(&self, reader:&mut SampleReader, start:usize, frames:usize, scratch:&mut [Vec<Frame>]) -> io::Result<()> {
        if frames>PAGE_FRAMES || scratch.len()!=self.packs.len() || scratch.iter().any(|p|p.len()!=PAGE_FRAMES) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,"logical page scratch geometry"));
        }
        let scratch_bytes=scratch.iter().try_fold(scratch.len()*size_of::<Vec<Frame>>(),|n,p|n.checked_add(p.capacity()*size_of::<Frame>()))
            .ok_or_else(||io::Error::new(io::ErrorKind::OutOfMemory,"logical scratch accounting overflow"))?;
        let reader_limit=self.workspace_limit.checked_sub(scratch_bytes).ok_or_else(||io::Error::new(io::ErrorKind::OutOfMemory,"logical scratch exhausted workspace"))?;
        // This small destination-reference vector is control/worker-only and
        // charged alongside the persistent page workspace before decoder use.
        let refs_bytes=scratch.len().checked_mul(size_of::<&mut [Frame]>()).ok_or_else(||io::Error::new(io::ErrorKind::OutOfMemory,"logical reference overflow"))?;
        let reader_limit=reader_limit.checked_sub(refs_bytes).ok_or_else(||io::Error::new(io::ErrorKind::OutOfMemory,"logical reference workspace limit"))?;
        let mut refs=Vec::new();refs.try_reserve_exact(scratch.len()).map_err(io::Error::other)?;
        for page in scratch.iter_mut() {refs.push(&mut page[..frames]);}
        if refs.capacity().checked_mul(size_of::<&mut [Frame]>()).is_none_or(|n|n>refs_bytes) {
            return Err(io::Error::new(io::ErrorKind::OutOfMemory,"logical reference capacity exceeds admission"));
        }
        let mut out=crate::channel_reader::ChannelPackDestinations::new(self.channels,frames,&mut refs)?;
        reader.read_packs(start,&mut out,reader_limit)?;
        drop(out);drop(refs);
        if scratch.iter().any(|page|page[..frames].iter().flatten().any(|v|!v.is_finite()))
            || self.channels%2!=0 && scratch.last().is_some_and(|page|page[..frames].iter().any(|f|f[1].to_bits()!=0)) {
            return Err(io::Error::new(io::ErrorKind::InvalidData,"logical page contains invalid original channels"));
        }
        Ok(())
    }
}
struct LogicalPackSource { recipe: Arc<LogicalStreamRecipe>, ordinal: usize }
impl AssetSource for LogicalPackSource {
    fn open(&self) -> io::Result<SampleReader> {
        // A single-pack legacy reader must never silently decode/truncate this
        // family. Normal workers and heads dispatch explicit logical batches.
        Err(io::Error::new(io::ErrorKind::Unsupported,"logical family requires batch reader"))
    }
    fn header(&self)->Option<(u32,usize)> {Some((self.recipe.rate,self.recipe.frames))}
    fn channel_pack(&self)->Option<(&LogicalStreamRecipe,usize)> {Some((&self.recipe,self.ordinal))}
}
/// Construct unpublished normal streamed storage and real physical recipe keys.
/// Caller owns the frozen ordinary AssetSource; custom opaque readers refuse.
pub fn logical_stream_source(source:Arc<dyn AssetSource>, workspace_limit:usize)
    ->io::Result<(sampler_core::SourceStorage,Vec<(AssetId,Arc<dyn AssetSource>)>)> {
    let reader=source.open_channels(workspace_limit)?.ok_or_else(||io::Error::new(io::ErrorKind::Unsupported,"typed original-channel reader unavailable"))?;
    let channels=reader.original_channels().ok_or_else(||io::Error::new(io::ErrorKind::Unsupported,"opaque channel geometry"))?;
    if channels<=2 {return Err(io::Error::new(io::ErrorKind::InvalidInput,"legacy source uses normal stereo constructor"));}
    if reader.native_workspace_bound().is_none_or(|n|n>workspace_limit) {return Err(io::Error::new(io::ErrorKind::OutOfMemory,"logical decoder workspace admission"));}
    let rate=reader.rate();let frames=reader.frames();drop(reader);
    let count=sampler_core::ChannelCount::new(channels).map_err(io::Error::other)?;
    let mut packs=Vec::new();packs.try_reserve_exact(count.pack_count()).map_err(io::Error::other)?;
    for _ in 0..count.pack_count() {packs.push(Pcm::streamed(rate,frames).map_err(io::Error::other)?);}
    let ids=packs.iter().map(Pcm::asset_id).collect::<Vec<_>>().into_boxed_slice();
    let family=sampler_core::SourceStorageFamily::new_paged(count,packs).map_err(io::Error::other)?;
    let recipe=Arc::new(LogicalStreamRecipe {id:family.id(),source,channels,rate,frames,packs:ids,workspace_limit});
    // Reserve all metadata/page scratch capacity before exposing recipe keys.
    let scratch=recipe.scratch()?;let mut reader=recipe.open_initial()?;
    recipe.read(&mut reader,0,frames.min(PAGE_FRAMES),&mut {scratch})?;
    let registry=recipe.packs.iter().copied().enumerate().map(|(ordinal,id)|
        (id,Arc::new(LogicalPackSource{recipe:recipe.clone(),ordinal}) as Arc<dyn AssetSource>)).collect();
    Ok((family.into(),registry))
}

/// NCW blocks that failed to decode, by sample file. A damaged block plays
/// silence and the file is reported as partial. Damage is a fact about the
/// file, so every instrument that uses the file reports it.
static DAMAGED_BLOCKS: std::sync::Mutex<std::collections::BTreeMap<String, std::collections::BTreeSet<usize>>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Identity of one sample: its file, and its data offset inside an archive.
pub(crate) fn source_label(source: &Source) -> String {
    format!("{}#{}", source.path.display(), source.offset)
}

fn record_damaged_block(label: &str, block: usize) {
    if let Ok(mut damaged) = DAMAGED_BLOCKS.lock() {
        damaged.entry(label.to_string()).or_default().insert(block);
    }
}

/// Damaged sample files (path → number of silent blocks) seen so far.
pub fn damaged_sources() -> std::collections::BTreeMap<String, usize> {
    DAMAGED_BLOCKS
        .lock()
        .map(|damaged| damaged.iter().map(|(path, blocks)| (path.clone(), blocks.len())).collect())
        .unwrap_or_default()
}

/// Random-access frames of one sample, converted exactly as a full decode.
pub struct SampleReader {
    codec: Codec,
    /// The sample file, named in decode errors so a bad block points at its asset.
    label: String,
    /// The WAV byte stream (NCW keeps its own inside the decoder).
    bytes: Option<Bytes>,
    rate: u32,
    frames: usize,
    scratch: Vec<u8>,
}

impl SampleReader {
    /// Charged owned userspace capacities for the built-in WAV/NCW reader,
    /// including its File Arc allocation. Allocator and kernel FD overhead are
    /// separate from this capacity budget. A custom
    /// closure can own opaque allocations, so it keeps the legacy entry cap
    /// rather than entering the expanded byte-budget cache.
    fn cache_bytes(&self) -> Option<usize> {
        let bytes = |b: &Bytes| {
            let owned_handle = if b.source.handle.is_none() {
                (std::mem::size_of::<File>() + 2 * std::mem::size_of::<usize>()).next_multiple_of(
                    std::mem::align_of::<usize>().max(std::mem::align_of::<File>()),
                )
            } else {
                0
            };
            b.buffer.capacity() + b.source.path.capacity() + owned_handle
        };
        let codec = match &self.codec {
            Codec::Wav(_) => self.bytes.as_ref().map_or(0, bytes),
            Codec::Ncw(reader) => {
                std::mem::size_of_val(&**reader)
                    + reader.workspace_bytes()
                    + bytes(reader.get_ref())
            }
            Codec::Encoded(reader)=>return reader.accounted_workspace_bytes(),
            Codec::Custom(_) => return None,
        };
        Some(self.scratch.capacity() + codec)
    }

    pub fn open(source: &Source) -> io::Result<Self> {
        Self::open_counted(source, false)
    }

    fn open_counted(source: &Source, counted: bool) -> io::Result<Self> {
        let invalid = |e: String| io::Error::new(io::ErrorKind::InvalidData, e);
        let mut bytes = Bytes::open(source, counted)?;
        let mut head = Vec::new();
        (&mut bytes).take(1 << 12).read_to_end(&mut head)?;
        let pcm = head.starts_with(b"RIFF") || head.starts_with(b"FORM");
        if pcm && crate::samples::wav_layout(&head).is_err() {
            // A WAV header past 4 KiB: read up to 64 KiB.
            bytes.seek(SeekFrom::Start(0))?;
            head.clear();
            (&mut bytes).take(1 << 16).read_to_end(&mut head)?;
        }
        if pcm {
            let wav = if head.starts_with(b"FORM") {
                bytes.seek(SeekFrom::Start(0))?;
                crate::samples::aiff_reader(&mut bytes).map_err(invalid)?
            } else {
                crate::samples::wav_layout(&head).map_err(invalid)?
            };
            let align = wav.width * wav.channels;
            if head.starts_with(b"FORM") {
                let form_end = 8 + u64::from(u32::from_be_bytes(head[4..8].try_into().unwrap()));
                if form_end > source.size || wav.data.end as u64 > source.size {
                    return Err(invalid("truncated AIFF sample data".into()));
                }
            }
            let end = wav
                .data
                .end
                .min(usize::try_from(source.size).unwrap_or(usize::MAX));
            let frames = end.saturating_sub(wav.data.start) / align;
            return Ok(Self {
                rate: wav.rate,
                frames,
                codec: Codec::Wav(wav),
                label: source_label(source),
                bytes: Some(bytes),
                scratch: Vec::new(),
            });
        }
        bytes.seek(SeekFrom::Start(0))?;
        if head.starts_with(b"FORM") {
            let mut reader = crate::pcm::Reader::open(Box::new(bytes))
                .map_err(|e| invalid(format!("AIFF: {e:#}")))?;
            let frames =
                usize::try_from(reader.frames).map_err(|_| invalid("AIFF too long".into()))?;
            return Ok(Self::custom(reader.rate, frames, move |start, out| {
                reader
                    .read(start as u64, out)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            }));
        }
        let reader = ncw::NcwReader::read(bytes)
            .map_err(|e| invalid(format!("NCW {}: {e}", source_label(source))))?;
        Ok(Self {
            rate: reader.header.sample_rate,
            frames: reader.header.num_samples as usize,
            codec: Codec::Ncw(Box::new(reader)),
            label: source_label(source),
            bytes: None,
            scratch: Vec::new(),
        })
    }

    /// A reader of `frames` frames at `rate` that fills output from `read(start, out)`.
    pub fn custom(
        rate: u32,
        frames: usize,
        read: impl FnMut(usize, &mut [Frame]) -> io::Result<()> + Send + 'static,
    ) -> Self {
        Self {
            codec: Codec::Custom(Box::new(read)),
            label: String::new(),
            bytes: None,
            rate,
            frames,
            scratch: Vec::new(),
        }
    }

    /// Same checked ordinary PCM decoder plus explicit caller accounting of
    /// its source owner. The codec bound is verified; an arbitrary MediaSource
    /// cost is a separate contract, not a certified total-owner capability.
    /// Legacy and opaque Custom decoders cannot enter this typed variant.
    pub fn accounted_encoded(reader:crate::EncodedChannelReader)->io::Result<Self> {
        reader.accounted_workspace_bytes().ok_or_else(||io::Error::new(io::ErrorKind::Unsupported,"encoded reader has no checked codec accounting"))?;
        let frames=usize::try_from(reader.frames()).map_err(io::Error::other)?;let rate=reader.rate();
        Ok(Self{codec:Codec::Encoded(Box::new(reader)),label:String::new(),bytes:None,rate,frames,scratch:Vec::new()})
    }
    pub fn rate(&self) -> u32 {
        self.rate
    }
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Typed normal WAV/AIFF/NCW original width. A custom stereo closure is
    /// intentionally not evidence of its original encoded source width.
    pub fn original_channels(&self) -> Option<usize> {
        match &self.codec { Codec::Wav(w) => Some(w.channels), Codec::Ncw(n) => Some(usize::from(n.header.channels)), Codec::Encoded(reader)=>Some(reader.channels()), Codec::Custom(_) => None }
    }
    /// Channel count and sample width from the header; `None` for encoded or custom readers.
    pub(crate) fn layout(&self) -> Option<FrameLayout> {
        match &self.codec {
            Codec::Wav(w) => Some(FrameLayout { channels: w.channels, bits: w.width * 8 }),
            Codec::Ncw(n) => Some(FrameLayout {
                channels: usize::from(n.header.channels),
                bits: usize::from(n.header.bits_per_sample),
            }),
            Codec::Encoded(_) | Codec::Custom(_) => None,
        }
    }
    pub fn owned_channel_reader_bytes(&self) -> Option<usize> { self.cache_bytes() }
    fn native_workspace_bound(&self) -> Option<usize> {
        let current=self.cache_bytes()?;
        match &self.codec {
            Codec::Wav(w)=>current.checked_add(PAGE_FRAMES.checked_mul(w.width)?.checked_mul(w.channels)?),
            Codec::Ncw(n)=> {
                // Existing native decoder owns a geometric outer Vec and at
                // most FRAMES_PER_BLOCK i32 values per original channel.
                let channels=usize::from(n.header.channels);
                current.checked_add(channels.checked_next_power_of_two()?.checked_mul(size_of::<Vec<i32>>())?)?
                    .checked_add(channels.checked_mul(ncw::NcwReader::<Bytes>::FRAMES_PER_BLOCK)?.checked_mul(size_of::<i32>())?)?
                    // Zero-bit/raw NCW read_block retains one raw input Vec
                    // while extending its channel output: up to512*4 bytes.
                    .checked_add(ncw::NcwReader::<Bytes>::FRAMES_PER_BLOCK.checked_mul(size_of::<i32>())?)
            }
            Codec::Encoded(reader)=>reader.accounted_workspace_bytes(),
            Codec::Custom(_)=>None,
        }
    }
    /// One normal decoder traversal scatters every original ordinal. Output is
    /// unpublished staging; any error discards the complete caller-owned batch.
    pub fn read_packs(&mut self, start: usize, out: &mut crate::channel_reader::ChannelPackDestinations<'_, '_>, workspace_limit: usize) -> io::Result<()> {
        if let Codec::Encoded(reader)=&mut self.codec {
            if reader.accounted_workspace_bytes().is_none_or(|n|n>workspace_limit) {return Err(io::Error::new(io::ErrorKind::OutOfMemory,"encoded channel workspace admission"));}
            out.clear();return reader.read_packs(start as u64,out.frames(),out);
        }
        let channels = self.original_channels().ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported,"opaque custom reader has no original-channel contract"))?;
        let frames = out.frames();
        if channels == 0 || channels != out.channels() || start.checked_add(frames).is_none_or(|end|end>self.frames) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,"channel read geometry mismatch"));
        }
        let current = self.cache_bytes().ok_or_else(||io::Error::new(io::ErrorKind::Unsupported,"reader accounting unavailable"))?;
        if self.native_workspace_bound().is_none_or(|n|n>workspace_limit) {
            return Err(io::Error::new(io::ErrorKind::OutOfMemory,"native channel workspace preflight"));
        }
        if current > workspace_limit {return Err(io::Error::new(io::ErrorKind::OutOfMemory,"channel reader workspace limit"));}
        // Native NCW decoding owns its existing bounded codec block. WAV/AIFF
        // conversion reserves exact input scratch before touching destinations.
        if let Codec::Wav(wav) = &self.codec {
            let bytes = frames.checked_mul(wav.width).and_then(|n|n.checked_mul(channels))
                .ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"channel scratch overflow"))?;
            let fixed = current.checked_sub(self.scratch.capacity()).ok_or_else(||io::Error::new(io::ErrorKind::InvalidData,"reader accounting mismatch"))?;
            if fixed.checked_add(bytes.max(self.scratch.capacity())).is_none_or(|n|n>workspace_limit) {
                return Err(io::Error::new(io::ErrorKind::OutOfMemory,"channel scratch limit"));
            }
            if bytes>self.scratch.capacity() {self.scratch.try_reserve_exact(bytes-self.scratch.len()).map_err(|e|io::Error::new(io::ErrorKind::OutOfMemory,e))?;}
            if fixed.checked_add(self.scratch.capacity()).is_none_or(|n|n>workspace_limit) {
                return Err(io::Error::new(io::ErrorKind::OutOfMemory,"channel scratch capacity limit"));
            }
            self.scratch.resize(bytes,0);
        }
        out.clear();
        match &mut self.codec {
            Codec::Wav(wav) => {
                let align=wav.width.checked_mul(channels).ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"WAV channel alignment"))?;
                let at=start.checked_mul(align).and_then(|n|n.checked_add(wav.data.start)).ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"WAV read offset overflow"))?;
                let bytes=self.bytes.as_mut().expect("native WAV owns guarded Bytes");bytes.seek(SeekFrom::Start(at as u64))?;bytes.read_exact(&mut self.scratch)?;
                for (frame,data) in self.scratch.chunks_exact(align).enumerate() {
                    for channel in 0..channels {out.put(frame,channel,wav.sample(&data[channel*wav.width..][..wav.width]));}
                }
            }
            Codec::Ncw(reader) => {
                const BLOCK:usize=ncw::NcwReader::<Bytes>::FRAMES_PER_BLOCK;
                let convert=crate::samples::ncw_sample(reader.sample_format,reader.header.bits_per_sample);
                let mut at=start;let mut done=0;
                while done<frames {
                    let values=match reader.decode_block(at/BLOCK) {
                        Ok(values)=>values,
                        Err(_)=>{
                            // A damaged block plays silence for its frames; the file is reported as partial.
                            record_damaged_block(&self.label,at/BLOCK);
                            let count=(BLOCK-at%BLOCK).min(frames-done);
                            for frame in 0..count {for channel in 0..channels {out.put(done+frame,channel,0.);}}
                            at+=count;done+=count;
                            continue;
                        }
                    };
                    if values.len()!=channels {return Err(io::Error::new(io::ErrorKind::InvalidData,"NCW channel width changed"));}
                    let offset=at%BLOCK;let count=values[0].len().saturating_sub(offset).min(frames-done);
                    if count==0 || values.iter().any(|v|v.len()<offset+count) {return Err(io::ErrorKind::UnexpectedEof.into());}
                    for frame in 0..count {for channel in 0..channels {out.put(done+frame,channel,convert(values[channel][offset+frame]));}}
                    at+=count;done+=count;
                }
            }
            Codec::Encoded(_)=>return Err(io::Error::new(io::ErrorKind::InvalidData,"encoded channel dispatch changed")),
            Codec::Custom(_)=>return Err(io::Error::new(io::ErrorKind::Unsupported,"custom reader cannot decode original packs")),
        }
        if self.cache_bytes().is_none_or(|bytes|bytes>workspace_limit) {return Err(io::Error::new(io::ErrorKind::OutOfMemory,"grown channel reader workspace limit"));}
        Ok(())
    }
    /// Fill `out` with frames from `start`.
    pub fn read(&mut self, start: usize, out: &mut [Frame]) -> io::Result<()> {
        let invalid = |e: String| io::Error::new(io::ErrorKind::InvalidData, e);
        if start + out.len() > self.frames {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        match &mut self.codec {
            Codec::Encoded(_)=>return Err(io::Error::new(io::ErrorKind::Unsupported,"accounted encoded source requires complete pack destinations")),
            Codec::Custom(read) => read(start, out)?,
            Codec::Wav(wav) => {
                let align = wav.width * wav.channels;
                let bytes = self.bytes.as_mut().expect("WAV bytes");
                bytes.seek(SeekFrom::Start((wav.data.start + start * align) as u64))?;
                self.scratch.resize(out.len() * align, 0);
                bytes.read_exact(&mut self.scratch)?;
                wav.frames_into(&self.scratch, out);
            }
            Codec::Ncw(reader) => {
                const BLOCK: usize = ncw::NcwReader::<Bytes>::FRAMES_PER_BLOCK;
                let convert =
                    crate::samples::ncw_sample(reader.sample_format, reader.header.bits_per_sample);
                let last = reader.header.channels.min(2) as usize - 1;
                let mut at = start;
                let mut out = &mut out[..];
                while !out.is_empty() {
                    let channels = match reader.decode_block(at / BLOCK) {
                        Ok(channels) => channels,
                        Err(_) => {
                            // A damaged block plays silence for its frames; the file is reported as partial.
                            record_damaged_block(&self.label, at / BLOCK);
                            let n = (BLOCK - at % BLOCK).min(out.len());
                            out[..n].fill([0.; 2]);
                            at += n;
                            out = &mut out[n..];
                            continue;
                        }
                    };
                    let offset = at % BLOCK;
                    let n = (channels[0].len().saturating_sub(offset)).min(out.len());
                    if n == 0 {
                        return Err(io::ErrorKind::UnexpectedEof.into());
                    }
                    for (i, frame) in out[..n].iter_mut().enumerate() {
                        *frame = [
                            convert(channels[0][offset + i]),
                            convert(channels[last][offset + i]),
                        ];
                    }
                    at += n;
                    out = &mut out[n..];
                }
            }
        }
        Ok(())
    }
}

/// How streamed assets were sized, and what is resident.
#[derive(Clone, Copy, Debug, Default)]
pub struct StreamReport {
    /// Frames the law guarantees resident past any permitted start, over the
    /// streamed resident zones (`runway_frames`). The stream runway and horizon
    /// are sized from this, not from `head_frames`.
    pub runway_frames: usize,
    /// Head read latency over the probed assets, open plus first page.
    pub latency_p50: Duration,
    pub latency_p95: Duration,
    pub latency_p99: Duration,
    /// Zero means the latency is unmeasured, not a zero-latency disk.
    pub latency_samples: usize,
    /// Largest zone preload head, in source frames (the preload law).
    pub head_frames: usize,
    /// Packed (resident source) bytes of the heads, as held.
    pub head_bytes: usize,
    /// Packed source bytes of Kontakt's DFD windows over the same zones, each sample
    /// counted once at its largest zone window. `head_bytes` should sit within 10% of it.
    pub kontakt_window_bytes: usize,
    /// Authored onset/offset/reverse ranges, before any lazy publication. These
    /// bounds describe all mapped starts, including currently muted groups.
    pub head_assets: usize,
    pub head_ranges: usize,
    pub head_source_frames: u64,
    /// Shared physical cap on required full PCM and packed starts, including
    /// staged/current/retired storage; decode workspaces/page pools are separate.
    pub head_budget: usize,
    pub required_resident_assets: usize,
    pub required_resident_bytes: usize,
    /// Eager attack preparation explicitly fell back to cold starts after this
    /// payload-budget failure. Mandatory full sources/wavetable ranges stay resident.
    pub head_fallback: Option<HeadBudgetExceeded>,
    /// Bytes every asset would take fully decoded, as `load` holds them.
    pub full_bytes: u64,
    pub pool_pages: usize,
    pub pool_bytes: usize,
}

/// Control-side snapshot of exact authored start-range coverage. A page that
/// is only in the evictable stream pool is not a permanently resident head.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeadReadiness {
    pub assets: usize,
    pub ranges: usize,
    pub source_frames: u64,
    pub covered_assets: usize,
    pub covered_ranges: usize,
    pub covered_source_frames: u64,
    pub packed_bytes: usize,
    pub budget_bytes: usize,
}
impl HeadReadiness {
    pub fn complete(&self) -> bool {
        self.ranges == self.covered_ranges
    }
    /// The narrowest supported lossless representation is mono 16-bit. This
    /// lower bound can prove a budget insufficient without decoding payloads.
    pub fn minimum_packed_bytes(&self) -> u64 {
        self.source_frames.saturating_mul(2)
    }
    pub fn decoded_bytes(&self) -> u64 {
        self.source_frames.saturating_mul(size_of::<Frame>() as u64)
    }
}

/// How much to keep resident.
#[derive(Clone, Copy, Debug)]
pub struct StreamPolicy {
    /// Pitch a voice may rise above its zone's highest key, in semitones
    /// (bend and modulation headroom), for head sizing.
    pub headroom: f64,
    /// Worst wait between a page request and its decode starting (worker poll
    /// and the host's service period), on top of measured read latency.
    pub slack: Duration,
    /// The host's largest render block, in output frames. A voice that starts
    /// inside a block is first serviced before the next one, so its head must
    /// also cover that whole block before its next page can even be requested.
    pub block_frames: usize,
    /// Voices that can stream at once; each needs a few pool pages.
    pub voices: usize,
    /// Decode/preload threads, clamped to 1..=8.
    pub decoders: usize,
    /// Shared cap on required full PCM and packed staged/current/retired starts. Decoder
    /// workspaces and the stream page pool have independent bounds.
    pub head_budget: usize,
    /// Publish before heads are read. Runtime must enable cold-start holding.
    pub lazy: bool,
    /// Opt in to cold starts when optional attack preparation exceeds the shared
    /// payload budget. Resident sources that do not fit the budget (smallest
    /// first kept whole) stream from disk. Source, metadata and cancellation
    /// failures propagate.
    pub allow_cold_fallback: bool,
    /// RAM-only: keep whole samples smallest first within this safe byte budget.
    pub resident_budget: Option<usize>,
    /// Ceiling on the head budget that cold fallback may raise to the law heads.
    /// A head above it takes the fallback tiers instead of growing the budget.
    pub physical_limit: usize,
}

impl Default for StreamPolicy {
    fn default() -> Self {
        Self {
            headroom: 2.0,
            slack: Duration::from_millis(10),
            block_frames: 64,
            voices: 256,
            decoders: 4,
            // Unbounded heads reached 20 GB on ANALOG STRINGS; over budget, starts
            // fall back to the bounded initial-state windows instead of failing.
            head_budget: cold_head_budget(),
            lazy: false,
            allow_cold_fallback: true,
            resident_budget: None,
            physical_limit: usize::MAX,
        }
    }
}

/// The cold-stream head budget for this machine: 1/32 of physical RAM, clamped
/// to [256 MiB, 4 GiB]. The single place the streamed head and residency cap
/// is chosen; hosts use it as their Auto budget and `StreamPolicy::default`
/// uses it too.
pub fn cold_head_budget() -> usize {
    cold_head_budget_for(system_ram_total())
}

/// [`cold_head_budget`] for a known physical RAM size (`None`: unknown, floor).
pub fn cold_head_budget_for(total_ram: Option<usize>) -> usize {
    const FLOOR: usize = 256 << 20;
    const CEILING: usize = 4 << 30;
    total_ram.map_or(FLOOR, |total| (total / 32).clamp(FLOOR, CEILING))
}

/// `MemTotal` from `/proc/meminfo`, in bytes.
fn system_ram_total() -> Option<usize> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = info.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kib: usize = line["MemTotal:".len()..].trim().trim_end_matches("kB").trim().parse().ok()?;
    Some(kib << 10)
}

/// Pool pages per streaming voice, at least: the page it reads, the next, and
/// one more for a window or crossfade spanning a page boundary. The real figure
/// is the largest zone reach of the voice law, see [`voice_pages`].
const PAGES_PER_VOICE: usize = 3;

/// The host's largest render block (`src/sound/mod.rs` `MAX_BLOCK`): the stream
/// horizon covers one block beyond the largest head.
const HOST_MAX_BLOCK: usize = 128;

/// Output frames past every permitted start that the window law keeps resident,
/// over the streamed resident zones. Per zone: the head less its `start_range`
/// (a start can sit that far into the window), capped at the sample's remaining
/// frames, then less the resampler guard, divided by the zone's fastest step
/// (`zone_step`, the same one `start_ranges_geometry` reads at). The runway and
/// the stream horizon count output frames, so this is the unit that matches
/// them. A zone whose head covers its whole sample is fully resident and needs
/// none. Zero when no zone streams.
fn runway_frames(
    instrument: &ir::Instrument,
    geometry: &[(u32, usize)],
    rate: u32,
    heads: &[usize],
    policy: &StreamPolicy,
) -> usize {
    instrument
        .zones
        .iter()
        .zip(heads)
        .filter_map(|(zone, &head)| {
            let asset = zone.asset?.0;
            let (sample_rate, length) = geometry[asset];
            if !zone_resident(instrument, zone) || head >= length {
                return None;
            }
            let start = usize::try_from(zone.playback.start).unwrap_or(usize::MAX).min(length);
            let remaining = if zone.playback.reverse { start } else { length - start };
            let guaranteed = head
                .saturating_sub(usize::try_from(zone.playback.start_range).unwrap_or(usize::MAX))
                .min(remaining);
            let ratio = f64::from(sample_rate) / f64::from(rate);
            let step = zone_step(zone, &instrument.groups, ratio, policy);
            let guard = sampler_core::read_radius(step) + 1;
            Some((guaranteed.saturating_sub(guard) as f64 / step).floor() as usize)
        })
        .min()
        .unwrap_or(0)
}

/// Pool pages one streaming voice can reserve at the host's stream horizon,
/// which is the largest head plus one block (`src/sound/v2.rs`
/// `sampler_stream_contract`). Each zone contributes its voice reach at its
/// fastest step, with the legs its looping adds, capped at its own pages; the
/// largest zone sets the figure. Sized from the law so admission cannot run out
/// of pool before voices do.
fn voice_pages(
    instrument: &ir::Instrument,
    geometry: &[(u32, usize)],
    rate: u32,
    runway: usize,
    policy: &StreamPolicy,
) -> usize {
    let horizon = u32::try_from(runway.max(PAGE_FRAMES) + policy.block_frames.max(HOST_MAX_BLOCK))
        .unwrap_or(u32::MAX);
    instrument
        .zones
        .iter()
        .filter_map(|zone| {
            let asset = zone.asset?.0;
            let (sample_rate, length) = geometry[asset];
            let full = length.div_ceil(PAGE_FRAMES) + 1;
            if zone.group.is_some_and(|g| instrument.groups[g.0].wavetable.is_some()) {
                return Some(full);
            }
            let legs = match zone.playback.looping {
                ir::Looping::None | ir::Looping::OneShot => 1,
                ir::Looping::Continuous(_) | ir::Looping::UntilRelease(_) => 2,
                ir::Looping::Slots(slots) => 1 + 2 * slots.iter().flatten().count(),
            };
            // The runtime cursor's step is this zone's source/engine ratio, pitched.
            let ratio = f64::from(sample_rate) / f64::from(rate);
            let step = zone_step(zone, &instrument.groups, ratio, policy);
            Some(reservation_reach_pages(horizon, step, legs).min(full))
        })
        .fold(PAGES_PER_VOICE, usize::max)
}

/// A source's channel count and sample width, from its header. `bytes_per_frame`
/// is `F` in the window law: the packed bytes of one frame across all channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameLayout {
    pub channels: usize,
    pub bits: usize,
}

impl FrameLayout {
    /// Packed source bytes per frame across all channels.
    pub fn bytes_per_frame(self) -> usize {
        self.bits.div_ceil(8) * self.channels
    }
}

/// Header layout of a source whose header cannot be read: 16-bit stereo, the
/// width every unparsed source used before the window law.
// ponytail: Encoded, Custom and unparsed headers assume 16-bit stereo; read the real header when a format needs it.
const FALLBACK_LAYOUT: FrameLayout = FrameLayout { channels: 2, bits: 16 };

/// The instrument's DFD preload, bytes per channel: the program's
/// `dfdChannelPreloadSize`, or Kontakt's init default for a source with no Kontakt program.
/// Library loads never set the global override, so the authored value applies.
fn authored_preload(instrument: &ir::Instrument) -> i32 {
    let authored = instrument
        .kontakt_objects
        .as_ref()
        .map_or(dfd_window::PRELOAD_DEFAULT, |objects| objects.program.dfd_channel_preload_size);
    dfd_window::effective_preload(authored, None)
}

/// Kontakt's DFD window for one zone (`dfd_window::window`): its start, reverse
/// flag and S.Mod range (`playback.start_range`, source frames; 0 when unset),
/// over its asset's header layout.
fn zone_window(zone: &ir::Zone, preload: i32, layout: FrameLayout) -> dfd_window::Window {
    let start = i64::try_from(zone.playback.start).unwrap_or(i64::MAX);
    dfd_window::window(dfd_window::Input {
        bits: u32::try_from(layout.bits).unwrap_or(u32::MAX),
        channels: i64::try_from(layout.channels).unwrap_or(i64::MAX),
        preload,
        streamed: true,
        reverse: zone.playback.reverse,
        position: start,
        // A reversed window starts at `-reverse_position`, so both fields name the zone start.
        reverse_position: -start,
        offset: i64::try_from(zone.playback.start_range).unwrap_or(i64::MAX),
        linked: None,
        blocked: false,
    })
}

/// Whether a zone's group is resident at load. Kontakt saves purged groups
/// (`ir::Group::purged`) non-resident; they get no head until a purge/load request.
fn zone_resident(instrument: &ir::Instrument, zone: &ir::Zone) -> bool {
    zone.group.is_none_or(|g| !instrument.groups[g.0].purged)
}

/// One preload head per resident zone, in source frames: its DFD window in packed
/// source bytes divided by the frame size, capped at the sample. Zones in purged
/// groups, and zones without an asset, get 0. `frames` and `layouts` are indexed
/// by logical asset.
pub(crate) fn preload_heads(
    instrument: &ir::Instrument,
    frames: &[usize],
    layouts: &[FrameLayout],
) -> Vec<usize> {
    heads_where(instrument, frames, layouts, |zone| zone_resident(instrument, zone))
}

/// Every zone's head, purged groups included: the head a purge reload loads
/// for its group (the same law as `preload_heads`).
pub(crate) fn reload_heads(
    instrument: &ir::Instrument,
    frames: &[usize],
    layouts: &[FrameLayout],
) -> Vec<usize> {
    heads_where(instrument, frames, layouts, |_| true)
}

fn heads_where(
    instrument: &ir::Instrument,
    frames: &[usize],
    layouts: &[FrameLayout],
    include: impl Fn(&ir::Zone) -> bool,
) -> Vec<usize> {
    let preload = authored_preload(instrument);
    instrument
        .zones
        .iter()
        .map(|zone| match zone.asset.map(|a| a.0) {
            Some(asset) if include(zone) => {
                let layout = layouts[asset];
                let bytes = zone_window(zone, preload, layout).bytes.unsigned_abs();
                let head = usize::try_from(bytes / layout.bytes_per_frame().max(1) as u64).unwrap_or(usize::MAX);
                head.min(frames[asset])
            }
            _ => 0,
        })
        .collect()
}

/// Kontakt's window bytes for the resident zones: per sample, the union of its
/// zones' window ranges (start `max(pos - off, 0)`, length `|W|`), in packed bytes.
pub(crate) fn kontakt_window_bytes(instrument: &ir::Instrument, layouts: &[FrameLayout]) -> usize {
    let preload = authored_preload(instrument);
    let mut per_asset: Vec<Vec<Range<usize>>> = vec![Vec::new(); layouts.len()];
    for zone in instrument.zones.iter().filter(|zone| zone_resident(instrument, zone)) {
        let Some(asset) = zone.asset.map(|a| a.0) else {
            continue;
        };
        let layout = layouts[asset];
        let window = zone_window(zone, preload, layout);
        let frames = window.bytes.unsigned_abs() / layout.bytes_per_frame().max(1) as u64;
        if frames == 0 {
            continue;
        }
        let start = usize::try_from(window.start.max(0)).unwrap_or(usize::MAX);
        per_asset[asset].push(start..start.saturating_add(frames as usize));
    }
    per_asset
        .iter_mut()
        .enumerate()
        .map(|(asset, list)| {
            merge_start_ranges(list);
            let frames: usize = list.iter().map(Range::len).sum();
            frames.saturating_mul(layouts[asset].bytes_per_frame())
        })
        .fold(0usize, usize::saturating_add)
}

/// Test-only law with a chosen base: each zone's `base` frames plus its own start range.
#[cfg(test)]
pub(crate) fn law_heads(instrument: &ir::Instrument, base: usize) -> Vec<usize> {
    instrument
        .zones
        .iter()
        .map(|zone| base.saturating_add(usize::try_from(StartRange::of(instrument, zone).max_frames).unwrap_or(usize::MAX)))
        .collect()
}

/// Output frames whose reads at `step` cover `head` source frames.
fn output_horizon(head: usize, step: f64) -> usize {
    (head as f64 / step).ceil() as usize
}

/// Fastest a zone reads its asset, in source frames per output frame: its
/// highest key plus `policy.headroom`, its and its group's tuning, and the
/// asset's rate against the engine's.
fn zone_step(zone: &ir::Zone, groups: &[ir::Group], ratio: f64, policy: &StreamPolicy) -> f64 {
    let keys = match zone.pitch {
        ir::KeyTracking::Tracked { root } => f64::from(zone.keys.high) - f64::from(root),
        ir::KeyTracking::Scaled {
            root,
            cents_per_key,
        } => {
            let span = f64::from(zone.keys.high) - f64::from(root);
            let low = f64::from(zone.keys.low) - f64::from(root);
            (span * f64::from(cents_per_key)).max(low * f64::from(cents_per_key)) / 100.0
        }
        ir::KeyTracking::Fixed => 0.0,
    };
    let group = zone.group.map_or(0.0, |g| groups[g.0].tune.semitones());
    let semitones = keys + zone.tune.semitones() + group + policy.headroom;
    ratio * (semitones / 12.0).exp2()
}

/// Frame ranges of each asset that zone starts read before streaming catches
/// up: `heads[zone]` source frames from the start (forward, or back from the
/// end when reversed), plus the interpolation guard. Ascending and merged.
pub(crate) fn start_ranges(
    instrument: &ir::Instrument,
    assets: &[Pcm],
    rate: u32,
    heads: &[usize],
    policy: &StreamPolicy,
) -> Vec<Vec<Range<usize>>> {
    assets_start_ranges(instrument, assets, rate, heads, policy, |zone| zone_resident(instrument, zone))
}

/// The table a purge reload restores from: the start ranges of every zone,
/// purged groups included, each with its own law head (`reload_heads`).
pub(crate) fn reload_start_ranges(
    instrument: &ir::Instrument,
    assets: &[Pcm],
    rate: u32,
    reload_heads: &[usize],
    policy: &StreamPolicy,
) -> Vec<Vec<Range<usize>>> {
    assets_start_ranges(instrument, assets, rate, reload_heads, policy, |_| true)
}

fn assets_start_ranges(
    instrument: &ir::Instrument,
    assets: &[Pcm],
    rate: u32,
    heads: &[usize],
    policy: &StreamPolicy,
    include: impl Fn(&ir::Zone) -> bool,
) -> Vec<Vec<Range<usize>>> {
    let geometry = assets.iter().map(|p| (p.sample_rate(), p.frame_count())).collect::<Vec<_>>();
    let mut ranges = start_ranges_geometry(instrument, &geometry, rate, heads, policy, include);
    if let Some(room) = policy.resident_budget {
        keep_whole(assets, &mut ranges, room);
    }
    ranges
}

fn start_ranges_geometry(
    instrument: &ir::Instrument,
    geometry: &[(u32, usize)],
    rate: u32,
    heads: &[usize],
    policy: &StreamPolicy,
    include: impl Fn(&ir::Zone) -> bool,
) -> Vec<Vec<Range<usize>>> {
    let mut ranges = vec![Vec::new(); geometry.len()];
    for (zone, &head) in instrument.zones.iter().zip(heads) {
        let Some(asset) = zone.asset.map(|a| a.0) else {
            continue;
        };
        if !include(zone) {
            continue;
        }
        let playback = &zone.playback;
        let (sample_rate, frame_count) = geometry[asset];
        if zone
            .group
            .is_some_and(|g| instrument.groups[g.0].wavetable.is_some())
        {
            // port v1 bank.rs::spans: any cycle can be read at every oscillator sample.
            let end = playback
                .end
                .unwrap_or(frame_count as u64)
                .min(frame_count as u64);
            if playback.start < end {
                ranges[asset].push(playback.start as usize..end as usize);
            }
            continue;
        }
        let ratio = f64::from(sample_rate) / f64::from(rate);
        let step = zone_step(zone, &instrument.groups, ratio, policy);
        // The resampling window reads this far either side of the position.
        let guard = sampler_core::read_radius(step) as u64 + 1;
        // `head` already covers the whole sample-start modulation range (`StartRange`).
        let frames = (head as u64).saturating_add(2 * guard);
        let length = frame_count as u64;
        let end = playback.end.unwrap_or(length).min(length);
        let (from, to) = if playback.reverse {
            (end.saturating_sub(frames), end)
        } else {
            let from = playback.start.min(length).saturating_sub(guard);
            (from, from.saturating_add(frames).min(length))
        };
        if from < to {
            ranges[asset].push(from as usize..to as usize);
        }
    }
    for list in &mut ranges {
        list.sort_unstable_by_key(|r| r.start);
        let mut merged: Vec<Range<usize>> = Vec::with_capacity(list.len());
        for range in list.drain(..) {
            match merged.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => merged.push(range),
            }
        }
        *list = merged;
    }
    ranges
}

struct HintFallback {
    head_frames: usize,
    ranges: Vec<Vec<Range<usize>>>,
}

/// Physical bytes the given per-asset ranges occupy: resident sources whole,
/// streamed sources by the union of their ranges. `None` on overflow.
pub(crate) fn physical_head_bytes(assets: &[Pcm], ranges: &[Vec<Range<usize>>]) -> Option<usize> {
    let union = physical_start_ranges(assets, ranges);
    let physical: HashMap<_, _> = assets.iter().enumerate().map(|(i, p)| (p.asset_id(), i)).collect();
    physical.values().try_fold(0usize, |sum, &i| {
        let pcm = &assets[i];
        let frames = if pcm.requires_resident() && pcm.resident_frames().is_some() { Some(0) }
            else if pcm.requires_resident() { Some(pcm.frame_count()) }
            else { union[&pcm.asset_id()].iter().try_fold(0usize, |sum, r| sum.checked_add(r.len())) };
        sum.checked_add(frames?.checked_mul(size_of::<Frame>())?)
    })
}

/// Packed source bytes the ranges hold, each physical sample once: resident samples
/// whole, streamed ones by their merged ranges, at the header's frame size.
pub(crate) fn packed_head_bytes(assets: &[Pcm], ranges: &[Vec<Range<usize>>], layouts: &[FrameLayout]) -> Option<usize> {
    let union = physical_start_ranges(assets, ranges);
    let mut seen = HashSet::new();
    assets.iter().enumerate().filter(|(_, pcm)| seen.insert(pcm.asset_id())).try_fold(0usize, |sum, (i, pcm)| {
        let frames = if pcm.requires_resident() {
            pcm.frame_count()
        } else {
            union[&pcm.asset_id()].iter().try_fold(0usize, |sum, r| sum.checked_add(r.len()))?
        };
        sum.checked_add(frames.checked_mul(layouts[i].bytes_per_frame())?)
    })
}

/// Opt-in control-side evidence only. The counterfactual removes static start
/// modulation coverage to attribute demand; it never changes published ranges.
/// A control-side, non-looping effective onset window. The offset is already
/// evaluated by the prepared voice consumer; this only clips physical reads.
/// Offset-past-view voices are silent, not reads beyond the authored end.
fn constant_start_window(
    playback: &ir::Playback,
    length: usize,
    offset: u32,
    step: f64,
    head: usize,
) -> Option<Range<usize>> {
    let end = playback.end.unwrap_or(length as u64).min(length as u64);
    let start = playback.start.min(end);
    let offset = u64::from(offset);
    if offset >= end - start {
        return None;
    }
    let guard = sampler_core::read_radius(step) as u64 + 1;
    // `head` is source frames (the preload law), so the window travels that far.
    let travel = head as u64;
    let (from, to) = if playback.reverse {
        let edge = end - offset;
        (edge.saturating_sub(travel).saturating_sub(guard).max(start), edge.saturating_add(guard).min(end))
    } else {
        let edge = start + offset;
        (edge.saturating_sub(guard).max(start), edge.saturating_add(travel).saturating_add(guard).min(end))
    };
    (from < to).then_some(from as usize..to as usize)
}

fn merge_start_ranges(ranges: &mut Vec<Range<usize>>) {
    ranges.sort_unstable_by_key(|r| r.start);
    let mut len = 0;
    for i in 0..ranges.len() {
        if len != 0 && ranges[i].start <= ranges[len - 1].end {
            ranges[len - 1].end = ranges[len - 1].end.max(ranges[i].end);
        } else {
            ranges[len] = ranges[i].clone();
            len += 1;
        }
    }
    ranges.truncate(len);
}

fn physical_start_ranges(assets: &[Pcm], ranges: &[Vec<Range<usize>>]) -> HashMap<AssetId, Vec<Range<usize>>> {
    let mut physical = HashMap::<AssetId, Vec<Range<usize>>>::new();
    for (pcm, ranges) in assets.iter().zip(ranges) {
        physical.entry(pcm.asset_id()).or_default().extend(ranges.iter().cloned());
    }
    for ranges in physical.values_mut() { merge_start_ranges(ranges); }
    physical
}

/// Exact current-plan proof only. Loop prefixes require the real cursor demand
/// law to prove no transition; unknown starts retain their base coverage.
struct ConstantStartHints {
    offsets: Box<[Option<u32>]>,
    /// Per-zone heads (source frames) that eligibility was proved at.
    requested: Box<[usize]>,
    prefixes: HashMap<usize, sampler_core::ConstantStartPrefix>,
    loop_prefix: usize,
    constant: usize,
    zero: usize,
    dependent: usize,
    loops: usize,
    wavetable: usize,
    silent: usize,
    eligible: usize,
}
impl ConstantStartHints {
    fn new(instrument: &ir::Instrument, prepared: &sampler_core::Prepared, assets: &[Pcm],
        rate: u32, heads: &[usize], policy: &StreamPolicy,
        canceled: &(dyn Fn() -> bool + Sync)) -> io::Result<Self> {
        let mut this = Self { offsets: vec![None; instrument.zones.len()].into_boxed_slice(),
            requested: heads.into(), prefixes: HashMap::new(), loop_prefix: 0,
            constant: 0, zero: 0, dependent: 0, loops: 0, wavetable: 0, silent: 0, eligible: 0 };
        for (region, zone) in instrument.zones.iter().enumerate() {
            if canceled() { return Err(canceled_head()); }
            // A purged group's zone gets no start hint at load; its purge reload restores it.
            if !zone_resident(instrument, zone) { continue; }
            if !zone.routes.iter().any(|r| instrument.routes.get(r.0).is_some_and(|r| r.target == ir::Target::SampleStart)) { continue; }
            let Some(offset) = prepared.constant_start_offset(region) else { this.dependent += 1; continue; };
            this.constant += 1;
            if offset == 0 { this.zero += 1; continue; }
            let Some(asset) = zone.asset.map(|a| a.0) else { continue; };
            if zone.group.is_some_and(|g| instrument.groups[g.0].wavetable.is_some()) { this.wavetable += 1; continue; }
            let end = zone.playback.end.unwrap_or(assets[asset].frame_count() as u64).min(assets[asset].frame_count() as u64);
            if u64::from(offset) >= end.saturating_sub(zone.playback.start.min(end)) { this.silent += 1; continue; }
            if !matches!(zone.playback.looping, ir::Looping::None | ir::Looping::OneShot) {
                let Some(prefix) = prepared.constant_start_forward_prefix(region) else { this.loops += 1; continue; };
                if prefix.asset_id() != assets[asset].asset_id() {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "constant onset source identity mismatch"));
                }
                let step = zone_step(zone, &instrument.groups,
                    f64::from(assets[asset].sample_rate()) / f64::from(rate), policy);
                // Eligibility is fixed at the requested horizon. Shorter quota
                // candidates cannot introduce a window outside that authority.
                if prefix.window(output_horizon(heads[region], step), step).is_none() { this.loops += 1; continue; }
                this.prefixes.insert(region, prefix);
                this.loop_prefix += 1;
            }
            this.eligible += 1;
            this.offsets[region] = Some(offset);
        }
        Ok(this)
    }
    fn ranges(&self, instrument: &ir::Instrument, assets: &[Pcm], rate: u32, heads: &[usize],
        policy: &StreamPolicy, canceled: &(dyn Fn() -> bool + Sync)) -> io::Result<Vec<Vec<Range<usize>>>> {
        if heads.iter().zip(self.requested.iter()).any(|(head, requested)| head > requested) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "constant onset horizon exceeds requested authority"));
        }
        let mut hints = vec![Vec::new(); assets.len()];
        for (region, (zone, offset)) in instrument.zones.iter().zip(&self.offsets).enumerate() {
            if canceled() { return Err(canceled_head()); }
            let Some(offset) = offset else { continue; };
            let asset = zone.asset.expect("a proved hint owns a source").0;
            let pcm = &assets[asset];
            let step = zone_step(zone, &instrument.groups, f64::from(pcm.sample_rate()) / f64::from(rate), policy);
            let head = heads[region];
            let range = match self.prefixes.get(&region) {
                Some(prefix) => prefix.window(output_horizon(head, step), step),
                None => constant_start_window(&zone.playback, pcm.frame_count(), *offset, step, head),
            };
            if let Some(range) = range {
                hints[asset].push(range);
            }
        }
        for ranges in &mut hints { merge_start_ranges(ranges); }
        Ok(hints)
    }
}

/// Numeric experiment only: does not alter requested heads or publication.
/// Only loop prefixes proved by the prepared cursor are included. Wavetable,
/// controlled, crossing and complex loop geometry remain explicitly omitted.
fn constant_start_geometry(
    instrument: &ir::Instrument,
    prepared: &sampler_core::Prepared,
    assets: &[Pcm],
    base: &[Vec<Range<usize>>],
    pinned: &HashSet<AssetId>,
    rate: u32,
    heads: &[usize],
    policy: &StreamPolicy,
    canceled: &(dyn Fn() -> bool + Sync),
) -> io::Result<serde_json::Value> {
    let recipe = ConstantStartHints::new(instrument, prepared, assets, rate, heads, policy, canceled)?;
    let hints = recipe.ranges(instrument, assets, rate, heads, policy, canceled)?;
    let hints = physical_start_ranges(assets, &hints);
    let mut combined = physical_start_ranges(assets, base);
    let frame_count = |table: &HashMap<AssetId, Vec<Range<usize>>>| {
        table.values().flatten().fold(0u64, |n,r| n.saturating_add(r.len() as u64))
    };
    let base_frames = frame_count(&combined);
    let hint_frames = frame_count(&hints);
    for (id, ranges) in &hints { combined.entry(*id).or_default().extend(ranges.iter().cloned()); }
    for ranges in combined.values_mut() { merge_start_ranges(ranges); }
    let combined_frames = frame_count(&combined);
    let mut mandatory = 0u64;
    let mut optional = 0u64;
    let mut seen = HashSet::new();
    for pcm in assets {
        if !seen.insert(pcm.asset_id()) { continue; }
        if pcm.requires_resident() {
            mandatory = mandatory.saturating_add(pcm.frame_count() as u64 * size_of::<Frame>() as u64);
        } else {
            optional = optional.saturating_add(combined.get(&pcm.asset_id()).into_iter().flatten().map(|r| r.len() as u64).sum::<u64>());
        }
    }
    if canceled() { return Err(canceled_head()); }
    Ok(serde_json::json!({
        "constant_offset_zones":recipe.constant,"zero_constant_offset_zones":recipe.zero,"dependent_offset_zones":recipe.dependent,
        "nonloop_window_zones":recipe.eligible - recipe.loop_prefix,"loop_prefix_window_zones":recipe.loop_prefix,"loop_geometry_omitted_zones":recipe.loops,"wavetable_geometry_omitted_zones":recipe.wavetable,
        "silent_past_view_zones":recipe.silent,"hint_assets":hints.values().filter(|r| !r.is_empty()).count(),
        "hint_ranges":hints.values().map(Vec::len).sum::<usize>(),"hint_source_frames":hint_frames,
        "base_source_frames_physical_union":base_frames,"base_plus_hint_source_frames":combined_frames,
        "hint_extra_source_frames":combined_frames.saturating_sub(base_frames),
        "mandatory_full_decoded_bytes":mandatory,"base_plus_hint_optional_source_frames":optional,
        "base_plus_hint_payload_lower_bytes":mandatory.saturating_add(optional.saturating_mul(2)),
        "base_plus_hint_payload_upper_bytes":mandatory.saturating_add(optional.saturating_mul(size_of::<Frame>() as u64)),
        "pinned_assets":pinned.len(),"hinted_preload_enabled":policy.allow_cold_fallback,
        "scope":"current prepared onset law; non-loop windows and proved pre-transition loop prefixes; physical AssetId union; payload bounds exclude shared already-held quota and metadata; actual selected coverage is reported separately; no admission or native-parity claim"
    }))
}

/// `KONTRA_AUDIT_LOAD` evidence for the preload law: total physical head bytes,
/// the excess over the head budget (kept, never truncated), and the zones whose
/// law heads contribute most. Observation only; it changes nothing.
fn audit_start_ranges(
    instrument: &ir::Instrument,
    prepared: &sampler_core::Prepared,
    assets: &[Pcm],
    ranges: &[Vec<Range<usize>>],
    pinned: &HashSet<AssetId>,
    rate: u32,
    heads: &[usize],
    layouts: &[FrameLayout],
    scripts: &[sampler_ksp::ScriptView],
    policy: &StreamPolicy,
    canceled: &(dyn Fn() -> bool + Sync),
) -> io::Result<()> {
    if std::env::var_os("KONTRA_AUDIT_LOAD").is_none() { return Ok(()); }
    let constant_hints = constant_start_geometry(instrument, prepared, assets, ranges, pinned, rate, heads, policy, canceled)?;
    // `head_bytes` is packed source bytes, the same unit as `kontakt_window_bytes`; the budget charges decoded f32.
    let head_bytes = packed_head_bytes(assets, ranges, layouts).unwrap_or(usize::MAX);
    let decoded_head_bytes = physical_head_bytes(assets, ranges).unwrap_or(usize::MAX);
    let excess_bytes = decoded_head_bytes.saturating_sub(policy.head_budget);
    let windows = kontakt_window_bytes(instrument, layouts);
    let mut zones: Vec<_> = instrument.zones.iter().enumerate().filter_map(|(zone, z)| {
        let asset = z.asset?.0;
        let bytes = heads[zone].saturating_mul(layouts[asset].bytes_per_frame());
        Some((bytes, zone, asset, heads[zone], z.playback.start_range))
    }).collect();
    zones.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let top: Vec<_> = zones.iter().take(16).map(|&(bytes, zone, asset, head, start_range)|
        serde_json::json!({"zone":zone,"asset_index":asset,"head_source_frames":head,"head_bytes":bytes,
            "sample_start_range_frames":start_range,"source_frames":assets[asset].frame_count()})).collect();
    let max_head = heads.iter().copied().max().unwrap_or(0);
    let mut reverse_zones = 0usize;
    let mut bounded_end_zones = 0usize;
    let mut sample_start_zones = 0usize;
    let mut max_offset = 0u64;
    let mut ignored_script_offset_zones = 0usize;
    let script_max_micros = scripts.iter().map(|s| s.start_offsets.max_micros).max().unwrap_or(0);
    for zone in &instrument.zones {
        reverse_zones += usize::from(zone.playback.reverse);
        bounded_end_zones += usize::from(zone.playback.end.is_some());
        if zone.routes.iter().any(|r| instrument.routes.get(r.0).is_some_and(|r| r.target == ir::Target::SampleStart)) {
            sample_start_zones += 1;
            max_offset = max_offset.max(zone.playback.start_range);
        }
        if let Some(asset) = zone.asset {
            // A script offset past the S.Mod range is ignored at the voice start (KSP play_note, DFD mode).
            let script_frames = u64::from(script_max_micros) * u64::from(assets[asset.0].sample_rate()) / 1_000_000;
            ignored_script_offset_zones += usize::from(script_frames > zone.playback.start_range);
        }
    }
    eprintln!("stream_preload_law {}", serde_json::json!({
        "schema":"kontra-preload-law-v2","sample_rate":rate,"block_frames":policy.block_frames,
        "law":"kontakt dfd_window: preload per channel + 2 x S.Mod, 6 KiB granularity","zones":instrument.zones.len(),"max_head_source_frames":max_head,
        "reverse_zones":reverse_zones,"bounded_end_zones":bounded_end_zones,
        "sample_start_route_zones":sample_start_zones,"max_sample_start_offset_frames":max_offset,
        "script_offset_ignored_zones":ignored_script_offset_zones,
        "head_bytes":head_bytes,"decoded_head_bytes":decoded_head_bytes,"kontakt_window_bytes":windows,
        "head_budget_bytes":policy.head_budget,
        "over_budget_excess_bytes":excess_bytes,"top_zones":top,
        "constant_onset_hints":constant_hints,
        "scope":"law heads kept whole even over budget; excess reported; constant hints are attribution only; no payload or source names"
    }));
    Ok(())
}

// Port from v1 0cb7a8a0:src/engine/bank.rs (Builder::keep_whole).
/// Load samples whole instead of streaming them, smallest first, while
/// they fit `room` bytes. Returns the bytes the rest would need.
fn keep_whole(assets: &[Pcm], ranges: &mut [Vec<Range<usize>>], room: usize) -> usize {
    let size = |i: usize| assets[i].frame_count().saturating_mul(8);
    let resident = |list: &[Range<usize>]| list.iter().map(|r| r.end - r.start).sum::<usize>() * 8;
    let mut streamed: Vec<_> = (0..ranges.len())
        .filter(|&i| !ranges[i].is_empty() && resident(&ranges[i]) < size(i))
        .collect();
    streamed.sort_by_key(|&i| size(i));
    let bytes: usize = ranges.iter().map(|r| resident(r)).sum();
    let mut room = room.saturating_sub(bytes);
    let mut needed = 0;
    for i in streamed {
        let more = size(i).saturating_sub(resident(&ranges[i]));
        if more <= room {
            room -= more;
            ranges[i] = std::iter::once(0..assets[i].frame_count()).collect();
        } else {
            needed += more - room.min(more);
            room = 0;
        }
    }
    needed
}

/// Streamed assets before their start ranges are read.
pub(crate) struct Opened {
    pub logical_sources: Option<Vec<sampler_core::SourceStorage>>,
    pub assets: Vec<Pcm>,
    pub sources: HashMap<AssetId, Arc<dyn AssetSource>>,
    pub report: StreamReport,
    /// Bytes per source frame of each logical asset (the preload law's input).
    pub layouts: Vec<FrameLayout>,
    pub rate: u32,
    pub policy: StreamPolicy,
}

/// Read `ranges` of `pcm` from `reader` and make them resident; their bytes.
fn load_ranges(pcm: &Pcm, reader: &mut SampleReader, ranges: &[Range<usize>], page:&mut[Frame]) -> io::Result<usize> {
    let prepared = match prepare_head_ranges(&pcm.head_preparer(), reader, ranges, page, &|| !pcm.residency_permitted(), None, None) {
        Ok(prepared) => prepared,
        // A concurrent KSP purge can remove the final residency owner while
        // this worker reads. That transaction is obsolete, not a source fault.
        Err(_) if !pcm.residency_permitted() => return Ok(0),
        Err(e)
            if e.get_ref()
                .and_then(|e| e.downcast_ref::<sampler_core::Error>())
                == Some(&sampler_core::Error::NotReady) =>
        {
            return Ok(0);
        }
        Err(e) => return Err(e),
    };
    if !pcm.residency_permitted() {
        return Ok(0);
    }
    let bytes = prepared.bytes();
    match pcm.publish_ranges(prepared) {
        Ok(old) => {drop(old);pcm.head_bytes();Ok(bytes)},
        Err(sampler_core::Error::NotReady) => Ok(0),
        Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
    }
}

/// Keep every item when all of them fit `budget`. Otherwise keep the smallest
/// ones that fit half of it: the rest is headroom for staging and publishing
/// the admitted heads. Items not kept stream from disk.
fn admit_within_budget<T>(mut items: Vec<(usize, T)>, budget: usize) -> Vec<T> {
    let total = items.iter().fold(0usize, |sum, (bytes, _)| sum.saturating_add(*bytes));
    if total <= budget {
        return items.into_iter().map(|(_, item)| item).collect();
    }
    items.sort_by_key(|(bytes, _)| *bytes);
    let room = budget / 2;
    let mut held = 0usize;
    let mut kept = Vec::new();
    for (bytes, item) in items {
        if held.saturating_add(bytes) <= room {
            held += bytes;
            kept.push(item);
        }
    }
    kept
}

/// Smallest stored form of a source's start: 2 bytes a frame (`Packed` never stores less).
fn head_floor_bytes(pcm: &Pcm, ranges: &[Range<usize>]) -> io::Result<usize> {
    if pcm.requires_resident() {
        return head_cost(pcm, ranges);
    }
    Ok(range_bounds(pcm, ranges)?.0)
}

/// The head-budget cost of a source's start, in the one unit the budget and every
/// admission check share: decoded `Frame` bytes. A required-resident source is all of its
/// frames; anything else is the full extent of its start ranges. A packed head is stored at
/// most this wide (`Packed` never exceeds an f32 frame), so the cost never understates a
/// reservation, and the reservation itself stays the exact check.
fn head_cost(pcm: &Pcm, ranges: &[Range<usize>]) -> io::Result<usize> {
    if pcm.requires_resident() {
        pcm.frame_count().checked_mul(size_of::<Frame>()).ok_or_else(|| io::Error::other("resident source size overflow"))
    } else {
        Ok(range_bounds(pcm, ranges)?.1)
    }
}

/// The head budget after cold fallback raises it to the law heads, clamped to
/// `physical_limit`. Strict loads keep the configured budget.
fn raised_head_budget(policy: &StreamPolicy, head_bytes: usize) -> usize {
    if policy.allow_cold_fallback {
        policy.head_budget.max(head_bytes).min(policy.physical_limit)
    } else {
        policy.head_budget
    }
}

/// Under cold fallback, resident sources are admitted whole while they fit the
/// shared head budget (see `admit_within_budget`); the rest stream from disk
/// instead of failing the load. Unknown sizes stay resident.
fn admit_resident_within_budget(
    requirements: &[ir::AssetResidency],
    frames: &[Option<usize>],
    policy: &StreamPolicy,
) -> Vec<ir::AssetResidency> {
    let mut out = requirements.to_vec();
    if !policy.allow_cold_fallback {
        return out;
    }
    let sized = |i: usize| requirements[i] == ir::AssetResidency::Resident && frames[i].is_some();
    let items = (0..requirements.len())
        .filter(|&i| sized(i))
        .map(|i| (frames[i].unwrap_or(0).saturating_mul(size_of::<Frame>()), i))
        .collect();
    let kept = admit_within_budget(items, policy.head_budget);
    for i in (0..requirements.len()).filter(|&i| sized(i)) {
        if !kept.contains(&i) {
            out[i] = ir::AssetResidency::Streamable;
        }
    }
    out
}

/// A startup capacity failure, distinct from unreadable/missing source bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadBudgetExceeded {
    pub budget_bytes: usize,
    pub required_at_least_bytes: usize,
}
impl std::fmt::Display for HeadBudgetExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "resident PCM requires at least {} bytes, exceeding configured payload budget {} bytes",
            self.required_at_least_bytes, self.budget_bytes
        )
    }
}
impl std::error::Error for HeadBudgetExceeded {}
fn head_budget_error(budget: usize, required: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::OutOfMemory,
        HeadBudgetExceeded {
            budget_bytes: budget,
            required_at_least_bytes: required,
        },
    )
}
fn canceled_head() -> io::Error {
    io::Error::new(
        io::ErrorKind::Interrupted,
        "sample residency preparation canceled",
    )
}
fn range_bounds(pcm: &Pcm, ranges: &[Range<usize>]) -> io::Result<(usize, usize)> {
    if ranges.windows(2).any(|r| r[0].end > r[1].start)
        || ranges
            .iter()
            .any(|r| r.start >= r.end || r.end > pcm.frame_count())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid sample head ranges",
        ));
    }
    let frames = ranges
        .iter()
        .try_fold(0usize, |n, r| n.checked_add(r.len()))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "sample head size overflow"))?;
    Ok((
        frames
            .checked_mul(2)
            .ok_or_else(|| io::Error::other("minimum sample head size overflow"))?,
        frames
            .checked_mul(size_of::<Frame>())
            .ok_or_else(|| io::Error::other("decoded sample head size overflow"))?,
    ))
}
pub(crate) fn prepare_head_ranges(
    recipe: &HeadPreparer,
    reader: &mut SampleReader,
    ranges: &[Range<usize>],
    page:&mut[Frame],
    canceled: &(dyn Fn() -> bool + Sync),
    mut timing: Option<&mut head_prepare::Timing>,
    cache: Option<head_cache::ChunkCache<'_>>,
) -> io::Result<sampler_core::PreparedRanges> {
    // Metadata and all geometry are validated before quota admission or payload
    // allocation. The decoder workspace is one page, independent of source size.
    if ranges.windows(2).any(|r| r[0].end > r[1].start)
        || ranges
            .iter()
            .any(|r| r.start >= r.end || r.end > recipe.frame_count())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid sample head ranges",
        ));
    }
    if reader.rate() != recipe.sample_rate() || reader.frames() != recipe.frame_count() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "sample metadata changed since admission",
        ));
    }
    let mut builder = recipe.builder();
    if page.len()!=PAGE_FRAMES {return Err(io::Error::new(io::ErrorKind::InvalidInput,"invalid head decoder workspace"));}
    for range in ranges {
        let mut position = range.start;
        while position < range.end {
            if canceled() {
                return Err(canceled_head());
            }
            // Chunks end on the absolute page grid, so presets that cover the
            // same source page produce identical chunks for the head cache.
            let end = ((position / PAGE_FRAMES + 1) * PAGE_FRAMES).min(range.end);
            if let Some(packed) = cache.as_ref().and_then(|c| c.get(position, end - position)) {
                builder
                    .push_packed(position, (*packed).clone())
                    .map_err(|e| head_prepare_error(recipe, e))?;
                position = end;
                continue;
            }
            let frames = &mut page[..end - position];
            let read_start = timing.as_ref().map(|_| Instant::now());
            let result = reader.read(position, frames);
            if let Some(timing) = &mut timing {
                timing.read_decode_us = timing
                    .read_decode_us
                    .saturating_add(read_start.unwrap().elapsed().as_micros() as u64);
            }
            result?;
            if canceled() {
                return Err(canceled_head());
            }
            let pack_start = timing.as_ref().map(|_| Instant::now());
            let result = builder
                .push(position, frames)
                .map_err(|e| head_prepare_error(recipe, e));
            if let Some(timing) = &mut timing {
                timing.validate_pack_us = timing
                    .validate_pack_us
                    .saturating_add(pack_start.unwrap().elapsed().as_micros() as u64);
            }
            result?;
            if let Some(cache) = cache {
                if let Some(packed) = builder.last_packed().filter(|p| p.len() == frames.len()) {
                    cache.put(position, packed.clone());
                }
            }
            position += frames.len();
        }
    }
    if canceled() {
        return Err(canceled_head());
    }
    Ok(builder.finish())
}
fn head_prepare_error(recipe: &HeadPreparer, error: sampler_core::Error) -> io::Error {
    match recipe.budget_status() {
        Some(status) => head_budget_capacity(status, error),
        None => io::Error::new(io::ErrorKind::InvalidData, error),
    }
}
/// A failed exact reservation against the shared head budget is the typed
/// `HeadBudgetExceeded`, which the cold-fallback ladder degrades on.
pub(super) fn head_budget_capacity(status: sampler_core::HeadBudgetStatus, error: sampler_core::Error) -> io::Error {
    if error == sampler_core::Error::Capacity && status.limit_bytes < usize::MAX {
        // A failed exact reservation proves this lower bound; concurrent
        // transactions may release storage before this diagnostic is read.
        return head_budget_error(status.limit_bytes, status.limit_bytes + 1);
    }
    io::Error::new(io::ErrorKind::InvalidData, error)
}
/// A bounded registry for sources published after initial plan compilation.
/// Only decoder/control workers take its lock; audio carries numeric AssetIds.
#[derive(Clone, Default)]
pub struct DynamicSources(Arc<Mutex<HashMap<AssetId, Arc<dyn AssetSource>>>>);
impl DynamicSources {
    pub fn register(
        &self,
        pcm: &Pcm,
        source: Arc<dyn AssetSource>,
    ) -> io::Result<SourceRegistration> {
        let mut entries = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if entries.len() >= 65_600 || entries.contains_key(&pcm.asset_id()) {
            return Err(io::Error::other(
                "dynamic source registry capacity/identity conflict",
            ));
        }
        entries.insert(pcm.asset_id(), source);
        Ok(SourceRegistration {
            registry: self.clone(),
            asset: pcm.asset_id(),
        })
    }
    fn source(&self, asset: AssetId) -> Option<Arc<dyn AssetSource>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&asset)
            .cloned()
    }
}
/// Keep inside an immutable revision. Drop only after its reply reaches control;
/// removal cannot invalidate a decoder's independently owned open reader.
pub struct SourceRegistration {
    registry: DynamicSources,
    asset: AssetId,
}

type StartTable = HashMap<AssetId, Vec<Range<usize>>>;

/// Per-physical-source restore authority. An unadmitted empty optional head
/// must still attempt its original request later, rather than report success.
enum RestoreRange {
    Admitted(Vec<Range<usize>>),
    DeferredRequested(Vec<Range<usize>>),
}
impl AsRef<[Range<usize>]> for RestoreRange {
    fn as_ref(&self) -> &[Range<usize>] {
        match self { Self::Admitted(ranges) | Self::DeferredRequested(ranges) => ranges }
    }
}
type RestoreTable = HashMap<AssetId, RestoreRange>;

fn checked_start_table(assets: &[Pcm], mut ranges: Vec<Vec<Range<usize>>>, enclosing: Option<&StartTable>) -> io::Result<StartTable> {
    if ranges.len() != assets.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "sample start asset/range count mismatch"));
    }
    for (pcm, ranges) in assets.iter().zip(&mut ranges) {
        range_bounds(pcm, ranges)?;
        // A start outside the requested coverage (the resident budget trimmed it) streams on
        // demand instead of failing the load: keep only the part the coverage holds.
        if let Some(table) = enclosing {
            let covered = table.get(&pcm.asset_id()).map_or(&[][..], Vec::as_slice);
            *ranges = intersect_ranges(ranges, covered);
        }
    }
    Ok(physical_start_ranges(assets, &ranges))
}

/// The parts of `ranges` that `covered` also holds. Both sorted and merged.
fn intersect_ranges(ranges: &[Range<usize>], covered: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    for range in ranges {
        for full in covered {
            let (start, end) = (range.start.max(full.start), range.end.min(full.end));
            if start < end {
                out.push(start..end);
            }
        }
    }
    merge_start_ranges(&mut out);
    out
}

fn select_attack_ranges<'a, 'r>(
    assets: &'a [Pcm], table: &'r StartTable, optional: Option<&'r StartTable>,
    pinned: &HashSet<AssetId>, lazy: bool,
) -> Vec<(&'a Pcm, &'r [Range<usize>])> {
    let mut seen = HashSet::new();
    assets.iter().filter(|pcm| seen.insert(pcm.asset_id())).filter_map(|pcm| {
        let required = pcm.requires_resident() || pinned.contains(&pcm.asset_id());
        let ranges = if !required { optional.unwrap_or(table) } else { table };
        let ranges = &ranges[&pcm.asset_id()];
        ((!lazy || required) && (!ranges.is_empty() || pcm.requires_resident())
            && !(pcm.requires_resident() && pcm.resident_frames().is_some()))
            .then_some((pcm, ranges.as_slice()))
    }).collect()
}
impl Drop for SourceRegistration {
    fn drop(&mut self) {
        self.registry
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.asset);
    }
}

/// Control side: where every streamed asset lives, its start ranges, and the
/// decode thread. Purge and reload start ranges here; the runtime owns the
/// page cache.
pub struct Streamer {
    dynamic: DynamicSources,
    pinned_wavetables: HashSet<AssetId>,
    sources: Arc<HashMap<AssetId, Arc<dyn AssetSource>>>,
    ranges: Arc<HashMap<AssetId, Vec<Range<usize>>>>,
    restore_ranges: Arc<RestoreTable>,
    /// Authored onsets without the complete possible start-offset interval.
    /// The full table above remains authoritative for requested coverage reporting.
    onset_ranges: Option<Arc<HashMap<AssetId, Vec<Range<usize>>>>>,
    hint_ranges: Option<Arc<StartTable>>,
    selected_hint_ranges: Option<Arc<StartTable>>,
    selected_hint_head_frames: Option<usize>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    reloader: Option<JoinHandle<()>>,
    head_budget: usize,
    residency_budget: HeadBudget,
    head_fallback: Option<HeadBudgetExceeded>,
    // Control/reload writers share one budget snapshot; audio never takes this lock.
    head_mutation: Arc<Mutex<()>>,
}

impl Streamer {
    /// Packed current, staged and retired heads across all shared writers.
    /// Decode workspaces and page pools are reported separately.
    pub fn head_budget_status(&self)->HeadBudgetStatus {self.residency_budget.status()}
    pub fn head_fallback(&self)->Option<HeadBudgetExceeded> {self.head_fallback}
    pub fn selected_hint_head_frames(&self) -> Option<usize> { self.selected_hint_head_frames }
    /// Physical coverage of startup-predicted Constant/initialized-CC windows
    /// at the original horizon. This is not current engine-state readiness:
    /// later controls, callbacks and source/plan revisions use strict demand.
    pub fn hint_readiness(&self, assets: &[Pcm]) -> Option<HeadReadiness> {
        Some(self.ranges_readiness_matching(assets, |_| true, self.hint_ranges.as_deref()?))
    }
    /// Exact windows selected after quota admission, possibly shorter than the
    /// requested startup predictions. This never certifies new controller
    /// states, source revisions, omitted loop transitions or script offsets.
    pub fn selected_hint_readiness(&self, assets: &[Pcm]) -> Option<HeadReadiness> {
        Some(self.ranges_readiness_matching(assets, |_| true, self.selected_hint_ranges.as_deref()?))
    }
    pub fn dynamic_sources(&self) -> DynamicSources {
        self.dynamic.clone()
    }
    /// Exact, bounded metadata/residency scan. Call off audio. Dynamic revisions
    /// own their own admitted heads and are not included in this initial plan.
    pub fn head_readiness(&self, assets: &[Pcm]) -> HeadReadiness {
        self.head_readiness_matching(assets, |_| true)
    }
    /// Base authored forward/reverse starts. Complete onset coverage is distinct
    /// from covering every possible sample-start modulation offset.
    pub fn onset_readiness(&self, assets: &[Pcm]) -> HeadReadiness {
        self.ranges_readiness_matching(assets, |_| true, self.onset_ranges.as_deref().unwrap_or(&self.ranges))
    }
    /// Coverage of every authored range belonging to the selected asset IDs.
    /// This does not infer script/group/round-robin reachability.
    pub fn head_readiness_matching(
        &self,
        assets: &[Pcm],
        include: impl Fn(AssetId) -> bool,
    ) -> HeadReadiness {
        self.ranges_readiness_matching(assets, include, &self.ranges)
    }
    fn ranges_readiness_matching(
        &self,
        assets: &[Pcm],
        include: impl Fn(AssetId) -> bool,
        table: &HashMap<AssetId, Vec<Range<usize>>>,
    ) -> HeadReadiness {
        let mut state = HeadReadiness {
            budget_bytes: self.head_budget,
            ..Default::default()
        };
        let mut seen = HashSet::new();
        for pcm in assets {
            if !include(pcm.asset_id()) || !seen.insert(pcm.asset_id()) {
                continue;
            }
            let Some(ranges) = table.get(&pcm.asset_id()).filter(|r| !r.is_empty()) else {
                continue;
            };
            state.assets += 1;
            state.packed_bytes = state.packed_bytes.saturating_add(pcm.head_bytes());
            let mut covered = true;
            for range in ranges {
                state.ranges += 1;
                state.source_frames = state.source_frames.saturating_add(range.len() as u64);
                let covered_frames = pcm.resident_covered_frames(range.clone());
                state.covered_source_frames = state.covered_source_frames.saturating_add(covered_frames as u64);
                if covered_frames == range.len() {
                    state.covered_ranges += 1;
                } else {
                    covered = false;
                }
            }
            state.covered_assets += usize::from(covered);
        }
        state
    }
    /// Control-only clone of the exact startup physical payload authority.
    pub fn physical_budget(&self)->HeadBudget {self.residency_budget.clone()}
    /// Clone the existing source for bounded control-worker reads. Opening or
    /// decoding it belongs off the audio thread; this does not preload frames.
    pub fn source(&self, asset: AssetId) -> Option<Arc<dyn AssetSource>> {
        self.sources.get(&asset).cloned().or_else(|| self.dynamic.source(asset))
    }

    /// Open every source as a streamed asset, timing an open plus first-page
    /// read on up to `probe` of them to size heads by `policy`.
    #[cfg(test)]
    pub(crate) fn open(
        sources: Vec<(Arc<dyn AssetSource>, &std::path::Path)>,
        rate: u32,
        policy: &StreamPolicy,
        probe: usize,
        canceled: &(dyn Fn() -> bool + Sync),
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Opened, LoadError> {
        let requirements=vec![ir::AssetResidency::Streamable;sources.len()];
        Self::open_required(sources,&requirements,rate,policy,probe,canceled,progress)
    }
    pub(crate) fn open_required(sources:Vec<(Arc<dyn AssetSource>,&std::path::Path)>,
        requirements:&[ir::AssetResidency],rate:u32,policy:&StreamPolicy,probe:usize,
        canceled:&(dyn Fn()->bool+Sync),progress:&mut dyn FnMut(usize,usize))->Result<Opened,LoadError> {
        if sources.len()!=requirements.len() {return Err(LoadError::Invalid{path:"logical sources".into(),reason:"source requirement count mismatch".into()});}
        let mut wide=false;
        let mut frames=Vec::with_capacity(sources.len());
        let mut layouts=Vec::with_capacity(sources.len());
        for (i,(source,path)) in sources.iter().enumerate() {
            if canceled(){return Err(LoadError::Canceled);}
            let reader=source.open_channels(READER_CACHE_BYTES/policy.decoders.clamp(1,8)/2).map_err(|e|LoadError::io(path,e))?;
            // Sizing a resident source needs its length; an unknown one is left resident.
            frames.push(match (&reader,source.header()) {
                (Some(reader),_)=>Some(reader.frames()),
                (None,Some((_,frames)))=>Some(frames),
                (None,None) if requirements[i]==ir::AssetResidency::Resident=>source.open().ok().map(|r|r.frames()),
                (None,None)=>None,
            });
            // Mono and stereo sources have no channel reader, so their width comes from the header.
            // ponytail: Encoded, Custom and unparsed headers keep 4 bytes/frame (16-bit stereo).
            layouts.push(reader.as_ref().and_then(SampleReader::layout)
                .or_else(|| source.header_layout())
                .unwrap_or(FALLBACK_LAYOUT));
            if let Some(reader)=reader {
                wide |= reader.original_channels().is_some_and(|n|n>2);
            }
        }
        let requirements=&admit_resident_within_budget(requirements,&frames,policy)[..];
        if !wide {return Self::open_required_stereo(sources,requirements,layouts,rate,policy,probe,canceled,progress);}
        if policy.resident_budget.is_some() {return Err(LoadError::Invalid{path:"logical stream".into(),reason:"whole-source promotion requires resident logical producer".into()});}
        let mut logical=Vec::new();let mut assets=Vec::new();let mut registry=HashMap::new();let mut latency=Vec::new();let mut full=0u64;let mut max_packs=1usize;
        for (index,(source,path)) in sources.into_iter().enumerate() {
            if canceled(){return Err(LoadError::Canceled);}
            let started=Instant::now();
            let header=source.open_channels(READER_CACHE_BYTES/policy.decoders.clamp(1,8)/2).map_err(|e|LoadError::io(path,e))?;
            let storage=if header.as_ref().and_then(SampleReader::original_channels).is_some_and(|n|n>2) {
                if requirements[index]==ir::AssetResidency::Resident {return Err(LoadError::Invalid{path:path.into(),reason:"mandatory wide resident source requires resident source pipeline".into()});}
                let (storage,keys)=logical_stream_source(source,READER_CACHE_BYTES/policy.decoders.clamp(1,8)/2).map_err(|e|LoadError::io(path,e))?;
                registry.extend(keys);storage
            } else {
                let mut reader=match header {Some(reader)=>reader,None=>source.open().map_err(|e|LoadError::io(path,e))?};
                let pcm=if requirements[index]==ir::AssetResidency::Resident {Pcm::pending_resident(reader.rate(),reader.frames())} else {Pcm::streamed(reader.rate(),reader.frames())}.map_err(|e|LoadError::Invalid{path:path.into(),reason:e.to_string()})?;
                if index<probe {let mut page=vec![[0.;2];reader.frames().min(PAGE_FRAMES)];reader.read(0,&mut page).map_err(|e|LoadError::io(path,e))?;}
                registry.insert(pcm.asset_id(),source);sampler_core::SourceStorage::Stereo(pcm)
            };
            let view=storage.as_source();max_packs=max_packs.max(view.physical_count());
            view.physical_dependencies(|pack|assets.push(pack.clone()));
            full=full.checked_add((view.frame_count() as u64).checked_mul(view.physical_count() as u64).and_then(|n|n.checked_mul(size_of::<Frame>() as u64)).ok_or_else(||LoadError::Invalid{path:path.into(),reason:"logical stream full-byte overflow".into()})?).ok_or_else(||LoadError::Invalid{path:path.into(),reason:"logical stream aggregate overflow".into()})?;
            if index<probe {latency.push(started.elapsed());}
            logical.push(storage);progress(index+1,requirements.len());
        }
        latency.sort_unstable();let at=|q:usize|latency.get((latency.len()*q/100).min(latency.len().saturating_sub(1))).copied().unwrap_or_default();
        let report=StreamReport{latency_p50:at(50),latency_p95:at(95),latency_p99:at(99),latency_samples:latency.len(),head_budget:policy.head_budget,
            full_bytes:full,pool_pages:policy.voices.checked_mul(max_packs).ok_or_else(||LoadError::Invalid{path:"logical page pool".into(),reason:"physical pool overflow".into()})?,..Default::default()};
        Ok(Opened{logical_sources:Some(logical),assets,sources:registry,report,layouts,rate,policy:*policy})
    }
    fn open_required_stereo(
        sources:Vec<(Arc<dyn AssetSource>,&std::path::Path)>,
        requirements:&[ir::AssetResidency],layouts:Vec<FrameLayout>,rate:u32,policy:&StreamPolicy,probe:usize,
        canceled:&(dyn Fn()->bool+Sync),progress:&mut dyn FnMut(usize,usize),
    )->Result<Opened,LoadError> {
        if requirements.len()!=sources.len() {return Err(LoadError::Invalid {path:"sample residency".into(),reason:"source requirement count mismatch".into()});}
        let _span = crate::audit::Span::new("sample_headers_latency_probe");
        let done = std::sync::atomic::AtomicUsize::new(0);
        let opened = std::thread::scope(|scope| {
            let (sources, done) = (&sources, &done);
            let count = std::thread::available_parallelism()
                .map_or(1, |n| n.get())
                .min(8)
                .min(sources.len());
            let workers: Vec<_> = (0..count)
                .map(|worker| {
                    scope.spawn(move || {
                        let mut page = vec![[0.0; 2]; PAGE_FRAMES];
                        (worker..sources.len())
                            .step_by(count)
                            .map(|i| {
                                let (source, path) = &sources[i];
                                let make=if requirements[i]==ir::AssetResidency::Resident {
                                    Pcm::pending_resident as fn(u32,usize)->Result<Pcm,sampler_core::Error>
                                }else{Pcm::streamed};
                                let result = (|| {
                                    if canceled() {
                                        return Err(LoadError::Canceled);
                                    }
                                    if let Some((rate, frames)) = source.header().filter(|_|i>=probe) {
                                        let pcm = make(rate, frames).map_err(|e| {
                                            LoadError::Invalid {
                                                path: (*path).into(),
                                                reason: e.to_string(),
                                            }
                                        })?;
                                        return Ok((pcm, None));
                                    }
                                    let begin = Instant::now();
                                    let mut reader =
                                        source.open().map_err(|e| LoadError::io(path, e))?;
                                    if source.header().is_some_and(|h|h!=(reader.rate(),reader.frames())) {
                                        return Err(LoadError::Invalid {path:(*path).into(),reason:"cached sample header disagrees with admitted source".into()});
                                    }
                                    let latency = if i < probe {
                                        let n = reader.frames().min(PAGE_FRAMES);
                                        reader
                                            .read(0, &mut page[..n])
                                            .map_err(|e| LoadError::io(path, e))?;
                                        Some(begin.elapsed())
                                    } else {
                                        None
                                    };
                                    let pcm = make(reader.rate(), reader.frames())
                                        .map_err(|e| LoadError::Invalid {
                                            path: (*path).into(),
                                            reason: e.to_string(),
                                        })?;
                                    Ok((pcm, latency))
                                })();
                                done.fetch_add(1, Ordering::Relaxed);
                                (i, result)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            // Cold headers take seconds: report them from this (the progress) thread.
            while !workers.iter().all(|w| w.is_finished()) {
                progress(done.load(Ordering::Relaxed), sources.len());
                std::thread::sleep(Duration::from_millis(15));
            }
            progress(sources.len(), sources.len());
            let mut opened: Vec<_> = workers
                .into_iter()
                .flat_map(|w| w.join().expect("sample header worker"))
                .collect();
            opened.sort_unstable_by_key(|(i, _)| *i);
            opened
                .into_iter()
                .map(|(_, result)| result)
                .collect::<Result<Vec<_>, LoadError>>()
        })?;
        let mut latencies = Vec::new();
        let mut assets = Vec::with_capacity(sources.len());
        let mut registry = HashMap::with_capacity(sources.len());
        let mut full = 0;
        for ((source, _), (pcm, latency)) in sources.into_iter().zip(opened) {
            if let Some(latency) = latency {
                latencies.push(latency);
            }
            full += pcm.frame_count() as u64 * size_of::<Frame>() as u64;
            registry.insert(pcm.asset_id(), source);
            assets.push(pcm);
        }
        latencies.sort_unstable();
        let at = |q: usize| {
            let last = latencies.len().saturating_sub(1);
            latencies
                .get((latencies.len() * q / 100).min(last))
                .copied()
                .unwrap_or_default()
        };
        // p95: a rarer slower read starves a voice briefly (it waits and
        // resumes) rather than failing; heads sized for the tail cost far more.
        let report = StreamReport {
            runway_frames: 0,
            latency_p50: at(50),
            latency_p95: at(95),
            latency_p99: at(99),
            latency_samples: latencies.len(),
            head_frames: 0,
            head_bytes: 0,
            kontakt_window_bytes: 0,
            head_assets: 0,
            head_ranges: 0,
            head_source_frames: 0,
            head_budget: policy.head_budget,
            head_fallback: None,
            required_resident_assets: 0,
            required_resident_bytes: 0,
            full_bytes: full,
            pool_pages: policy.voices,
            pool_bytes: 0,
        };
        Ok(Opened {
            logical_sources: None,
            assets,
            sources: registry,
            report,
            layouts,
            rate,
            policy: *policy,
        })
    }

    /// Read every asset's `ranges` and start `decoders` threads for `worker`.
    /// Returns the streamer and the resident bytes of those ranges.
    #[cfg(test)]
    pub(crate) fn start(
        sources: HashMap<AssetId, Arc<dyn AssetSource>>,
        assets: &[Pcm],
        ranges: Vec<Vec<Range<usize>>>,
        worker: StreamWorker,
        decoders: usize,
        lazy: bool,
        head_budget: usize,
        pinned_wavetables: HashSet<AssetId>,
    ) -> io::Result<(Self, usize)> {
        Self::start_cancelable(
            sources,
            assets,
            ranges,
            worker,
            decoders,
            lazy,
            head_budget,
            pinned_wavetables,
            false,
            &|| false,
        )
    }
    #[cfg(test)]
    fn start_cancelable(
        sources: HashMap<AssetId, Arc<dyn AssetSource>>,
        assets: &[Pcm],
        ranges: Vec<Vec<Range<usize>>>,
        worker: StreamWorker,
        decoders: usize,
        lazy: bool,
        head_budget: usize,
        pinned_wavetables: HashSet<AssetId>,
        allow_cold_fallback: bool,
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> io::Result<(Self, usize)> {
        Self::start_with_onsets_cancelable(sources, assets, ranges, None, worker, decoders,
            lazy, head_budget, pinned_wavetables, allow_cold_fallback, canceled)
    }
    fn start_with_onsets_cancelable(
        sources: HashMap<AssetId, Arc<dyn AssetSource>>,
        assets: &[Pcm],
        ranges: Vec<Vec<Range<usize>>>,
        onsets: Option<Vec<Vec<Range<usize>>>>,
        worker: StreamWorker,
        decoders: usize,
        lazy: bool,
        head_budget: usize,
        pinned_wavetables: HashSet<AssetId>,
        allow_cold_fallback: bool,
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> io::Result<(Self, usize)> {
        Self::start_with_fallbacks_cancelable(sources,assets,ranges,onsets,worker,decoders,
            lazy,head_budget,pinned_wavetables,allow_cold_fallback,canceled)
    }
    fn start_with_fallbacks_cancelable(
        sources: HashMap<AssetId, Arc<dyn AssetSource>>,
        assets: &[Pcm],
        ranges: Vec<Vec<Range<usize>>>,
        onsets: Option<Vec<Vec<Range<usize>>>>,
        worker: StreamWorker,
        decoders: usize,
        lazy: bool,
        head_budget: usize,
        pinned_wavetables: HashSet<AssetId>,
        allow_cold_fallback: bool,
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> io::Result<(Self, usize)> {
        Self::start_with_hint_fallbacks_cancelable(sources, assets, ranges, onsets, None,
            worker, decoders, lazy, head_budget, pinned_wavetables, allow_cold_fallback, canceled)
    }
    fn start_with_hint_fallbacks_cancelable(
        sources: HashMap<AssetId, Arc<dyn AssetSource>>,
        assets: &[Pcm],
        ranges: Vec<Vec<Range<usize>>>,
        onsets: Option<Vec<Vec<Range<usize>>>>,
        hints: Option<HintFallback>,
        worker: StreamWorker,
        decoders: usize,
        lazy: bool,
        head_budget: usize,
        pinned_wavetables: HashSet<AssetId>,
        allow_cold_fallback: bool,
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> io::Result<(Self, usize)> {
        Self::start_with_reload_cancelable(sources, assets, ranges, None, onsets, hints, worker,
            decoders, lazy, head_budget, pinned_wavetables, allow_cold_fallback, canceled)
    }
    /// `reload` is the purge-reload table when some zones sit in purged groups:
    /// per asset, every zone's start ranges (`reload_start_ranges`). Assets whose
    /// load table is shorter restore from it, so a purge reload loads the law head.
    fn start_with_reload_cancelable(
        sources: HashMap<AssetId, Arc<dyn AssetSource>>,
        assets: &[Pcm],
        ranges: Vec<Vec<Range<usize>>>,
        purge_reload: Option<Vec<Vec<Range<usize>>>>,
        onsets: Option<Vec<Vec<Range<usize>>>>,
        hints: Option<HintFallback>,
        worker: StreamWorker,
        decoders: usize,
        lazy: bool,
        head_budget: usize,
        pinned_wavetables: HashSet<AssetId>,
        allow_cold_fallback: bool,
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> io::Result<(Self, usize)> {
        let span = crate::audit::Span::new("sample_preload");
        let decoders = decoders.clamp(1, 8);
        let table = checked_start_table(assets, ranges, None)?;
        let reload_table = purge_reload.map(|ranges| checked_start_table(assets, ranges, None)).transpose()?;
        let onset_table = onsets.map(|ranges| checked_start_table(assets, ranges, Some(&table))).transpose()?;
        let (hint_table, requested_hint_head_frames) = match hints {
            Some(hints) => (Some(checked_start_table(assets, hints.ranges, Some(&table))?), Some(hints.head_frames)),
            None => (None, None),
        };
        let hinted_table = hint_table.as_ref().filter(|h| h.values().any(|r| !r.is_empty())).map(|hints| {
            let mut union = onset_table.as_ref().unwrap_or(&table).clone();
            for (id, ranges) in hints { union.entry(*id).or_default().extend(ranges.iter().cloned()); }
            for ranges in union.values_mut() { merge_start_ranges(ranges); }
            union
        });
        if pinned_wavetables.iter().any(|id| !table.contains_key(id)) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown pinned wavetable asset"));
        }
        if canceled() { return Err(canceled_head()); }
        // Pinned wavetables are mandatory heads; under cold fallback the ones that
        // cannot fit the shared budget together stream like any other source.
        let pinned_wavetables = if allow_cold_fallback {
            let items = assets.iter()
                .filter(|p| pinned_wavetables.contains(&p.asset_id()))
                .map(|p| (range_bounds(p, &table[&p.asset_id()]).map_or(usize::MAX, |b| b.0), p.asset_id()))
                .collect();
            admit_within_budget(items, head_budget).into_iter().collect()
        } else {
            pinned_wavetables
        };
        let residency_budget = HeadBudget::new(head_budget);
        for pcm in assets {
            let bound = if pcm.requires_resident() { pcm.bind_resident_budget(&residency_budget) } else { pcm.bind_head_budget(&residency_budget) };
            bound.map_err(|e| if e == sampler_core::Error::Capacity {
                head_budget_error(head_budget, head_budget.saturating_add(1))
            } else { io::Error::new(io::ErrorKind::InvalidInput, e) })?;
        }
        // Every attempt keeps its chosen asset identities with its owned staging.
        // Range-table borrows end before trying a new candidate or publishing.
        let prepare = |optional: Option<&StartTable>, lazy: bool, phase: &str, greedy: bool|
            -> io::Result<(Vec<head_prepare::Prepared>, usize, Vec<&Pcm>, Arc<RestoreTable>)> {
            // A purged group's assets are not preloaded: they stay non-resident
            // until a purge/load request acquires them (residency_permitted).
            let mut selected: Vec<_> = select_attack_ranges(assets, &table, optional, &pinned_wavetables, lazy)
                .into_iter().filter(|(pcm, _)| pcm.residency_permitted()).collect();
            if greedy {
                // Degrade, never fail: the cheapest starts are admitted first within the budget
                // that is left, and a source that does not fit is skipped (it streams on demand).
                // Pinned wavetables are mandatory heads and are always kept.
                let room = head_budget.saturating_sub(residency_budget.status().held_bytes);
                let mut keep = Vec::new();
                let mut pinned_bytes = 0usize;
                let mut rest = Vec::new();
                for (index, (pcm, ranges)) in selected.iter().enumerate() {
                    let bytes = head_cost(pcm, ranges)?;
                    if pinned_wavetables.contains(&pcm.asset_id()) {
                        pinned_bytes = pinned_bytes.saturating_add(bytes);
                        keep.push(index);
                    } else {
                        rest.push((bytes, index));
                    }
                }
                keep.extend(admit_within_budget(rest, room.saturating_sub(pinned_bytes)));
                let keep: HashSet<usize> = keep.into_iter().collect();
                selected = selected.into_iter().enumerate()
                    .filter(|(index, _)| keep.contains(index)).map(|(_, item)| item).collect();
            }
            let audit = std::env::var_os("KONTRA_AUDIT_LOAD").is_some();
            let started = audit.then(Instant::now);
            let result = (|| {
                // Early out only: even the smallest stored form (2 bytes a frame) cannot
                // fit. Admission itself is the exact reservation inside `head_prepare`.
                let minimum = selected.iter().try_fold(0usize, |sum, (pcm, ranges)| {
                    let min = head_floor_bytes(pcm, ranges)?;
                    sum.checked_add(min).ok_or_else(|| io::Error::other("sample head size overflow"))
                })?;
                let minimum = residency_budget.status().held_bytes.checked_add(minimum)
                    .ok_or_else(|| io::Error::other("sample head size overflow"))?;
                if minimum > head_budget { return Err(head_budget_error(head_budget, minimum)); }
                head_prepare::prepare(&selected, &sources, &residency_budget, decoders, canceled)
            })();
            if audit {
                eprintln!("stream_preload_attempt {}", serde_json::json!({
                    "phase":phase,"assets":selected.len(),
                    "requested_source_frames":selected.iter().fold(0u64, |sum,(pcm,ranges)| sum.saturating_add(
                        if pcm.requires_resident() { pcm.frame_count() as u64 } else { ranges.iter().map(|r|r.len() as u64).sum() })),
                    "elapsed_us":started.unwrap().elapsed().as_micros() as u64,
                    "admitted_payload_bytes":result.as_ref().ok().map(|(_,bytes)|*bytes),
                    "error_kind":result.as_ref().err().map(|error| if error.get_ref().is_some_and(|e|e.is::<HeadBudgetExceeded>()) {"capacity"}
                        else if error.kind()==io::ErrorKind::Interrupted {"canceled"} else {"source_or_metadata"}),
                    "peak_payload_bytes":residency_budget.status().peak_bytes,"budget_bytes":head_budget,
                    "scope":"control-only candidate; requested frames are not decoded-byte counters; failed attempts publish nothing"
                }));
            }
            let (staged,bytes) = result?;
            let mut restore: RestoreTable = table.iter().map(|(id, requested)| {
                let selected = &optional.unwrap_or(&table)[id];
                let recipe = if lazy || selected.is_empty() && !requested.is_empty() {
                    RestoreRange::DeferredRequested(requested.clone())
                } else { RestoreRange::Admitted(selected.clone()) };
                (*id, recipe)
            }).collect();
            // Required/pinned ranges were never shortened by the selector,
            // including mandatory entries in otherwise deferred lazy startup.
            for pcm in assets.iter().filter(|p| p.requires_resident() || pinned_wavetables.contains(&p.asset_id())) {
                restore.insert(pcm.asset_id(), RestoreRange::Admitted(table[&pcm.asset_id()].clone()));
            }
            // Purged-group zones are not in `table`; their purge reload restores the law head.
            for (id, full) in reload_table.iter().flat_map(|table| table.iter()) {
                if table[id] != *full {
                    restore.insert(*id, RestoreRange::DeferredRequested(full.clone()));
                }
            }
            let restore = Arc::new(restore);
            Ok((staged,bytes,selected.into_iter().map(|(pcm,_)|pcm).collect(),restore))
        };
        let mut head_fallback = None;
        let mut selected_hint_table = None;
        let mut selected_hint_head_frames = if !lazy && hinted_table.is_some() { requested_hint_head_frames } else { None };
        let mut selected_requested_hints = selected_hint_head_frames.is_some();
        let (staged, bytes, selected_assets, restore_ranges) = match prepare(None, lazy, "full_possible", false) {
            Ok(staged) => staged,
            Err(error) => {
                let reason = error.get_ref().and_then(|e|e.downcast_ref::<HeadBudgetExceeded>()).copied();
                let Some(reason) = reason.filter(|_| allow_cold_fallback && !lazy) else { return Err(error); };
                selected_requested_hints = false;
                selected_hint_head_frames = None;
                let mut accepted = None;
                for (candidate, hinted, phase) in [(hinted_table.as_ref(), true, "initial_state_onsets"),
                    (onset_table.as_ref(), false, "base_onsets")] {
                    if accepted.is_some() { break; }
                    let Some(candidate) = candidate else { continue; };
                    match prepare(Some(candidate), false, phase, false) {
                        Ok(staged) => {
                            selected_requested_hints = hinted;
                            selected_hint_head_frames = if hinted { requested_hint_head_frames } else { None };
                            accepted = Some(staged);
                            break;
                        }
                        Err(error) if error.get_ref().is_some_and(|e|e.is::<HeadBudgetExceeded>()) => {}
                        Err(error) => return Err(error),
                    }
                }
                let staged = match accepted {
                    Some(staged) => staged,
                    None => match prepare(None, true, "mandatory_only", false) {
                        Ok(staged) => staged,
                        // Even the mandatory sources overflow the budget: admit the cheapest
                        // starts first instead of failing the whole preload.
                        Err(error) if error.get_ref().is_some_and(|e| e.is::<HeadBudgetExceeded>()) =>
                            prepare(None, false, "budget_priority", true)?,
                        Err(error) => return Err(error),
                    },
                };
                head_fallback = Some(reason);
                staged
            }
        };
        if canceled() {
            return Err(canceled_head());
        }
        // No source I/O or packing remains. Only this private initial plan is
        // published after exact lossless capacity admission of every head.
        {
            let _publication = crate::audit::Span::new("sample_head_publication");
            for (pcm, prepared) in selected_assets.into_iter().zip(staged) {
                match prepared {
                    head_prepare::Prepared::Head(head)=>{pcm.publish_ranges(head).map(|_|())},
                    head_prepare::Prepared::Resident(full)=>pcm.publish_initial_resident(full),
                }.map_err(|e| io::Error::new(io::ErrorKind::InvalidData,e))?;
            }
        }
        // One page workspace shared by the explicit-residency worker recipes,
        // rather than a retained decoder or head-sized buffer per source.
        let residency_page=Arc::new(Mutex::new(vec![[0.;2];PAGE_FRAMES]));
        // Each asset keeps source metadata, never a backup of sample buffers.
        // Explicit KSP reload returns staging ranges to the core's residency worker.
        let mut registered = HashSet::new();
        for pcm in assets {
            if !registered.insert(pcm.asset_id()) { continue; }
            // Assets without a stream source (fully resident) have nothing to reload.
            let (Some(source), Some(ranges)) = (
                sources.get(&pcm.asset_id()).cloned(),
                restore_ranges.get(&pcm.asset_id()).map(|r| r.as_ref().to_vec()),
            ) else {
                continue;
            };
            if pcm.requires_resident() {
                let (rate,frames,budget)=(pcm.sample_rate(),pcm.frame_count(),residency_budget.clone());
                pcm.set_budgeted_resident_loader(move|| {
                    let buffer=ResidentBuffer::new(rate,frames,&budget)?;
                    let mut reader=source.open().map_err(|_|sampler_core::Error::NotReady)?;
                    head_prepare::fill_resident(rate,frames,buffer,&mut reader,&||false,None).map_err(|_|sampler_core::Error::NotReady)
                }).map_err(|e|io::Error::other(e))?;
                continue;
            }
            let recipe = pcm.head_preparer();
            if source.channel_pack().is_some() {
                pcm.set_prepared_residency_loader(move || {
                    let (logical,ordinal)=source.channel_pack().ok_or(sampler_core::Error::InvalidInput)?;
                    head_prepare::prepare_logical_pack(logical,ordinal,&recipe,&ranges,&||false)
                        .map_err(|e|if e.kind()==io::ErrorKind::OutOfMemory {sampler_core::Error::Capacity}else{sampler_core::Error::NotReady})
                }).map_err(io::Error::other)?;
                continue;
            }
            let residency_page=residency_page.clone();
            let content=source.content_key();
            pcm.set_prepared_residency_loader(move || {
                let mut page=residency_page.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut reader = source.open().map_err(|_| sampler_core::Error::NotReady)?;
                let cache=content.as_ref().map(|key| head_cache::ChunkCache::new(head_cache::process(), key));
                prepare_head_ranges(&recipe, &mut reader, &ranges, &mut page, &|| false, None, cache).map_err(
                    |error| {
                        if error
                            .get_ref()
                            .is_some_and(|e| e.is::<HeadBudgetExceeded>())
                        {
                            sampler_core::Error::Capacity
                        } else {
                            sampler_core::Error::NotReady
                        }
                    },
                )
            })
            .map_err(|e| io::Error::other(e.to_string()))?;
        }
        drop(span);
        let sources = Arc::new(sources);
        let stop = Arc::new(AtomicBool::new(false));
        // Several decoders overlap reads, so one slow read does not hold up
        // the pages queued behind it; they share the single worker endpoint.
        let worker = Arc::new(Mutex::new(worker));
        let hint_ranges = hint_table.map(Arc::new);
        let selected_hint_ranges = if selected_requested_hints { hint_ranges.clone() } else { selected_hint_table.map(Arc::new) };
        let mut streamer = Self {
            dynamic: DynamicSources::default(),
            pinned_wavetables,
            sources,
            ranges: Arc::new(table),
            restore_ranges,
            onset_ranges: onset_table.map(Arc::new),
            hint_ranges,
            selected_hint_ranges,
            selected_hint_head_frames,
            stop,
            threads: Vec::new(),
            reloader: None,
            head_budget,
            residency_budget,
            head_fallback,
            head_mutation: Arc::new(Mutex::new(())),
        };
        for _ in 0..decoders.max(1) {
            let thread = std::thread::Builder::new()
                .name("sampler-stream".into())
                .spawn({
                    let (sources, stop) = (streamer.sources.clone(), streamer.stop.clone());
                    let dynamic = streamer.dynamic.clone();
                    let worker = worker.clone();
                    move || {
                        decode(
                            &worker,
                            &sources,
                            &dynamic,
                            &stop,
                            READER_CACHE_BYTES / decoders.max(1),
                        )
                    }
                })?;
            streamer.threads.push(thread);
        }
        // Restore purged start ranges a start found missing, off the audio
        // and control threads.
        let reloader = std::thread::Builder::new()
            .name("sampler-reload".into())
            .spawn({
                let assets: Arc<[Pcm]> = assets.into();
                let (sources, ranges) = (streamer.sources.clone(), streamer.restore_ranges.clone());
                let stop = streamer.stop.clone();
                let head_mutation = streamer.head_mutation.clone();
                move || {
                    // Woken by the audio side (`StreamCache::set_reloader`) when a
                    // start finds an asset cold, and by Drop.
                    while !stop.load(Ordering::Relaxed) {
                        if reload(&assets, &sources, &ranges, head_budget, &head_mutation).is_err()
                        {
                            std::thread::park_timeout(Duration::from_millis(100));
                        } else {
                            std::thread::park();
                        }
                    }
                }
            })?;
        streamer.reloader = Some(reloader);
        Ok((streamer, bytes))
    }
    /// Purge the start ranges of streamed assets not played since `before`
    /// (runtime clock). Returns bytes removed from current coverage; retired
    /// storage remains charged until control reclaims its final audio reader.
    /// Their next start fails
    /// `NotReady` and marks them for `reload`.
    pub fn purge(&self, assets: &[Pcm], before: u64) -> usize {
        self.trim(assets, 0, before)
    }

    /// Purge start ranges, least recently played first, until those of
    /// `assets` take at most `budget` bytes; only assets not played since
    /// `before` are purged. Returns bytes removed from current coverage, which
    /// may remain physically held in retired generations.
    pub fn trim(&self, assets: &[Pcm], budget: usize, before: u64) -> usize {
        let _mutation = self
            .head_mutation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut seen = HashSet::new();
        let mut held = 0;
        let mut idle = Vec::new();
        for pcm in assets {
            if !seen.insert(pcm.asset_id()) { continue; }
            held += pcm.head_bytes();
            if !self.pinned_wavetables.contains(&pcm.asset_id())
                && pcm.head_frames() > 0 && pcm.last_played() < before {
                idle.push(pcm);
            }
        }
        idle.sort_unstable_by_key(|pcm| pcm.last_played());
        let mut freed = 0;
        for pcm in idle {
            if held <= budget {
                break;
            }
            let old = pcm.set_ranges(Vec::new()).expect("no ranges are valid");
            let n = old.iter().map(|(_, f)| f.bytes()).sum::<usize>();
            drop(old);
            pcm.head_bytes();
            (held, freed) = (held - n, freed + n);
        }
        freed
    }

    /// Read start ranges again for assets whose start found them purged.
    /// Returns how many were reloaded.
    /// A background thread already does this when a start finds one cold.
    pub fn reload(&self, assets: &[Pcm]) -> io::Result<usize> {
        reload(
            assets,
            &self.sources,
            &self.restore_ranges,
            self.head_budget,
            &self.head_mutation,
        )
    }
}

fn reload<R: AsRef<[Range<usize>]>>(
    assets: &[Pcm],
    sources: &HashMap<AssetId, Arc<dyn AssetSource>>,
    ranges: &HashMap<AssetId, R>,
    _budget: usize,
    head_mutation: &Mutex<()>,
) -> io::Result<usize> {
    let _mutation = head_mutation
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut count = 0;
    let mut page=Vec::new();
    // Reclaim only on this worker. Dropping an audio read guard alone must
    // leave its retired generation and lease charged.
    for pcm in assets {
        pcm.head_bytes();
    }
    for pcm in assets {
        if !pcm.residency_permitted() || pcm.requires_resident() || pcm.resident_frames().is_some() || !pcm.take_cold() {
            continue;
        }
        let id = pcm.asset_id();
        let (Some(source), Some(ranges)) = (sources.get(&id), ranges.get(&id)) else {
            continue;
        };
        if page.is_empty() {page.resize(PAGE_FRAMES,[0.;2]);}
        let loaded = match if let Some((recipe,ordinal))=source.channel_pack() {
            head_prepare::prepare_logical_pack(recipe,ordinal,&pcm.head_preparer(),ranges.as_ref(),&||!pcm.residency_permitted())
                .and_then(|head| {let bytes=head.bytes();pcm.publish_ranges(head).map_err(io::Error::other)?;Ok(bytes)})
        } else {source.open_stream().and_then(|mut reader|load_ranges(pcm,&mut reader,ranges.as_ref(),&mut page))}
        {
            Ok(bytes) => bytes,
            Err(error) => {
                if !matches!(
                    error.kind(),
                    io::ErrorKind::InvalidData
                        | io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::NotFound
                        | io::ErrorKind::PermissionDenied
                ) {
                    pcm.mark_cold();
                }
                return Err(error);
            }
        };
        count += usize::from(loaded > 0);
    }
    Ok(count)
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..).chain(self.reloader.take()) {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

/// Open readers kept per decode thread between pages of the same assets
/// (each holds a 16 KiB read buffer).
const OPEN_READERS: usize = 16;
const MAX_CACHED_READERS: usize = 128;
/// Across all workers; PCM stream pages and authored resident heads are separate.
const READER_CACHE_BYTES: usize = 8 << 20;

struct CachedReader {
    asset: AssetId,
    reader: SampleReader,
    used: u64,
    bytes: Option<usize>,
}
struct ReaderCache {
    entries: Vec<CachedReader>,
    total_limit: usize,
    byte_limit: usize,
    bytes: usize,
    opaque: usize,
    evictions: u64,
    peak_retained_bytes: usize,
    storage_bytes: usize,
    entry_limit: usize,
}
impl ReaderCache {
    fn new(byte_limit: usize) -> Self {
        let entry_limit =
            MAX_CACHED_READERS.min(byte_limit / std::mem::size_of::<CachedReader>() / 2);
        let entries = Vec::with_capacity(entry_limit);
        let storage_bytes = entries.capacity() * std::mem::size_of::<CachedReader>();
        Self {
            entries,
            total_limit: byte_limit,
            byte_limit: byte_limit.saturating_sub(storage_bytes),
            bytes: 0,
            opaque: 0,
            evictions: 0,
            peak_retained_bytes: storage_bytes,
            storage_bytes,
            entry_limit,
        }
    }
    fn remove(&mut self, index: usize) {
        let entry = self.entries.swap_remove(index);
        match entry.bytes {
            Some(bytes) => self.bytes -= bytes,
            None => self.opaque -= 1,
        }
        self.evictions += 1;
    }
    fn insert(&mut self, asset: AssetId, reader: SampleReader, used: u64) -> usize {
        if self.entry_limit > 0 && self.entries.len() == self.entry_limit {
            let oldest = (0..self.entries.len())
                .min_by_key(|&i| self.entries[i].used)
                .unwrap();
            self.remove(oldest);
        }
        let bytes = reader.cache_bytes();
        match bytes {
            Some(bytes) => self.bytes += bytes,
            None => self.opaque += 1,
        }
        self.entries.push(CachedReader {
            asset,
            reader,
            used,
            bytes,
        });
        self.storage_bytes = self.entries.capacity() * std::mem::size_of::<CachedReader>();
        self.byte_limit = self.total_limit.saturating_sub(self.storage_bytes);
        self.entries.len() - 1
    }
    /// Scratch and channel capacities may grow during decoding. Charge actual
    /// capacities before retaining the reader. An oversized reader is released
    /// after its current job, so it never persists above the byte budget.
    fn trim_after_read(&mut self, index: usize) {
        let entry = &mut self.entries[index];
        if let Some(old) = entry.bytes {
            let next = entry.reader.cache_bytes().expect("built-in codec");
            entry.bytes = Some(next);
            self.bytes = self.bytes - old + next;
        }
        while self.bytes > self.byte_limit || self.opaque > OPEN_READERS {
            let known = self.bytes > self.byte_limit;
            let oldest = (0..self.entries.len())
                .filter(|&i| self.entries[i].bytes.is_some() == known)
                .min_by_key(|&i| self.entries[i].used)
                .unwrap();
            self.remove(oldest);
        }
        if self.entry_limit == 0 {
            // Tiny per-worker budgets can serve a job but retain no reader or
            // vector capacity. Decoder workspaces remain transient worker work.
            self.entries.clear();
            self.entries = Vec::new();
            self.bytes = 0;
            self.opaque = 0;
            self.storage_bytes = 0;
            self.byte_limit = self.total_limit;
        }
        self.peak_retained_bytes = self
            .peak_retained_bytes
            .max(self.bytes + self.storage_bytes);
    }
}

/// Opt-in bounded worker receipts. Audio adds a clock only when a new request
/// is audited; it never allocates or logs. No PCM/resource content leaves RAM.
#[derive(Default)]
struct DecodeAudit {
    jobs: u64,
    opens: u64,
    evictions: u64,
    failures: u64,
    open_nanos: u64,
    decode_nanos: u64,
    first_128_job_nanos: u64,
    max_job_nanos: u64,
    job_us_log2: [u64; 32],
    reader_budget_bytes: usize,
    peak_retained_reader_bytes: usize,
    retained_opaque_readers: usize,
    reply_backpressure: u64,
    queue_count: u64,
    queue_total_us: u64,
    queue_max_us: u64,
    queue_us_log2: [u64; 32],
    open_max_us: u64,
    open_us_log2: [u64; 32],
    read_max_us: u64,
    read_us_log2: [u64; 32],
    publish_attempts: u64,
    publish_total_us: u64,
    publish_max_us: u64,
    page_probes: Vec<serde_json::Value>,
    dropped_page_probes: u64,
    cpu_clock_jobs: u64,
    worker_cpu_nanos: u64,
    read_cpu_nanos: u64,
    open_cpu_nanos: u64,
}
impl Drop for DecodeAudit {
    fn drop(&mut self) {
        eprintln!(
            "stream_decode_audit {}",
            serde_json::json!({
                "worker": format!("{:?}", std::thread::current().id()),
                "jobs": self.jobs, "opens": self.opens, "reader_evictions": self.evictions,
                "failures": self.failures, "open_total_us": self.open_nanos / 1000,
                "decode_total_us": self.decode_nanos / 1000,
                "first_128_job_total_us": self.first_128_job_nanos / 1000,
                "max_job_us": self.max_job_nanos / 1000,
                "job_us_log2_histogram": self.job_us_log2,
                "reader_budget_bytes": self.reader_budget_bytes,
                "peak_retained_reader_bytes": self.peak_retained_reader_bytes,
                "retained_opaque_readers": self.retained_opaque_readers,
                "reply_backpressure": self.reply_backpressure,
                "queue_count":self.queue_count,"queue_total_us":self.queue_total_us,"queue_max_us":self.queue_max_us,"queue_us_log2":self.queue_us_log2,
                "open_max_us":self.open_max_us,"open_us_log2":self.open_us_log2,"seek_read_decode_max_us":self.read_max_us,"seek_read_decode_us_log2":self.read_us_log2,
                "publish_attempts":self.publish_attempts,"publish_total_us":self.publish_total_us,"publish_max_us":self.publish_max_us,
                "page_probes": self.page_probes, "dropped_page_probes": self.dropped_page_probes,
                "cpu_clock_jobs":self.cpu_clock_jobs,"worker_cpu_us":self.worker_cpu_nanos/1000,
                "read_cpu_us":self.read_cpu_nanos/1000,"open_cpu_us":self.open_cpu_nanos/1000,
            })
        );
    }
}

fn decode(
    worker: &Mutex<StreamWorker>,
    sources: &HashMap<AssetId, Arc<dyn AssetSource>>,
    dynamic: &DynamicSources,
    stop: &AtomicBool,
    reader_byte_limit: usize,
) {
    decode_with_publish(
        worker,
        sources,
        dynamic,
        stop,
        reader_byte_limit,
        |job, result| {
            worker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .complete(job, result)
        },
    );
}
fn decode_with_publish(
    worker: &Mutex<StreamWorker>,
    sources: &HashMap<AssetId, Arc<dyn AssetSource>>,
    dynamic: &DynamicSources,
    stop: &AtomicBool,
    reader_byte_limit: usize,
    publish: impl Fn(
        sampler_core::DecodeJob,
        Result<(), DecodeFailure>,
    ) -> Result<(), sampler_core::RejectedDecode>,
) {
    let mut audit = std::env::var_os("KONTRA_STREAM_AUDIT").map(|_| DecodeAudit::default());
    let worker = || {
        worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    // Linear LRU is bounded at 128 readers per worker; keep it off audio.
    let mut readers = ReaderCache::new(reader_byte_limit);
    if let Some(audit) = &mut audit {
        audit.reader_budget_bytes = reader_byte_limit;
        audit.peak_retained_reader_bytes = readers.peak_retained_bytes;
    }
    let max_packs=sources.values().filter_map(|source|source.channel_pack().map(|(recipe,_)|recipe.packs.len())).max().unwrap_or(0);
    let mut cohort=Vec::new();
    if cohort.try_reserve_exact(max_packs).is_err() {return;}
    let cohort_bytes=cohort.capacity().saturating_mul(size_of::<sampler_core::DecodeJob>());
    if cohort_bytes>reader_byte_limit {return;}
    let mut logical_reader:Option<(sampler_core::SourceStorageId,SampleReader,Vec<Vec<Frame>>)>=None;
    let mut tick = 0u64;
    let mut held: Option<(sampler_core::DecodeJob, Result<(), DecodeFailure>)> = None;
    while !stop.load(Ordering::Relaxed) {
        if let Some((job, result)) = held.take() {
            let publish_start = audit.as_ref().map(|_| Instant::now());
            let result = publish(job, result);
            if let Some(a) = &mut audit {
                let us = publish_start.unwrap().elapsed().as_micros() as u64;
                a.publish_attempts += 1;
                a.publish_total_us += us;
                a.publish_max_us = a.publish_max_us.max(us);
            }
            if let Err(rejected) = result {
                if rejected.reason != sampler_core::StreamError::Capacity {
                    return;
                }
                if let Some(audit) = &mut audit {
                    audit.reply_backpressure += 1;
                }
                held = Some((rejected.job, rejected.result));
                std::thread::sleep(Duration::from_micros(500));
            }
            continue;
        }
        let next={
            let mut coordinator=worker();
            let first=coordinator.next_job();
            if let Some(first)=&first {
                if let Some((recipe,_))=sources.get(&first.key().asset).and_then(|s|s.channel_pack()) {
                    while let Some(related)=coordinator.next_related_job(first.key().index,&recipe.packs) {
                        // One request exists per actual physical key/slot.
                        if cohort.len()==max_packs {return;}
                        cohort.push(related);
                    }
                }
            }
            first
        };
        let Some(mut job) = next else {
            std::thread::park(); continue;
        };
        if let Some((recipe,ordinal))=sources.get(&job.key().asset).and_then(|s|s.channel_pack()) {
            if cohort.len()==max_packs {return;}
            cohort.push(job);
            // Cache one logical original-channel reader, never one decoder per
            // output pack. A different logical source releases it off audio.
            readers.entries.clear();readers.bytes=0;readers.opaque=0;
            let result=(|| -> io::Result<()> {
                let available=reader_byte_limit.checked_sub(cohort_bytes)
                    .and_then(|n|n.checked_sub(readers.storage_bytes))
                    .ok_or_else(||io::Error::new(io::ErrorKind::OutOfMemory,"logical worker metadata exceeds workspace admission"))?;
                if recipe.workspace_limit>available {
                    return Err(io::Error::new(io::ErrorKind::OutOfMemory,"logical worker aggregate workspace admission"));
                }
                recipe.reader_workspace_limit()?;
                if logical_reader.as_ref().is_none_or(|(id,_,_)|*id!=recipe.id) {
                    logical_reader=None;
                    let scratch=recipe.scratch()?;
                    let reader=recipe.open()?;
                    logical_reader=Some((recipe.id,reader,scratch));
                }
                let (_,reader,scratch)=logical_reader.as_mut().expect("logical reader opened");
                let range=cohort.last().expect("first page retained").range();
                if cohort.iter().any(|j|j.range()!=range) {return Err(io::Error::new(io::ErrorKind::InvalidInput,"logical batch range mismatch"));}
                recipe.read(reader,range.start,range.len(),scratch)?;
                for job in &mut cohort {
                    let index=recipe.packs.iter().position(|id|*id==job.key().asset).ok_or_else(||io::Error::new(io::ErrorKind::InvalidData,"logical key missing"))?;
                    job.frames_mut().copy_from_slice(&scratch[index][..range.len()]);
                }
                let _=ordinal;
                Ok(())
            })().map_err(|error|match error.kind() {
                io::ErrorKind::InvalidData|io::ErrorKind::UnexpectedEof=>DecodeFailure::InvalidSamples,
                _=>DecodeFailure::Unavailable,
            });
            if result.is_err() {logical_reader=None;}
            // All decode/validation completed before any result enters the
            // normal queue. Runtime's all-pack barrier prevents partial render.
            while let Some(mut done)=cohort.pop() {
                loop {
                    match publish(done,result) {
                        Ok(())=>break,
                        Err(rejected)=>{
                            if rejected.reason!=sampler_core::StreamError::Capacity || stop.load(Ordering::Relaxed) {return;}
                            done=rejected.job;
                            std::thread::sleep(Duration::from_micros(500));
                        }
                    }
                }
            }
            continue;
        }
        debug_assert!(cohort.is_empty());
        logical_reader=None;
        let job_start = audit.as_ref().map(|_| Instant::now());
        let cpu_start = audit.as_ref().and_then(|_| crate::audit::thread_cpu_nanos());
        let queue_us = job
            .queued_at()
            .zip(job_start)
            .map(|(queued, start)| start.saturating_duration_since(queued).as_micros() as u64);
        tick += 1;
        let asset = job.key().asset;
        let open_start = audit.as_ref().map(|_| Instant::now());
        let mut opened = false;
        let index = match readers
            .entries
            .iter()
            .position(|entry| entry.asset == asset)
        {
            Some(i) => Some(i),
            None => {
                opened = true;
                sources
                    .get(&asset)
                    .cloned()
                    .or_else(|| dynamic.source(asset))
                    .and_then(|s| s.open_stream().ok())
                    .map(|reader| readers.insert(asset, reader, tick))
            }
        };
        let cpu_after_open = cpu_start.and_then(|_| crate::audit::thread_cpu_nanos());
        let open_cpu = cpu_start.zip(cpu_after_open).map(|(a,b)|b.saturating_sub(a));
        let open_us = open_start.map(|start| start.elapsed().as_micros() as u64);
        if let Some(audit) = &mut audit {
            audit.opens += u64::from(opened);
            if opened {
                audit.open_nanos += open_start.unwrap().elapsed().as_nanos() as u64;
            }
        }
        let decode_start = audit.as_ref().map(|_| Instant::now());
        let read_cpu_start = cpu_start.and_then(|_| crate::audit::thread_cpu_nanos());
        let result = match index {
            Some(i) => {
                readers.entries[i].used = tick;
                let start = job.range().start;
                readers.entries[i]
                    .reader
                    .read(start, job.frames_mut())
                    .map_err(|error| match error.kind() {
                        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => {
                            DecodeFailure::InvalidSamples
                        }
                        _ => DecodeFailure::Unavailable,
                    })
            }
            None => Err(DecodeFailure::Unavailable),
        };
        let read_us = decode_start.map(|s| s.elapsed().as_micros() as u64);
        let cpu_after_read = cpu_start.and_then(|_| crate::audit::thread_cpu_nanos());
        let read_cpu = read_cpu_start.zip(cpu_after_read).map(|(a,b)|b.saturating_sub(a));
        if let Some(index) = index {
            if result == Err(DecodeFailure::Unavailable) {
                readers.remove(index);
            } else {
                readers.trim_after_read(index);
            }
        }
        let worker_cpu = cpu_start.zip(cpu_start.and_then(|_| crate::audit::thread_cpu_nanos()))
            .map(|(a,b)|b.saturating_sub(a));
        if let Some(audit) = &mut audit {
            let nanos = job_start.unwrap().elapsed().as_nanos() as u64;
            audit.jobs += 1;
            if let Some(cpu) = worker_cpu {
                audit.cpu_clock_jobs += 1;
                audit.worker_cpu_nanos = audit.worker_cpu_nanos.saturating_add(cpu);
                audit.read_cpu_nanos = audit.read_cpu_nanos.saturating_add(read_cpu.unwrap_or(0));
                if opened {audit.open_cpu_nanos = audit.open_cpu_nanos.saturating_add(open_cpu.unwrap_or(0));}
            }
            audit.failures += u64::from(result.is_err());
            audit.decode_nanos += decode_start.unwrap().elapsed().as_nanos() as u64;
            audit.max_job_nanos = audit.max_job_nanos.max(nanos);
            if audit.jobs <= 128 {
                audit.first_128_job_nanos += nanos;
            }
            let us = (nanos / 1000).max(1);
            audit.job_us_log2[(63 - us.leading_zeros() as usize).min(31)] += 1;
            audit.evictions = readers.evictions;
            audit.reader_budget_bytes = readers.byte_limit + readers.storage_bytes;
            audit.peak_retained_reader_bytes = readers.peak_retained_bytes;
            audit.retained_opaque_readers = readers.opaque;
            let bucket = |us: u64| (63 - us.max(1).leading_zeros() as usize).min(31);
            if let Some(us) = queue_us {
                audit.queue_count += 1;
                audit.queue_total_us = audit.queue_total_us.saturating_add(us);
                audit.queue_max_us = audit.queue_max_us.max(us);
                audit.queue_us_log2[bucket(us)] += 1;
            }
            if opened {
                let us = open_us.unwrap();
                audit.open_max_us = audit.open_max_us.max(us);
                audit.open_us_log2[bucket(us)] += 1;
            }
            let us = read_us.unwrap();
            audit.read_max_us = audit.read_max_us.max(us);
            audit.read_us_log2[bucket(us)] += 1;
            if audit.page_probes.len() < 512 {
                audit.page_probes.push(serde_json::json!({
                "asset":format!("{asset:?}"),"page":job.key().index,"frames":job.range().len(),
                "deadline_engine_frame":job.deadline(),"queue_us":queue_us,"reader_cache_hit":!opened,
                "open_us":if opened{open_us}else{Some(0)},"seek_read_decode_us":read_us,
                "worker_total_us":nanos/1000,"success":result.is_ok(),
                "worker_cpu_us":worker_cpu.map(|n|n/1000),
                "seek_read_decode_cpu_us":read_cpu.map(|n|n/1000),
                "open_cpu_us":if opened{open_cpu.map(|n|n/1000)}else{cpu_start.map(|_|0)}
            }));
            } else {
                audit.dropped_page_probes += 1;
            }
        }
        let publish_start = audit.as_ref().map(|_| Instant::now());
        let completed = publish(job, result);
        if let Some(a) = &mut audit {
            let us = publish_start.unwrap().elapsed().as_micros() as u64;
            a.publish_attempts += 1;
            a.publish_total_us += us;
            a.publish_max_us = a.publish_max_us.max(us);
        }
        if let Err(rejected) = completed {
            if rejected.reason != sampler_core::StreamError::Capacity {
                return;
            }
            // The audio side has not drained completions yet: retry later.
            if let Some(audit) = &mut audit {
                audit.reply_backpressure += 1;
            }
            held = Some((rejected.job, rejected.result));
            std::thread::sleep(Duration::from_micros(500));
        }
    }
}

/// A streamed load: the plan's assets hold only heads; `cache` goes to the
/// runtime (`Runtime::with_stream_cache`), which must call
/// `service_streaming` every block with a horizon of at least
/// `report.head_frames` plus the block length: a page is then requested no
/// later than the latency budget before a voice reads it.
pub struct Streamed {
    pub loaded: crate::Loaded,
    pub assets: Vec<Pcm>,
    pub cache: StreamCache,
    pub streamer: Streamer,
    pub report: StreamReport,
}

impl Streamed {
    pub(crate) fn new_sources(loaded:crate::Loaded,mut opened:Opened,kept:Vec<sampler_core::SourceStorage>,canceled:&(dyn Fn()->bool+Sync))->Result<Self,LoadError> {
        if kept.iter().all(|s|s.as_source().stereo().is_some()) {
            let pcm=kept.into_iter().map(|s|match s {
                sampler_core::SourceStorage::Stereo(p)=>Ok(p),
                sampler_core::SourceStorage::Channels(_)=>Err(LoadError::Invalid {
                    path:"logical stream".into(),reason:"stereo stream storage changed layout".into(),
                }),
            }).collect::<Result<Vec<_>,_>>()?;
            return Self::new(loaded,opened,pcm,canceled);
        }
        if canceled(){return Err(LoadError::Canceled);}
        let invalid=|e:io::Error|if canceled(){LoadError::Canceled}else{LoadError::Invalid{path:"logical stream".into(),reason:e.to_string()}};
        let geometry=kept.iter().map(|s|(s.as_source().sample_rate(),s.as_source().frame_count())).collect::<Vec<_>>();
        let frames=geometry.iter().map(|g|g.1).collect::<Vec<_>>();
        let heads=preload_heads(&loaded.instrument,&frames,&opened.layouts);
        opened.report.head_frames=heads.iter().copied().max().unwrap_or(0);
        opened.report.runway_frames=runway_frames(&loaded.instrument,&geometry,opened.rate,&heads,&opened.policy);
        opened.report.kontakt_window_bytes=kontakt_window_bytes(&loaded.instrument,&opened.layouts);
        let logical_ranges=start_ranges_geometry(&loaded.instrument,&geometry,opened.rate,&heads,&opened.policy,|zone|zone_resident(&loaded.instrument,zone));
        // Purged groups are only in the purge-reload table, and only when some group is purged.
        let logical_reload=loaded.instrument.groups.iter().any(|g|g.purged).then(|| {
            let reload=reload_heads(&loaded.instrument,&frames,&opened.layouts);
            start_ranges_geometry(&loaded.instrument,&geometry,opened.rate,&reload,&opened.policy,|_|true)
        });
        let mut physical=Vec::new();let mut ranges=Vec::new();let mut reloads=Vec::new();
        for (index,(source,logical_ranges)) in kept.iter().zip(logical_ranges).enumerate() {source.as_source().physical_dependencies(|pack|{
            physical.push(pack.clone());ranges.push(logical_ranges.clone());
            if let Some(reload)=&logical_reload {reloads.push(reload[index].clone());}
        });}
        let used=physical.iter().map(Pcm::asset_id).collect::<HashSet<_>>();opened.sources.retain(|id,_|used.contains(id));
        let mut pinned=HashSet::new();
        for zone in &loaded.instrument.zones {if zone.group.is_some_and(|g|loaded.instrument.groups[g.0].wavetable.is_some()) {
            if let Some(storage)=zone.asset.and_then(|a|kept.get(a.0)) {storage.as_source().physical_dependencies(|p|{pinned.insert(p.asset_id());});}
        }}
        let mut report=opened.report;
        report.pool_pages=report.pool_pages.checked_mul(voice_pages(&loaded.instrument,&geometry,opened.rate,report.runway_frames,&opened.policy)).ok_or_else(||invalid(io::Error::other("physical pool overflow")))?;
        let (mut cache,worker)=StreamCache::new(report.pool_pages.max(1)).map_err(|e|invalid(io::Error::other(e)))?;
        report.pool_bytes=cache.bytes();report.head_assets=ranges.iter().filter(|r|!r.is_empty()).count();report.head_ranges=ranges.iter().map(Vec::len).sum();report.head_source_frames=ranges.iter().flatten().map(|r|r.len() as u64).sum();
        let (streamer,_)=Streamer::start_with_reload_cancelable(opened.sources,&physical,ranges,logical_reload.is_some().then_some(reloads),None,None,worker,opened.policy.decoders,opened.policy.lazy,opened.policy.head_budget,pinned,opened.policy.allow_cold_fallback,canceled).map_err(invalid)?;
        report.head_bytes=physical.iter().map(Pcm::head_bytes).sum();report.head_fallback=streamer.head_fallback();
        report.required_resident_assets=physical.iter().filter(|p|p.requires_resident()).count();report.required_resident_bytes=physical.iter().filter_map(|p|p.resident_frames().map(|f|f.len()*size_of::<Frame>())).sum();
        cache.set_wake(streamer.threads.iter().map(|t|t.thread().clone()).collect());cache.set_reloader(streamer.reloader.as_ref().map(|t|t.thread().clone()));
        Ok(Self{loaded,assets:physical,cache,streamer,report})
    }
    /// Read the start ranges of `opened` assets that `loaded`'s zones use and
    /// start streaming. `kept` are the assets of `loaded`'s plan, in its order.
    pub(crate) fn new(
        loaded: crate::Loaded,
        opened: Opened,
        kept: Vec<Pcm>,
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Self, LoadError> {
        let Opened {
            logical_sources: _,
            assets,
            sources,
            mut report,
            layouts,
            rate,
            policy,
        } = opened;
        let invalid = |reason: String| LoadError::Invalid {
            path: "stream cache".into(),
            reason,
        };
        let rates: Vec<u32> = kept.iter().map(Pcm::sample_rate).collect();
        let frames: Vec<usize> = kept.iter().map(Pcm::frame_count).collect();
        let starts = zone_start_ranges(&loaded.instrument, &rates, &loaded.scripts);
        let heads = preload_heads(&loaded.instrument, &frames, &layouts);
        report.head_frames = heads.iter().copied().max().unwrap_or(0);
        let geometry: Vec<(u32, usize)> = kept.iter().map(|p| (p.sample_rate(), p.frame_count())).collect();
        report.runway_frames = runway_frames(&loaded.instrument, &geometry, rate, &heads, &policy);
        // The pool holds the voices × packs that `open` counted, each at the
        // pages its largest reach needs (heads are known only now).
        let pool_span = crate::audit::Span::new("stream_page_pool");
        report.pool_pages = report.pool_pages
            .checked_mul(voice_pages(&loaded.instrument, &geometry, rate, report.runway_frames, &policy))
            .ok_or_else(|| invalid("physical pool overflow".into()))?;
        let (mut cache, worker) =
            StreamCache::new(report.pool_pages.max(1)).map_err(|e| invalid(e.to_string()))?;
        report.pool_bytes = cache.bytes();
        drop(pool_span);
        report.kontakt_window_bytes = kontakt_window_bytes(&loaded.instrument, &layouts);
        let ranges = start_ranges(&loaded.instrument, &kept, rate, &heads, &policy);
        // Purged groups' law heads are restored only by a purge reload (see `reload`).
        let reload = loaded.instrument.groups.iter().any(|g| g.purged).then(|| {
            let reload = reload_heads(&loaded.instrument, &frames, &layouts);
            reload_start_ranges(&loaded.instrument, &kept, rate, &reload, &policy)
        });
        // Over budget the law heads are kept whole: the budget is raised to them,
        // and the excess is reported (KONTRA_AUDIT_LOAD) rather than truncated.
        let head_bytes = physical_head_bytes(&kept, &ranges).unwrap_or(usize::MAX);
        // Strict loads (no cold fallback) keep the configured budget and fail over it.
        let head_budget = raised_head_budget(&policy, head_bytes);
        report.head_assets = ranges.iter().filter(|r| !r.is_empty()).count();
        report.head_ranges = ranges.iter().map(Vec::len).sum();
        report.head_source_frames = ranges.iter().flatten().map(|r| r.len() as u64).sum();
        let pinned_wavetables = loaded
            .instrument
            .zones
            .iter()
            .filter(|zone| {
                zone.group
                    .is_some_and(|g| loaded.instrument.groups[g.0].wavetable.is_some())
            })
            .filter_map(|zone| zone.asset.map(|a| kept[a.0].asset_id()))
            .collect();
        audit_start_ranges(&loaded.instrument, &loaded.plan, &kept, &ranges, &pinned_wavetables, rate, &heads, &layouts, &loaded.scripts, &policy, canceled)
            .map_err(|e| if canceled() { LoadError::Canceled } else { invalid(e.to_string()) })?;
        let hint_recipe = InitialStartHints::new(&loaded.instrument, &loaded.plan, &kept, rate, &heads, &starts, &policy, canceled)
            .map_err(|e| if canceled() { LoadError::Canceled } else { invalid(e.to_string()) })?;
        if std::env::var_os("KONTRA_AUDIT_LOAD").is_some() {
            eprintln!("stream_initial_start_hints {}", serde_json::json!({
                "controller_window_zones":hint_recipe.controller_zones(),
                "unsupported_start_zones":hint_recipe.unsupported,
                "unbounded_script_offsets":loaded.scripts.iter().filter(|s| s.unbounded_line.is_some()).count(),
                "controller_loop_zones_omitted":hint_recipe.controller_loops,
                "scope":"one-shot bound initial-state prediction; future input uses strict demand"
            }));
        }
        let hints = (hint_recipe.eligible != 0).then(|| hint_recipe.ranges(canceled).map(|ranges| HintFallback {
            head_frames: report.head_frames, ranges,
        })).transpose().map_err(|e| if canceled() { LoadError::Canceled } else { invalid(e.to_string()) })?;
        let (streamer, bytes) = Streamer::start_with_reload_cancelable(
            sources,
            &kept,
            ranges,
            reload,
            None,
            hints,
            worker,
            policy.decoders,
            policy.lazy,
            head_budget,
            pinned_wavetables,
            policy.allow_cold_fallback,
            canceled,
        )
        .map_err(|e| {
            if canceled() {
                LoadError::Canceled
            } else {
                invalid(e.to_string())
            }
        })?;
        let _=bytes;
        report.head_bytes = kept.iter().map(Pcm::head_bytes).sum();
        report.required_resident_assets=kept.iter().filter(|p|p.requires_resident()).count();
        report.required_resident_bytes=kept.iter().filter(|p|p.requires_resident()).map(|p|p.resident_frames().map_or(0,|f|f.len()*size_of::<Frame>())).sum();
        report.head_fallback = streamer.head_fallback();
        cache.set_wake(
            streamer
                .threads
                .iter()
                .map(|t| t.thread().clone())
                .collect(),
        );
        cache.set_reloader(streamer.reloader.as_ref().map(|t| t.thread().clone()));
        drop(assets);
        Ok(Self {
            loaded,
            assets: kept,
            cache,
            streamer,
            report,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A start range the resident budget did not cover is clipped to the covered part (it
    /// streams on demand), not an error that stops the instrument from loading.
    #[test]
    fn start_ranges_outside_the_requested_coverage_are_clipped() {
        let clipped = intersect_ranges(&[0..100, 150..200], &[50..120, 160..170]);
        assert_eq!(clipped, vec![50..100, 160..170]);
        assert!(intersect_ranges(&[0..10], &[]).is_empty());
    }

    #[test]
    fn cold_fallback_head_budget_is_clamped_to_physical_limit() {
        let mut policy = StreamPolicy { head_budget: 1 << 10, ..Default::default() };
        // Uncapped: the raise reaches the law heads, as before.
        assert_eq!(raised_head_budget(&policy, 1 << 20), 1 << 20);
        // Capped: a head above the limit gets the limit, so it takes the fallback tiers.
        policy.physical_limit = 1 << 19;
        assert_eq!(raised_head_budget(&policy, 1 << 20), 1 << 19);
        // Strict loads never raise.
        policy.allow_cold_fallback = false;
        assert_eq!(raised_head_budget(&policy, 1 << 20), 1 << 10);
    }

    /// A mono 24-bit 48 kHz AIFF with four frames. FORM headers must parse as AIFF,
    /// not fall through to the 16-bit stereo fallback.
    #[test]
    fn aiff_header_layout_is_mono_24_bit() {
        let frames = 4u32;
        let mut blob = Vec::new();
        blob.extend_from_slice(b"FORM");
        blob.extend_from_slice(&(4 + 8 + 18 + 8 + 8 + frames * 3).to_be_bytes());
        blob.extend_from_slice(b"AIFF");
        blob.extend_from_slice(b"COMM");
        blob.extend_from_slice(&18u32.to_be_bytes());
        blob.extend_from_slice(&1u16.to_be_bytes()); // channels
        blob.extend_from_slice(&frames.to_be_bytes());
        blob.extend_from_slice(&24u16.to_be_bytes()); // sample size
        // 48000 Hz as an 80-bit IEEE extended float.
        blob.extend_from_slice(&[0x40, 0x0E, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]);
        blob.extend_from_slice(b"SSND");
        blob.extend_from_slice(&(8 + frames * 3).to_be_bytes());
        blob.extend_from_slice(&0u32.to_be_bytes()); // offset
        blob.extend_from_slice(&0u32.to_be_bytes()); // block size
        blob.extend_from_slice(&vec![0u8; (frames * 3) as usize]);
        let layout = layout_from_head(&blob).expect("AIFF header parses");
        assert_eq!(layout, FrameLayout { channels: 1, bits: 24 });
    }

    #[test]
    fn cached_numeric_headers_keep_bounded_real_latency_probes() {
        struct Cached(Arc<AtomicU64>);
        impl AssetSource for Cached {
            fn header(&self) -> Option<(u32, usize)> {
                Some((48000, 64))
            }
            fn open(&self) -> io::Result<SampleReader> {
                self.0.fetch_add(1, Ordering::Relaxed);
                Ok(SampleReader::custom(48000, 64, |_, out| {
                    std::thread::sleep(Duration::from_millis(1));
                    out.fill([0.5; 2]);
                    Ok(())
                }))
            }
        }
        let opens = Arc::new(AtomicU64::new(0));
        let sources = (0..8)
            .map(|_| {
                (
                    Arc::new(Cached(opens.clone())) as Arc<dyn AssetSource>,
                    std::path::Path::new("cached-numeric"),
                )
            })
            .collect();
        let opened = Streamer::open(
            sources,
            48000,
            &StreamPolicy::default(),
            3,
            &|| false,
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(opens.load(Ordering::Relaxed), 3);
        assert_eq!(opened.report.latency_samples, 3);
        assert!(opened.report.latency_p95 >= Duration::from_millis(1));
        assert_eq!(opened.assets.len(), 8);
    }

    #[test]
    fn parallel_head_preparation_is_bounded_and_preserves_every_authored_asset_and_direction() {
        struct Numeric {
            base: f32,
            active: Arc<AtomicUsize>,
            peak: Arc<AtomicUsize>,
        }
        impl AssetSource for Numeric {
            fn open(&self) -> io::Result<SampleReader> {
                let n = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(n, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(2));
                self.active.fetch_sub(1, Ordering::SeqCst);
                let base = self.base;
                Ok(SampleReader::custom(48000, 128, move |start, out| {
                    for (i, f) in out.iter_mut().enumerate() {
                        let v = base + (start + i) as f32 / 65536.;
                        *f = [v, -v];
                    }
                    Ok(())
                }))
            }
        }
        for requested in [1, 4, 32] {
            let active = Arc::new(AtomicUsize::new(0));
            let peak = Arc::new(AtomicUsize::new(0));
            let assets: Vec<_> = (0..16)
                .map(|_| Pcm::streamed(48000, 128).unwrap())
                .collect();
            let sources = assets
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    (
                        p.asset_id(),
                        Arc::new(Numeric {
                            base: i as f32 / 32.,
                            active: active.clone(),
                            peak: peak.clone(),
                        }) as Arc<dyn AssetSource>,
                    )
                })
                .collect();
            let (_, worker) = StreamCache::new(1).unwrap();
            let (streamer, _) = Streamer::start(
                sources,
                &assets,
                vec![vec![8..56]; 16],
                worker,
                requested,
                false,
                16 * 48 * 8,
                HashSet::new(),
            )
            .unwrap();
            assert_eq!(streamer.threads.len(), requested.clamp(1, 8));
            assert!(peak.load(Ordering::SeqCst) <= requested.clamp(1, 8));
            if requested > 1 {
                assert!(peak.load(Ordering::SeqCst) > 1);
            }
            assert!(streamer.head_readiness(&assets).complete());
            for (i, pcm) in assets.iter().enumerate() {
                let golden = Pcm::new(
                    48000,
                    (0..128)
                        .map(|frame| {
                            let v = i as f32 / 32. + frame as f32 / 65536.;
                            [v, -v]
                        })
                        .collect(),
                )
                .unwrap();
                let render = |pcm: Pcm| {
                    let plan = sampler_core::Prepared::new(
                        48000,
                        vec![pcm],
                        vec![sampler_core::Region {
                            sample: 0,
                            key_low: 60,
                            key_high: 60,
                            root_key: None,
                            velocity_low: 0.,
                            velocity_high: 1.,
                            gain: 1.,
                            envelope: Default::default(),
                            playback: sampler_core::Playback {
                                start: 16,
                                end: Some(48),
                                direction: if i % 2 == 1 {
                                    sampler_core::Direction::Reverse
                                } else {
                                    sampler_core::Direction::Forward
                                },
                                ..Default::default()
                            },
                        }],
                        1,
                    )
                    .unwrap();
                    let limits = sampler_core::Limits::for_plan(&plan, 2, 1);
                    let (cache,_worker)=StreamCache::new(4).unwrap();
                    let mut runtime = sampler_core::Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
                    runtime
                        .trigger(
                            sampler_core::Input {
                                protocol: sampler_core::Protocol::Native,
                                port: 0,
                                group: 0,
                                channel: 0,
                                key: 60,
                                external_id: None,
                            },
                            60,
                            1.,
                        )
                        .unwrap();
                    assert_eq!(runtime.voice_count(),1);
                    let mut frames = [[0.; 2]; 64];
                    runtime.render(&mut frames).unwrap();
                    assert!(frames.iter().flatten().any(|v|v.abs()>0.0001),"golden source must actually sound");
                    frames.map(|f| f.map(f32::to_bits))
                };
                assert_eq!(
                    render(pcm.clone()),
                    render(golden),
                    "requested={requested} asset={i}"
                );
            }
        }
    }

    fn identity_wav() -> Vec<u8> {
        let data_bytes = 50000u32 * 4;
        let mut v = b"RIFF".to_vec();
        v.extend((36 + data_bytes).to_le_bytes());
        v.extend(b"WAVEfmt ");
        v.extend(16u32.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(2u16.to_le_bytes());
        v.extend(48000u32.to_le_bytes());
        v.extend(192000u32.to_le_bytes());
        v.extend(4u16.to_le_bytes());
        v.extend(16u16.to_le_bytes());
        v.extend(b"data");
        v.extend(data_bytes.to_le_bytes());
        for i in 0..50000 {
            v.extend(((i % 30000) as i16).to_le_bytes());
            v.extend((-(i % 30000) as i16).to_le_bytes());
        }
        v
    }

    #[test]
    #[cfg(unix)]
    fn stream_sources_preserve_admission_across_eviction_rewrite_and_retained_handles() {
        let dir =
            std::env::temp_dir().join(format!("kontakt-stream-identity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let original = identity_wav();
        let full = crate::decode(&original).unwrap();
        for (retained, replace) in [(false, false), (false, true), (true, false), (true, true)] {
            let path = dir.join(format!("{retained}-{replace}.wav"));
            std::fs::write(&path, &original).unwrap();
            let mut source = crate::Samples::new(&dir).source(&path).unwrap();
            if retained {
                source.handle = Some(Arc::new(File::open(&path).unwrap()));
            }
            let mut reader = source.open().unwrap();
            let mut out = [[0.; 2]; 32];
            reader.read(0, &mut out).unwrap();
            assert_eq!(out, full.frames[..32]);
            let stamp = std::fs::metadata(&path).unwrap().modified().unwrap();
            let mut changed = original.clone();
            changed[160000..160032].fill(127);
            let written = if replace {
                dir.join("replacement.wav")
            } else {
                path.clone()
            };
            std::fs::write(&written, &changed).unwrap();
            File::options()
                .write(true)
                .open(&written)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(stamp))
                .unwrap();
            if replace {
                std::fs::rename(written, &path).unwrap();
            }
            if !retained || !replace {
                assert!(
                    source.open_stream().is_err(),
                    "a reopened revision must reject changed source"
                );
                assert!(
                    reader.read(40000, &mut out).is_err(),
                    "physical refill must validate its original handle"
                );
            } else {
                // A retained original FD can either fail conservatively on its
                // change time or return original bytes after a path rename.
                if let Ok(mut reopened) = source.open_stream() {
                    reopened.read(40000, &mut out).unwrap();
                    assert_eq!(out, full.frames[40000..40032]);
                }
                if reader.read(40000, &mut out).is_ok() {
                    assert_eq!(out, full.frames[40000..40032]);
                }
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    struct HeadFixture {
        opens: Arc<AtomicU64>,
        cancel: Option<Arc<AtomicBool>>,
        frames: usize,
        wide: bool,
    }
    impl AssetSource for HeadFixture {
        fn open(&self) -> io::Result<SampleReader> {
            self.opens.fetch_add(1, Ordering::Relaxed);
            let (cancel, wide) = (self.cancel.clone(), self.wide);
            Ok(SampleReader::custom(48000, self.frames, move |_, out| {
                out.fill(if wide {
                    [f32::from_bits(0x3f123457), f32::from_bits(0x3f234569)]
                } else {
                    [0.5; 2]
                });
                if let Some(c) = &cancel {
                    c.store(true, Ordering::Relaxed);
                }
                Ok(())
            }))
        }
    }

    #[test]
    fn eager_heads_enforce_exact_lossless_budget_without_partial_publication() {
        for decoders in [1, 4] {
            for (wide, budget, ok, opens_expected) in [
                (false, 0, false, 0),
                (false, 63, false, 0),
                (false, 64, true, 2),
                (true, 255, false, 2),
                (true, 256, true, 2),
            ] {
                let assets: Vec<_> = (0..2).map(|_| Pcm::streamed(48000, 64).unwrap()).collect();
                let opens = Arc::new(AtomicU64::new(0));
                let sources = assets
                    .iter()
                    .map(|p| {
                        (
                            p.asset_id(),
                            Arc::new(HeadFixture {
                                opens: opens.clone(),
                                cancel: None,
                                frames: 64,
                                wide,
                            }) as Arc<dyn AssetSource>,
                        )
                    })
                    .collect();
                let (_, worker) = StreamCache::new(1).unwrap();
                let result = Streamer::start(
                    sources,
                    &assets,
                    vec![vec![8..16, 48..56]; 2],
                    worker,
                    decoders,
                    false,
                    budget,
                    HashSet::new(),
                );
                assert_eq!(result.is_ok(), ok, "wide={wide} budget={budget}");
                assert_eq!(opens.load(Ordering::Relaxed), opens_expected);
                if ok {
                    let (streamer, bytes) = result.unwrap();
                    assert_eq!(bytes, if wide { 256 } else { 64 });
                    assert_eq!(assets.iter().map(Pcm::head_bytes).sum::<usize>(), bytes);
                    assert!(streamer.head_readiness(&assets).complete());
                } else {
                    let error = result.err().unwrap();
                    let budget_error = error
                        .get_ref()
                        .unwrap()
                        .downcast_ref::<HeadBudgetExceeded>()
                        .unwrap();
                    assert_eq!(budget_error.budget_bytes, budget);
                    assert!(budget_error.required_at_least_bytes > budget);
                    assert!(
                        assets.iter().all(|p| p.head_bytes() == 0),
                        "failure cannot publish an earlier staged asset"
                    );
                }
            }
        }
    }

    #[test]
    fn cold_fallback_is_opt_in_typed_and_preserves_mandatory_residency() {
        // (allow, pinned, budget, success, partial, resident): a pinned range over
        // the budget streams under cold fallback instead of failing the load.
        for (allow,pinned,budget,success,partial,resident) in [
            (false,false,80,false,false,false),
            (true,false,80,true,true,false),
            (true,true,80,true,true,true),
            (true,true,63,true,true,false),
            (false,true,63,false,false,false),
        ] {
            let pcm=Pcm::streamed(48000,8192).unwrap();
            let other=Pcm::streamed(48000,8192).unwrap();
            let assets=vec![pcm,other];
            let sources=assets.iter().map(|p|(p.asset_id(),Arc::new(HeadFixture{opens:Arc::new(AtomicU64::new(0)),cancel:None,frames:8192,wide:false}) as Arc<dyn AssetSource>)).collect();
            let (_,worker)=StreamCache::new(1).unwrap();
            let pinned_ids=if pinned {HashSet::from([assets[0].asset_id()])} else {HashSet::new()};
            let result=Streamer::start_cancelable(sources,&assets,vec![vec![0..32];2],worker,2,false,budget,pinned_ids,allow,&||false);
            assert_eq!(result.is_ok(),success,"allow={allow} pinned={pinned} budget={budget}");
            if success {
                let (streamer,bytes)=result.unwrap();
                assert_eq!(streamer.head_fallback().is_some(),partial);
                assert_eq!(bytes,if resident{64}else{0});
                assert_eq!(streamer.head_budget_status().held_bytes,bytes);
                assert!(streamer.head_budget_status().peak_bytes<=budget);
                assert_eq!(assets[0].resident_covers(0..32),resident);
                assert!(!assets[1].resident_covers(0..32));
                assert!(!streamer.head_readiness(&assets).complete());
            } else {
                assert!(result.err().unwrap().get_ref().is_some_and(|e|e.is::<HeadBudgetExceeded>()));
                assert!(assets.iter().all(|p|p.head_bytes()==0));
            }
        }
    }

    #[test]
    fn purged_group_head_is_restored_by_its_purge_reload_not_at_load() {
        // Asset 0 is used only by a purged group: no load range, a law range for reload.
        // Asset 1 is resident and loads its head at start.
        let assets = vec![Pcm::streamed(48000, 8192).unwrap(), Pcm::streamed(48000, 8192).unwrap()];
        let sources = assets.iter()
            .map(|p| (p.asset_id(), Arc::new(HeadFixture { opens: Arc::new(AtomicU64::new(0)), cancel: None, frames: 8192, wide: false }) as Arc<dyn AssetSource>))
            .collect();
        let (_, worker) = StreamCache::new(1).unwrap();
        let (streamer, _) = Streamer::start_with_reload_cancelable(
            sources, &assets, vec![vec![], vec![0..32]], Some(vec![vec![0..64], vec![0..32]]), None, None,
            worker, 2, false, 1 << 20, HashSet::new(), false, &|| false,
        ).unwrap();
        assert_eq!(assets[0].head_bytes(), 0, "a purged group gets no head at load");
        assert!(assets[1].resident_covers(0..32));
        // The purge reload restores the purged group's law head from the reload table.
        assets[0].mark_cold();
        assert_eq!(streamer.reload(&assets).unwrap(), 1);
        assert!(assets[0].resident_covers(0..64), "purge reload loads the law head");
    }

    #[test]
    fn constant_onset_windows_clip_reverse_fractional_guards_and_silent_offsets() {
        for reverse in [false, true] {
            for step in [0.25, 1., 1.78125, 16.] {
                let playback = ir::Playback { start: 37, end: Some(150), reverse, ..Default::default() };
                let radius = sampler_core::read_radius(step) as usize;
                for head in [0, 1, 7, 1024] {
                    let window = constant_start_window(&playback, 200, 10, step, head).unwrap();
                    let onset = if reverse { 139 } else { 47 };
                    assert!(window.contains(&onset));
                    assert!(window.start >= 37 && window.end <= 150);
                    assert!(window.start <= onset.saturating_sub(radius).max(37));
                    assert!(window.end >= (onset + radius + 1).min(150));
                }
                assert!(constant_start_window(&playback, 200, 113, step, 128).is_none());
                assert!(constant_start_window(&playback, 100, 63, step, 128).is_none());
            }
        }
    }

    #[test]
    fn constant_onset_geometry_deduplicates_asset_revisions_without_admitting_pcm() {
        use sampler_core::{Envelope, ModProgram, ModRoute, ModSource, ModTarget, Playback, Prepared, Region};
        let pcm = Pcm::streamed(48000, 8192).unwrap();
        let other = Pcm::streamed(48000, 8192).unwrap();
        let assets = vec![pcm.clone(), pcm.clone(), other.clone()];
        let raw = vec![vec![0..20], vec![10..30], vec![0..20]];
        let merged = physical_start_ranges(&assets, &raw);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[&pcm.asset_id()], [0..30]);
        assert_eq!(merged[&other.asset_id()], [0..20]);
        let mut instrument = ir::Instrument::default();
        instrument.zones = (0..3).map(|sample| {
            let mut z = ir::Zone::new(ir::AssetRef(sample));
            z.routes.push(ir::RouteRef(0));
            z.playback.start_range = 1000;
            z.pitch = ir::KeyTracking::Fixed;
            z
        }).collect();
        instrument.routes.push(ir::Route::new(ir::ModulatorRef(0), ir::Target::SampleStart, ir::Depth::Normalized(0.25)));
        instrument.modulators.push(ir::Modulator { scope: ir::Scope::Voice, source: ir::ModulationSource::Constant });
        let regions = (0..3).map(|sample| Region { sample, key_low: 60, key_high: 60,
            root_key: None, velocity_low: 0., velocity_high: 1., gain: 1.,
            envelope: Envelope::default(), playback: Playback::default() }).collect();
        let prepared = Prepared::new(48000, assets.clone(), regions, 3).unwrap()
            .with_voice_modulation(vec![ModProgram { sources: vec![ModSource::Constant],
                routes: vec![ModRoute::new(0, ModTarget::SampleStart, 0.25)], ..Default::default() }],
                vec![Some(0); 3], vec![1000; 3]).unwrap();
        let policy = StreamPolicy { head_budget: 1, ..Default::default() };
        let base = start_ranges(&instrument, &assets, 48000, &law_heads(&instrument, 64), &policy);
        let audit = constant_start_geometry(&instrument, &prepared, &assets, &base, &HashSet::new(), 48000, &law_heads(&instrument, 64), &policy, &||false).unwrap();
        assert_eq!(audit["constant_offset_zones"], 3);
        assert_eq!(audit["hint_assets"], 2);
        assert_eq!(audit["hint_ranges"], 2);
        assert!(audit["hint_extra_source_frames"].as_u64().unwrap() > 0);
        assert!(audit["base_plus_hint_payload_lower_bytes"].as_u64().unwrap() > 1);
        assert!(assets.iter().all(|p| p.head_bytes() == 0), "audit must not decode/admit even above quota");
        assert_eq!(constant_start_geometry(&instrument, &prepared, &assets, &base, &HashSet::new(), 48000, &law_heads(&instrument, 64), &policy, &||true).unwrap_err().kind(), io::ErrorKind::Interrupted);
        instrument.zones[0].playback.looping = ir::Looping::Continuous(ir::LoopRange { start: 100, end: 1000, crossfade: ir::Span::Frames(0), alternating: true });
        let audit = constant_start_geometry(&instrument, &prepared, &assets, &base, &HashSet::new(), 48000, &law_heads(&instrument, 64), &policy, &||false).unwrap();
        assert_eq!(audit["loop_geometry_omitted_zones"], 1);
        assert_eq!(audit["nonloop_window_zones"], 2);
    }

    #[test]
    fn over_budget_resident_source_degrades_to_the_cheapest_starts() {
        let assets = vec![Pcm::streamed(48000,16).unwrap(), Pcm::pending_resident(48000,64).unwrap()];
        let opens = Arc::new(AtomicU64::new(0));
        let sources = HashMap::from([
            (assets[0].asset_id(), Arc::new(HeadFixture{opens:opens.clone(),cancel:None,frames:16,wide:false}) as Arc<dyn AssetSource>),
            (assets[1].asset_id(), Arc::new(HeadFixture{opens:opens.clone(),cancel:None,frames:64,wide:false}) as Arc<dyn AssetSource>),
        ]);
        let (_,worker) = StreamCache::new(1).unwrap();
        // The resident source needs 512 bytes, the budget is 256: the mandatory-only attempt
        // overflows, and the cheap streamed head (16 frames) is admitted in its place.
        let (_streamer,bytes) = Streamer::start_with_fallbacks_cancelable(sources,&assets,
            vec![vec![0..16],vec![0..64]],None,worker,1,false,256,HashSet::new(),true,&||false)
            .expect("an over-budget resident source must degrade, not fail the load");
        assert!(bytes > 0 && bytes <= 256, "admitted {bytes} bytes");
        assert!(assets[0].head_frames() > 0);
        assert!(assets[1].resident_frames().is_none());
    }

    #[test]
    fn head_floor_and_cost_bracket_the_stored_form_of_a_start() {
        // Two start ranges of one streamed source: 4288 frames. The floor is the narrowest
        // stored form (2 bytes a frame), the cost the widest (an f32 frame, 8 bytes).
        let pcm = Pcm::streamed(48000, 8192).unwrap();
        let ranges = [2000..4096, 6000..8192];
        assert_eq!(head_floor_bytes(&pcm, &ranges).unwrap(), 4288 * 2);
        assert_eq!(head_cost(&pcm, &ranges).unwrap(), 4288 * size_of::<Frame>());
    }

    #[test]
    fn onset_fallback_keeps_reverse_ranges_and_mandatory_sources_atomic() {
        for (budget, allowed, expected) in [(672, true, Some(true)), (671, true, Some(false)),
            (575, true, None), (672, false, None)] {
            for decoders in [1, 4] {
                let assets = vec![Pcm::streamed(48000,8192).unwrap(), Pcm::streamed(48000,8192).unwrap(),
                    Pcm::streamed(48000,8192).unwrap(), Pcm::pending_resident(48000,64).unwrap()];
                let sources = assets.iter().map(|pcm| (pcm.asset_id(), Arc::new(HeadFixture {
                    opens: Arc::new(AtomicU64::new(0)), cancel: None, frames: pcm.frame_count(), wide: false,
                }) as Arc<dyn AssetSource>)).collect();
                let ranges = vec![vec![0..2048],vec![2000..4096,6000..8192],vec![0..32],vec![0..64]];
                // Two distinct authored starts of one asset, including a reverse
                // suffix. This is not a generic prefix truncation of its union.
                let onsets = vec![vec![0..16],vec![2000..2016,8176..8192],vec![0..4],vec![0..64]];
                let (_,worker) = StreamCache::new(4).unwrap();
                let result = Streamer::start_with_onsets_cancelable(sources,&assets,ranges,Some(onsets),
                    worker,decoders,false,budget,HashSet::from([assets[2].asset_id()]),allowed,&||false);
                if let Some(onset_ready) = expected {
                    let (streamer,bytes) = result.unwrap();
                    assert_eq!(bytes,if onset_ready {672} else {576});
                    assert!(streamer.head_fallback().is_some());
                    assert!(streamer.head_budget_status().held_bytes<=budget);
                    assert!(streamer.head_budget_status().peak_bytes<=budget);
                    assert_eq!(streamer.onset_readiness(&assets).complete(),onset_ready);
                    assert!(!streamer.head_readiness(&assets).complete(),"full offset request remains truthful");
                    assert!(assets[2].resident_covers(0..32),"wavetable ignores shortened onset alternative");
                    assert_eq!(assets[3].resident_frames().unwrap().len(),64);
                    assert_eq!(assets[0].resident_covers(0..16),onset_ready);
                    assert_eq!(assets[1].resident_covers(2000..2016),onset_ready);
                    assert_eq!(assets[1].resident_covers(8176..8192),onset_ready);
                    assert!(!assets[0].resident_covers(0..2048));
                    assert!(!assets[1].resident_covers(6000..8192));
                    // An expanded reload cannot replace an admitted attack with
                    // a partial/over-budget generation or lose its quota lease.
                    assets[0].mark_cold();
                    assert!(reload(&assets, &streamer.sources, &streamer.ranges, streamer.head_budget, &streamer.head_mutation).is_err());
                    assert_eq!(assets[0].resident_covers(0..16),onset_ready);
                    assert!(streamer.head_budget_status().held_bytes<=budget);
                } else if allowed {
                    // The over-budget required source streams on demand; the load keeps what fits.
                    let (streamer, bytes) = result.expect("an over-budget required source must degrade, not fail");
                    assert!(bytes <= budget, "admitted {bytes} of {budget}");
                    assert!(streamer.head_budget_status().held_bytes <= budget);
                    assert!(assets[3].resident_frames().is_none());
                } else {
                    let error = result.err().unwrap();
                    assert!(error.get_ref().is_some_and(|e|e.is::<HeadBudgetExceeded>()));
                    assert!(assets.iter().all(|p|p.head_bytes()==0 && p.resident_frames().is_none()),
                        "no optional or mandatory source publishes on total startup failure");
                }
            }
        }
    }

    #[test]
    fn onset_retry_never_converts_source_failure_or_cancellation_to_cold_success() {
        for cancel in [false,true] {
            let flag = Arc::new(AtomicBool::new(false));
            let pcm = Pcm::streamed(48000,8192).unwrap();
            struct Failure { cancel: bool, flag: Arc<AtomicBool> }
            impl AssetSource for Failure {
                fn open(&self)->io::Result<SampleReader> {
                    let (cancel,flag) = (self.cancel,self.flag.clone());
                    Ok(SampleReader::custom(48000,8192,move|_,out| {
                        if !cancel {return Err(io::Error::new(io::ErrorKind::InvalidData,"authored source failure"));}
                        out.fill([0.5;2]);flag.store(true,Ordering::Release);Ok(())
                    }))
                }
            }
            let sources = HashMap::from([(pcm.asset_id(),Arc::new(Failure{cancel,flag:flag.clone()}) as Arc<dyn AssetSource>)]);
            let (_,worker) = StreamCache::new(1).unwrap();
            let result = Streamer::start_with_onsets_cancelable(sources,std::slice::from_ref(&pcm),
                vec![vec![0..8192]],Some(vec![vec![0..16]]),worker,1,false,32,HashSet::new(),true,
                &||flag.load(Ordering::Acquire));
            let error = result.err().expect("source/cancel failure must propagate");
            assert_eq!(error.kind(),if cancel {io::ErrorKind::Interrupted}else{io::ErrorKind::InvalidData});
            assert_eq!(pcm.head_bytes(),0);
        }
    }

    #[test]
    fn mandatory_full_sources_are_not_published_when_later_source_fails_or_read_is_canceled() {
        struct Fixture { cancel: Arc<AtomicBool>, fail: bool, cancel_read: bool }
        impl AssetSource for Fixture {
            fn open(&self)->io::Result<SampleReader> {
                let (cancel,fail,cancel_read)=(self.cancel.clone(),self.fail,self.cancel_read);
                Ok(SampleReader::custom(48000,8192,move|_,out| {
                    if fail {return Err(io::Error::new(io::ErrorKind::InvalidData,"independent fixture source failure"));}
                    out.fill([0.25;2]);
                    if cancel_read {cancel.store(true,Ordering::Release);}
                    Ok(())
                }))
            }
        }
        for cancel_read in [false,true] {
            let cancel=Arc::new(AtomicBool::new(false));
            let assets=vec![Pcm::pending_resident(48000,8192).unwrap(),Pcm::streamed(48000,8192).unwrap()];
            let sources=HashMap::from([
                (assets[0].asset_id(),Arc::new(Fixture {cancel:cancel.clone(),fail:false,cancel_read}) as Arc<dyn AssetSource>),
                (assets[1].asset_id(),Arc::new(Fixture {cancel:cancel.clone(),fail:true,cancel_read:false}) as Arc<dyn AssetSource>),
            ]);
            let (_cache,worker)=StreamCache::new(8).unwrap();
            let result=Streamer::start_cancelable(sources,&assets,vec![vec![0..16];2],worker,1,false,8192*8+32,HashSet::new(),true,&||cancel.load(Ordering::Acquire));
            assert!(result.is_err(),"neither a source failure nor cancellation can become a cold fallback success");
            assert!(assets[0].resident_frames().is_none());
            assert_eq!(assets[1].head_bytes(),0);
            assert_eq!(cancel.load(Ordering::Acquire),cancel_read);
        }
    }
    #[test]
    fn exact_packing_failure_drops_all_staged_heads_before_mandatory_fallback() {
        let assets=vec![Pcm::streamed(48000,16).unwrap(),Pcm::streamed(48000,16).unwrap()];
        let opened:[Arc<AtomicU64>;2]=std::array::from_fn(|_|Arc::new(AtomicU64::new(0)));
        let sources=assets.iter().enumerate().map(|(i,pcm)|(pcm.asset_id(),Arc::new(HeadFixture{opens:opened[i].clone(),cancel:None,frames:16,wide:i==1}) as Arc<dyn AssetSource>)).collect();
        let (_,worker)=StreamCache::new(1).unwrap();
        // The two mono16 minima fit 64 bytes. The first mono head stages 32;
        // the second is stereo-f32 (128), forcing exact-byte admission failure.
        let (streamer,bytes)=Streamer::start_cancelable(sources,&assets,vec![vec![0..16];2],worker,1,false,64,HashSet::from([assets[0].asset_id()]),true,&||false).unwrap();
        assert!(streamer.head_fallback().is_some());
        assert_eq!((opened[0].load(Ordering::Relaxed),opened[1].load(Ordering::Relaxed)),(2,1));
        assert_eq!(bytes,32);
        assert_eq!(streamer.head_budget_status().held_bytes,32,"no abandoned staged quota survives the retry");
        assert_eq!(streamer.head_budget_status().peak_bytes,32);
        assert!(assets[0].resident_covers(0..16));
        assert_eq!(assets[1].head_bytes(),0);
        assert!(!streamer.head_readiness(&assets).complete());
    }

    #[test]
    fn cold_fallback_does_not_swallow_source_or_metadata_failures() {
        for wrong_header in [false,true] {
            struct Failed(bool);
            impl AssetSource for Failed {
                fn open(&self)->io::Result<SampleReader> {
                    if self.0 {Ok(SampleReader::custom(44100,64,|_,out|{out.fill([0.5;2]);Ok(())}))}
                    else {Err(io::Error::new(io::ErrorKind::InvalidData,"admitted source changed"))}
                }
            }
            let pcm=Pcm::streamed(48000,64).unwrap();
            let sources=HashMap::from([(pcm.asset_id(),Arc::new(Failed(wrong_header)) as Arc<dyn AssetSource>)]);
            let (_,worker)=StreamCache::new(1).unwrap();
            let error=Streamer::start_cancelable(sources,std::slice::from_ref(&pcm),vec![vec![0..32]],worker,1,false,1024,HashSet::new(),true,&||false).err().unwrap();
            assert_eq!(error.kind(),io::ErrorKind::InvalidData);
            assert!(!error.get_ref().is_some_and(|e|e.is::<HeadBudgetExceeded>()));
            assert_eq!(pcm.head_bytes(),0);
            assert_eq!(pcm.head_preparer().budget_status().unwrap().held_bytes,0);
        }
    }

    #[test]
    fn eager_heads_reject_short_source_and_cancel_before_publication() {
        for action in ["short", "cancel_before", "cancel_during"] {
            let pcm = Pcm::streamed(48000, 8192).unwrap();
            let cancel = Arc::new(AtomicBool::new(action == "cancel_before"));
            let opens = Arc::new(AtomicU64::new(0));
            let source = HeadFixture {
                opens: opens.clone(),
                cancel: (action == "cancel_during").then(|| cancel.clone()),
                frames: if action == "short" { 4096 } else { 8192 },
                wide: false,
            };
            let sources =
                HashMap::from([(pcm.asset_id(), Arc::new(source) as Arc<dyn AssetSource>)]);
            let (_, worker) = StreamCache::new(1).unwrap();
            let result = Streamer::start_cancelable(
                sources,
                std::slice::from_ref(&pcm),
                vec![vec![0..8192]],
                worker,
                1,
                false,
                8192 * 2,
                HashSet::new(),
                false,
                &|| cancel.load(Ordering::Relaxed),
            );
            assert_eq!(
                result.err().unwrap().kind(),
                if action == "short" {
                    io::ErrorKind::InvalidData
                } else {
                    io::ErrorKind::Interrupted
                }
            );
            assert_eq!(
                opens.load(Ordering::Relaxed),
                u64::from(action != "cancel_before")
            );
            assert_eq!(pcm.head_bytes(), 0);
        }
    }

    fn cache_fixture_reader() -> SampleReader {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor/ncw/tests/data/16-bit-mono.ncw");
        let source = Source {
            identity: Arc::new(
                crate::FileIdentity::capture(&std::fs::metadata(&path).unwrap()).unwrap(),
            ),
            size: std::fs::metadata(&path).unwrap().len(),
            path,
            offset: 0,
            key: None,
            handle: None,
            header: None,
        };
        SampleReader::open(&source).unwrap()
    }

    #[test]
    fn ncw_block_error_names_the_sample_file() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor/ncw/tests/data/16-bit-mono.ncw");
        let mut bytes = std::fs::read(&fixture).unwrap();
        // Break the first block signature (BE 0x160C9A3E) past the 120-byte header.
        let at = bytes
            .windows(4)
            .skip(120)
            .position(|w| w == [0x16, 0x0C, 0x9A, 0x3E])
            .map(|i| i + 120)
            .expect("fixture has a block signature");
        bytes[at..at + 4].fill(0);
        let dir = std::env::temp_dir().join(format!("kontakt-ncw-label-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broken-block.ncw");
        std::fs::write(&path, &bytes).unwrap();
        let source = crate::Samples::new(&dir).source(&path).unwrap();
        let err = source.open().err().expect("corrupt first block fails at open");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(err.to_string().contains("broken-block.ncw"), "{err}");
    }

    #[test]
    fn damaged_ncw_block_plays_silence_and_is_recorded() {
        const BLOCK: usize = ncw::NcwReader::<Bytes>::FRAMES_PER_BLOCK;
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor/ncw/tests/data/16-bit-mono.ncw");
        let pristine = std::fs::read(&fixture).unwrap();
        let layout = ncw::NcwReader::read(std::io::Cursor::new(pristine.clone())).unwrap();
        assert!(layout.header.num_samples as usize > 2 * BLOCK, "fixture needs two blocks");
        let at = layout.header.data_offset as usize + layout.block_offsets[1] as usize;
        let mut damaged = pristine.clone();
        damaged[at..at + 4].fill(0);
        let dir = std::env::temp_dir().join(format!("kontakt-ncw-silence-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pristine.ncw"), &pristine).unwrap();
        let path = dir.join("damaged-block.ncw");
        std::fs::write(&path, &damaged).unwrap();
        let mut samples = crate::Samples::new(&dir);
        let mut good = samples.source(&dir.join("pristine.ncw")).unwrap().open().unwrap();
        let source = samples.source(&path).unwrap();
        let label = source_label(&source);
        let mut reader = source.open().unwrap();
        let frames = reader.frames();

        let mut expected = vec![[0.; 2]; BLOCK];
        let mut actual = vec![[0.; 2]; BLOCK];
        good.read(0, &mut expected).unwrap();
        reader.read(0, &mut actual).unwrap();
        assert_eq!(actual, expected, "undamaged block decodes as before");

        let n = BLOCK.min(frames - BLOCK);
        reader.read(BLOCK, &mut actual[..n]).expect("damaged block reads as silence");
        assert!(actual[..n].iter().all(|f| *f == [0.; 2]));
        assert_eq!(damaged_sources().get(&label), Some(&1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn completion_backpressure_retries_publication_without_reopening_or_decoding() {
        for invalid in [false, true] {
            struct Numeric {
                opens: Arc<AtomicU64>,
                reads: Arc<AtomicU64>,
                invalid: bool,
            }
            impl AssetSource for Numeric {
                fn open(&self) -> io::Result<SampleReader> {
                    self.opens.fetch_add(1, Ordering::Relaxed);
                    let reads = self.reads.clone();
                    let invalid = self.invalid;
                    Ok(SampleReader::custom(48000, PAGE_FRAMES, move |_, out| {
                        reads.fetch_add(1, Ordering::Relaxed);
                        if invalid {
                            return Err(io::ErrorKind::InvalidData.into());
                        }
                        out.fill([0.125; 2]);
                        Ok(())
                    }))
                }
            }
            let (pcm, opens, reads) = (
                Pcm::streamed(48000, PAGE_FRAMES).unwrap(),
                Arc::new(AtomicU64::new(0)),
                Arc::new(AtomicU64::new(0)),
            );
            let (mut cache, worker) = StreamCache::new(2).unwrap();
            cache.request(&pcm, 0, 0).unwrap();
            let worker = Mutex::new(worker);
            let sources = HashMap::from([(
                pcm.asset_id(),
                Arc::new(Numeric {
                    opens: opens.clone(),
                    reads: reads.clone(),
                    invalid,
                }) as Arc<dyn AssetSource>,
            )]);
            let stop = AtomicBool::new(false);
            let attempts = AtomicU64::new(0);
            decode_with_publish(
                &worker,
                &sources,
                &DynamicSources::default(),
                &stop,
                1 << 20,
                |job, result| {
                    if attempts.fetch_add(1, Ordering::Relaxed) < 3 {
                        return Err(sampler_core::RejectedDecode {
                            reason: sampler_core::StreamError::Capacity,
                            job,
                            result,
                        });
                    }
                    assert_eq!(
                        result,
                        if invalid {
                            Err(DecodeFailure::InvalidSamples)
                        } else {
                            Ok(())
                        }
                    );
                    let result = worker.lock().unwrap().complete(job, result);
                    stop.store(true, Ordering::Relaxed);
                    result
                },
            );
            assert_eq!(
                (
                    opens.load(Ordering::Relaxed),
                    reads.load(Ordering::Relaxed),
                    attempts.load(Ordering::Relaxed)
                ),
                (1, 1, 4)
            );
            assert!(cache.poll().is_some());
            assert_eq!(
                cache.status(sampler_core::PageKey {
                    asset: pcm.asset_id(),
                    index: 0
                }),
                if invalid {
                    sampler_core::PageStatus::Failed(DecodeFailure::InvalidSamples)
                } else {
                    sampler_core::PageStatus::Ready
                }
            );
            if !invalid {
                assert_eq!(cache.frame(pcm.asset_id(), 17), Some([0.125; 2]));
            }
        }
    }

    #[test]
    fn dynamic_registry_pages_use_exact_registered_source_and_retire_off_audio() {
        struct Numeric(f32);
        impl AssetSource for Numeric {
            fn open(&self) -> io::Result<SampleReader> {
                let offset = self.0;
                Ok(SampleReader::custom(
                    48000,
                    PAGE_FRAMES * 3,
                    move |start, out| {
                        for (i, f) in out.iter_mut().enumerate() {
                            *f = [offset + (start + i) as f32; 2];
                        }
                        Ok(())
                    },
                ))
            }
        }
        let (mut cache, worker) = StreamCache::new(2).unwrap();
        let (streamer, _) = Streamer::start(
            HashMap::new(),
            &[],
            vec![],
            worker,
            1,
            true,
            1 << 20,
            HashSet::new(),
        )
        .unwrap();
        let original = streamer.sources.clone();
        let dynamic = streamer.dynamic_sources();
        let old = Pcm::streamed(48000, PAGE_FRAMES * 3).unwrap();
        let new = Pcm::streamed(48000, PAGE_FRAMES * 3).unwrap();
        let old_registration = dynamic.register(&old, Arc::new(Numeric(1.))).unwrap();
        let new_registration = dynamic.register(&new, Arc::new(Numeric(100.))).unwrap();
        assert!(dynamic.register(&old, Arc::new(Numeric(999.))).is_err());
        for (pcm, offset) in [(&old, 1.), (&new, 100.)] {
            let mut reopened = streamer.source(pcm.asset_id()).unwrap().open().unwrap();
            let mut check = [[0.;2];1];
            reopened.read(PAGE_FRAMES+17, &mut check).unwrap();
            assert_eq!(check, [[offset+(PAGE_FRAMES+17) as f32;2]]);
            cache.begin_epoch().unwrap();
            cache.request(pcm, 1, 0).unwrap();
            for t in &streamer.threads {
                t.thread().unpark();
            }
            let until = Instant::now() + Duration::from_secs(2);
            let key = sampler_core::PageKey {
                asset: pcm.asset_id(),
                index: 1,
            };
            while cache.status(key) != sampler_core::PageStatus::Ready {
                cache.poll();
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(
                cache.frame(pcm.asset_id(), PAGE_FRAMES + 17),
                Some([offset + (PAGE_FRAMES + 17) as f32; 2])
            );
        }
        assert!(Arc::ptr_eq(&original, &streamer.sources));
        assert!(original.is_empty());
        drop(old_registration);
        assert!(dynamic.source(old.asset_id()).is_none());
        assert!(dynamic.source(new.asset_id()).is_some());
        drop(new_registration);
        assert!(dynamic.0.lock().unwrap().is_empty());
    }

    #[test]
    fn decoder_cache_reuses_many_small_readers_without_changing_pcm() {
        let mut cache = ReaderCache::new(2 << 20);
        let mut expected = [[0.; 2]; 64];
        cache_fixture_reader().read(0, &mut expected).unwrap();
        let assets: Vec<_> = (0..40)
            .map(|_| Pcm::streamed(48000, 1024).unwrap())
            .collect();
        for (tick, asset) in assets.iter().enumerate() {
            let index = cache.insert(asset.asset_id(), cache_fixture_reader(), tick as u64);
            let mut actual = [[0.; 2]; 64];
            cache.entries[index].reader.read(0, &mut actual).unwrap();
            cache.trim_after_read(index);
            assert_eq!(
                actual.map(|f| f.map(f32::to_bits)),
                expected.map(|f| f.map(f32::to_bits))
            );
        }
        assert_eq!(
            cache.entries.len(),
            40,
            "active small decoders must not churn after 16 assets"
        );
        assert_eq!(cache.evictions, 0);
        assert!(cache.bytes + cache.storage_bytes <= 2 << 20);
        for asset in &assets {
            let index = cache
                .entries
                .iter()
                .position(|e| e.asset == asset.asset_id())
                .unwrap();
            let mut actual = [[0.; 2]; 64];
            cache.entries[index].reader.read(0, &mut actual).unwrap();
            cache.trim_after_read(index);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn decoder_cache_evicts_by_actual_grown_capacity_and_does_not_retain_oversized_readers() {
        let mut cache = ReaderCache::new(64 << 10);
        for tick in 0..10 {
            let asset = Pcm::streamed(48000, 1024).unwrap();
            let index = cache.insert(asset.asset_id(), cache_fixture_reader(), tick);
            cache.entries[index]
                .reader
                .read(0, &mut [[0.; 2]; 64])
                .unwrap();
            cache.trim_after_read(index);
            assert!(cache.bytes + cache.storage_bytes <= 64 << 10);
        }
        assert!(cache.evictions > 0);
        let asset = Pcm::streamed(48000, 1024).unwrap();
        let index = cache.insert(asset.asset_id(), cache_fixture_reader(), 100);
        cache.entries[index].reader.scratch.resize(128 << 10, 0);
        cache.trim_after_read(index);
        assert!(!cache.entries.iter().any(|e| e.asset == asset.asset_id()));
        assert!(cache.bytes + cache.storage_bytes <= 64 << 10);
        let mut opaque = ReaderCache::new(2 << 20);
        for tick in 0..32 {
            let asset = Pcm::streamed(48000, 1024).unwrap();
            let index = opaque.insert(
                asset.asset_id(),
                SampleReader::custom(48000, 1024, |_, out| {
                    out.fill([0.1; 2]);
                    Ok(())
                }),
                tick,
            );
            opaque.trim_after_read(index);
        }
        assert_eq!(
            opaque.opaque, OPEN_READERS,
            "opaque workspaces keep their legacy cap"
        );
        for budget in [0, 1, std::mem::size_of::<CachedReader>()] {
            let mut tiny = ReaderCache::new(budget);
            let index = tiny.insert(asset.asset_id(), cache_fixture_reader(), 0);
            tiny.entries[index]
                .reader
                .read(0, &mut [[0.; 2]; 64])
                .unwrap();
            tiny.trim_after_read(index);
            assert!(tiny.entries.is_empty());
            assert!(tiny.bytes + tiny.storage_bytes <= budget);
            assert!(tiny.peak_retained_bytes <= budget);
        }
    }

    // No decoder/reloader threads: each test controls the exact cold-head order.
    fn reload_fixture(count: usize, budget: usize) -> (Streamer, Vec<Pcm>) {
        struct Head;
        impl AssetSource for Head {
            fn open(&self) -> io::Result<SampleReader> {
                Ok(SampleReader::custom(48000, 64, |_, out| {
                    out.fill([0.5; 2]);
                    Ok(())
                }))
            }
        }
        let assets: Vec<_> = (0..count)
            .map(|_| Pcm::streamed(48000, 64).unwrap())
            .collect();
        let residency_budget=HeadBudget::new(budget);
        for pcm in &assets {pcm.bind_head_budget(&residency_budget).unwrap();}
        let streamer = Streamer {
            dynamic: DynamicSources::default(),
            pinned_wavetables: HashSet::new(),
            sources: Arc::new(
                assets
                    .iter()
                    .map(|pcm| (pcm.asset_id(), Arc::new(Head) as Arc<dyn AssetSource>))
                    .collect(),
            ),
            ranges: Arc::new(
                assets
                    .iter()
                    .map(|pcm| (pcm.asset_id(), vec![0..32]))
                    .collect(),
            ),
            restore_ranges: Arc::new(assets.iter()
                .map(|pcm| (pcm.asset_id(), RestoreRange::Admitted(vec![0..32]))).collect()),
            onset_ranges: None,
            hint_ranges: None,
            selected_hint_ranges: None,
            selected_hint_head_frames: None,
            stop: Arc::new(AtomicBool::new(false)),
            threads: Vec::new(),
            reloader: None,
            head_budget: budget,
            residency_budget,
            head_fallback: None,
            head_mutation: Arc::new(Mutex::new(())),
        };
        (streamer, assets)
    }

    #[test]
    fn reload_accounts_for_packed_replacements_throughout_one_batch() {
        let (streamer, assets) = reload_fixture(3, 32 * 8 + 32 * 2);
        assets[0]
            .set_ranges(vec![(0, vec![[0.5; 2]; 32].into_boxed_slice())])
            .unwrap();
        for pcm in &assets {
            pcm.mark_cold();
        }
        // The first head is transactionally replaced. Both missing mono heads
        // also fit exact packed admission, including temporary old generations.
        assert_eq!(streamer.reload(&assets).unwrap(), 3);
        assert_eq!(assets.iter().map(Pcm::head_bytes).sum::<usize>(), 192);
        assert_eq!(assets[2].head_frames(), 32);
        assert_eq!(streamer.head_budget_status().held_bytes,192);
    }

    #[test]
    fn head_readiness_requires_exact_offset_and_reverse_range_coverage() {
        let (mut streamer, assets) = reload_fixture(2, 128);
        streamer.ranges = Arc::new(HashMap::from([
            (assets[0].asset_id(), vec![8..16, 24..32]),
            (assets[1].asset_id(), vec![0..8]),
        ]));
        let before = streamer.head_readiness(&assets);
        assert_eq!(
            (before.assets, before.ranges, before.source_frames),
            (2, 3, 24)
        );
        assert_eq!(
            (
                before.minimum_packed_bytes(),
                before.decoded_bytes(),
                before.budget_bytes
            ),
            (48, 192, 128)
        );
        assert!(!before.complete());
        // A head can be nonzero yet cover neither authored offset/reverse start.
        assets[0]
            .set_ranges(vec![(0, vec![[0.25; 2]; 8].into_boxed_slice())])
            .unwrap();
        assert_eq!(streamer.head_readiness(&assets).covered_ranges, 0);
        assets[0]
            .set_ranges(vec![
                (8, vec![[0.25; 2]; 8].into_boxed_slice()),
                (24, vec![[0.5; 2]; 8].into_boxed_slice()),
            ])
            .unwrap();
        let partial = streamer.head_readiness(&assets);
        assert_eq!(
            (
                partial.covered_assets,
                partial.covered_ranges,
                partial.covered_source_frames
            ),
            (1, 2, 16)
        );
        assert!(!partial.complete());
        assets[1]
            .set_ranges(vec![(0, vec![[0.25; 2]; 8].into_boxed_slice())])
            .unwrap();
        assert!(streamer.head_readiness(&assets).complete());
        let selected = streamer.head_readiness_matching(&assets, |id| id == assets[0].asset_id());
        assert_eq!(
            (selected.assets, selected.ranges, selected.source_frames),
            (1, 2, 16)
        );
        assert!(selected.complete());
        assert!(!assets[0].resident_covers(0..8));
        assert!(!assets[0].resident_covers(8..33));
        assert!(!assets[0].resident_covers(8..8));
    }

    #[test]
    fn reload_rescans_the_budget_after_trim_and_a_transient_read_failure() {
        struct Unavailable;
        impl AssetSource for Unavailable {
            fn open(&self) -> io::Result<SampleReader> {
                Err(io::Error::from(io::ErrorKind::WouldBlock))
            }
        }
        let (mut streamer, assets) = reload_fixture(1, 32 * 8);
        let source = streamer.sources.clone();
        streamer.sources = Arc::new(HashMap::from([(
            assets[0].asset_id(),
            Arc::new(Unavailable) as Arc<dyn AssetSource>,
        )]));
        assets[0].mark_cold();
        assert_eq!(
            streamer.reload(&assets).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(assets[0].head_bytes(), 0);
        streamer.sources = source;
        assert_eq!(
            streamer.reload(&assets).unwrap(),
            1,
            "transient failure remains cold"
        );
        assert_eq!(streamer.trim(&assets, 0, 1), 64);
        assets[0].mark_cold();
        assert_eq!(
            streamer.reload(&assets).unwrap(),
            1,
            "trim must not leave stale accounting"
        );
    }

    struct PausedHead {
        entered: std::sync::mpsc::Sender<()>,
        resume: Mutex<std::sync::mpsc::Receiver<()>>,
        frame: [f32; 2],
    }
    impl AssetSource for PausedHead {
        fn open(&self) -> io::Result<SampleReader> {
            self.entered.send(()).unwrap();
            self.resume
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            let frame = self.frame;
            Ok(SampleReader::custom(48000, 64, move |_, out| {
                out.fill(frame);
                Ok(())
            }))
        }
    }

    #[test]
    fn reload_serializes_publication_with_control_side_trim() {
        let (mut streamer, assets) = reload_fixture(1, 32 * 8);
        let (entered, received) = std::sync::mpsc::channel();
        let (resume, resume_received) = std::sync::mpsc::channel();
        streamer.sources = Arc::new(HashMap::from([(
            assets[0].asset_id(),
            Arc::new(PausedHead {
                entered,
                resume: Mutex::new(resume_received),
                frame: [0.5; 2],
            }) as Arc<dyn AssetSource>,
        )]));
        assets[0].mark_cold();
        std::thread::scope(|scope| {
            let loading = scope.spawn(|| streamer.reload(&assets));
            let entered = received.recv_timeout(Duration::from_secs(5));
            let locked = streamer.head_mutation.try_lock().is_err();
            let trimming = scope.spawn(|| streamer.trim(&assets, 0, 1));
            // Release even if an assertion fails, so scoped threads cannot hang.
            resume.send(()).unwrap();
            assert!(
                entered.is_ok(),
                "reload never reached the controlled reader"
            );
            assert!(
                locked,
                "head-budget writers must stay serialized through IO/publication"
            );
            assert_eq!(loading.join().unwrap().unwrap(), 1);
            assert_eq!(trimming.join().unwrap(), 64);
        });
        assert_eq!(assets[0].head_bytes(), 0);
    }

    #[test]
    fn ksp_residency_purge_cannot_be_undone_by_an_in_flight_cold_reloader() {
        let (mut streamer, assets) = reload_fixture(1, 32 * 16);
        assets[0]
            .set_ranges(vec![(0, vec![[0.25; 2]; 32].into())])
            .unwrap();
        let (entered, received) = std::sync::mpsc::channel();
        let (resume, resume_received) = std::sync::mpsc::channel();
        streamer.sources = Arc::new(HashMap::from([(
            assets[0].asset_id(),
            Arc::new(PausedHead {
                entered,
                resume: Mutex::new(resume_received),
                frame: [0.5; 2],
            }) as Arc<dyn AssetSource>,
        )]));
        let plan = sampler_core::Prepared::new(
            48000,
            assets.clone(),
            vec![sampler_core::Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Default::default(),
                playback: Default::default(),
            }],
            1,
        )
        .unwrap()
        .with_groups(1, vec![Some(0)])
        .unwrap();
        let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
        let mut rt = sampler_core::Runtime::new(plan, limits).unwrap();
        let plan = rt.active_plan();
        assets[0].mark_cold();
        std::thread::scope(|scope| {
            let loading = scope.spawn(|| streamer.reload(&assets));
            let entered = received.recv_timeout(Duration::from_secs(3)).is_ok();
            if entered {
                rt.request_group_residency(plan, 0, false, sampler_core::ScriptInstanceId(0))
                    .unwrap();
            }
            resume.send(()).unwrap();
            assert!(entered);
            assert_eq!(loading.join().unwrap().unwrap(), 0,
                "purged staging transaction is not a committed reload");
        });
        let until = Instant::now() + Duration::from_secs(3);
        while assets[0].head_bytes() != 0 {
            rt.service_residency().unwrap();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!rt.group_resident(plan, 0).unwrap());
        assets[0].mark_cold();
        assert_eq!(streamer.reload(&assets).unwrap(), 0);
        assert_eq!(assets[0].head_bytes(), 0);
    }

    #[test]
    fn ksp_residency_streamer_installs_reload_recipe_for_authored_heads() {
        struct Head;
        impl AssetSource for Head {
            fn open(&self) -> io::Result<SampleReader> {
                Ok(SampleReader::custom(48000, 64, |_, out| {
                    out.fill([0.25; 2]);
                    Ok(())
                }))
            }
        }
        let pcm = Pcm::streamed(48000, 64).unwrap();
        let sources = HashMap::from([(pcm.asset_id(), Arc::new(Head) as Arc<dyn AssetSource>)]);
        let (cache, worker) = StreamCache::new(4).unwrap();
        let (_streamer, bytes) = Streamer::start(
            sources,
            std::slice::from_ref(&pcm),
            vec![vec![8..24, 40..56]],
            worker,
            1,
            false,
            1024,
            HashSet::new(),
        )
        .unwrap();
        assert_eq!(bytes, 64);
        let plan = sampler_core::Prepared::new(
            48000,
            vec![pcm.clone()],
            vec![sampler_core::Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Default::default(),
                playback: sampler_core::Playback {
                    start: 8,
                    ..Default::default()
                },
            }],
            1,
        )
        .unwrap()
        .with_groups(1, vec![Some(0)])
        .unwrap();
        let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
        let mut rt = sampler_core::Runtime::new(plan, limits)
            .unwrap()
            .with_stream_cache(cache);
        let plan = rt.active_plan();
        rt.request_group_residency(plan, 0, false, sampler_core::ScriptInstanceId(0))
            .unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while pcm.head_bytes() != 0 {
            rt.render(&mut []).unwrap();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        rt.request_group_residency(plan, 0, true, sampler_core::ScriptInstanceId(0))
            .unwrap();
        while !rt.group_resident(plan, 0).unwrap() {
            rt.render(&mut []).unwrap();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(pcm.head_frames(), 32);
        assert_eq!(pcm.head_bytes(), bytes);
    }

    #[test]
    fn concurrent_reloads_share_the_same_admission_budget() {
        let (mut streamer, assets) = reload_fixture(2, 32 * 8);
        let (entered, received) = std::sync::mpsc::channel();
        let (resume, resume_received) = std::sync::mpsc::channel();
        Arc::get_mut(&mut streamer.sources).unwrap().insert(
            assets[0].asset_id(),
            Arc::new(PausedHead {
                entered,
                resume: Mutex::new(resume_received),
                frame: [0.1, 0.3], // Non-integer stereo PCM: occupies the whole raw budget.
            }),
        );
        assets[0].mark_cold();
        std::thread::scope(|scope| {
            let first = scope.spawn(|| streamer.reload(&assets[..1]));
            let entered = received.recv_timeout(Duration::from_secs(5));
            let locked = streamer.head_mutation.try_lock().is_err();
            assets[1].mark_cold();
            let second = scope.spawn(|| streamer.reload(&assets[1..]));
            resume.send(()).unwrap();
            assert!(entered.is_ok());
            assert!(
                locked,
                "a second reload cannot take a stale budget snapshot"
            );
            assert_eq!(first.join().unwrap().unwrap(),1);
            let error=second.join().unwrap().err().unwrap();
            assert!(error.get_ref().is_some_and(|e|e.is::<HeadBudgetExceeded>()));
        });
        assert_eq!(assets[0].head_bytes(), streamer.head_budget);
        assert_eq!(assets[1].head_bytes(), 0);
    }

    #[test]
    fn wavetable_start_ranges_keep_every_cycle_under_a_tiny_head_budget() {
        let mut zone = ir::Zone::new(ir::AssetRef(0));
        zone.group = Some(ir::GroupRef(0));
        zone.playback.start = 2048;
        zone.playback.end = Some(7 * 2048);
        let instrument = ir::Instrument {
            zones: vec![zone],
            groups: vec![ir::Group {
                wavetable: Some(ir::Wavetable::default()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let pcm = Pcm::streamed(22050, 8 * 2048).unwrap();
        let policy = StreamPolicy {
            head_budget: 1,
            resident_budget: Some(1),
            ..Default::default()
        };
        assert_eq!(
            start_ranges(&instrument, &[pcm], 48000, &law_heads(&instrument, 1), &policy),
            vec![vec![2048..7 * 2048]]
        );
    }

    #[test]
    fn wavetable_pin_reads_lazy_cycles_and_survives_purge_with_explicit_budget_failure() {
        struct TableSource(Arc<std::sync::atomic::AtomicUsize>);
        impl AssetSource for TableSource {
            fn open(&self) -> io::Result<SampleReader> {
                self.0.fetch_add(1, Ordering::Relaxed);
                Ok(SampleReader::custom(48000, 8192, |_, out| {
                    out.fill([0.5; 2]);
                    Ok(())
                }))
            }
        }
        for budget in [1, 8192 * 8] {
            let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let pcm = Pcm::streamed(48000, 8192).unwrap();
            let sources = HashMap::from([(
                pcm.asset_id(),
                Arc::new(TableSource(reads.clone())) as Arc<dyn AssetSource>,
            )]);
            let (_, worker) = StreamCache::new(1).unwrap();
            let pinned = [pcm.asset_id(), pcm.asset_id()].into_iter().collect();
            let result = Streamer::start(
                sources,
                std::slice::from_ref(&pcm),
                vec![vec![0..8192]],
                worker,
                1,
                true,
                budget,
                pinned,
            );
            if budget == 1 {
                assert!(result.is_err());
                assert_eq!(pcm.head_frames(), 0);
                assert_eq!(reads.load(Ordering::Relaxed), 0);
            } else {
                let (streamer, bytes) = result.unwrap();
                assert_eq!(
                    reads.load(Ordering::Relaxed),
                    1,
                    "shared oscillator asset is read once"
                );
                assert_eq!(bytes, pcm.head_bytes());
                assert_eq!(pcm.head_frames(), 8192);
                assert_eq!(streamer.purge(std::slice::from_ref(&pcm), 1), 0);
                assert_eq!(pcm.head_frames(), 8192);
                assert_eq!(streamer.reload(std::slice::from_ref(&pcm)).unwrap(), 0);
            }
        }
    }

    #[test]
    fn headers_open_in_bounded_parallel_workers_and_keep_asset_order() {
        struct Header {
            frames: usize,
            active: Arc<std::sync::atomic::AtomicUsize>,
            peak: Arc<std::sync::atomic::AtomicUsize>,
        }
        impl AssetSource for Header {
            fn open(&self) -> io::Result<SampleReader> {
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(active, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(5));
                self.active.fetch_sub(1, Ordering::SeqCst);
                Ok(SampleReader::custom(48000, self.frames, |_, out| {
                    out.fill([0.5; 2]);
                    Ok(())
                }))
            }
        }
        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let peak = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sources = (1..=8)
            .map(|frames| {
                (
                    Arc::new(Header {
                        frames,
                        active: active.clone(),
                        peak: peak.clone(),
                    }) as Arc<dyn AssetSource>,
                    std::path::Path::new("test"),
                )
            })
            .collect();
        let opened = Streamer::open(
            sources,
            48000,
            &StreamPolicy::default(),
            0,
            &|| false,
            &mut |_, _| (),
        )
        .unwrap();
        assert_eq!(
            opened
                .assets
                .iter()
                .map(Pcm::frame_count)
                .collect::<Vec<_>>(),
            (1..=8).collect::<Vec<_>>()
        );
        assert!((1..=8).contains(&peak.load(Ordering::SeqCst)));
    }

    #[test]
    #[ignore = "fixed pool trial 56f1822b rejected: storage underruns and RSS; retained as trial evidence"]
    fn authored_polyphony_sizes_new_stream_pools_without_shrinking_the_policy_floor() {
        use sampler_ir::{Instrument, Kill, Time, VoiceLimit};
        // Rebuilding a source with a changed limit allocates its new pool here,
        // before any runtime/audio callback owns the cache.
        for limit in [None, Some(2), Some(12), Some(1000)] {
            let instrument = Instrument {
                voice_limit: limit.map(|voices| VoiceLimit {
                    voices,
                    kill: Kill::Oldest,
                    exclusive_set: None,
                    prefer_released: true,
                    fade: Time::Milliseconds(0.),
                }),
                ..Default::default()
            };
            let streamed = crate::stream_instrument(
                instrument,
                vec![],
                vec![],
                &crate::Options::default(),
                &StreamPolicy {
                    voices: 4,
                    lazy: true,
                    ..Default::default()
                },
            )
            .unwrap();
            let pages = usize::try_from(limit.unwrap_or(4)).unwrap().max(4) * PAGES_PER_VOICE;
            assert_eq!(streamed.report.pool_pages, pages);
            assert_eq!(
                streamed.report.pool_bytes,
                pages * PAGE_FRAMES * size_of::<Frame>()
            );
            assert_eq!(streamed.cache.bytes(), streamed.report.pool_bytes);
            assert_eq!(streamed.report.head_bytes, 0);
        }
    }

    #[test]
    fn lazy_heads_are_not_read_until_requested_and_respect_the_budget() {
        struct Head(Arc<std::sync::atomic::AtomicUsize>);
        impl AssetSource for Head {
            fn open(&self) -> io::Result<SampleReader> {
                let reads = self.0.clone();
                Ok(SampleReader::custom(48000, 8192, move |_, out| {
                    reads.fetch_add(1, Ordering::Relaxed);
                    out.fill([0.5; 2]);
                    Ok(())
                }))
            }
        }
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let pcm = Pcm::streamed(48000, 8192).unwrap();
        let sources = HashMap::from([(
            pcm.asset_id(),
            Arc::new(Head(reads.clone())) as Arc<dyn AssetSource>,
        )]);
        let (_, worker) = StreamCache::new(1).unwrap();
        let (streamer, bytes) = Streamer::start(
            sources,
            std::slice::from_ref(&pcm),
            vec![vec![0..4096]],
            worker,
            1,
            true,
            1,
            HashSet::new(),
        )
        .unwrap();
        assert_eq!(
            (bytes, reads.load(Ordering::Relaxed), pcm.head_bytes()),
            (0, 0, 0)
        );
        // No on-demand reload can exceed a budget smaller than this head.
        assert_eq!(streamer.reload(std::slice::from_ref(&pcm)).unwrap(), 0);
        assert_eq!(pcm.head_bytes(), 0);
    }

    #[test]
    fn v1_ram_mode_fills_small_samples_first_and_streams_over_budget() {
        let assets: Vec<_> = [1000, 4000, 2000]
            .into_iter()
            .map(|n| Pcm::streamed(48000, n).unwrap())
            .collect();
        let mut heads = vec![vec![0..100]; 3];
        let needed = keep_whole(&assets, &mut heads, 24000 + 800);
        assert_eq!(heads[0], vec![0..1000]);
        assert_eq!(heads[2], vec![0..2000]);
        assert_eq!(
            heads[1],
            vec![0..100],
            "large sample falls back to streaming"
        );
        assert_eq!(needed, (4000 - 100) * 8);
        keep_whole(&assets, &mut heads, usize::MAX);
        assert_eq!(heads[1], vec![0..4000]);
    }

    #[test]
    fn trimming_purges_idle_heads_until_within_budget() {
        let head = [[0.5f32; 2]; 1000];
        let assets: Vec<Pcm> = (0..3)
            .map(|_| Pcm::headed(48000, 8000, &head).unwrap())
            .collect();
        let (_, worker) = StreamCache::new(1).unwrap();
        let (streamer, _) = Streamer::start(
            HashMap::new(),
            &assets,
            vec![vec![]; 3],
            worker,
            1,
            false,
            usize::MAX,
            HashSet::new(),
        )
        .unwrap();
        // Packed: 16-bit mono.
        let size = assets[0].head_bytes();
        assert_eq!(size, 1000 * 2);
        // Nothing has played since 0: nothing is idle before it.
        assert_eq!(streamer.trim(&assets, 0, 0), 0);
        assert_eq!(streamer.trim(&assets, size * 3 / 2, 1), 2 * size);
        let left: usize = assets.iter().map(Pcm::head_frames).sum();
        assert_eq!(left, 1000);
        assert_eq!(streamer.purge(&assets, 1), size);
    }

    #[test]
    fn random_access_reads_match_a_full_decode() {
        let dir = std::env::temp_dir().join(format!("kontakt-stream-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pcm: Vec<i32> = (0..40000 * 2).map(|i| (i * 37 % 2000) - 1000).collect();
        let spec = ncw::PcmSpec {
            channels: 2,
            bits_per_sample: 16,
            sample_rate: 44100,
        };
        let ncw = ncw::encode_pcm(&pcm, spec, ncw::StereoMode::Direct).unwrap();
        let mut wav =
            b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x44\xac\0\0\x88\x58\x01\0\x02\0\x10\0data"
                .to_vec();
        let samples: Vec<u8> = (0..40000i32)
            .flat_map(|i| ((i * 11) as i16).to_le_bytes())
            .collect();
        wav.extend((samples.len() as u32).to_le_bytes());
        wav.extend(samples);
        for (name, bytes) in [("a.ncw", ncw), ("b.wav", wav)] {
            let path = dir.join(name);
            std::fs::write(&path, &bytes).unwrap();
            let full = crate::decode(&bytes).unwrap().frames;
            let source = crate::Samples::new(&dir).source(&path).unwrap();
            let before = DISK_READ.load(Ordering::Relaxed);
            let mut reader = SampleReader::open(&source).unwrap();
            assert_eq!((reader.frames(), reader.rate()), (40000, 44100));
            for range in [0..40000, 511..1025, 39999..40000, 1000..1000, 32000..33000] {
                let mut out = vec![[0.0; 2]; range.len()];
                reader.read(range.start, &mut out).unwrap();
                assert_eq!(out, full[range], "{name}");
            }
            assert!(reader.read(39999, &mut [[0.0; 2]; 2]).is_err());
            assert_eq!(
                DISK_READ.load(Ordering::Relaxed),
                before,
                "loading reads do not count"
            );
            let mut reader = source.open_stream().unwrap();
            reader.read(0, &mut vec![[0.; 2]; 40000]).unwrap();
            assert!(
                DISK_READ.load(Ordering::Relaxed) >= before + bytes.len() as u64,
                "physical playback reads count"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
#[path = "stream_onset_tests.rs"]
mod stream_onset_tests;

#[cfg(test)]
#[path = "stream_hint_tests.rs"]
mod stream_hint_tests;

#[cfg(test)]
mod wide_ncw_workspace_tests {
    use super::*;
    #[test]
    fn synthetic_zero_bit_ncw_admission_charges_its_simultaneous_raw_block_buffer() {
        // Synthetic legacy zero-bit interpretation, not a Kontakt/native
        // format-parity assertion. It still consumes real decoder workspace.
        let channels=3usize;let frames=512usize;let mut bytes=vec![0u8;120];
        bytes[..8].copy_from_slice(&0x01A89ED631010000u64.to_be_bytes());
        bytes[8..10].copy_from_slice(&(channels as u16).to_le_bytes());bytes[10..12].copy_from_slice(&32u16.to_le_bytes());
        bytes[12..16].copy_from_slice(&48000u32.to_le_bytes());bytes[16..20].copy_from_slice(&(frames as u32).to_le_bytes());
        bytes[20..24].copy_from_slice(&120u32.to_le_bytes());bytes[24..28].copy_from_slice(&128u32.to_le_bytes());
        let mut payload=Vec::new();for channel in 0..channels {
            payload.extend(0x3e9a0c16u32.to_le_bytes());payload.extend(0i32.to_le_bytes());payload.extend(0i16.to_le_bytes());payload.extend(0u16.to_le_bytes());payload.extend(0u32.to_le_bytes());
            for _ in 0..frames {payload.extend(((channel as i32+1)*0x1000_0000).to_le_bytes());}
        }
        bytes[28..32].copy_from_slice(&(payload.len() as u32).to_le_bytes());bytes.extend(0u32.to_le_bytes());bytes.extend((payload.len() as u32).to_le_bytes());bytes.extend(payload);
        static SERIAL:AtomicU64=AtomicU64::new(0);
        let directory=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.test-artifacts").join(format!("wide-ncw-workspace-{}-{}",std::process::id(),SERIAL.fetch_add(1,Ordering::Relaxed)));
        std::fs::create_dir_all(&directory).unwrap();let path=directory.join("synthetic-zero-bit.ncw");std::fs::write(&path,&bytes).unwrap();
        let source=Source {identity:Arc::new(crate::FileIdentity::capture(&std::fs::metadata(&path).unwrap()).unwrap()),
            size:bytes.len() as u64,path,offset:0,key:None,handle:None,header:None};
        let initial=SampleReader::open(&source).unwrap();
        let current=initial.cache_bytes().unwrap();
        let final_channels=channels.next_power_of_two()*size_of::<Vec<i32>>()+channels*frames*size_of::<i32>();
        let transient=frames*size_of::<i32>();assert_eq!(transient,2048);
        assert_eq!(initial.native_workspace_bound(),Some(current+final_channels+transient));
        let preopen=4*size_of::<u32>()+final_channels+transient+BUFFER+source.path.capacity()+size_of::<ncw::NcwReader<Bytes>>()+size_of::<SampleReader>();
        let admitted=preopen.max(initial.native_workspace_bound().unwrap());drop(initial);
        assert!(open_native_channel_reader(&source,false,admitted-1).is_err());
        for limit in [admitted,admitted+1] {
            let mut reader=open_native_channel_reader(&source,false,limit).unwrap().unwrap();
            let mut first=vec![[0.;2];frames];let mut second=vec![[0.;2];frames];let mut parts=[first.as_mut_slice(),second.as_mut_slice()];
            let mut output=crate::channel_reader::ChannelPackDestinations::new(channels,frames,&mut parts).unwrap();
            reader.read_packs(0,&mut output,limit).unwrap();drop(output);
            assert!(first.iter().all(|f|*f==[0.125,0.25]));assert!(second.iter().all(|f|*f==[0.375,0.]));
            assert!(reader.cache_bytes().unwrap()+transient<=limit);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}

/// Ignored CPU receipt for the disk read path: sequential `read` of a 16-bit stereo WAV that
/// does not fit one `BUFFER` refill. Run: `KONTRA_STREAM_BENCH_DIR=<scratch dir> nice -n 10
/// build-slot cargo test --release -p sampler-kontakt --lib stream_read_receipt -- --ignored --nocapture`.
#[cfg(test)]
mod stream_read_receipt {
    use super::*;
    use std::{hint::black_box, time::Instant};

    const FRAMES: usize = 1 << 22;
    const REPEATS: usize = 7;

    fn wav() -> Vec<u8> {
        let data_bytes = (FRAMES * 4) as u32;
        let mut v = b"RIFF".to_vec();
        v.extend((36 + data_bytes).to_le_bytes());
        v.extend(b"WAVEfmt ");
        v.extend(16u32.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(2u16.to_le_bytes());
        v.extend(48000u32.to_le_bytes());
        v.extend(192000u32.to_le_bytes());
        v.extend(4u16.to_le_bytes());
        v.extend(16u16.to_le_bytes());
        v.extend(b"data");
        v.extend(data_bytes.to_le_bytes());
        for i in 0..FRAMES {
            v.extend(((i % 30000) as i16).to_le_bytes());
            v.extend((-((i % 30000) as i16)).to_le_bytes());
        }
        v
    }

    #[test]
    #[ignore = "disk read CPU receipt; needs KONTRA_STREAM_BENCH_DIR, run --release"]
    fn stream_read_receipt() {
        let dir = std::path::PathBuf::from(
            std::env::var("KONTRA_STREAM_BENCH_DIR").expect("set KONTRA_STREAM_BENCH_DIR"),
        );
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("receipt.wav");
        std::fs::write(&path, wav()).unwrap();
        let mut source = crate::Samples::new(&dir).source(&path).unwrap();
        source.handle = Some(Arc::new(File::open(&path).unwrap()));
        for block in [64usize, 4096] {
            let mut best = f64::MAX;
            for _ in 0..REPEATS {
                let mut reader = source.open().unwrap();
                let mut out = vec![[0.0_f32; 2]; block];
                let started = Instant::now();
                let mut start = 0;
                while start + block <= FRAMES {
                    reader.read(start, &mut out).unwrap();
                    black_box(&out);
                    start += block;
                }
                let ns = started.elapsed().as_nanos() as f64 / start as f64;
                best = best.min(ns);
            }
            println!("stream_read_receipt block={block:<5} ns/frame={best:.2}");
        }
    }
}

#[cfg(test)]
mod head_budget_tests {
    use super::cold_head_budget_for;

    #[test]
    fn cold_head_budget_scales_with_ram_within_its_clamp() {
        const MIB: usize = 1 << 20;
        assert_eq!(cold_head_budget_for(None), 256 * MIB);
        assert_eq!(cold_head_budget_for(Some(2 << 30)), 256 * MIB);
        assert_eq!(cold_head_budget_for(Some(46 << 30)), (46usize << 30) / 32);
        assert_eq!(cold_head_budget_for(Some(512 << 30)), 4 << 30);
    }
}
