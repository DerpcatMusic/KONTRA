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
    public.extend((-1i16).to_le_bytes()); // default key switch
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
    Ok(chunk(0x28, &structured(0xae, &hex(PROGRAM_PRIVATE_DEFAULT), &public, &children)))
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
        let mut public = Vec::new();
        wstr(&mut public, &g.name);
        public.extend(g.gain.to_le_bytes());
        public.extend(g.pan.to_le_bytes());
        public.extend((g.tune as f32).to_le_bytes());
        public.extend([g.key_tracking as u8, g.reverse as u8, g.release_trigger as u8, 0]);
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
        children.extend(chunk(0x38, &[0]));
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
        match &z.loop_range {
            Some(l) => {
                loops.extend([1, 0, 0x60, 0]);
                loops.extend(1i32.to_le_bytes()); // until end
                loops.extend((l.start as i32).to_le_bytes());
                loops.extend(((l.end - l.start) as i32).to_le_bytes());
                loops.extend(0i32.to_le_bytes()); // count
                loops.push(0); // alternating
                loops.extend(1.0f32.to_le_bytes());
                loops.extend((l.crossfade as i32).to_le_bytes());
            }
            None => loops.push(0),
        }
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
    fn checksums_match_their_references() {
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(hex(&super::md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&super::md5(b"The quick brown fox jumps over the lazy dog")), "9e107d9d372bb6826bd81d3542a419d6");
        assert_eq!(super::crc32(b"123456789"), 0xcbf4_3926);
    }
}
