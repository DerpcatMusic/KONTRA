//! Writes a Kontakt instrument (`.nki`) the way Kontakt 6.7.1 saves an
//! unencrypted one: an NI Sound container (NIS) holding a sound header, the
//! sound info, and a FastLZ-compressed subtree with the preset chunks
//! (program, groups, zones, sample file table).
//!
//! The layout was taken apart from unencrypted presets Kontakt 6.7.1 saved
//! (see `vendor/ni-file`) and every structure here is laid out as those
//! were, field for field. Values whose meaning is unknown are written as
//! Kontakt wrote them for a plain instrument; they are named `*_DEFAULT`
//! below. Known gaps, which only Kontakt itself can settle:
//! - the sound header's MD5 (16 bytes): what Kontakt hashes is unknown;
//!   this writes the MD5 of the preset chunks. Its CRC32 is verified.
//! - a zone's cached sample format (`sample_data_type`): 6 in every
//!   24-bit stereo zone seen, taken here as bytes per frame.

use crate::import::{Group, Zone};
use anyhow::{Context, Result, ensure};
use std::path::Path;

/// One sample file as the zones refer to it.
pub struct SampleFile {
    /// Path from the preset's folder, `/`-separated: `../Samples/Piano/C3.wav`.
    pub relative: String,
    pub rate: u32,
    pub channels: u16,
    pub bits: u16,
    pub frames: u64,
    /// Modification time, seconds since 1970.
    pub modified: u64,
}

/// An instrument to write: zones refer to `samples` by `Zone::sample`
/// matching `SampleFile::relative`.
pub struct Program<'a> {
    pub name: &'a str,
    pub author: &'a str,
    pub groups: &'a [Group],
    pub zones: &'a [Zone],
    pub samples: &'a [SampleFile],
    /// KSP source for the first script slot.
    pub script: Option<&'a str>,
    /// Reuse an imported native private record without reinterpreting its tail.
    pub native_private: Option<&'a ni_file::kontakt::objects::ProgramDataPrivateParams>,
    /// Preserve the saved public seed; newly authored instruments default to -1.
    pub default_key_switch: Option<i16>,
}

pub fn write(path: &Path, program: &Program) -> Result<()> {
    std::fs::write(path, container(program, path.file_name().context("preset has no file name")?.to_string_lossy().as_ref())?)
        .with_context(|| format!("Writing {}", path.display()))
}

// --- primitives ------------------------------------------------------------

fn wstr(out: &mut Vec<u8>, s: &str) {
    let units: Vec<u16> = s.encode_utf16().collect();
    out.extend((units.len() as u32).to_le_bytes());
    units.iter().for_each(|u| out.extend(u.to_le_bytes()));
}

fn chunk(id: u16, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 6);
    out.extend(id.to_le_bytes());
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

