//! Mutation fuzzing of the format readers (`KONTRA_FUZZ_DECODED=1` for the decoded mode below): each real file is truncated, bit
//! flipped and has length-like fields corrupted, and every mutant goes through
//! the translator and prepare path (a sample file through the stream decoder).
//! A child process runs the mutants so a hang (over 2 s), an allocation over
//! 1 GiB or a crash is caught and the run goes on; panics are caught in the
//! child and counted by source location.
//!
//! `cargo run --release -p kontra-host --features heap-profile --example fuzz -- <file>...`
//! (Unix only.) `KONTRA_FUZZ_MUTANTS=2000` per file (default), `KONTRA_FUZZ_SECONDS=240`
//! cap per file, `KONTRA_FUZZ_JOBS=3` files at once. Mutants are derived from
//! the file and the index, so `fuzz --repro <file> <index>` rebuilds one and
//! runs it with a backtrace. (The feature leaves the global allocator to this
//! binary, which aborts any single allocation over 1 GiB.)

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use kontra_host::instrument;
use kontra_stream::{BlobKey, PAGE_FRAMES, Store, StoreConfig};

const BIG: usize = 1 << 30;
const HANG: Duration = Duration::from_secs(2);

/// `KONTRA_FUZZ_DECODED=1`: `.nki` and `.nkm` mutants are made in the
/// decompressed chunk stream, past the compression layer that rejects most
/// outer-byte mutants, and a hang is anything over 1 s more than the baseline.
fn decoded_mode() -> bool {
    std::env::var_os("KONTRA_FUZZ_DECODED").is_some()
}

fn hang() -> Duration {
    if decoded_mode() {
        Duration::from_secs(1)
    } else {
        HANG
    }
}

fn is_preset(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("nki") || e.eq_ignore_ascii_case("nkm"))
}
/// A child gets longer to read the file and set up before its first mutant.
const SETUP: Duration = Duration::from_secs(60);

static CURRENT: AtomicU64 = AtomicU64::new(0);
/// Mutants the reader accepted, and refused: how deep the mutants get.
static ACCEPTED: AtomicU64 = AtomicU64::new(0);
static REFUSED: AtomicU64 = AtomicU64::new(0);
static MEMBERS: AtomicU64 = AtomicU64::new(0);
static MEMBERS_READ: AtomicU64 = AtomicU64::new(0);

struct Limited;

#[global_allocator]
static ALLOCATOR: Limited = Limited;

fn too_big(size: usize) -> ! {
    println!("A {} {size}", CURRENT.load(Ordering::Relaxed));
    std::process::abort()
}

// SAFETY: forwards to the system allocator; only refuses (aborts) an
// allocation over 1 GiB.
#[expect(unsafe_code, reason = "the measuring allocator forwards to System")]
unsafe impl GlobalAlloc for Limited {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() > BIG {
            too_big(layout.size());
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.size() > BIG {
            too_big(layout.size());
        }
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if size > BIG {
            too_big(size);
        }
        unsafe { System.realloc(ptr, layout, size) }
    }
}

// ---- mutants -------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// Writes `value` at `at` if it fits.
fn put(bytes: &mut [u8], at: usize, value: &[u8]) {
    if let Some(dst) = bytes.get_mut(at..at + value.len()) {
        dst.copy_from_slice(value);
    }
}

/// Files over this size are patched in place and restored, not rewritten.
const BIG_FILE: usize = 8 << 20;

/// The file as mutants start from: all of it in memory, or for a big file
/// just its length and a handle to read the few bytes an edit touches.
struct Original {
    /// For a decoded chunk stream: the byte ranges of the chunks' payloads,
    /// the only places its mutants edit (a chunk's own id and length are
    /// left alone, so a mutant is read as far as its payloads go wrong).
    payloads: Vec<std::ops::Range<usize>>,
    len: usize,
    bytes: Option<Vec<u8>>,
    file: std::fs::File,
}