/// A structured object: flag, version, private, public and child chunks.
fn structured(version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Vec<u8> {
    let mut out = vec![1];
    out.extend(version.to_le_bytes());
    for part in [private, public, children] {
        out.extend((part.len() as u32).to_le_bytes());
        out.extend(part);
    }
    out
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

// --- preset chunks ---------------------------------------------------------

/// An empty effect slot array (eight slots, none used).
const EMPTY_FX: &str = "0012000000000000000000";
/// Program private data of a plain Kontakt 6.7.1 instrument (v0xAE).
const PROGRAM_PRIVATE_DEFAULT: &str = "000000000101000000000100000000020000000100000000000000000000000101000000000100010001010115000000ffffffffffffffff00010200000000000000000000000000000000000000000000000000000800000004000000ffffffff000100000000ffffffff010000000000";
const PROGRAM_PUBLIC_TAIL_DEFAULT: &str = "00000000000000000000000000000000ffffffff";
const EMPTY_SCRIPT: &str = "006000ffffffff00000000000000ffffffffffffffff00000000";
const QUICK_BROWSE: &str = "00010000000000";
/// Program voice limit: "<instrument>", 999 voices, oldest killed, 10 ms fade; no voice groups.
const VOICE_GROUPS_DEFAULT: &str = "0060000c0000003c0069006e0073007400720075006d0065006e0074003e00010001e70300000a000000ffffffff00000000000000000000000000000000";
const SAVE_SETTINGS_DEFAULT: &str = "00100001000000ffffffff00000000000001";
/// Group private data: 136 parameter records (`4` or `@`: 0x04 or 0x40 in
/// the second field), then a trailer, the insert effects and a tail.
const GROUP_RECORDS: &str = "44@4@44@4@4@4@44@4@4@4@4@4@44@4@4@4@4@4@4@4@44@4@4@4@4@4@4@4@4@4@44@4@4@4@4@4@4@4@4@4@4@4@44@4@4@4@4@4@4@4@4@4@4@4@4@4@44@4@4@4@4@4@4@4@";
const GROUP_TRAILER_DEFAULT: &str = "0101010000000000000000000000000107000000ffffffff";
const GROUP_TAIL_DEFAULT: &str = "00000401030000000000803f000100000000000080bf000000000000803f0000000000ffffffff000001000000000000000000ffffffff";
/// The group's internal modulators: one AHDSR on volume. Its six floats
/// follow `AHDSR_AT` (attack curve, attack, decay, hold, release, sustain).
const GROUP_MODS_DEFAULT: &str = "001200010d00c4000000018100460000000100000006000000766f6c756d650000803fffff10000010000000454e565f41484453525f564f4c554d450000000001000000000009000000454e565f414844535202000000000000006f000000070069000000019000000000000400000000000000560000003f0050000000001100000028bd000000001650c34600000000409e48440000803f00000080bf000000000000803f00000080bf000000000000803f00000080bf000000000000803f00000080bf000000000000803f00000000000000000000000000000000";
const AHDSR_AT: &str = "3f0050000000001100";
const GROUP_EXTERNAL_MODS: &str = "0012000000000000000000000000000000000000000000000000000000000000000000";
const ZONE_PRIVATE_DEFAULT: &str = "ffffffff000000000110040400000040cdcccc3e0000010000000000803fffffffffffffffffffffffffffffffff00000000000000000000000000000000000000000000000000000000000000000001000000b5010000";
const ZONE_RAW: &str = "00100000ffffffff00000000";

fn program_chunk(p: &Program, total_bytes: u64) -> Result<Vec<u8>> {
    let mut public = Vec::new();
    wstr(&mut public, p.name);
    public.extend((total_bytes as f64).to_le_bytes());
    public.push(0); // transpose
    for v in [1.0f32, 0.0, 1.0] {
        public.extend(v.to_le_bytes()); // volume, pan, tune
    }
    public.extend([1, 127, 0, 127]); // velocity and key range
    public.extend(p.default_key_switch.unwrap_or(-1).to_le_bytes());
    public.extend(61440i32.to_le_bytes()); // DFD preload
    public.extend([0; 8]); // library id, fingerprint
    public.extend(0x20u32.to_le_bytes()); // loading flags
    public.push(0); // group solo
    public.extend(28i32.to_le_bytes()); // category icon: "New"
    wstr(&mut public, ""); // credits
    wstr(&mut public, p.author);
    wstr(&mut public, ""); // url
    public.extend([0; 6]); // categories
    public.extend(hex(PROGRAM_PUBLIC_TAIL_DEFAULT));

    let mut children = Vec::new();
    children.extend(chunk(0x3a, &hex(EMPTY_FX)));
    children.extend(chunk(0x3a, &hex(EMPTY_FX)));
    for n in 1..=16 {
        let mut bus = Vec::new();
        wstr(&mut bus, &format!("Bus {n}"));
        bus.extend(1.0f32.to_le_bytes());
        bus.extend(0.0f32.to_le_bytes());
        bus.extend((-1i32).to_le_bytes());
        children.extend(chunk(0x45, &structured(0x11, &[], &bus, &chunk(0x3a, &hex(EMPTY_FX)))));
    }
    for slot in 0..5 {
        children.extend(chunk(6, &match (slot, p.script) {
            (0, Some(source)) => script(source, "Round Robin"),
            _ => hex(EMPTY_SCRIPT),
        }));
    }
    children.extend(chunk(0x4e, &hex(QUICK_BROWSE)));
    children.extend(chunk(0x3a, &hex(EMPTY_FX)));
    children.extend(chunk(0x32, &hex(VOICE_GROUPS_DEFAULT)));
    children.extend(chunk(0x33, &groups(p.groups)?));
    children.extend(chunk(0x34, &zones(p)?));
    let default_private = hex(PROGRAM_PRIVATE_DEFAULT);
    let (version, private) = p.native_private.map_or((0xae, default_private.as_slice()),
        |record| (record.version, record.data.as_slice()));
    ensure!(version == 0xae, "NKI writer public layout requires program version 0xae; private record version {version:#x} cannot be combined with it");
    Ok(chunk(0x28, &structured(version, private, &public, &children)))
}

/// A script slot (v0x60, unstructured) holding `source`.
fn script(source: &str, title: &str) -> Vec<u8> {
    let mut out = vec![0, 0x60, 0];
    out.extend((source.len() as u32).to_le_bytes());
    out.extend(source.as_bytes());
    out.extend([0, 0, 0]); // editor open, touched, bypass
    out.extend(0u32.to_le_bytes()); // no password
    out.extend((title.len() as u32).to_le_bytes());
    out.extend(title.as_bytes());
    out.extend(u32::MAX.to_le_bytes()); // no linked file
    out.extend(0u32.to_le_bytes()); // no saved variables
    out
}

fn groups(groups: &[Group]) -> Result<Vec<u8>> {
    ensure!(groups.len() <= crate::engine::MAX_GROUPS, "Too many groups for Kontakt");
    ensure!(groups.iter().all(|g| g.wavetable.is_none()),
        "The new-instrument writer does not encode wavetable sources; use the native group source writer to preserve an existing record");
    ensure!(groups.iter().all(|g| matches!(g.source_mode, None | Some(0 | 3))),
        "The new-instrument writer only encodes ordinary source modes 0 and 3; preserve the native source record for other modes");
    let mut private = Vec::new();
    for flag in GROUP_RECORDS.bytes() {
        private.extend(8u32.to_le_bytes());
        private.extend(if flag == b'4' { 4u32 } else { 0x40 }.to_le_bytes());
        private.extend(0u32.to_le_bytes());
    }
    private.extend(hex(GROUP_TRAILER_DEFAULT));
    private.extend(hex(EMPTY_FX));
    private.extend(hex(GROUP_TAIL_DEFAULT));
    // Every v0x95 group Kontakt 6.7 saves has 1722 private bytes.
    assert_eq!(private.len(), 1722, "group template");

    let mods = hex(GROUP_MODS_DEFAULT);
    let anchor = hex(AHDSR_AT);
    let at = mods.windows(anchor.len()).position(|w| w == anchor).expect("AHDSR in the group template") + anchor.len();

    let mut out = (groups.len() as u32).to_le_bytes().to_vec();
    for g in groups {
        let mut private = private.clone();
        // The existing unstructured v0x104 source record follows its presence
        // byte: one structured byte, version u16, then source mode u32.
        if let Some(mode) = g.source_mode {
            let at = private.len() - GROUP_TAIL_DEFAULT.len() / 2 + 4;
            private[at..at + 4].copy_from_slice(&mode.to_le_bytes());
        }
        let mut public = Vec::new();
        wstr(&mut public, &g.name);
        public.extend(g.gain.to_le_bytes());
        public.extend(g.pan.to_le_bytes());
        public.extend((g.tune as f32).to_le_bytes());
        public.extend([g.key_tracking as u8, g.reverse as u8, g.release_trigger as u8, g.release_trigger_note_monophonic as u8]);
        public.extend(g.release_counter_ms.to_le_bytes());
        public.extend(g.channel.to_le_bytes());
        public.extend(g.voice_group.map_or(-1, |v| v as i32).to_le_bytes());
        public.extend(6i32.to_le_bytes()); // amp position among the insert effects
        public.extend([g.muted as u8, g.soloed as u8]);
        public.extend(g.interp_quality.to_le_bytes());

        let mut mods = mods.clone();
        if let Some(env) = &g.volume_env {
            let values = [env.attack_curve, env.attack_ms, env.decay_ms, env.hold_ms, env.release_ms, env.sustain];
            for (n, v) in values.iter().enumerate() {
                mods[at + 4 * n..at + 4 * n + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        let mut children = chunk(0x3b, &mods);
        children.extend(chunk(0x3c, &hex(GROUP_EXTERNAL_MODS)));
        let mut criteria = Vec::new();
        g.start_criteria.write(&mut criteria)?;
        children.extend(chunk(0x38, &criteria));
        let mut dynamics = vec![0, 0x10, 0];
        dynamics.resize(259, 0);
        children.extend(chunk(0x4a, &dynamics));
        out.extend(structured(0x95, &private, &public, &children));
    }
    Ok(out)
}

fn zones(p: &Program) -> Result<Vec<u8>> {
    let mut out = (p.zones.len() as u32).to_le_bytes().to_vec();
    for (n, z) in p.zones.iter().enumerate() {
        let file = p
            .samples
            .iter()
            .position(|s| Path::new(&s.relative) == z.sample)
            .with_context(|| format!("Zone sample {} is not in the file table", z.sample.display()))?;
        let s = &p.samples[file];
        let mut public = Vec::new();
        public.extend((z.start as i32).to_le_bytes());
        public.extend(z.end.to_le_bytes());
        public.extend(z.start_mod.map_or(-1, |m| m as i32).to_le_bytes());
        for v in [z.low_velocity, z.high_velocity, z.low_key, z.high_key, z.fade_low_velocity, z.fade_high_velocity, z.fade_low_key, z.fade_high_key, z.root] {
            public.extend(i16::from(v).to_le_bytes());
        }
        public.extend(z.gain.to_le_bytes());
        public.extend(z.pan.to_le_bytes());
        public.extend((z.tune as f32).to_le_bytes());
        public.extend([0, 1, 0xff, 0xff, 0xff, 0xff]); // unknown, the same in every zone
        public.extend((file as i32).to_le_bytes());
        public.extend((i32::from(s.channels) * i32::from(s.bits.div_ceil(8))).to_le_bytes());
        public.extend((s.rate as i32).to_le_bytes());
        public.push(s.channels as u8);
        public.extend((s.frames.min(i32::MAX as u64) as i32).to_le_bytes());
        public.extend(0i32.to_le_bytes());
        public.extend(i32::from(z.root).to_le_bytes());
        public.extend(1.0f32.to_le_bytes());
        public.push(0);
        public.extend(0i32.to_le_bytes());

        let mut private = hex(ZONE_PRIVATE_DEFAULT);
        let id = n as u32;
        private[22..24].copy_from_slice(&(id as u16).to_le_bytes());
        let len = private.len();
        private[len - 8..len - 4].copy_from_slice(&id.to_le_bytes());
        private[len - 4..].copy_from_slice(&id.to_le_bytes());

        let mut loops = Vec::new();
        if !z.native_loops.is_empty() {
            anyhow::ensure!(z.native_loops.len() <= 8
                && z.native_loops.iter().all(|l| l.slot < 8)
                && z.native_loops.windows(2).all(|w| w[0].slot < w[1].slot),
                "Native sample loops must contain at most eight distinct ascending slots");
            loops.push(z.native_loops.iter().fold(0u8, |mask,l| mask | (1 << l.slot)));
            for l in &z.native_loops {
                loops.extend([0, 0x60, 0]);
                for v in [l.mode,l.start,l.length,l.count] { loops.extend(v.to_le_bytes()); }
                loops.push(u8::from(l.alternating));
                loops.extend(l.tuning.to_le_bytes());
                loops.extend(l.crossfade.to_le_bytes());
            }
        } else { match &z.loop_range {
            Some(l) => {
                loops.extend([1, 0, 0x60, 0]);
                loops.extend((if l.until_release { 2i32 } else { 1 }).to_le_bytes());
                loops.extend((l.start as i32).to_le_bytes());
                loops.extend(((l.end - l.start) as i32).to_le_bytes());
                loops.extend(0i32.to_le_bytes()); // count
                loops.push(u8::from(l.alternating));
                loops.extend(1.0f32.to_le_bytes());
                loops.extend((l.crossfade as i32).to_le_bytes());
            }
            None => loops.push(0),
        } }
        let mut children = chunk(0x39, &loops);
        children.extend(chunk(0x4e, &hex(QUICK_BROWSE)));
        children.extend(chunk(0x35, &hex(ZONE_RAW)));
        out.extend((z.group as u32).to_le_bytes());
        out.extend(structured(0x9a, &private, &public, &children));
    }
    Ok(out)
}

/// A file name as path segments: `..` (3), folders (2), the file (4).
fn file_name(out: &mut Vec<u8>, relative: &str) {
    let parts: Vec<&str> = relative.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    out.extend((parts.len() as i32).to_le_bytes());
    for (n, part) in parts.iter().enumerate() {
        if *part == ".." {
            out.push(3);
        } else {
            out.push(if n + 1 == parts.len() { 4 } else { 2 });
            wstr(out, part);
        }
    }
}

fn file_table(p: &Program, preset_name: &str) -> Vec<u8> {
    let mut out = 2u16.to_le_bytes().to_vec();
    out.extend(2u32.to_le_bytes()); // special files: no resource container
    out.extend([0; 8]);
    out.extend((p.samples.len() as u32).to_le_bytes());
    for s in p.samples {
        file_name(&mut out, &s.relative);
    }
    for s in p.samples {
        out.extend(s.modified.to_le_bytes());
    }
    for _ in p.samples {
        out.extend(0u32.to_le_bytes()); // offset in a monolith
    }
    out.extend(1u32.to_le_bytes()); // other files: the preset itself
    file_name(&mut out, preset_name);
    out.extend([1, 0]);
    out
}

// --- the NI Sound container ------------------------------------------------

const NISD: [u8; 4] = *b"DSIN";
const NIK4: [u8; 4] = *b"4KIN";
const REPOSITORY_ROOT_DEFAULT: &str = "010000000e7010000000000001000000010000000000000000000000000000000300000030003000300000000000000000000000000000000000";
const AUTHORIZATION_DEFAULT: &str = "0100000001000000010000000000000000000000000000000d626585";
const ITEM: &str = "01000000";

/// Item data layers, outermost first; each wraps the next and carries its own data after it.
fn layers(list: &[([u8; 4], u32, Vec<u8>)]) -> Vec<u8> {
    let mut inner = Vec::new();
    for (domain, id, data) in list.iter().rev() {
        let mut out = ((20 + inner.len() + data.len()) as u64).to_le_bytes().to_vec();
        out.extend(domain);
        out.extend(id.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        out.extend(&inner);
        out.extend(data);
        inner = out;
    }
    inner
}

fn uuid() -> [u8; 16] {
    use std::hash::BuildHasher;
    static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let state = std::collections::hash_map::RandomState::new();
    let mut out = [0; 16];
    out[..8].copy_from_slice(&state.hash_one(n).to_le_bytes());
    out[8..].copy_from_slice(&state.hash_one(!n).to_le_bytes());
    out
}

/// An item: header, data layers (ending in the plain `Item` layer), children.
fn item(mut list: Vec<([u8; 4], u32, Vec<u8>)>, children: &[(u32, [u8; 4], u32, Vec<u8>)]) -> Vec<u8> {
    list.push((NISD, 1, hex(ITEM)));
    let data = layers(&list);
    let mut table = 1u32.to_le_bytes().to_vec();
    table.extend((children.len() as u32).to_le_bytes());
    for (index, domain, id, child) in children {
        table.extend(index.to_le_bytes());
        table.extend(domain);
        table.extend(id.to_le_bytes());
        table.extend(child);
    }
    let mut out = ((40 + data.len() + table.len()) as u64).to_le_bytes().to_vec();
    out.extend(1u32.to_le_bytes());
    out.extend(b"hsin");
    out.extend(1u32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(uuid());
    out.extend(data);
    out.extend(table);
    out
}

/// The preset item's properties: saved by Kontakt 6.7.1.0.
fn preset_properties() -> Vec<u8> {
    let mut out = hex("01000000000200000001000000");
    wstr(&mut out, "6.7.1.0");
    out
}

fn sound_info(name: &str) -> Vec<u8> {
    let mut out = hex("01000000020000000100000000000000");
    wstr(&mut out, name);
    out.extend([0; 16]); // author, vendor, comment, ...
    out.extend([0xff; 8]);
    out.extend([0; 16]);
    out.extend(hex("010000000100000001000000"));
    wstr(&mut out, "Kontakt");
    out.extend(1u32.to_le_bytes());
    wstr(&mut out, "KontaktInstrument");
    out.extend(0u32.to_le_bytes());
    let properties = [("\\@color", "0"), ("\\@devicetypeflags", "1"), ("\\@soundtype", "7"), ("\\@tempo", "0"), ("\\@verl", "1.0.0"), ("\\@verm", "1.0.0"), ("\\@visib", "0")];
    out.extend((properties.len() as u32).to_le_bytes());
    for (key, value) in properties {
        wstr(&mut out, key);
        wstr(&mut out, value);
    }
    out
}

/// The 222-byte sound header (BPatchHeaderV42) Kontakt reads before the preset.
fn sound_header(p: &Program, chunks: &[u8], total_bytes: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(222);
    out.extend(0x7fa8_9012u32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(0x0110u16.to_le_bytes());
    out.extend(0xea37_631au32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // an instrument
    out.extend([0xff, 1, 7, 6]); // saved by 6.7.1
    out.extend(b"6noK"); // "Kon6", stored reversed
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    out.extend((now as u32).to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend((p.zones.len() as u16).to_le_bytes());
    out.extend((p.groups.len() as u16).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend((total_bytes.min(u32::MAX as u64) as u32).to_le_bytes());
    out.extend(0u32.to_le_bytes()); // not a monolith
    out.extend([0, 0, 7, 6]); // opens in 6.7.0 and later
    out.extend(0u32.to_le_bytes());
    out.extend(28u32.to_le_bytes());
    out.extend(b"Kontakt\0");
    out.extend([0; 3]);
    let mut url = b"(null)".to_vec();
    url.resize(85, 0);
    out.extend(url);
    out.extend(0u32.to_le_bytes());
    out.extend(0x20u32.to_le_bytes());
    out.extend(md5(chunks));
    out.extend(0u32.to_le_bytes());
    out.extend(crc32(chunks).to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend([0; 32]);
    debug_assert_eq!(out.len(), 222);
    out
}

fn container(p: &Program, preset_name: &str) -> Result<Vec<u8>> {
    let total_bytes: u64 = p.samples.iter().map(|s| s.frames * u64::from(s.channels) * u64::from(s.bits.div_ceil(8))).sum();
    let mut chunks = program_chunk(p, total_bytes)?;
    chunks.extend(chunk(0x47, &hex(SAVE_SETTINGS_DEFAULT)));
    chunks.extend(chunk(0x4b, &file_table(p, preset_name)));

    let mut preset_chunk = hex("010000000100000001000000");
    preset_chunk.extend((chunks.len() as u64).to_le_bytes());
    preset_chunk.extend(&chunks);
    preset_chunk.extend(0u32.to_le_bytes());
    preset_chunk.extend(hex("0d626585"));
    let inner = item(vec![], &[(0, NISD, 0x6d, item(vec![(NISD, 0x6d, preset_chunk)], &[]))]);
    let mut packed = vec![0; inner.len() + inner.len() / 16 + 128];
    let packed = fastlz::compress(&inner, &mut packed).map_err(|()| anyhow::anyhow!("FastLZ compression failed"))?.to_vec();
    let mut subtree = 1u32.to_le_bytes().to_vec();
    subtree.push(1);
    subtree.extend((inner.len() as u32).to_le_bytes());
    subtree.extend((packed.len() as u32).to_le_bytes());
    subtree.extend(packed);

    let sound_info = item(vec![(NISD, 0x6c, sound_info(p.name))], &[]);
    let controllers = item(vec![(NISD, 0x79, hex("010000000100000000000000"))], &[]);
    let encryption = item(vec![(NISD, 0x74, hex("0100000000")), (NISD, 0x73, subtree)], &[]);
    let header = item(vec![(NIK4, 4, sound_header(p, &chunks, total_bytes))], &[]);
    let preset = item(
        vec![(NIK4, 3, vec![0, 0]), (NISD, 0x65, preset_properties()), (NISD, 0x6a, hex("0100000002000000"))],
        &[(0, NISD, 0x6c, sound_info), (2, NISD, 0x79, controllers), (1, NISD, 0x74, encryption), (1001, NIK4, 4, header)],
    );
    Ok(item(
        vec![(NISD, 0x76, hex(REPOSITORY_ROOT_DEFAULT)), (NISD, 0x6a, hex(AUTHORIZATION_DEFAULT))],
        &[(0, NIK4, 3, preset)],
    ))
}

// --- checksums -------------------------------------------------------------

pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (!(crc & 1)).wrapping_add(1));
        }
    }
    !crc
}

/// MD5 (RFC 1321).
pub(crate) fn md5(data: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32).collect();
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend(((data.len() as u64).wrapping_mul(8)).to_le_bytes());
    let mut h = [0x6745_2301u32, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    for block in msg.chunks(64) {
        let m: Vec<u32> = block.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let [mut a, mut b, mut c, mut d] = h;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0; 16];
    for (n, x) in h.iter().enumerate() {
        out[4 * n..4 * n + 4].copy_from_slice(&x.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn unsupported_native_loop_tuning_never_silently_reaches_untuned_loop_pcm() {
        use crate::{audio::Sample, engine::{Bank, Engine, Streaming, MEMORY_LIMIT}, import::{Group, Loop, Zone}};
        use ni_file::nis::{ItemContainer, ItemType, PresetChunkItemProperties, SubtreeItem};
        fn find_mut<'a>(item: &'a mut ItemContainer, kind: &ItemType) -> Option<&'a mut ItemContainer> {
            if item.id() == *kind { return Some(item); }
            item.children.iter_mut().find_map(|item| find_mut(item, kind))
        }
        let dir = tempfile::tempdir().unwrap();
        let sample = dir.path().join("loop.wav");
        let frames:Vec<_> = (0..4096).map(|i| [0.2 + i as f32 / 32768.;2]).collect();
        let mut wav = hound::WavWriter::create(&sample, hound::WavSpec { channels:2,
            sample_rate:48000, bits_per_sample:32, sample_format:hound::SampleFormat::Float }).unwrap();
        for frame in &frames { for channel in frame { wav.write_sample(*channel).unwrap(); } }
        wav.finalize().unwrap();
        let group = Group::default();
        let zone = Zone { sample:"loop.wav".into(), loop_range:Some(Loop { start:256, end:1024,
            crossfade:0, alternating:false, until_release:false }), ..Default::default() };
        let program = super::Program { name:"Loop tuning", author:"Test", groups:&[group], zones:&[zone],
            samples:&[super::SampleFile { relative:"loop.wav".into(), rate:48000, channels:2,
                bits:32, frames:4096, modified:0 }], script:None, native_private:None, default_key_switch:None };
        // This is the exact existing writer's loop record, checked through the
        // pinned native reader before changing tuning/count or adding a later slot.
        let mut record = vec![1,0,0x60,0];
        for value in [1i32,256,768,0] { record.extend(value.to_le_bytes()); }
        record.push(0); record.extend(1f32.to_le_bytes()); record.extend(0i32.to_le_bytes());
        let cases = [1f32.to_bits(),1f32.to_bits()+1,1f32.to_bits()-1,1.0005f32.to_bits(),f32::NAN.to_bits()]
            .map(|bits| (bits,0i32,1i32,false)).into_iter().chain([
                (1f32.to_bits()+1,0,1,true), // Unlimited tuned first loop shadows the later loop.
                (1f32.to_bits()+1,7,0,true), // Inactive slots still permit the next active loop.
                (1f32.to_bits(),0,1,true), // Supported first loop remains the selected loop.
            ]);
        for (bits,count,mode,second) in cases {
            let ni_file::NIFile::NISoundContainer(mut outer) = ni_file::NIFile::read(
                std::io::Cursor::new(super::container(&program,"loop.nki").unwrap())).unwrap()
                else { panic!("creator writes NIS") };
            let encryption = find_mut(&mut outer,&ItemType::EncryptionItem).unwrap();
            let frame = encryption.data.inner.as_mut().unwrap();
            let subtree = SubtreeItem::read(std::io::Cursor::new(&frame.data)).unwrap();
            let mut inner = ItemContainer::read(std::io::Cursor::new(subtree.inner_data)).unwrap();
            let preset = find_mut(&mut inner,&ItemType::PresetChunkItem).unwrap();
            let bytes = PresetChunkItemProperties::try_from(&preset.data).unwrap().0;
            let matches:Vec<_> = bytes.windows(record.len()).enumerate().filter(|(_,part)|*part==record).map(|(at,_)|at).collect();
            assert_eq!(matches.len(),1,"fixture must edit exactly one native loop");
            let at = matches[0];
            let mut changed = record.clone(); changed[21..25].copy_from_slice(&bits.to_le_bytes());
            changed[4..8].copy_from_slice(&mode.to_le_bytes());
            changed[16..20].copy_from_slice(&count.to_le_bytes());
            if second {
                changed[0] = 3; // Native slot mask: slots zero and one are present.
                let mut later = record[1..].to_vec();
                later[7..11].copy_from_slice(&1536i32.to_le_bytes());
                later[11..15].copy_from_slice(&512i32.to_le_bytes());
                changed.extend(later);
            }
            let decoded = ni_file::kontakt::objects::LoopArray::read(std::io::Cursor::new(&changed)).unwrap();
            assert_eq!(decoded.items[0].loop_tuning.to_bits(),bits);
            assert_eq!(decoded.items[0].loop_count,count);
            assert_eq!(decoded.items[0].mode,mode);
            assert_eq!(decoded.items.len(),if second { 2 } else { 1 });
            if second {
                // Rebuild native chunk lengths through the existing writer after adding
                // the second slot; then round-trip the actual NIS container as below.
                use ni_file::kontakt::{Chunk,StructuredObject};
                let mut cursor = std::io::Cursor::new(&bytes);
                let mut program = StructuredObject::try_from(&Chunk::read(&mut cursor).unwrap()).unwrap();
                let zones = program.children.iter_mut().find(|child|child.id==0x34).unwrap();
                let mut zone = StructuredObject::read(std::io::Cursor::new(&zones.data[8..])).unwrap();
                zone.children.iter_mut().find(|child|child.id==0x39).unwrap().data = changed;
                let children:Vec<_> = zone.children.iter().flat_map(|child|super::chunk(child.id,&child.data)).collect();
                let mut data = zones.data[..8].to_vec();
                data.extend(super::structured(zone.version,&zone.private_data,&zone.public_data,&children));
                zones.data = data;
                let children:Vec<_> = program.children.iter().flat_map(|child|super::chunk(child.id,&child.data)).collect();
                let mut data = super::chunk(0x28,&super::structured(program.version,&program.private_data,&program.public_data,&children));
                data.extend(&bytes[cursor.position() as usize..]);
                preset.data.data.splice(20..20+bytes.len(),data.iter().copied());
                preset.data.data[12..20].copy_from_slice(&(data.len() as u64).to_le_bytes());
            } else {
                preset.data.data[20+at..20+at+changed.len()].copy_from_slice(&changed);
            }
            let mut bytes = Vec::new(); inner.write(&mut bytes).unwrap();
            frame.data.clear(); SubtreeItem { inner_data:bytes }.write_with_key(&mut frame.data,true,None).unwrap();
            let path = dir.path().join(format!("loop-{bits}-{count}-{mode}-{second}.nki"));
            outer.write(&mut std::fs::File::create(&path).unwrap()).unwrap();
            let instrument = crate::import::read(&path).unwrap();
            let accepted = mode == 0 || (bits == 1f32.to_bits() && count == 0);
            let legacy = accepted && (!second || mode == 0);
            assert_eq!(instrument.zones[0].loop_range.is_some(),legacy,"saved tuning bits{bits:#x}");
            if legacy {
                assert_eq!(instrument.zones[0].loop_range.as_ref().unwrap().start,if mode == 0 { 1536 } else { 256 });
            }
            let mut actual = Engine::default(); actual.attack=0.;
            let bank = Bank::load_counting(&instrument,MEMORY_LIMIT,Streaming::RamOnly,
                &[],&std::sync::atomic::AtomicU32::new(0)).unwrap();
            assert_eq!(bank.issues.iter().any(|warning| warning.contains("nonunity loop tuning")), !accepted);
            actual.set_bank(Some(Box::new(bank)));
            let mut oracle = Engine::default(); oracle.attack=0.;
            let mut zone = instrument.zones[0].clone(); zone.loop_range=None; zone.native_loops.clear();
            oracle.set_bank(Some(Box::new(Bank::from_samples(instrument.groups.clone(),vec![zone],
                vec![(sample.clone(),Sample { rate:48000,frames:frames.clone() })]).unwrap())));
            actual.note_on(0,60,127); oracle.note_on(0,60,127);
            let (mut pcm,mut reference) = ([[0.;8192];2],[[0.;8192];2]);
            assert_eq!(crate::plugin::tests::allocations(|| {
                for start in (0..8192).step_by(128) {
                    let [left,right]=&mut pcm; actual.render(&mut left[start..start+128],&mut right[start..start+128]);
                    let [left,right]=&mut reference; oracle.render(&mut left[start..start+128],&mut right[start..start+128]);
                }
            }),0);
            assert!(pcm[0][..512].iter().any(|x|x.abs()>0.01));
            if accepted { assert!(pcm[0][4096..].iter().any(|x|x.abs()>0.01),"unity loop must repeat"); }
            else { assert_eq!(pcm,reference,"unsupported native loop must report and play the ordinary finite source"); }
        }
    }

    #[test]
    fn saved_counted_ordered_loops_match_handwritten_physical_pcm_in_ram_and_stream() {
        use crate::{audio::Sample, engine::{Bank, Engine, EventChange, NoteEvent, Streaming, MEMORY_LIMIT},
            import::{Group, NativeLoop, Zone}};
        // Each phase is an independently written physical interval, with the
        // native left/right stencil flags. No production cursor/map is used.
        type Phase = (usize, usize, usize, Option<(i64,i64,bool,bool)>);
        let first = |left,right| Some((256,1024,left,right));
        let second = |left,right| Some((768,1536,left,right));
        let third = |left,right| Some((2048,2560,left,right));
        let row = |slot,mode,start,length,count| NativeLoop { slot, mode, start, length, count,
            alternating:false, tuning:1., crossfade:0 };
        let xf = |mut looped: NativeLoop, width| { looped.crossfade = width; looped };
        let inactive = NativeLoop { slot:0, mode:0, start:-12, length:-3, count:-99,
            alternating:true, tuning:7., crossfade:-4 };
        let cases: Vec<(Vec<NativeLoop>, Vec<Phase>, usize, f64, usize, bool)> = vec![
            (vec![row(2,1,256,768,1)], vec![(0,8192,0,None)], 0,1.,0,false),
            (vec![row(2,1,256,768,2)], vec![(0,1024,0,first(false,true)),
                (1024,1792,256,first(true,false)),(1792,8960,1024,None)], 0,0.5,0,false),
            (vec![row(2,1,256,768,3)], vec![(0,1024,0,first(false,true)),
                (1024,1792,256,first(true,true)),(1792,2560,256,first(true,false)),
                (2560,9728,1024,None)], 0,2.,0,false),
            (vec![inactive.clone(),row(2,1,256,768,2),row(5,1,768,768,3)],
                vec![(0,1024,0,first(false,true)),(1024,1792,256,first(true,false)),
                    (1792,2304,1024,second(false,true)),(2304,3072,768,second(true,true)),
                    (3072,3840,768,second(true,false)),(3840,10496,1536,None)], 480,0.5,0,true),
            (vec![row(2,1,256,768,2),row(5,1,768,768,3)],
                vec![(1200,1536,1200,second(false,true)),(1536,2304,768,second(true,true)),
                    (2304,3072,768,second(true,false)),(3072,9728,1536,None)], 1200,1.,0,false),
            (vec![row(1,2,256,768,0),row(4,2,768,768,0),row(7,1,2048,512,2)],
                vec![(0,1024,0,first(false,true)),(1024,1400,256,first(true,true)),
                    (1400,1792,632,first(true,false)),(1792,3328,1024,third(false,true)),
                    (3328,3840,2048,third(true,false)),(3840,9472,2560,None)], 0,1.,1400,false),
            // Negative counters repeat indefinitely but never wrap their
            // right stencil. Release still sets one final pass, skips the
            // later UntilRelease row, and admits the later counted end row.
            (vec![row(1,2,256,768,-1),row(4,2,768,768,0),row(7,1,2048,512,2)],
                vec![(0,1024,0,first(false,false)),(1024,1792,256,first(true,false)),
                    (1792,3328,1024,third(false,true)),(3328,3840,2048,third(true,false)),
                    (3840,9472,2560,None)], 0,0.5,2801,false),
            (vec![row(1,2,256,768,i32::MIN),row(4,2,768,768,0),row(7,1,2048,512,2)],
                vec![(0,1024,0,first(false,false)),(1024,1792,256,first(true,false)),
                    (1792,3328,1024,third(false,true)),(3328,3840,2048,third(true,false)),
                    (3840,9472,2560,None)], 0,0.5,2801,false),
            // The64-frame source step crosses the first end1025 at1088 and
            // skips a later unlimited1025..1029 loop. A static unroll would hang.
            (vec![row(1,1,256,769,1),row(3,1,1025,4,0),row(7,1,2048,512,2)],
                vec![(0,1088,0,None),(1088,2560,1088,third(false,true)),
                    (2560,3072,2048,third(true,false)),(3072,8704,2560,None)], 0,64.,0,false),
            // Release exactly at the first boundary precedes its pending wrap.
            (vec![row(1,2,256,768,0),row(4,2,768,768,0),row(7,1,2048,512,2)],
                vec![(0,1024,0,first(false,true)),(1024,2560,1024,third(false,true)),
                    (2560,3072,2048,third(true,false)),(3072,8704,2560,None)], 0,1.,1024,false),
            // The single unlimited loop retains its ordinary fast map. At
            // exact end the pending wrap is canceled; just past it one wrap
            // has already occurred. These explicit trajectories also cover
            // the half-frame left/right interpolation boundary on release.
            (vec![row(2,2,256,768,0)], vec![(0,1024,0,first(false,true)),
                (1024,8192,1024,None)], 0,1.,1023,false),
            (vec![row(2,2,256,768,0)], vec![(0,1024,0,first(false,true)),
                (1024,8192,1024,None)], 0,1.,1024,false),
            (vec![row(2,2,256,768,0)], vec![(0,1024,0,first(false,true)),
                (1024,1792,256,first(true,true)),(1792,8960,1024,None)], 0,1.,1025,false),
            (vec![row(2,2,256,768,0)], vec![(0,1024,0,first(false,true)),
                (1024,8192,1024,None)], 0,0.5,2047,false),
            (vec![row(2,2,256,768,0)], vec![(0,1024,0,first(false,true)),
                (1024,8192,1024,None)], 0,0.5,2048,false),
            (vec![row(2,2,256,768,0)], vec![(0,1024,0,first(false,true)),
                (1024,1792,256,first(true,true)),(1792,8960,1024,None)], 0,0.5,2049,false),
            // Native crossfade remains on the last pass, even count one.
            // Integral phases make its separately weighted source streams
            // independent of the native fractional pre-fade fragment gate.
            (vec![xf(row(2,1,256,768,1),128)], vec![(0,1024,0,first(false,false)),
                (1024,8192,1024,None)], 0,1.,0,false),
            (vec![xf(row(2,1,256,768,2),128)], vec![(0,1024,0,first(false,false)),
                (1024,1792,256,first(false,false)),(1792,8960,1024,None)], 0,1.,0,false),
            (vec![xf(row(2,1,256,768,2),128),xf(row(5,1,768,768,2),256)],
                vec![(0,1024,0,first(false,false)),(1024,1792,256,first(false,false)),
                    (1792,2304,1024,second(false,false)),(2304,3072,768,second(false,false)),
                    (3072,9728,1536,None)], 0,2.,0,true),
            // Key-up inside the first fade forces its final pass. It never
            // adds a repeat merely because that fade already began.
            (vec![xf(row(2,2,256,768,0),128)], vec![(0,1024,0,first(false,false)),
                (1024,8192,1024,None)], 0,1.,950,false),
            (vec![xf(row(2,2,256,768,0),128)], vec![(0,1024,0,first(false,false)),
                (1024,1792,256,first(false,false)),(1792,8960,1024,None)], 0,1.,1400,false),
            // Saved nonzero widths can clamp to zero. Fractional phases
            // stay valid for those ordinary, effective-zero geometries.
            (vec![xf(row(2,1,256,768,2),-4)], vec![(0,1024,0,first(false,true)),
                (1024,1792,256,first(true,false)),(1792,8960,1024,None)], 0,0.5,0,false),
            (vec![xf(row(2,1,0,1024,2),128)], vec![(0,1024,0,Some((0,1024,false,true))),
                (1024,2048,0,Some((0,1024,true,false))),(2048,9216,1024,None)], 0,0.5,0,false),
            // Nonbinary reciprocal gains cross decoded physical block1024;
            // a direct tap/width formula would miss native accumulated bits.
            (vec![xf(row(2,1,300,768,2),127)], vec![(0,1068,0,Some((300,1068,false,false))),
                (1068,1836,300,Some((300,1068,false,false))),(1836,8960,1068,None)], 0,1.,0,false),
            (vec![xf(row(2,2,300,768,-1),127)], vec![(0,1068,0,Some((300,1068,false,false))),
                (1068,1836,300,Some((300,1068,false,false))),(1836,8960,1068,None)], 0,1.,1400,false),
            // Native positive width clamps to start300 or length-minus-one3.
            (vec![xf(row(2,1,300,768,2),50_000)], vec![(0,1068,0,Some((300,1068,false,false))),
                (1068,1836,300,Some((300,1068,false,false))),(1836,8960,1068,None)], 0,1.,0,false),
            (vec![xf(row(2,1,1000,4,2),4000)], vec![(0,1004,0,Some((1000,1004,false,false))),
                (1004,1008,1000,Some((1000,1004,false,false))),(1008,8196,1004,None)], 0,1.,0,false),
        ];
        let dir = tempfile::tempdir().unwrap();
        let sample = dir.path().join("counted.wav");
        let source: Vec<_> = (0..8192).map(|i| {
            let x = 0.15 * (i as f32 * 0.03125).sin() + (i % 29) as f32 / 1024.; [x,-x*0.75]
        }).collect();
        let mut wav = hound::WavWriter::create(&sample, hound::WavSpec { channels:2,
            sample_rate:48000, bits_per_sample:32, sample_format:hound::SampleFormat::Float }).unwrap();
        for frame in &source { for value in frame { wav.write_sample(*value).unwrap(); } } wav.finalize().unwrap();
        for (case,(rows,phases,offset,step,release,mute)) in cases.into_iter().enumerate() {
            let zone = Zone { sample:"counted.wav".into(), tune:step, start_mod:Some(8191), native_loops:rows,
                ..Default::default() };
            let groups = [Group::default()]; let zones = [zone.clone()];
            let samples = [super::SampleFile { relative:"counted.wav".into(), rate:48000, channels:2,
                bits:32, frames:8192, modified:0 }];
            let program = super::Program { name:"Counted loops", author:"Test", groups:&groups, zones:&zones,
                samples:&samples, script:None, native_private:None, default_key_switch:None };
            let path = dir.path().join(format!("counted-{case}.nki"));
            std::fs::write(&path,super::container(&program,"counted.nki").unwrap()).unwrap();
            let cache = dir.path().join(format!("cache-{case}"));
            std::fs::create_dir_all(&cache).unwrap();
            let mut instrument = crate::import::read(&path).unwrap();
            assert_eq!(instrument.zones[0].native_loops,zone.native_loops,"saved holes/inactive/raw fields survive");
            crate::cache::store(&cache,&path,0,&instrument);
            assert_eq!(crate::cache::load(&cache,&path,0).unwrap().zones[0].native_loops,zone.native_loops);
            // Keep release amplitude longer than this finite physical tail;
            // both engines use the same independent envelope/gain clocks.
            instrument.groups[0].volume_env = Some(crate::import::Ahdsr {
                attack_curve:0., attack_ms:0., decay_ms:0., hold_ms:0.,
                release_ms:1000., sustain:1., unknown_flag:0, unknown_tail:Vec::new(),
            });
            instrument.groups[0].native_volume_env = false;
            let total = ((phases.last().unwrap().1 - offset) as f64 / step) as usize;
            let signed = zone.native_loops.iter().any(|row| row.mode != 0 && row.count < 0 && row.crossfade == 0);
            // Independent decoded-block reference: create the complete native
            // gain buffers in512-frame chunks, then select authored physical
            // centers. Production instead reconstructs a requested tap prefix.
            let decoded: Vec<_> = zone.native_loops.iter().filter(|row| row.mode != 0).filter_map(|row| {
                let width = row.crossfade.max(0).min(row.length-1).min(row.start);
                if width <= 0 { return None; }
                let end = row.start + row.length; let begin = end-width;
                let gains: [Vec<f32>;2] = std::array::from_fn(|mode| {
                    let (first,last) = if mode == 0 { (begin,end) } else { (begin-row.length,end-row.length) };
                    let mut values = vec![1.;8192];
                    for (block,chunk) in values.chunks_mut(512).enumerate() {
                        let at = (block*512) as i32;
                        if mode == 0 && at >= last || mode == 1 && at+512 < first { chunk.fill(0.); }
                        else if mode == 0 && first < at+512 || mode == 1 && at < last {
                            let increment = if mode == 0 { -1f32 } else { 1f32 } / width as f32;
                            let mut value = (at-first) as f32 * increment + if mode == 0 { 1. } else { 0. };
                            for gain in chunk { *gain = value.clamp(0.,1.); value += increment; }
                        }
                    }
                    values
                });
                // Literal landmarks derived from native float instructions,
                // including the discontinuous recomputation at block1024.
                let landmarks: &[(usize,u32,u32)] = match (row.start,width) {
                    (300,127) => &[(941,0x3f800000,0),(942,0x3f7dfc10,0x3c00ffa4),
                        (1023,0x3eb56b10,0x3f254a87),(1024,0x3eb162c6,0x3f274e8f),(1067,0x3c0102a4,0x3f7dfbe7)],
                    (300,300) => &[(768,0x3f800000,0),(769,0x3f7f259a,0x3b5a740e),
                        (1023,0x3e199a08,0x3f59998e),(1024,0x3e162fc8,0x3f5a7402),(1067,0x3b5a74e4,0x3f7f257e)],
                    (1000,3) => &[(1001,0x3f800000,0),(1002,0x3f2ab510,0x3eaa99e1),(1003,0x3eaabf75,0x3f2aa246)],
                    _ => &[],
                };
                for &(point,own,partner) in landmarks {
                    assert_eq!(gains[0][point].to_bits(),own);
                    assert_eq!(gains[1][point-row.length as usize].to_bits(),partner);
                }
                Some((i64::from(row.start),i64::from(end),row.length as usize,gains))
            }).collect();
            let mut expected: Vec<_> = (0..total).map(|i| {
                let v = offset as f64 + i as f64 * step;
                let &(begin,_,physical,active) = phases.iter().find(|&&(a,b,_,_)| v >= a as f64 && v < b as f64).unwrap();
                let p = physical as f64 + v - begin as f64;
                if let Some((_,_,length,gains)) = active.and_then(|(lo,hi,_,_)|
                    decoded.iter().find(|(a,b,_,_)| *a == lo && *b == hi)) {
                    assert_eq!(p.fract(),0.,"authored integral crossfade trajectory");
                    let at = p as usize;
                    return std::array::from_fn(|c| source[at][c]*gains[0][at]
                        + if at < *length { 0. } else { source[at-length][c]*gains[1][at-length] });
                }
                let floor = p.floor() as i64; let t = p.fract() as f32;
                let taps: [[f32;2];4] = std::array::from_fn(|j| {
                    let mut tap = floor - 1 + j as i64;
                    if let Some((lo,hi,left,mut right)) = active {
                        // A single UntilRelease row disables its right wrap
                        // immediately on key-up, retaining any earlier left
                        // wrap in its final traversal. The six saved cases
                        // above write the trajectory independently of maps.
                        if zone.native_loops.len() == 1 && zone.native_loops[0].mode == 2
                            && zone.native_loops[0].count == 0 && i >= release { right = false; }
                        if left && tap < lo { tap += hi-lo; }
                        if right && tap >= hi { tap -= hi-lo; }
                    }
                    if tap < 0 || tap >= 8192 { [0.;2] } else { source[tap as usize] }
                });
                std::array::from_fn(|c| {
                    let [a,b,d,e] = taps.map(|tap|tap[c]);
                    let slope = (d-a)*0.5;
                    let quadratic = a-2.5*b+2.*d-0.5*e;
                    let cubic = 0.5*(e-a)+1.5*(b-d);
                    ((cubic*t+quadratic)*t+slope)*t+b
                })
            }).collect();
            // A nonzero authored sample offset also starts the existing
            // one-millisecond declick. The unrolled reference starts at zero,
            // so include that independent amplitude law in its first48 frames.
            if offset != 0 {
                let mut gain = 0f32;
                for (i, frame) in expected.iter_mut().take(48).enumerate() {
                    gain = if i == 47 { 1. } else { gain + 1. / 48. };
                    for channel in frame { *channel *= gain; }
                }
            }
            if signed {
                assert_eq!(total,18944,"authored signed-loop physical tail");
                // Half-frame Hermite weights are [-1,9,9,-1]/16. The
                // initial right seam uses physical1024/1025, while the
                // repeated left seam uses1023 rather than physical255.
                let wrong_right = (-source[1022][0] + 9.*source[1023][0]
                    + 9.*source[256][0] - source[257][0]) / 16.;
                let wrong_left = (-source[255][0] + 9.*source[256][0]
                    + 9.*source[257][0] - source[258][0]) / 16.;
                assert!((expected[2047][0]-wrong_right).abs()>1e-4,
                    "the negative first-pass right stencil must be audibly distinguishable");
                assert!((expected[2049][0]-wrong_left).abs()>1e-4,
                    "the repeated-pass left stencil must be audibly distinguishable");
            }
            let create = |kind| {
                let bank = if kind == 0 { Bank::load_counting(&instrument,MEMORY_LIMIT,Streaming::RamOnly,
                    &[],&std::sync::atomic::AtomicU32::new(0)).unwrap() }
                else if kind < 3 { Bank::load_bare(&instrument).unwrap() }
                else { Bank::from_samples(instrument.groups.clone(),vec![Zone { sample:sample.clone(),
                    ..Zone::default() }],vec![(sample.clone(),Sample { rate:48000,frames:expected.clone() })]).unwrap() };
                assert!(bank.issues.iter().all(|issue| !issue.contains("Native sample loops unsupported")),"{:?}",bank.issues);
                let mut engine = Engine::default(); engine.reset(48000.); engine.attack=0.; engine.release=1.;
                engine.blocking_streams=true; engine.set_bank(Some(Box::new(bank)));
                let mut note = NoteEvent::new(0,60,127);
                if kind < 3 { note.offset_us = (offset * 1_000_000 / 48000) as u64; }
                let id = engine.start_event(&note).unwrap(); (engine,id)
            };
            if !decoded.is_empty() {
                for kind in [0,1] { for started in [false,true] {
                    let (mut engine,id) = create(kind);
                    if started { engine.render_checked(&mut [0.;64],&mut [0.;64]).unwrap(); }
                    engine.change_event(id,EventChange::Tune(12.*(0.5/step).log2()));
                    let (mut left,mut right) = ([9.;1],[9.;1]);
                    let mut failure = None;
                    assert_eq!(crate::plugin::tests::allocations(|| {
                        failure = Some(engine.render_checked(&mut left,&mut right).unwrap_err());
                    }),0);
                    assert_eq!(failure.unwrap().kind,crate::engine::StreamErrorKind::UnsupportedWindow,
                        "fractional crossfade pending endpoint must fail even when the only output phase is integral");
                    assert_eq!((left,right),([0.;1],[0.;1]),"no PCM from an unsupported initial or live-retuned phase");
                } }
            }
            let mut engines: Vec<_> = (0..5).map(create).collect();
            let length = total + 256;
            // The step-two ordered crossfade tail ends before frame7000.
            // Resume early enough to hear its second row's repeated fade.
            let resume = if mute && !decoded.is_empty() { 1248 } else { 7000 };
            let mut pcm = vec![vec![vec![0.;length];2];5];
            assert_eq!(crate::plugin::tests::allocations(|| {
                for (index,(engine,id)) in engines.iter_mut().enumerate() {
                    let mut at = 0; let mut block = 0;
                    while at < length {
                        if release != 0 && at == release { engine.note_off(0,60); }
                        if mute && at == 1000 { engine.change_event(*id,EventChange::Volume(0.)); }
                        if mute && at == resume { engine.change_event(*id,EventChange::Volume(1.)); }
                        let mut n = if index == 2 || index == 4 { [1,7,31,128][block % 4] } else { 128 };
                        n = n.min(length-at);
                        for boundary in [if release != 0 { release } else { length },if mute { 1000 } else { length },
                            if mute { resume } else { length },if signed { total-1 } else { length },
                            if signed { total } else { length }] { if at < boundary { n=n.min(boundary-at); } }
                        let (left,right) = pcm[index].split_at_mut(1);
                        engine.render_checked(&mut left[0][at..at+n],&mut right[0][at..at+n]).unwrap();
                        at += n; block += 1;
                        if signed && matches!(at,18943|18944) {
                            assert_eq!(engine.active_voices(),usize::from(at == 18943),
                                "signed-loop exact physical EOF case{case} engine{index} frame{at}");
                        }
                    }
                    assert_eq!(engine.active_voices(),0,"exact finite endpoint case{case} engine{index}");
                    assert_eq!(engine.underruns(),0);
                }
            }),0);
            if mute && !decoded.is_empty() {
                assert!(pcm[3].iter().all(|channel| channel[1240..1248].iter().all(|x| *x==0.)),
                    "ordered crossfade source must advance through actual silence");
                assert!(pcm[3][0][1408..1536].iter().any(|x| x.abs()>0.001),
                    "the resumed second-row crossfade must remain audibly distinguishable");
            }
            for index in 0..5 {
                let error = pcm[index].iter().flatten().zip(pcm[3].iter().flatten())
                    .map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
                assert!(error < 2e-6,"counted PCM case{case} engine{index}: {error}");
                assert!(pcm[index].iter().all(|channel|channel[total..].iter().all(|x| *x==0.)));
            }
        }

        // Finite counted tail with curved source motion: the independently
        // checked legacy LFO + lag clock must keep its exact phase through
        // sparse residency and arbitrary host fragments, including the fade.
        let zone = Zone { sample:"counted.wav".into(), native_loops:vec![row(2,1,256,768,3),
            row(5,1,768,768,2)], ..Default::default() };
        let groups = [Group::default()]; let zones = [zone];
        let samples = [super::SampleFile { relative:"counted.wav".into(), rate:48000, channels:2,
            bits:32, frames:8192, modified:0 }];
        let program = super::Program { name:"Curved counted loops", author:"Test", groups:&groups, zones:&zones,
            samples:&samples, script:None, native_private:None, default_key_switch:None };
        let path = dir.path().join("curved-counted.nki");
        std::fs::write(&path,super::container(&program,"curved-counted.nki").unwrap()).unwrap();
        let mut instrument = crate::import::read(&path).unwrap();
        instrument.groups[0].pitch_lfos = vec![crate::import::PitchLfo { extended_fade: false, fade_sync: None, shared: false, digital:None, start_phase:0.23,
            slot:0, count:1., note_value:1./24., sine:0.5, fade_ms:0., depth:0.1, targets:vec![], bypassed:false }];
        instrument.groups[0].mods = vec![crate::import::ModAssignment { cc_default:None,
            name:"Counted pitch lag".into(), source:crate::import::ModSource::MidiCc(1),
            target:crate::import::ModTarget::Pitch, intensity:0.125, invert:false, lag_ms:13, shaper:None }];
        // Physical concatenation is independent of the counted cursor. Its
        // ordinary sample EOF also owns the curved source's terminal time.
        let unfolded: Vec<_> = [0..1024,256..1024,256..1024,1024..1536,768..1536,1536..8192]
            .into_iter().flat_map(|range| source[range].iter().copied()).collect();
        assert_eq!(unfolded.len(),10496);
        // Retain Native policy in the independent unrolled source: the saved
        // Normal sampler now has a different cubic than Compatibility.
        let unfolded_path = dir.path().join("unfolded.wav");
        let mut wav = hound::WavWriter::create(&unfolded_path,hound::WavSpec {
            channels:2,sample_rate:48000,bits_per_sample:32,sample_format:hound::SampleFormat::Float,
        }).unwrap();
        for frame in &unfolded { for channel in frame { wav.write_sample(*channel).unwrap(); } }
        wav.finalize().unwrap();
        let unfolded_zones = [Zone { sample:"unfolded.wav".into(),..Default::default() }];
        let unfolded_samples = [super::SampleFile { relative:"unfolded.wav".into(),rate:48000,
            channels:2,bits:32,frames:10496,modified:0 }];
        let unfolded_program = super::Program { name:"Independent unrolled source",author:"Test",
            groups:&groups,zones:&unfolded_zones,samples:&unfolded_samples,script:None,
            native_private:None,default_key_switch:None };
        let unfolded_preset = dir.path().join("unfolded.nki");
        std::fs::write(&unfolded_preset,super::container(&unfolded_program,"unfolded.nki").unwrap()).unwrap();
        let mut unfolded_instrument = crate::import::read(&unfolded_preset).unwrap();
        unfolded_instrument.groups = instrument.groups.clone();
        let create = |kind| {
            let bank = if kind == 4 {
                Bank::load_counting(&unfolded_instrument,MEMORY_LIMIT,Streaming::RamOnly,&[],
                    &std::sync::atomic::AtomicU32::new(0)).unwrap()
            } else if kind >= 2 { Bank::load_bare(&instrument).unwrap() } else {
                Bank::load_counting(&instrument,MEMORY_LIMIT,Streaming::RamOnly,&[],
                    &std::sync::atomic::AtomicU32::new(0)).unwrap() };
            let mut engine = Engine::default(); engine.reset(48000.); engine.attack=0.;
            engine.blocking_streams=true; engine.set_bank(Some(Box::new(bank))); engine
        };
        let mut engines = [create(0),create(1),create(2),create(3),create(4)];
        let mut pcm = vec![vec![vec![0.;20000];2];5];
        let owner = crate::engine::HostNote { port:0,channel:0,key:60,id:71,clap:true };
        let mut ends = [None;5];
        assert_eq!(crate::plugin::tests::allocations(|| {
            for (index,engine) in engines.iter_mut().enumerate() {
                assert!(engine.host_note_on(owner,0,60,127,0.));
                let mut at=0; let mut block=0;
                while at < 20000 {
                    if at == 385 { engine.cc(0,1,127); }
                    let mut n=if index%2==0 { 128 } else { [1,7,31,128][block%4] };
                    n=n.min(20000-at); if at<385 { n=n.min(385-at); }
                    let (left,right)=pcm[index].split_at_mut(1);
                    engine.render_checked(&mut left[0][at..at+n],&mut right[0][at..at+n]).unwrap();
                    at+=n; block+=1;
                }
                assert_eq!(engine.active_voices(),0); assert_eq!(engine.underruns(),0);
                engine.mark_host_notes();
                assert_eq!(engine.host_note_end_at(owner),None,"physical held root survives its finite sample");
                ends[index]=engine.host_audio_end_at(owner);
                engine.host_note_off(crate::engine::HostPattern {
                    port:i32::from(owner.port),channel:i32::from(owner.channel),key:i32::from(owner.key),id:owner.id,clap:owner.clap,
                });
                engine.mark_host_notes();
                assert_eq!(engine.host_note_end_at(owner),Some(20000),"late physical key-up owns host completion independently of source EOF");
            }
        }),0);
        assert!(ends[4].is_some());
        for index in 0..4 {
            assert_eq!(ends[index],ends[4],"curved counted lifetime vs independent sample EOF engine{index}");
            let error=pcm[4].iter().flatten().zip(pcm[index].iter().flatten())
                .map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
            assert!(error<2e-6,"curved counted finite-tail PCM engine{index}: {error}");
        }
        assert!(pcm[0].iter().flatten().any(|value|value.abs()>0.01));
        assert!(pcm[0].iter().all(|channel|channel[18000..].iter().all(|value|*value==0.)));
    }

    #[test]
    fn saved_program_volume_readback_and_live_restore_reach_imported_pcm() {
        use super as nki;
        use crate::{engine::{Bank, Engine, Streaming, MEMORY_LIMIT, load_scripts}, ksp::Value};
        use ni_file::nis::{ItemContainer, ItemType, PresetChunkItemProperties, SubtreeItem};
        fn find_mut<'a>(item: &'a mut ItemContainer, kind: &ItemType) -> Option<&'a mut ItemContainer> {
            if item.id() == *kind { return Some(item); }
            item.children.iter_mut().find_map(|item| find_mut(item, kind))
        }
        let dir = tempfile::tempdir().unwrap();
        let sample = dir.path().join("tone.wav");
        let mut wav = hound::WavWriter::create(&sample, hound::WavSpec {
            channels: 2, sample_rate: 48_000, bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        }).unwrap();
        for _ in 0..4096 { wav.write_sample(0.25f32).unwrap(); wav.write_sample(0.25f32).unwrap(); }
        wav.finalize().unwrap();
        let mut wav = hound::WavWriter::create(dir.path().join("pitch.wav"), hound::WavSpec {
            channels: 2, sample_rate: 48_000, bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        }).unwrap();
        for frame in 0..65536 {
            let value = 0.25 * (std::f32::consts::TAU * frame as f32 / 192.).sin();
            wav.write_sample(value).unwrap(); wav.write_sample(value).unwrap();
        }
        wav.finalize().unwrap();
        let script = "on init\ndeclare ui_button $restore\ndeclare ui_button $reset_pitch_pan\ndeclare $saved := get_engine_par($ENGINE_PAR_VOLUME,-1,-1,-1)\ndeclare $saved_pan := get_engine_par($ENGINE_PAR_PAN,-1,-1,-1)\ndeclare $saved_tune := get_engine_par($ENGINE_PAR_TUNE,-1,-1,-1)\nmake_persistent($saved)\nmake_persistent($saved_pan)\nmake_persistent($saved_tune)\nend on\non ui_control($restore)\nset_engine_par($ENGINE_PAR_VOLUME,630859,-1,-1,-1)\nend on\non ui_control($reset_pitch_pan)\nset_engine_par($ENGINE_PAR_PAN,250000,-1,-1,-1)\nset_engine_par($ENGINE_PAR_TUNE,500000,-1,-1,-1)\n$saved_pan := get_engine_par($ENGINE_PAR_PAN,-1,-1,-1)\n$saved_tune := get_engine_par($ENGINE_PAR_TUNE,-1,-1,-1)\nend on";
        let sequence = std::cell::Cell::new(0_u32);
        let import_program = |controls: [f32; 3], transpose: i8, group: crate::import::Group, zone: crate::import::Zone| {
            let id = sequence.get(); sequence.set(id + 1);
            // Imported instruments retain the preset identity and version for
            // later preparation. Every independent variant needs its own file.
            let path = dir.path().join(format!("saved-{id}-{controls:?}-{transpose}.nki"));
            let expected_sample = zone.sample.file_name().unwrap().to_owned();
            nki::write(&path, &nki::Program {
                name: "Saved controls", author: "Test", groups: &[group], zones: &[zone],
                samples: &[nki::SampleFile { relative: "tone.wav".into(), rate: 48_000,
                    channels: 2, bits: 32, frames: 4096, modified: 0 },
                    nki::SampleFile { relative: "pitch.wav".into(), rate: 48_000,
                    channels: 2, bits: 32, frames: 65536, modified: 0 }], script: Some(script), native_private: None, default_key_switch: None,
            }).unwrap();
            // Edit only the public controls written by program_chunk:
            // UTF-16 name, sample-byte count, transpose, volume/pan/tune.
            // Decode and rewrite the supported container instead of guessing
            // compressed offsets or introducing a different preset format.
            let ni_file::NIFile::NISoundContainer(mut outer) = ni_file::NIFile::read(
                std::fs::File::open(&path).unwrap()).unwrap() else { panic!("creator writes NIS") };
            let encryption = find_mut(&mut outer, &ItemType::EncryptionItem).unwrap();
            let frame = encryption.data.inner.as_mut().unwrap();
            let subtree = SubtreeItem::read(std::io::Cursor::new(&frame.data)).unwrap();
            let mut inner = ItemContainer::read(std::io::Cursor::new(subtree.inner_data)).unwrap();
            let preset = find_mut(&mut inner, &ItemType::PresetChunkItem).unwrap();
            let bytes = PresetChunkItemProperties::try_from(&preset.data).unwrap().0;
            let mut chunks = ni_file::kontakt::KontaktChunks::read(std::io::Cursor::new(bytes)).unwrap();
            let chunk = chunks.0.iter_mut().find(|c| c.id == 0x28).unwrap();
            let program = ni_file::kontakt::objects::Program::try_from(&*chunk).unwrap();
            let params = program.params().unwrap();
            let private_len = u32::from_le_bytes(chunk.data[3..7].try_into().unwrap()) as usize;
            let volume_at = 7 + private_len + 4 + 4 + params.name.encode_utf16().count() * 2 + 8 + 1;
            chunk.data[volume_at - 1] = transpose as u8;
            for (n, value) in controls.iter().enumerate() {
                chunk.data[volume_at + 4*n..volume_at + 4*n + 4].copy_from_slice(&value.to_le_bytes());
            }
            let decoded = ni_file::kontakt::objects::Program::try_from(&*chunk).unwrap();
            let decoded = decoded.params().unwrap();
            assert_eq!([decoded.volume, decoded.pan, decoded.tune], controls);
            assert_eq!(decoded.transpose, transpose);
            let mut bytes = Vec::new(); chunks.write(&mut bytes).unwrap();
            preset.data.data[20..20 + bytes.len()].copy_from_slice(&bytes);
            let mut bytes = Vec::new(); inner.write(&mut bytes).unwrap();
            frame.data.clear();
            SubtreeItem { inner_data: bytes }.write_with_key(&mut frame.data, true, None).unwrap();
            outer.write(&mut std::fs::File::create(&path).unwrap()).unwrap();
            let instrument = crate::import::read(&path).unwrap();
            assert_eq!(instrument.zones[0].sample.file_name(), Some(expected_sample.as_os_str()));
            instrument
        };
        let import = |gain| import_program([gain, 0., 1.], 0, crate::import::Group::default(),
            crate::import::Zone { sample: "tone.wav".into(), ..Default::default() });
        let load = |instrument: crate::import::Instrument| {
            let (runtime, errors) = load_scripts(&instrument, Vec::new(), 48_000.);
            assert!(errors.is_empty(), "{errors:?}");
            let mut engine = Engine::default(); engine.attack = 0.;
            // This unpaced imported-state oracle must not race two disk workers.
            let bank = Bank::load_counting(&instrument, MEMORY_LIMIT, Streaming::RamOnly,
                &[], &std::sync::atomic::AtomicU32::new(0)).unwrap();
            assert_eq!(bank.streamed_samples(), 0, "the imported PCM oracle must be fully resident");
            engine.set_bank(Some(Box::new(bank)));
            engine.set_fx(instrument.fx.processor_for_groups(48_000., 128, &[], &instrument.groups));
            engine.set_script(runtime);
            engine
        };
        let (mut actual, mut oracle) = (load(import(0.)), load(import(1.)));
        let readback = actual.script().unwrap().persistence()[0]["$saved"].clone();
        actual.note_on(0, 60, 127); oracle.note_on(0, 60, 127);
        let (mut l, mut r, mut expected_l, mut expected_r) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
        for _ in 0..3 { actual.render(&mut l, &mut r); oracle.render(&mut expected_l, &mut expected_r); }
        for engine in [&mut actual, &mut oracle] { engine.ui_control(0, 0, 1); }
        for _ in 0..3 { actual.render(&mut l, &mut r); oracle.render(&mut expected_l, &mut expected_r); }
        let peak = expected_l.iter().chain(&expected_r).map(|x| x.abs()).fold(0f32, f32::max);
        let error = l.iter().chain(&r).zip(expected_l.iter().chain(&expected_r))
            .map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        eprintln!("saved program volume: readback={readback:?}, restored PCM error={error}, oracle peak={peak}");
        assert!(peak > 0.2, "the imported oracle must sound after the authored restore");
        assert!(readback == Value::Int(0) && error < 1e-6,
            "saved program volume was baked into zones instead of the writable instrument control");

        use crate::fx::{Chain, Effect, Kind, Params, params::{Field, Value as FxValue, SendLevels, StereoModeller}};
        let effect = |slot, kind, params| Effect {
            slot, kind, params, version: 0, bypass: false, output_gain: 1., dry_level: 0.,
        };
        // The creator writes empty FX. Augment each real imported program with
        // supported DSP, retaining its decoded Amplifier position (slot6).
        // The oracle folds volume into raw zone gain as the previous importer
        // did; scaling the completed unity output is a deliberately wrong stage.
        for (nonlinear_slot, tap_slot) in [(0, 1), (6, 7)] {
            let augment = |mut instrument: crate::import::Instrument| {
                assert_eq!(instrument.groups[0].amp_split_slot, Some(6));
                instrument.groups[0].fx = Chain { slots: vec![
                    effect(nonlinear_slot, Kind::SurroundPanner, Params::Fields(
                        crate::fx::params::layout_names(Kind::SurroundPanner).unwrap().iter().zip([1., 0.])
                            .map(|(&name, value)| Field { name, value: FxValue::Number(value) }).collect())),
                    effect(tap_slot, Kind::SendLevels, Params::SendLevels(SendLevels {
                        sends: vec![0.5], outputs: vec![1.; 17],
                    })),
                ] };
                let mut send = effect(0, Kind::StereoModeller, Params::StereoModeller(StereoModeller {
                    spread: 0., pan: 0., pseudo_stereo: false,
                }));
                send.output_gain = 1.5;
                instrument.fx.send = Chain { slots: vec![send] };
                instrument
            };
            let authored = augment(import(0.25));
            assert_eq!(authored.program.gain, 0.25);
            assert_eq!(authored.zones[0].gain, 1., "program gain remains independently writable");
            let mut folded = augment(import(1.));
            for zone in &mut folded.zones { zone.gain *= 0.25; }
            let (mut actual, mut oracle, mut unity) = (load(authored), load(folded), load(augment(import(1.))));
            let (mut a, mut ar, mut b, mut br, mut u, mut ur) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128]);
            let (mut authored_error, mut misplaced_error, mut restore_error) = (0f32, 0f32, 0f32);
            assert_eq!(crate::plugin::tests::allocations(|| {
                for engine in [&mut actual, &mut oracle, &mut unity] { engine.note_on(0, 60, 127); }
                for _ in 0..4 {
                    actual.render(&mut a, &mut ar); oracle.render(&mut b, &mut br); unity.render(&mut u, &mut ur);
                    authored_error = authored_error.max(a.iter().chain(&ar).zip(b.iter().chain(&br))
                        .map(|(a,b)| (a-b).abs()).fold(0., f32::max));
                }
                misplaced_error = a.iter().chain(&ar).zip(u.iter().chain(&ur))
                    .map(|(a,u)| (a-0.25*u).abs()).fold(0., f32::max);
                for engine in [&mut actual, &mut unity] { engine.ui_control(0, 0, 1); }
                for _ in 0..3 { actual.render(&mut a, &mut ar); unity.render(&mut u, &mut ur); }
                restore_error = a.iter().chain(&ar).zip(u.iter().chain(&ur))
                    .map(|(a,u)| (a-u).abs()).fold(0., f32::max);
            }), 0);
            eprintln!("imported program Amplifier placement: nonlinear slot{nonlinear_slot}, send slot{tap_slot}: authored error{authored_error}, final-output error{misplaced_error}, live restore error{restore_error}");
            assert!(authored_error < 1e-6 && restore_error < 1e-6,
                "authored and live program gain must use the decoded Amplifier stage");
            assert!(misplaced_error > 0.01, "the fixture must distinguish a final-output multiplier");
        }

        // Saved zone/program pan clips before adding the group's pan. The
        // instrument getter exposes its own +0.5, not the clipped zone sum.
        let group = || crate::import::Group { pan: -0.25, ..Default::default() };
        let zone = || crate::import::Zone { sample: "tone.wav".into(), pan: 0.75, ..Default::default() };
        let authored = import_program([1., 0.5, 2.], -12, group(), zone());
        assert_eq!(authored.program.pan, 0.5);
        assert_eq!(authored.program.tune, 12.);
        assert_eq!(authored.transpose, -12);
        assert_eq!(authored.zones[0].pan, 0.75);
        assert_eq!(authored.zones[0].tune, 1.);
        let mut folded = import_program([1., 0., 1.], 0, group(), zone());
        // Independent previous import pipeline: clip zone pan + saved pan,
        // and multiply native tune ratio2 by fixed transpose ratio0.5.
        folded.zones[0].pan = 1.; folded.zones[0].tune = 1.;
        let (mut actual, mut oracle, mut restored) = (load(authored), load(folded),
            load(import_program([1., -0.5, 1.], -12, group(), zone())));
        let saved = &actual.script().unwrap().persistence()[0];
        assert_eq!(saved["$saved_pan"], Value::Int(750000));
        assert_eq!(saved["$saved_tune"], Value::Int(666667), "fixed transpose is not editable tune");
        assert_eq!(crate::plugin::tests::allocations(|| {
            for engine in [&mut actual, &mut oracle, &mut restored] { engine.note_on(0, 60, 127); }
            for _ in 0..4 {
                actual.render(&mut l, &mut r); oracle.render(&mut expected_l, &mut expected_r);
                restored.render(&mut [0.; 128], &mut [0.; 128]);
                assert_eq!(l, expected_l, "saved pan uses the previous nested clip");
                assert_eq!(r, expected_r);
            }
            assert!(r.iter().copied().fold(0f32, f32::max) > 0.1);
            assert!(l.iter().zip(&r).all(|(l,r)| (4.*l-r).abs() < 1e-6),
                "pan +0.75 keeps a quarter of the left channel; a flattened sum would mute it");
            actual.ui_control(0, 1, 1);
            for _ in 0..6 {
                actual.render(&mut l, &mut r); restored.render(&mut expected_l, &mut expected_r);
            }
            assert!(l.iter().chain(&r).zip(expected_l.iter().chain(&expected_r))
                .all(|(a,b)| (a-b).abs() < 1e-6), "live pan replaces the authored value");
            assert!(l.iter().zip(&r).all(|(l,r)| (l-r).abs() < 1e-6), "the replacement centers this zone");
        }), 0);
        let saved = &actual.script().unwrap().persistence()[0];
        assert_eq!(saved["$saved_pan"], Value::Int(250000));
        assert_eq!(saved["$saved_tune"], Value::Int(500000));

        // Measure actual imported PCM against the source's 250Hz period,
        // independently of the engine's tuning/step calculation.
        let pitch_zone = || crate::import::Zone { sample: "pitch.wav".into(), ..Default::default() };
        let authored = import_program([1., 0.5, 2.], -12, crate::import::Group::default(), pitch_zone());
        let mut folded = import_program([1., 0., 1.], 0, crate::import::Group::default(), pitch_zone());
        folded.zones[0].pan = 0.5; folded.zones[0].tune = 1.;
        let (mut actual, mut oracle) = (load(authored), load(folded));
        let (mut initial, mut changed) = (Vec::with_capacity(4096), Vec::with_capacity(4096));
        assert_eq!(crate::plugin::tests::allocations(|| {
            actual.note_on(0, 60, 127); oracle.note_on(0, 60, 127);
            for _ in 0..4 { actual.render(&mut l, &mut r); oracle.render(&mut expected_l, &mut expected_r); }
        }), 0);
        for block in 0..32 {
            assert_eq!(crate::plugin::tests::allocations(|| {
                actual.render(&mut l, &mut r); oracle.render(&mut expected_l, &mut expected_r);
                initial.extend_from_slice(&r);
            }), 0);
            assert_eq!(l, expected_l, "saved/folded imported pitch PCM differs at captured block{block}");
            assert_eq!(r, expected_r, "saved/folded imported pitch PCM differs at captured block{block}");
        }
        assert_eq!(crate::plugin::tests::allocations(|| {
            actual.ui_control(0, 1, 1);
            for _ in 0..4 { actual.render(&mut l, &mut r); }
            for _ in 0..32 { actual.render(&mut l, &mut r); changed.extend_from_slice(&r); }
        }), 0);
        let frequency = |stage: &str, pcm: &[f32]| {
            let crossings: Vec<_> = pcm.windows(2).enumerate()
                .filter_map(|(i, pair)| (pair[0] <= 0. && pair[1] > 0.).then_some(i)).collect();
            let low = pcm.iter().copied().fold(f32::INFINITY, f32::min);
            let high = pcm.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            eprintln!("imported sine {stage}: frames={}, range=[{low},{high}], crossings={}", pcm.len(), crossings.len());
            assert!(crossings.len() > 8, "the imported sine {stage} must contain measurable periods: range=[{low},{high}], crossings={}", crossings.len());
            48_000. * (crossings.len()-1) as f32 / (crossings.last().unwrap()-crossings[0]) as f32
        };
        let (before, after) = (frequency("saved tune", &initial), frequency("live tune", &changed));
        eprintln!("imported program tuning: source250Hz, saved+12/fixed-12={before}Hz, editable0/fixed-12={after}Hz");
        assert!((before-250.).abs() < 0.1 && (after-125.).abs() < 0.1,
            "live tune replaces the saved octave while fixed transpose remains in the signal path");
    }


    #[test]
    fn wavetable_write_rejects_before_overwriting_an_existing_file() {
        let path = std::env::temp_dir().join(format!("kontra-wt-writer-{}-{}.nki", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::write(&path, b"authored unchanged file").unwrap();
        let group = crate::import::Group { wavetable: Some(Default::default()), ..Default::default() };
        let result = super::write(&path, &super::Program {
            name: "Authored", author: "Test", groups: &[group], zones: &[], samples: &[], script: None, native_private: None, default_key_switch: None,
        });
        let after = std::fs::read(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(result.unwrap_err().to_string().contains("does not encode wavetable"));
        assert_eq!(after, b"authored unchanged file");
    }

    #[test]
    fn native_start_conditions_survive_new_instrument_writing() {
        use ni_file::kontakt::objects::{GroupList, StartCriteriaList};
        use std::io::Cursor;
        // Authored raw IDs deliberately include an unknown mode/operator.
        let mut raw = vec![0b1000, 0, 0x70, 0];
        raw.extend(99i32.to_le_bytes());
        raw.extend((-7i32).to_le_bytes());
        for value in [24i16, 25, 64, 0, 127] { raw.extend(value.to_le_bytes()); }
        for value in [2i32, -1, -2] { raw.extend(value.to_le_bytes()); }
        raw.push(0);
        raw.extend([0xA5, 0, 0xFF]);
        let group = crate::import::Group {
            name: "Authored condition".into(),
            release_trigger: true,
            release_trigger_note_monophonic: true,
            start_criteria: StartCriteriaList::read(Cursor::new(&raw)).unwrap(),
            ..Default::default()
        };
        let json = serde_json::to_value(&group).unwrap();
        let saved: crate::import::Group = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(saved.start_criteria, group.start_criteria);
        let mut older = json;
        older.as_object_mut().unwrap().remove("start_criteria");
        older.as_object_mut().unwrap().remove("release_trigger_note_monophonic");
        let older: crate::import::Group = serde_json::from_value(older).unwrap();
        assert_eq!(older.start_criteria, StartCriteriaList::default());
        assert!(!older.release_trigger_note_monophonic, "older native presets keep their polyphonic releases");
        let bytes = super::container(&super::Program {
            name: "Authored", author: "Test", groups: &[group],
            zones: &[], samples: &[], script: None, native_private: None, default_key_switch: None,
        }, "Authored.nki").unwrap();
        let file = ni_file::NIFile::read(Cursor::new(bytes)).unwrap();
        let chunks = ni_file::kontakt::KontaktChunks::read(Cursor::new(file.inner_preset().unwrap())).unwrap();
        let program = chunks.program().unwrap().unwrap();
        let groups = GroupList::try_from(program.0.find_first(0x33).unwrap()).unwrap();
        let params = groups.groups[0].params().unwrap();
        assert!(params.release_trigger && params.release_trigger_note_monophonic);
        let criteria = params.start_criteria;
        let mut written = Vec::new();
        criteria.write(&mut written).unwrap();
        assert_eq!(written, raw, "NKI writer retains native fields without interpreting them");
    }

    #[test]
    fn ordinary_source_identity_roundtrip_preserves_mode_and_opaque_tail() {
        use ni_file::kontakt::objects::GroupList;
        use std::io::Cursor;
        let groups: Vec<_> = [None,Some(0),Some(3)].into_iter()
            .map(|source_mode| crate::import::Group { source_mode,..Default::default() }).collect();
        let bytes=super::container(&super::Program { name:"Source",author:"Test",groups:&groups,
            zones:&[],samples:&[],script:None,native_private:None,default_key_switch:None },"Source.nki").unwrap();
        let file=ni_file::NIFile::read(Cursor::new(bytes)).unwrap();
        let chunks=ni_file::kontakt::KontaktChunks::read(Cursor::new(file.inner_preset().unwrap())).unwrap();
        let program=chunks.program().unwrap().unwrap();
        let saved=GroupList::try_from(program.0.find_first(0x33).unwrap()).unwrap();
        let reference=&saved.groups[0].0.private_data;
        let at=reference.len()-super::GROUP_TAIL_DEFAULT.len()/2+4;
        for (group,mode) in saved.groups.iter().zip([3,0,3]) {
            let identity=group.source_identity().unwrap();
            assert_eq!((identity.version,identity.mode),(0x104,mode));
            let raw=&group.0.private_data;
            assert_eq!(&raw[..at],&reference[..at]);
            assert_eq!(&raw[at+4..],&reference[at+4..]);
        }
        let dir=tempfile::tempdir().unwrap();
        let path=dir.path().join("existing.nki");
        std::fs::write(&path,b"authored original").unwrap();
        let unsupported=[crate::import::Group { source_mode:Some(9),..Default::default() }];
        let error=super::write(&path,&super::Program { name:"Source",author:"Test",groups:&unsupported,
            zones:&[],samples:&[],script:None,native_private:None,default_key_switch:None }).unwrap_err();
        assert!(error.to_string().contains("only encodes ordinary source modes 0 and 3"));
        assert_eq!(std::fs::read(path).unwrap(),b"authored original");
    }

    #[test]
    fn native_program_private_options_and_opaque_tail_survive_writing() {
        use ni_file::kontakt::objects::{ProgramDataPrivateParams, SustainPedalMode};
        let mut raw = super::hex(super::PROGRAM_PRIVATE_DEFAULT);
        raw[..4].copy_from_slice(&2u32.to_le_bytes()); raw[4] = 0; raw[5] = 1;
        raw[6..10].copy_from_slice(&12i32.to_le_bytes());
        raw.extend([0xA5, 0, 0xFF]);
        let private = ProgramDataPrivateParams { version: 0xae, data: raw.clone() };
        let program = super::Program { name: "Native options", author: "Test", groups: &[],
            zones: &[], samples: &[], script: None, native_private: Some(&private), default_key_switch: None };
        let bytes = super::container(&program, "Native options.nki").unwrap();
        let file = ni_file::NIFile::read(std::io::Cursor::new(bytes)).unwrap();
        let chunks = ni_file::kontakt::KontaktChunks::read(std::io::Cursor::new(file.inner_preset().unwrap())).unwrap();
        let read = chunks.program().unwrap().unwrap().private_params();
        assert_eq!(read, private);
        let options = read.controller_options().unwrap().unwrap();
        assert_eq!(options.sustain_mode, SustainPedalMode::ControllerOnly);
        assert!(!options.standard_volume_pan && options.standard_note_modes);
        assert_eq!(options.cc7_max_db, 12);
        let json = serde_json::to_value(crate::import::Instrument {
            native_program_private: Some(read), ..Default::default() }).unwrap();
        assert_eq!(json["native_program_private"], serde_json::to_value(&private).unwrap());
        // The creator's public payload is 0xae; never pair another private
        // version with it merely to preserve bytes under the wrong layout.
        let other = ProgramDataPrivateParams { version: 0x80, data: raw };
        let program = super::Program { native_private: Some(&other), ..program };
        assert!(super::container(&program, "Other.nki").is_err());
    }

    #[test]
    fn public_default_key_switch_survives_native_write_and_import() {
        let dir = tempfile::tempdir().unwrap();
        for raw in [-1, 0, 24, 127, i16::MIN, i16::MAX] {
            let path = dir.path().join(format!("key-{raw}.nki"));
            super::write(&path, &super::Program { name: "Keyswitch seed", author: "Test",
                groups: &[], zones: &[], samples: &[], script: None,
                native_private: None, default_key_switch: Some(raw) }).unwrap();
            let file = ni_file::NIFile::read(std::fs::File::open(&path).unwrap()).unwrap();
            let chunks = ni_file::kontakt::KontaktChunks::read(std::io::Cursor::new(file.inner_preset().unwrap())).unwrap();
            assert_eq!(chunks.program().unwrap().unwrap().params().unwrap().default_key_switch, raw);
            assert_eq!(crate::import::read(&path).unwrap().default_key_switch, Some(raw));
        }
    }

    #[cfg(feature = "plugin")]
    #[test]
    fn saved_cropped_native_high_ultra_pcm_reads_physical_support_in_ram_and_stream() {
        use crate::{engine::{Ahdsr, Bank, Engine, Streaming, MEMORY_LIMIT}, import::{Group, Zone}};
        const FRAMES: usize = 262144;
        const N: usize = 128;
        let dir = tempfile::tempdir().unwrap();
        let sample = dir.path().join("crop.wav");
        let frames:Vec<_> = (0..FRAMES).map(|i| [(((i*73)%257) as f32-128.)/1024.;2]).collect();
        let mut wav = hound::WavWriter::create(&sample, hound::WavSpec { channels:2,
            sample_rate:780000, bits_per_sample:32, sample_format:hound::SampleFormat::Float }).unwrap();
        for frame in &frames { for value in frame { wav.write_sample(*value).unwrap(); } }
        wav.finalize().unwrap();
        for (quality,expected) in [(1,&NATIVE_CROP_1),(2,&NATIVE_CROP_2)] {
            let groups=[Group { interp_quality:quality, ..Default::default() }];
            let zones=[Zone { sample:"crop.wav".into(),start:1024,end:2048-FRAMES as i32,..Default::default() }];
            let samples=[super::SampleFile { relative:"crop.wav".into(),rate:780000,channels:2,bits:32,
                frames:FRAMES as u64,modified:0 }];
            let program=super::Program { name:"Cropped HQ",author:"Test",groups:&groups,zones:&zones,
                samples:&samples,script:None,native_private:None,default_key_switch:None };
            let path=dir.path().join(format!("crop-{quality}.nki"));
            super::write(&path,&program).unwrap();
            // Creating a child directory changes the imported library-root
            // dependency stamp; prepare the cache before capturing it.
            let cache=dir.path().join(format!("cache-{quality}"));
            std::fs::create_dir_all(&cache).unwrap();
            let imported=crate::import::read(&path).unwrap();
            assert_eq!(imported.groups[0].source_mode,Some(0));
            assert_eq!((imported.groups[0].interp_quality,imported.zones[0].start,imported.zones[0].end),
                (quality,1024,2048-FRAMES as i32));
            assert!(imported.missing_samples.is_empty(), "{:?}", imported.missing_samples);
            assert!(crate::cache::current(&imported.dependencies), "{:?}", imported.dependencies);
            crate::cache::store(&cache,&path,0,&imported);
        let restored=crate::cache::load(&cache,&path,0).unwrap();
        assert_eq!((restored.groups[0].interp_quality,restored.zones[0].start,restored.zones[0].end),
            (quality,1024,2048-FRAMES as i32));
        // A later prepared quality edit cannot manufacture a physical source
        // owner omitted by the original Normal-quality cropped preparation.
        let guard_name=format!("guard-{quality}.wav");
        std::fs::copy(&sample,dir.path().join(&guard_name)).unwrap();
        let guard_groups=[Group::default()];
        let guard_zones=[Zone { sample:guard_name.clone().into(),..zones[0].clone() }];
        let guard_samples=[super::SampleFile { relative:guard_name,rate:780000,channels:2,bits:32,
            frames:FRAMES as u64,modified:0 }];
        let guard_program=super::Program { name:"Prepared quality",author:"Test",groups:&guard_groups,
            zones:&guard_zones,samples:&guard_samples,script:None,native_private:None,default_key_switch:None };
        let guard_path=dir.path().join(format!("guard-{quality}.nki"));
        super::write(&guard_path,&guard_program).unwrap();
        let guard_imported=crate::import::read(&guard_path).unwrap();
        assert_eq!(guard_imported.groups[0].source_mode,Some(0));
        assert_eq!(guard_imported.groups[0].interp_quality,0);
        let guard=Bank::load(&guard_imported).unwrap();
        assert_eq!(guard.streamed_samples(),0);
        let error=guard.with_prepared_settings(|settings| settings[0].interp_quality=quality).err().unwrap();
        assert!(error.to_string().contains("reload bank at requested interpolation quality"));
            // The expected convolution is literal native output; the existing
            // ordinary sample EOF ramp is independently applied to that output.
            let mut reference=vec![[0.;2];4096];
            for (i,&bits) in expected.iter().enumerate() {
                let remaining=(1024./16.25-i as f64) as f32;
                reference[i]=[f32::from_bits(bits)*(remaining/(0.001f32*48000.)).clamp(0.,1.);2];
            }
            assert_ne!(expected[0],if quality==1 { 0xbb06f686 } else { 0xbb5d062f },"wrong pre-crop padding");
            assert_ne!(expected[63],if quality==1 { 0xbbc13f32 } else { 0xbbd40536 },"wrong post-crop padding");
            let mut plain=restored.groups[0].clone();plain.interp_quality=0;
            let reference_sample=dir.path().join(format!("reference-{quality}.wav"));
            let mut wav=hound::WavWriter::create(&reference_sample,hound::WavSpec { channels:2,
                sample_rate:48000,bits_per_sample:32,sample_format:hound::SampleFormat::Float }).unwrap();
            for frame in &reference { for value in frame { wav.write_sample(*value).unwrap(); } } wav.finalize().unwrap();
            let reference_name=format!("reference-{quality}.wav");
            let reference_groups=[plain];let reference_zones=[Zone { sample:reference_name.clone().into(),..Default::default() }];
            let reference_samples=[super::SampleFile { relative:reference_name,rate:48000,channels:2,bits:32,
                frames:4096,modified:0 }];
            let reference_program=super::Program { name:"Native output",author:"Test",groups:&reference_groups,
                zones:&reference_zones,samples:&reference_samples,script:None,native_private:None,default_key_switch:None };
            let reference_path=dir.path().join(format!("reference-{quality}.nki"));
            super::write(&reference_path,&reference_program).unwrap();
            let reference=Bank::load_counting(&crate::import::read(&reference_path).unwrap(),MEMORY_LIMIT,
                Streaming::RamOnly,&[],&std::sync::atomic::AtomicU32::new(0)).unwrap();
            let resident=Bank::load_counting(&restored,MEMORY_LIMIT,Streaming::RamOnly,&[],
                &std::sync::atomic::AtomicU32::new(0)).unwrap();
            let generous=Bank::load(&restored).unwrap();
            let limited=Bank::load_within(&restored,1).unwrap();
            assert_eq!(limited.streamed_samples(),1,"cropped playback coverage cannot suppress full physical HQ support");
            let whole=Bank::load_counting(&restored,MEMORY_LIMIT,Streaming::RamOnly,&[],
                &std::sync::atomic::AtomicU32::new(0)).unwrap();
            assert_eq!(whole.streamed_samples(),0);
            let banks=[resident,Bank::load_bare(&restored).unwrap(),generous,limited,whole,reference];
            let mut engines=banks.map(|bank| {
                let bank=bank.with_prepared_settings(|settings| { settings[0].envelope=Some(Ahdsr::UNITY); }).unwrap();
                let mut engine=Engine::default();engine.reset(48000.);engine.attack=0.;engine.blocking_streams=true;
                engine.set_bank(Some(Box::new(bank)));for _ in 0..4 { engine.note_on(0,60,127); } engine
            });
            let mut pcm=[[[0.;N];2];6];
            assert_eq!(crate::plugin::tests::allocations(|| {
                for (index,engine) in engines.iter_mut().enumerate() {
                    let mut at=0;let mut block=0;
                    while at<N {
                        let n=if index%2==0 { 128 } else { [1,7,31,127][block%4] }.min(N-at);
                        let [left,right]=&mut pcm[index];
                        engine.render_checked(&mut left[at..at+n],&mut right[at..at+n]).unwrap();at+=n;block+=1;
                    }
                    assert_eq!(engine.underruns(),0);
                }
            }),0);
            assert!(pcm[5].iter().flatten().any(|v|v.abs()>0.001));
            for index in 0..5 {
                let error=pcm[index].iter().flatten().zip(pcm[5].iter().flatten())
                    .map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
                assert!(error<2e-6,"native crop quality{quality} engine{index}: {error}");
                assert!(pcm[index].iter().all(|ch|ch[64..].iter().all(|v|v.abs()<1e-7)));
            }
        }
    }
    const NATIVE_CROP_1: [u32;64] = [
        0xbc2b5d4e,0x3c1d6c7e,0x3c04563b,0xb99a4c35,0xbc13c0d5,0xbc075e61,0x3c40a8f8,0x3c21aee1,
        0x3ab0d50e,0xbbb99c2b,0xbc55dffc,0x3b504b94,0x3c4d2996,0x3b4f8a9c,0xbb4b782f,0xbc4d6cb6,
        0xbbe99db7,0x3c3bb125,0x3bc93bf8,0xbac43ddf,0xbc2f41e2,0xbb9f3ecc,0x3c5d580a,0x3c016ced,
        0x39a4d3b9,0xbbf8260d,0xbc45ea1a,0x3be76786,0x3c370a80,0x3aed3baa,0xbb941ce4,0xbc60ed53,
        0xbb4c00a9,0x3c46f8fa,0x3b8debaa,0xbb3ba302,0xbc420033,0xba52957a,0x3c6716ac,0x3bc5fd75,
        0xba417659,0xbc1c3864,0xbc22f176,0x3c268f05,0x3c199b7c,0x3a288cb1,0xbbcad31a,0xbc66f8e5,
        0x3a97ffa7,0x3c40ed64,0x3b3398e6,0xbb929b49,0xbc46beb1,0x3b62c397,0x3c5fbff6,0x3b8feec5,
        0xbafa88fb,0xbc3935e6,0xbbdd9fe4,0x3c47f8e4,0x3bf2bf68,0xb9d31ced,0xbc0412f4,0xbc5bb114,
    ];
    const NATIVE_CROP_2: [u32;64] = [
        0xbc47040c,0x3c400bdc,0x3bdcde8b,0xb839717a,0xbc046eea,0xbc285130,0x3c613726,0x3c164e61,
        0x39f80080,0xbb72e504,0xbc7596e2,0x3b71ef37,0x3c6372f0,0x3a50a17a,0xba70953a,0xbc5c6ef4,
        0xbc03ae00,0x3c5cd247,0x3b8b5312,0xb9b4e7bb,0xbc2eddcb,0xbbd1fb08,0x3c819235,0x3bcc3433,
        0x3a170776,0xbbd13824,0xbc65284a,0x3c0655ad,0x3c40f974,0xb96d4aad,0xbb049c0d,0xbc7dc474,
        0xbb4c1c2d,0x3c60936a,0x3b156352,0xba8bd75a,0xbc506246,0xbad358ad,0x3c85b445,0x3b738fd1,
        0x3a1972a7,0xbc182bf6,0xbc3d2548,0x3c41537e,0x3c14390e,0xba1bdaa1,0xbb7fb435,0xbc85d7f8,
        0x3b057c2d,0x3c4e1cab,0x3a81113e,0xbb1e8245,0xbc612d2e,0x3b684178,0x3c7ba326,0x3af94d44,
        0x392e51f8,0xbc445d66,0xbc00a511,0x3c6770e1,0x3bc976ee,0xba13edb1,0xbbd3de4e,0xbc80a4e8,
    ];

    #[test]
    fn checksums_match_their_references() {
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(hex(&super::md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&super::md5(b"The quick brown fox jumps over the lazy dog")), "9e107d9d372bb6826bd81d3542a419d6");
        assert_eq!(super::crc32(b"123456789"), 0xcbf4_3926);
    }
}

#[cfg(test)]
#[path = "nki_envelope_bypass_tests.rs"]
mod envelope_bypass_tests;

#[cfg(all(test, feature = "plugin"))]
#[path = "nki_hq_loop_tests.rs"]
mod hq_loop_tests;

#[cfg(all(test, feature = "plugin"))]
#[path = "nki_source_identity_tests.rs"]
mod source_identity_tests;