impl Original {
    fn open(path: &Path) -> std::io::Result<Self> {
        let file = std::fs::File::open(path)?;
        let len = usize::try_from(file.metadata()?.len()).unwrap_or(usize::MAX);
        let bytes = if len > BIG_FILE {
            None
        } else {
            Some(std::fs::read(path)?)
        };
        Ok(Self {
            payloads: Vec::new(),
            len,
            bytes,
            file,
        })
    }

    fn big(&self) -> bool {
        self.bytes.is_none()
    }

    /// `n` bytes from `at` (fewer at the end).
    fn read(&self, at: usize, n: usize) -> Vec<u8> {
        if let Some(bytes) = &self.bytes {
            return bytes.iter().skip(at).take(n).copied().collect();
        }
        let mut out = vec![0; n.min(self.len.saturating_sub(at))];
        let _ = self.file.read_exact_at(&mut out, at as u64);
        out
    }

    fn byte(&self, at: usize) -> u8 {
        self.read(at, 1).first().copied().unwrap_or(0)
    }

    /// A stream already in memory (the decoded chunks of a preset).
    fn decoded(path: &Path, bytes: Vec<u8>) -> std::io::Result<Self> {
        // Chunks are `id: u16, length: u32, payload`, one after another.
        let mut payloads = Vec::new();
        let mut at = 0usize;
        while let Some(head) = bytes.get(at..at + 6) {
            let n = u32::from_le_bytes([head[2], head[3], head[4], head[5]]) as usize;
            let end = (at + 6).saturating_add(n).min(bytes.len());
            if end > at + 6 {
                payloads.push(at + 6..end);
            }
            at = end;
        }
        Ok(Self {
            payloads,
            len: bytes.len(),
            bytes: Some(bytes),
            file: std::fs::File::open(path)?,
        })
    }

    fn whole(&self) -> Vec<u8> {
        self.bytes.clone().unwrap_or_default()
    }
}

enum Mutant {
    /// Replacements at offsets, for a file too big to rewrite per mutant.
    Patches(Vec<(usize, Vec<u8>)>),
    Whole(Vec<u8>),
}

/// Mutant `index` of `original`: the same one for the same file and index.
fn mutant(original: &Original, file: &str, index: u64) -> Mutant {
    let mut rng = Rng::new(fnv(file) ^ index.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let len = original.len;
    let big = original.big();
    // Length fields live near the front (and a big file's index at its end):
    // half the edits land in the first 256 bytes, or a big file's ends.
    let total: usize = original.payloads.iter().map(|r| r.len()).sum();
    let spot = |rng: &mut Rng| {
        if total > 0 {
            // Anywhere inside a payload, evenly over all payload bytes.
            let mut n = rng.below(total);
            for r in &original.payloads {
                if n < r.len() {
                    return r.start + n;
                }
                n -= r.len();
            }
            0
        } else if big {
            let end = rng.below(2) == 0;
            let at = rng.below(1 << 16);
            if end { len.saturating_sub(1 + at) } else { at }
        } else if rng.below(2) == 0 {
            rng.below(len.min(256))
        } else {
            rng.below(len)
        }
    };
    let kind = rng.below(7);
    // A big file takes only the in-place kinds; so does a chunk stream, whose
    // top-level lengths stay true.
    let kind = if (big || total > 0) && matches!(kind, 0 | 5 | 6) {
        1 + rng.below(4)
    } else {
        kind
    };
    let byte = |at: usize| original.byte(at);
    match kind {
        0 => Mutant::Whole(original.read(0, rng.below(len))),
        1 => {
            // A chunk stream takes one or two flips: more is nearly always refused.
            let most = if total > 0 { 2 } else { 8 };
            let flips = (0..=rng.below(most))
                .map(|_| {
                    let at = spot(&mut rng);
                    (at, vec![byte(at) ^ (1 << rng.below(8))])
                })
                .collect();
            Mutant::Patches(flips)
        }
        2 | 3 => {
            // A length-like field: a 16, 32 or 64-bit value set to an edge.
            let width = [2usize, 4, 4, 8][rng.below(4)];
            let at = spot(&mut rng);
            let edges = [
                0u64,
                1,
                0xFF,
                0x7FFF,
                0xFFFF,
                0x7FFF_FFFF,
                0xFFFF_FFFF,
                u64::MAX,
                len as u64,
                len as u64 + 1,
                len as u64 / 2,
                rng.next(),
            ];
            let value = edges.get(rng.below(edges.len())).copied().unwrap_or(0);
            let bytes = value.to_le_bytes();
            Mutant::Patches(vec![(at, bytes.get(..width).unwrap_or_default().to_vec())])
        }
        4 => {
            let at = spot(&mut rng);
            let span = 1 + rng.below(if total > 0 { 8 } else { 64 });
            let fill = [0u8, 0xFF][rng.below(2)];
            Mutant::Patches(vec![(at, vec![fill; span])])
        }
        5 => {
            // Delete a chunk.
            let at = spot(&mut rng).min(len);
            let span = 1 + rng.below(64);
            let mut out = original.whole();
            out.drain(at..(at + span).min(len));
            Mutant::Whole(out)
        }
        _ => {
            // Repeat a chunk in place.
            let at = spot(&mut rng).min(len);
            let span = 1 + rng.below(64);
            let chunk = original.read(at, span);
            let mut out = original.whole();
            out.splice(at..at, chunk);
            Mutant::Whole(out)
        }
    }
}

// ---- running one mutant ----------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Instrument,
    Nkr,
    Bank,
    Sample,
}

fn kind_of(path: &Path) -> Option<Kind> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "nki" | "nkm" | "sfz" => Some(Kind::Instrument),
        "nkr" => Some(Kind::Nkr),
        "ufs" => Some(Kind::Bank),
        "ncw" | "wav" => Some(Kind::Sample),
        _ => None,
    }
}

/// A library root for `path`: the folder holding the `Instruments` (or
/// `Multis`) folder it sits in, when there is one.
fn library_root(path: &Path) -> Option<&Path> {
    path.ancestors()
        .skip(1)
        .find(|a| a.join("Instruments").is_dir() || a.join("Multis").is_dir())
}

/// Where the mutant of `original` is written: for an instrument, in a copy of
/// its library's top level (symlinks to everything but the instrument's own
/// folder), so its samples and resources still resolve.
fn place(original: &Path, dir: &Path) -> std::io::Result<PathBuf> {
    let name = original.file_name().unwrap_or_default();
    if kind_of(original) == Some(Kind::Instrument)
        && let Some(root) = library_root(original)
        && let Ok(rel) = original.strip_prefix(root)
    {
        let first = rel.components().next().map(|c| c.as_os_str().to_owned());
        for entry in std::fs::read_dir(root)?.flatten() {
            if Some(entry.file_name()) != first {
                #[cfg(unix)]
                let _ = std::os::unix::fs::symlink(entry.path(), dir.join(entry.file_name()));
            }
        }
        let target = dir.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        return Ok(target);
    }
    Ok(dir.join(name))
}

fn run_instrument(path: &Path, store: &mut Store) {
    let read = instrument::read(path);
    let counter = if read.is_ok() { &ACCEPTED } else { &REFUSED };
    counter.fetch_add(1, Ordering::Relaxed);
    if let Ok(ir) = read {
        let loaded = instrument::build(ir, 48_000, store);
        let engine =
            kontra_engine::Engine::new(std::sync::Arc::new(loaded.plan), store.client(), 48_000);
        drop(engine);
    }
}

fn run_sample(path: &Path, store: &mut Store) {
    let sample = kontra_engine::ir::Sample {
        name: String::new(),
        file: path.to_path_buf(),
        offset: 0,
        len: None,
        channels: 2,
        frames: 2_000_000,
        rate: 48_000,
        keystream: None,
    };
    let _ = kontra_stream::decode(&sample, 1 << 22);
    let Ok((source, fmt)) = kontra_stream::open(&sample) else {
        return;
    };
    let key = BlobKey {
        file: path.to_string_lossy().into(),
        offset: 0,
        fmt,
    };
    let frames = sample.frames;
    if let Ok(loaded) = store.load(&key, source, frames, &[0..PAGE_FRAMES * 8]) {
        let client = store.client();
        let mut out = vec![[0.0f32; 2]; 4096];
        for first in [0, 100_000, 1_900_000] {
            loaded.read(&client, first, &mut out);
        }
    }
}

fn run_nkr(path: &Path) {
    // The archive is found as a library resource, as an instrument's load does.
    if let Some(dir) = path.parent() {
        let mut resources =
            kontra_format_kontakt::Resources::open(&dir.join("Instruments").join("x.nki"));
        // The members the archive lists: a damaged member header or payload
        // is only reached by asking for that member.
        let names = resources.member_names();
        let listed = names.len();
        let mut read = 0;
        for name in names.iter().take(24) {
            let stored = name.replace(['|', '\\'], "/");
            let short = stored
                .rsplit_once("esources/")
                .map_or(stored.as_str(), |(_, n)| n);
            read += usize::from(resources.read(short).is_some());
        }
        MEMBERS.fetch_add(listed as u64, Ordering::Relaxed);
        MEMBERS_READ.fetch_add(read as u64, Ordering::Relaxed);
        for name in ["data/x.nka", "wavetables/x.wav", "x"] {
            let _ = resources.read(name);
        }
    }
}

fn run_one(kind: Kind, path: &Path, store: &mut Store) {
    match kind {
        Kind::Instrument => run_instrument(path, store),
        Kind::Sample => run_sample(path, store),
        Kind::Nkr => run_nkr(path),
        Kind::Bank => {
            let Ok(programs) = kontra_format_uvi::programs(path) else {
                return;
            };
            for program in programs.iter().take(2) {
                run_instrument(&path.join(program), store);
            }
        }
    }
}

// ---- the child: runs mutants of one file ----------------------------------

/// Mutants of a preset's decoded chunk stream: each is read as the file's own
/// instrument (its samples beside the real file, which is never written) and
/// built. A refusal must say why: an empty message is reported as `E`.
fn child_decoded(file: &Path, start: u64, count: u64) -> Result<(), Box<dyn std::error::Error>> {
    let (bytes, encrypted) = kontra_format_kontakt::decode(file)?;
    let original = Original::decoded(file, bytes)?;
    std::panic::set_hook(Box::new(|info| {
        let text = format!("{info}").replace('\n', " | ");
        println!("P {} {text}", CURRENT.load(Ordering::Relaxed));
    }));
    let mut store = Store::new(StoreConfig::DEFAULT)?;
    store.defer_heads(true);
    let name = file.to_string_lossy();
    let mut run = |bytes: Vec<u8>, i: u64| {
        let began = Instant::now();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match kontra_format_kontakt::load_chunks(file, bytes, encrypted) {
                Ok(ir) => {
                    ACCEPTED.fetch_add(1, Ordering::Relaxed);
                    let loaded = instrument::build(ir, 48_000, &mut store);
                    let plan = std::sync::Arc::new(loaded.plan);
                    drop(kontra_engine::Engine::new(plan, store.client(), 48_000));
                }
                Err(e) => {
                    REFUSED.fetch_add(1, Ordering::Relaxed);
                    let why = e.to_string();
                    if why.trim().is_empty() {
                        println!("E {i}");
                    }
                    // The reason without the file's path or any number, to count alike.
                    let why = why.replace(&*file.to_string_lossy(), "<file>");
                    let why: String = why
                        .chars()
                        .map(|c| if c.is_ascii_digit() { '#' } else { c })
                        .take(100)
                        .collect();
                    println!("R {i} {why}");
                }
            }
        }));
        began.elapsed()
    };
    let began = run(original.whole(), u64::MAX);
    println!("B {}", began.as_millis());
    ACCEPTED.store(0, Ordering::Relaxed);
    REFUSED.store(0, Ordering::Relaxed);
    for i in start..start + count {
        CURRENT.store(i, Ordering::Relaxed);
        println!("S {i}");
        let mut bytes = original.whole();
        match mutant(&original, &name, i) {
            Mutant::Whole(b) => bytes = b,
            Mutant::Patches(patches) => {
                for (at, value) in &patches {
                    put(&mut bytes, *at, value);
                }
            }
        }
        let took = run(bytes, i);
        if took > Duration::from_millis(500) {
            println!("T {i} {}", took.as_millis());
        }
    }
    println!(
        "D accepted {} refused {}",
        ACCEPTED.load(Ordering::Relaxed),
        REFUSED.load(Ordering::Relaxed)
    );
    Ok(())
}

fn child(file: &Path, start: u64, count: u64) -> Result<(), Box<dyn std::error::Error>> {
    if decoded_mode() && is_preset(file) {
        return child_decoded(file, start, count);
    }
    let Some(kind) = kind_of(file) else {
        return Err("unknown file kind".into());
    };
    let original = Original::open(file)?;
    let dir = scratch(file);
    std::fs::create_dir_all(&dir)?;
    let target = place(file, &dir)?;
    let backtrace = std::env::var_os("KONTRA_FUZZ_BACKTRACE").is_some();
    std::panic::set_hook(Box::new(move |info| {
        let text = format!("{info}").replace('\n', " | ");
        println!("P {} {text}", CURRENT.load(Ordering::Relaxed));
        if backtrace {
            println!("{}", std::backtrace::Backtrace::force_capture());
        }
    }));
    let mut store = Store::new(StoreConfig::DEFAULT)?;
    store.defer_heads(true);
    let name = file.to_string_lossy();
    let big = original.big();
    if big {
        // A copy (a reflink where the filesystem allows): the original is never touched.
        let copied = Command::new("cp")
            .arg("--reflink=auto")
            .arg(file)
            .arg(&target)
            .status()?;
        if !copied.success() {
            return Err("cannot copy the file".into());
        }
    }
    // The unmutated file first: a mutant "hangs" only when it takes over 2 s
    // longer than this.
    if !big {
        std::fs::write(&target, original.whole())?;
    }
    let begin = Instant::now();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_one(kind, &target, &mut store);
    }));
    println!("B {}", begin.elapsed().as_millis());
    for i in start..start + count {
        CURRENT.store(i, Ordering::Relaxed);
        println!("S {i}");
        let patched = match mutant(&original, &name, i) {
            Mutant::Whole(bytes) => {
                std::fs::write(&target, bytes)?;
                Vec::new()
            }
            Mutant::Patches(patches) if big => {
                let file = std::fs::OpenOptions::new().write(true).open(&target)?;
                let mut olds = Vec::new();
                for (at, bytes) in &patches {
                    olds.push((*at, original.read(*at, bytes.len())));
                    file.write_all_at(bytes, *at as u64)?;
                }
                olds
            }
            Mutant::Patches(patches) => {
                let mut bytes = original.whole();
                for (at, value) in &patches {
                    put(&mut bytes, *at, value);
                }
                std::fs::write(&target, bytes)?;
                Vec::new()
            }
        };
        let began = Instant::now();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // KONTRA_FUZZ_SELFTEST: every 500th mutant panics, to show the
            // harness counts panics.
            assert!(
                std::env::var_os("KONTRA_FUZZ_SELFTEST").is_none() || i % 500 != 7,
                "self-test panic"
            );
            run_one(kind, &target, &mut store);
        }));
        if began.elapsed() > Duration::from_millis(500) {
            println!("T {i} {}", began.elapsed().as_millis());
        }
        if !patched.is_empty() {
            let file = std::fs::OpenOptions::new().write(true).open(&target)?;
            for (at, old) in patched.iter().rev() {
                file.write_all_at(old, *at as u64)?;
            }
        }
    }
    println!(
        "D accepted {} refused {}",
        ACCEPTED.load(Ordering::Relaxed),
        REFUSED.load(Ordering::Relaxed)
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

fn scratch(file: &Path) -> PathBuf {
    PathBuf::from("/data/dev/workspace/BuildScratch/kontra-fuzz").join(format!(
        "{}-{:x}",
        std::process::id(),
        fnv(&file.to_string_lossy())
    ))
}

// ---- the parent: runs children, watches them ------------------------------

#[derive(Default)]
struct Findings {
    mutants: u64,
    /// Panic site (location and message) to (count, first mutant).
    panics: BTreeMap<String, (u64, u64)>,
    hangs: Vec<u64>,
    big: Vec<(u64, u64)>,
    crashes: Vec<u64>,
    /// Mutants refused with no message.
    unexplained: Vec<u64>,
    accepted: u64,
    refused: u64,
    /// Why mutants were refused, with their counts.
    reasons: BTreeMap<String, u64>,
}

fn fuzz_file(file: &Path, mutants: u64, budget: Duration) -> Findings {
    let mut found = Findings::default();
    let begun = Instant::now();
    let mut next = 0u64;
    while next < mutants && begun.elapsed() < budget {
        let Ok(exe) = std::env::current_exe() else {
            break;
        };
        let Ok(mut proc) = Command::new(exe)
            .arg("--child")
            .arg(file)
            .arg(next.to_string())
            .arg((mutants - next).to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            break;
        };
        let Some(out) = proc.stdout.take() else {
            break;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut last = next;
        let mut done = false;
        let mut dead = None;
        let mut limit = SETUP;
        loop {
            if begun.elapsed() > budget {
                let _ = proc.kill();
                break;
            }
            match rx.recv_timeout(limit) {
                Ok(line) => {
                    let mut parts = line.splitn(3, ' ');
                    let (tag, a, rest) = (parts.next(), parts.next(), parts.next());
                    let index = a.and_then(|a| a.parse::<u64>().ok());
                    match (tag, index) {
                        (Some("B"), Some(ms)) => limit = hang() + Duration::from_millis(ms),
                        (Some("S"), Some(i)) => {
                            last = i;
                            found.mutants = found.mutants.max(i + 1);
                        }
                        (Some("P"), Some(i)) => {
                            let site = rest.unwrap_or_default().to_string();
                            let e = found.panics.entry(site).or_insert((0, i));
                            e.0 += 1;
                        }
                        (Some("A"), Some(i)) => {
                            let size = rest.and_then(|r| r.parse().ok()).unwrap_or(0);
                            found.big.push((i, size));
                        }
                        (Some("E"), Some(i)) => found.unexplained.push(i),
                        (Some("R"), Some(_)) => {
                            *found
                                .reasons
                                .entry(rest.unwrap_or_default().to_owned())
                                .or_insert(0) += 1;
                        }
                        (Some("D"), _) => {
                            done = true;
                            let nums: Vec<u64> =
                                line.split(' ').filter_map(|w| w.parse().ok()).collect();
                            if let [a, r] = nums.as_slice() {
                                found.accepted += a;
                                found.refused += r;
                            }
                        }
                        _ => {}
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    found.hangs.push(last);
                    let _ = proc.kill();
                    dead = Some("hang");
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let status = proc.wait();
        if dead.is_none() && !done && begun.elapsed() <= budget {
            // The child went away mid-mutant: an aborted allocation (reported
            // already) or a crash.
            let reported = found.big.last().is_some_and(|(i, _)| *i == last);
            if !reported && !status.is_ok_and(|s| s.success()) {
                found.crashes.push(last);
            }
        }
        if done {
            break;
        }
        next = last + 1;
    }
    found
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--child") {
        let [_, file, start, count] = args.as_slice() else {
            return Err("usage: --child FILE START COUNT".into());
        };
        return child(Path::new(file), start.parse()?, count.parse()?);
    }
    if args.first().map(String::as_str) == Some("--repro") {
        let [_, file, index] = args.as_slice() else {
            return Err("usage: --repro FILE INDEX".into());
        };
        let index: u64 = index.parse()?;
        // SAFETY-free: the variable is read by this process's panic hook only.
        return child_repro(Path::new(file), index);
    }
    let env = |key: &str, default: u64| {
        std::env::var(key)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(default)
    };
    let mutants = env("KONTRA_FUZZ_MUTANTS", 2000);
    let budget = Duration::from_secs(env("KONTRA_FUZZ_SECONDS", 240));
    let jobs = usize::try_from(env("KONTRA_FUZZ_JOBS", 3))
        .unwrap_or(3)
        .max(1);
    let files: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    let queue = std::sync::Mutex::new(files.iter().collect::<Vec<_>>());
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..jobs {
            s.spawn(|| {
                loop {
                    let Some(file) = queue.lock().ok().and_then(|mut q| q.pop()) else {
                        break;
                    };
                    let found = fuzz_file(file, mutants, budget);
                    if let Ok(mut r) = results.lock() {
                        r.push((file.clone(), found));
                    }
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap_or_default();
    results.sort_by(|a, b| a.0.cmp(&b.0));
    let mut sites: BTreeMap<String, (u64, String)> = BTreeMap::new();
    let (mut run, mut panics, mut hangs, mut big, mut crashes) = (0, 0, 0, 0, 0);
    let (mut accepted, mut refused, mut unexplained) = (0u64, 0u64, 0usize);
    let mut reasons: BTreeMap<String, u64> = BTreeMap::new();
    for (file, f) in &results {
        let count: u64 = f.panics.values().map(|p| p.0).sum();
        println!(
            "{}: {} mutants, {} panics ({} sites), {} hangs, {} big allocs, {} crashes",
            file.display(),
            f.mutants,
            count,
            f.panics.len(),
            f.hangs.len(),
            f.big.len(),
            f.crashes.len()
        );
        run += f.mutants;
        panics += count;
        hangs += f.hangs.len();
        big += f.big.len();
        crashes += f.crashes.len();
        for (site, (n, first)) in &f.panics {
            let e = sites.entry(site.clone()).or_insert((0, String::new()));
            e.0 += n;
            if e.1.is_empty() {
                e.1 = format!("{}#{first}", file.display());
            }
        }
        for i in &f.hangs {
            println!("  HANG {}#{i}", file.display());
        }
        for (i, size) in &f.big {
            println!("  BIG ALLOC {size} bytes: {}#{i}", file.display());
        }
        for i in &f.crashes {
            println!("  CRASH {}#{i}", file.display());
        }
        for i in &f.unexplained {
            println!("  UNEXPLAINED REFUSAL {}#{i}", file.display());
        }
        for (why, n) in &f.reasons {
            *reasons.entry(why.clone()).or_insert(0) += n;
        }
        accepted += f.accepted;
        refused += f.refused;
        unexplained += f.unexplained.len();
    }
    println!(
        "total: {run} mutants, {panics} panics at {} distinct sites, {hangs} hangs, {big} big allocs, {crashes} crashes",
        sites.len()
    );
    if accepted + refused > 0 {
        println!(
            "reached build: {accepted} of {} ({:.0}%), refused {refused}, {unexplained} refused without a reason",
            accepted + refused,
            100.0 * accepted as f64 / (accepted + refused) as f64
        );
    }
    let mut top: Vec<_> = reasons.iter().collect();
    top.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (why, n) in top.iter().take(12) {
        println!("  refused {n:6} x {why}");
    }
    for (site, (n, example)) in &sites {
        println!("  {n:5} x {site}   e.g. {example}");
    }
    Ok(())
}

fn child_repro(file: &Path, index: u64) -> Result<(), Box<dyn std::error::Error>> {
    // The backtrace flag is read by `child` through the environment: set it
    // by running `KONTRA_FUZZ_BACKTRACE=1 fuzz --repro FILE INDEX`.
    child(file, index, 1)
}
