//! Bank-authorized artwork loading and caching, exclusively on the loader thread.
use super::{
    host::{UiArtwork, UiSnapshot, UiStrip},
    library::{Library, resolve_member},
    worker::StartConfig,
};
use crate::artwork::{self, Picture};
use anyhow::{Context, Result, ensure};
use moose::mui::mui::{scene::Image, prelude::Font};
use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    sync::Arc,
};

const COMPRESSED_LIMIT: usize = 16 << 20;
const FONT_LIMIT: usize = 4 << 20;
const IMAGE_LIMIT: usize = 64 << 20;
const CACHE_LIMIT: usize = 128 << 20;
const REFERENCE_LIMIT: usize = 4096;
const PATH_LIMIT: usize = 4096;

/// Preserve authored relative spelling; a bank-root reference has one leading slash.
pub fn key(artwork: &UiArtwork) -> String {
    if artwork.bank_root {
        format!(
            "/{}",
            artwork.path.replace('\\', "/").trim_start_matches('/')
        )
    } else if artwork.path.starts_with(['/', '\\']) {
        // Keep malformed relative references from poisoning a valid root cache entry.
        format!("\0relative:{}", artwork.path)
    } else {
        artwork.path.clone()
    }
}

/// NUL is forbidden in resource paths, so this cannot collide with a plain image.
pub fn strip_key(strip: &UiStrip) -> String {
    format!(
        "\0strip:{}:{}:{}",
        strip.frames,
        u8::from(strip.horizontal),
        key(&strip.artwork)
    )
}

fn resource_path(artwork: &UiArtwork) -> Result<String> {
    ensure!(
        !artwork.path.is_empty()
            && artwork.path.len() <= PATH_LIMIT
            && !artwork.path.contains('\0'),
        "Invalid UI artwork reference"
    );
    ensure!(
        artwork.bank_root || !artwork.path.starts_with(['/', '\\']),
        "Relative UI artwork cannot name the bank root"
    );
    // Library::data enforces traversal, volume and owning-bank authority.
    Ok(key(artwork))
}

fn rgba_bytes(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 {
        return None;
    }
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)
}

/// Inspect headers before the shared decoder can allocate its pixel buffer.
fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() > COMPRESSED_LIMIT {
        return None;
    }
    let size = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let decoder = png::Decoder::new_with_limits(
            Cursor::new(bytes),
            png::Limits {
                bytes: COMPRESSED_LIMIT,
            },
        );
        let reader = decoder.read_info().ok()?;
        (reader.info().width, reader.info().height)
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        use zune_jpeg::zune_core::{bytestream::ZCursor, options::DecoderOptions};
        let options = DecoderOptions::default()
            .set_max_width(IMAGE_LIMIT / 4)
            .set_max_height(IMAGE_LIMIT / 4);
        let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
        decoder.decode_headers().ok()?;
        let info = decoder.info()?;
        (u32::from(info.width), u32::from(info.height))
    } else {
        return None;
    };
    (rgba_bytes(size.0, size.1)? <= IMAGE_LIMIT).then_some(size)
}

/// Counts contain no resource names or bank paths. Failed references remain cached.
#[derive(Clone, Copy, Default)]
pub struct Diagnostics {
    pub failed: u32,
    pub limited: u32,
    pub font_failed: u32,
}

pub struct UiAssets {
    library: Library,
    program_path: String,
    sources: HashMap<String, Option<Arc<Image>>>,
    attempted: HashSet<String>,
    pictures: Arc<HashMap<String, Arc<Picture>>>,
    fonts: Arc<HashMap<String, Font>>,
    resident: usize,
    diagnostics: Diagnostics,
    reference_limit_reported: bool,
}

impl UiAssets {
    pub fn open(config: &StartConfig) -> Result<Self> {
        let library = Library::open(&config.bank, &config.metadata_namespace, config.content_key)?;
        ensure!(
            config
                .expected_bank_uuid
                .is_none_or(|uuid| uuid == library.bank.header.uuid),
            "UVI bank identity changed before UI artwork initialization"
        );
        if let Some(identity) = &config.content_bank {
            ensure!(
                identity == &library.bank.header.bank_name
                    || std::fs::canonicalize(identity).ok()
                        == Some(std::fs::canonicalize(&config.bank)?),
                "Content state belongs to a different bank"
            );
        }
        let program_path = resolve_member(&library.directory, &config.member)?
            .path
            .clone()
            .context("UVI program has no decoded directory path")?;
        // Opening UI assets requires only member identity, never program execution.
        Ok(Self {
            library,
            program_path,
            sources: HashMap::new(),
            attempted: HashSet::new(),
            pictures: Arc::new(HashMap::new()),
            fonts: Arc::default(),
            resident: 0,
            diagnostics: Diagnostics::default(),
            reference_limit_reported: false,
        })
    }

    pub fn diagnostics(&self) -> Diagnostics {
        self.diagnostics
    }
    pub fn resident_bytes(&self) -> usize {
        self.resident
    }

    fn reserve(&mut self, bytes: usize) -> bool {
        if self
            .resident
            .checked_add(bytes)
            .is_none_or(|total| total > CACHE_LIMIT)
        {
            self.diagnostics.limited = self.diagnostics.limited.saturating_add(1);
            false
        } else {
            true
        }
    }

    fn source(&mut self, artwork: &UiArtwork) -> Option<Arc<Image>> {
        let source_key = key(artwork);
        if let Some(image) = self.sources.get(&source_key) {
            return image.clone();
        }
        let image = (|| {
            let path = resource_path(artwork).ok()?;
            let bytes = self
                .library
                .data(&self.program_path, &path, COMPRESSED_LIMIT as u64)
                .ok()?;
            let (width, height) = dimensions(&bytes)?;
            let size = rgba_bytes(width, height)?;
            if !self.reserve(size) {
                return None;
            }
            let image = artwork::decode(&bytes)?;
            if image.width != width || image.height != height || image.rgba.len() != size {
                return None;
            }
            self.resident += size;
            Some(Arc::new(image))
        })();
        if image.is_none() {
            self.diagnostics.failed = self.diagnostics.failed.saturating_add(1);
        }
        self.sources.insert(source_key, image.clone());
        image
    }

    fn load(&mut self, artwork: &UiArtwork, strip: Option<&UiStrip>) {
        // Validate before allocating cache keys, even for caller-created snapshots.
        if artwork.path.is_empty() || artwork.path.len() > PATH_LIMIT || artwork.path.contains('\0')
        {
            if !self.reference_limit_reported {
                self.diagnostics.limited = self.diagnostics.limited.saturating_add(1);
                self.reference_limit_reported = true;
            }
            return;
        }
        let picture_key = strip.map_or_else(|| key(artwork), strip_key);
        if self.attempted.contains(&picture_key) {
            return;
        }
        if self.attempted.len() >= REFERENCE_LIMIT {
            if !self.reference_limit_reported {
                self.diagnostics.limited = self.diagnostics.limited.saturating_add(1);
                self.reference_limit_reported = true;
            }
            return;
        }
        self.attempted.insert(picture_key.clone());
        let Some(image) = self.source(artwork) else {
            return;
        };
        let frames = strip.map_or(1, |strip| strip.frames);
        let horizontal = strip.is_some_and(|strip| strip.horizontal);
        let Some((frame_width, frame_height)) = strip_dimensions(&image, frames, horizontal) else {
            self.diagnostics.failed = self.diagnostics.failed.saturating_add(1);
            return;
        };
        // A strip is cropped only once; ordinary images and single frames share pixels.
        let copied = if frames > 1 { image.rgba.len() } else { 0 };
        let allocation = copied + frames as usize * std::mem::size_of::<Arc<Image>>();
        if !self.reserve(allocation) {
            return;
        }
        let pictures = if frames == 1 {
            Some(vec![image])
        } else {
            (0..frames)
                .map(|frame| {
                    artwork::crop(
                        &image,
                        if horizontal { frame * frame_width } else { 0 },
                        if horizontal { 0 } else { frame * frame_height },
                        frame_width,
                        frame_height,
                    )
                })
                .collect::<Option<Vec<_>>>()
        };
        let Some(frames) = pictures else {
            self.diagnostics.failed = self.diagnostics.failed.saturating_add(1);
            return;
        };
        self.resident += allocation;
        Arc::make_mut(&mut self.pictures).insert(
            picture_key,
            Arc::new(Picture {
                frames,
                stretch: [true, true],
                atlas: None,
            }),
        );
    }

    /// Faces own their validated bytes and IDs. No global registration outlives
    /// this load; the published scene retains only the handles it still draws.
    pub fn fonts(&self) -> Arc<HashMap<String, Font>> { self.fonts.clone() }

    fn load_font(&mut self, path: &str) {
        if path.is_empty() { return; }
        if path.len() > PATH_LIMIT || path.contains('\0') {
            if !self.reference_limit_reported {
                self.diagnostics.limited = self.diagnostics.limited.saturating_add(1);
                self.reference_limit_reported = true;
            }
            return;
        }
        // Font and picture names share one reference budget without collisions.
        let attempted = format!("\0font:{path}");
        if self.attempted.contains(&attempted) { return; }
        if self.attempted.len() >= REFERENCE_LIMIT {
            if !self.reference_limit_reported {
                self.diagnostics.limited = self.diagnostics.limited.saturating_add(1);
                self.reference_limit_reported = true;
            }
            return;
        }
        self.attempted.insert(attempted);
        let loaded = (|| {
            let artwork = UiArtwork { path: path.replace('\\', "/"),
                bank_root: path.starts_with(['/', '\\']) };
            let path = resource_path(&artwork).ok()?;
            let remaining = CACHE_LIMIT.saturating_sub(self.resident).min(FONT_LIMIT);
            let bytes = self.library.data(&self.program_path, &path, remaining as u64).ok()?;
            if !self.reserve(bytes.len()) { return None; }
            let size = bytes.len();
            let font = Font::new(bytes).ok()?;
            self.resident += size;
            Some(font)
        })();
        if let Some(font) = loaded {
            Arc::make_mut(&mut self.fonts).insert(path.into(), font);
        } else {
            self.diagnostics.failed = self.diagnostics.failed.saturating_add(1);
            self.diagnostics.font_failed = self.diagnostics.font_failed.saturating_add(1);
        }
    }

    /// Refresh only on the loader/control thread; published pictures are immutable.
    /// Missing or rejected artwork leaves renderer controls intact with their fallback.
    pub fn refresh(&mut self, snapshots: &[UiSnapshot]) -> Arc<HashMap<String, Arc<Picture>>> {
        for snapshot in snapshots {
            if let Some(artwork) = &snapshot.root.background {
                self.load(artwork, None);
            }
            for widget in &snapshot.widgets {
                let style = &widget.style;
                if let Some(font) = &style.font { self.load_font(font); }
                for artwork in [
                    &style.background_image,
                    &style.image,
                    &style.normal_image,
                    &style.pressed_image,
                    &style.over_image,
                ]
                .into_iter()
                .flatten()
                {
                    self.load(artwork, None);
                }
                if let Some(strip) = &style.strip_image {
                    self.load(&strip.artwork, Some(strip));
                }
            }
        }
        self.pictures.clone()
    }
}

fn strip_dimensions(image: &Image, frames: u32, horizontal: bool) -> Option<(u32, u32)> {
    if frames == 0 || frames as usize > REFERENCE_LIMIT {
        return None;
    }
    let extent = if horizontal {
        image.width
    } else {
        image.height
    };
    if extent % frames != 0 {
        return None;
    }
    let size = if horizontal {
        (image.width / frames, image.height)
    } else {
        (image.width, image.height / frames)
    };
    rgba_bytes(size.0, size.1)?;
    Some(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uvi::crypto;

    fn png(width: u32, height: u32, colours: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(colours)
                .unwrap();
        }
        bytes
    }

    // Original, clear artwork inside authored UFS records. The deliberately
    // non-XML program demonstrates that artwork opening never executes a program.
    fn bank() -> StartConfig {
        fn append(bytes: &mut Vec<u8>, payload: &[u8]) -> u64 {
            let pointer = bytes.len() as u64 + 8;
            bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
            bytes.extend_from_slice(payload);
            pointer
        }
        fn named(bytes: &mut Vec<u8>, tag: u32, name: &str, length: usize, key: u64) -> u64 {
            let mut payload = vec![0; length];
            payload[..4].copy_from_slice(&tag.to_le_bytes());
            payload[4..4 + name.len()].copy_from_slice(name.as_bytes());
            crypto::transform(&mut payload[4..260], key, bytes.len() as u64 + 12);
            append(bytes, &payload)
        }
        let namespace = b"authored UI metadata";
        let key = crypto::metadata_key(namespace, "UiAuthored");
        let mut bytes = vec![0; 320];
        bytes[..4].copy_from_slice(b"UFS2");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        bytes[8..24].fill(7);
        bytes[48..58].copy_from_slice(b"UiAuthored");
        let root = named(&mut bytes, 0x2fba_3632, "Root", 272, key);
        let stripe = [
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
        ];
        let files = [
            ("program.uvip", b"not a program document".to_vec()),
            ("root.png", png(1, 1, &[255, 0, 0, 255])),
            ("relative.png", png(1, 1, &[0, 255, 0, 255])),
            ("horizontal.png", png(4, 1, &stripe)),
            ("vertical.png", png(1, 4, &stripe)),
            ("bad.png", b"invalid picture".to_vec()),
            ("font.ttf", include_bytes!("../../assets/NotoSans.ttf").to_vec()),
            ("bad.ttf", b"invalid font".to_vec()),
        ];
        let mut members = Vec::new();
        for (name, data) in &files {
            let member = named(&mut bytes, 0x6758_50e4, name, 289, key);
            let pointer = append(&mut bytes, data);
            bytes[member as usize + 260..member as usize + 268]
                .copy_from_slice(&(data.len() as u64).to_le_bytes());
            bytes[member as usize + 268..member as usize + 276]
                .copy_from_slice(&pointer.to_le_bytes());
            members.push(member);
        }
        let mut descriptor = vec![0; 34];
        descriptor[..4].copy_from_slice(&0x1847_b398u32.to_le_bytes());
        let descriptor = append(&mut bytes, &descriptor);
        bytes[root as usize + 260..root as usize + 268].copy_from_slice(&descriptor.to_le_bytes());
        let pointer = bytes.len() as u64 + 8;
        let mut table = vec![0; 24 + files.len() * 264];
        table[..4].copy_from_slice(&0x3ca8_6aafu32.to_le_bytes());
        table[4..8].copy_from_slice(&(files.len() as u32).to_le_bytes());
        for (index, ((name, _), member)) in files.iter().zip(members).enumerate() {
            let at = 8 + index * 264;
            table[at..at + name.len()].copy_from_slice(name.as_bytes());
            crypto::transform(&mut table[at..at + 256], key, pointer + at as u64);
            table[at + 256..at + 264].copy_from_slice(&member.to_le_bytes());
        }
        table[8 + files.len() * 264..].fill(255);
        append(&mut bytes, &table);
        for at in [4, 12, 20] {
            bytes[descriptor as usize + at..descriptor as usize + at + 8]
                .copy_from_slice(&pointer.to_le_bytes());
        }
        let length = bytes.len() as u64;
        bytes[32..40].copy_from_slice(&length.to_le_bytes());
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let bank = std::env::temp_dir().join(format!(
            "kontra-ui-assets-{}-{unique}.ufs",
            std::process::id()
        ));
        std::fs::write(&bank, bytes).unwrap();
        StartConfig {
            bank,
            expected_bank_uuid: Some([7; 16]),
            member: "program.uvip".into(),
            metadata_namespace: namespace.to_vec(),
            program_namespace: Vec::new(),
            content_key: None,
            content_bank: Some("UiAuthored".into()),
            sample_rate: 48000,
        }
    }

    fn artwork(path: &str, bank_root: bool) -> UiArtwork {
        UiArtwork {
            path: path.into(),
            bank_root,
        }
    }

    #[test]
    fn assets_enforce_bank_authority_and_cache_original_reference_identity() {
        let mut config = bank();
        config.expected_bank_uuid = Some([8; 16]);
        assert!(UiAssets::open(&config).is_err());
        config.expected_bank_uuid = Some([7; 16]);
        let mut assets = UiAssets::open(&config).unwrap();
        // Authored nested member identities exercise the same Library::data
        // resolution used by directory links, without duplicating a tree writer.
        assets.program_path = "Programs/program.uvip".into();
        for member in &mut assets.library.directory.files {
            member.path = Some(match member.name.as_str() {
                "root.png" => "picture.png".into(),
                "relative.png" => "Programs/picture.png".into(),
                name => format!("Programs/{name}"),
            });
        }
        let relative = artwork("picture.png", false);
        let root = artwork("\\picture.png", true);
        let malformed = artwork("/picture.png", false);
        assert_ne!(key(&relative), key(&root));
        assert_ne!(key(&malformed), key(&root));
        assets.load(&malformed, None);
        assets.load(&root, None);
        assets.load(&relative, None);
        assert_eq!(
            &assets.pictures[&key(&root)].frames[0].rgba[..4],
            &[255, 0, 0, 255]
        );
        assert_eq!(
            &assets.pictures[&key(&relative)].frames[0].rgba[..4],
            &[0, 255, 0, 255]
        );
        for path in [
            "../../escape.png",
            "C:/escape.png",
            "$volume/escape.png",
            "missing.png",
            "bad.png",
        ] {
            let reference = artwork(path, false);
            assets.load(&reference, None);
            let failures = assets.diagnostics().failed;
            assets.load(&reference, None);
            assert_eq!(assets.diagnostics().failed, failures);
            assert!(!assets.pictures.contains_key(&key(&reference)));
        }
        let snapshot = UiSnapshot {
            paint_order: Vec::new(),
            processor: 0,
            root: super::super::host::UiRoot {
                width: 100.,
                height: 100.,
                performance_view: true,
                background: Some(root),
                background_colour: None,
                key_colours: None,
            },
            widgets: Vec::new(),
        };
        let published = assets.refresh(&[snapshot.clone()]);
        assert!(Arc::ptr_eq(&published, &assets.refresh(&[snapshot])));
        std::fs::remove_file(config.bank).unwrap();
    }

    #[test]
    fn bank_fonts_own_bytes_cache_failures_and_obey_shared_limits() {
        let config = bank();
        let mut assets = UiAssets::open(&config).unwrap();
        assets.load_font("font.ttf");
        let published = assets.fonts();
        let font = published.get("font.ttf").unwrap().clone();
        let resident = assets.resident_bytes();
        assert_eq!(font.as_ref().len(), resident);
        assets.load_font("font.ttf");
        assert_eq!(assets.fonts()["font.ttf"].id(), font.id());
        assert_eq!(assets.resident_bytes(), resident);
        for path in ["bad.ttf", "missing.ttf", "../../outside.ttf", "C:/outside.ttf"] {
            assets.load_font(path);
            let failed = assets.diagnostics().font_failed;
            assets.load_font(path);
            assert_eq!(assets.diagnostics().font_failed, failed);
            assert!(!assets.fonts().contains_key(path));
        }
        assert_eq!(assets.diagnostics().font_failed, 4);
        drop(assets);
        // A scene can finish drawing after the load owner is removed.
        assert!(!font.as_ref().is_empty());
        let mut next = UiAssets::open(&config).unwrap();
        next.load_font("font.ttf");
        assert_ne!(next.fonts()["font.ttf"].id(), font.id());
        let mut bounded = UiAssets::open(&config).unwrap();
        bounded.resident = CACHE_LIMIT;
        bounded.load_font("font.ttf");
        assert!(bounded.fonts().is_empty());
        assert_eq!(bounded.resident_bytes(), CACHE_LIMIT);
        bounded.resident = 0;
        while bounded.attempted.len() < REFERENCE_LIMIT {
            bounded.attempted.insert(format!("authored-{}", bounded.attempted.len()));
        }
        bounded.load_font("/font.ttf");
        assert!(bounded.fonts().is_empty());
        assert!(bounded.diagnostics().limited > 0);
        std::fs::remove_file(config.bank).unwrap();
    }

    #[test]
    fn strips_crop_once_with_exact_orientation_and_bounded_cache() {
        let config = bank();
        let mut assets = UiAssets::open(&config).unwrap();
        for horizontal in [true, false] {
            let strip = UiStrip {
                artwork: artwork(
                    if horizontal {
                        "horizontal.png"
                    } else {
                        "vertical.png"
                    },
                    false,
                ),
                frames: 4,
                horizontal,
            };
            assert_ne!(key(&strip.artwork), strip_key(&strip));
            assets.load(&strip.artwork, Some(&strip));
            let picture = assets.pictures[&strip_key(&strip)].clone();
            assert_eq!(picture.frames.len(), 4);
            for (frame, image) in picture.frames.iter().enumerate() {
                assert_eq!((image.width, image.height), (1, 1));
                assert_eq!(image.rgba[0], 10 + frame as u8 * 30);
            }
            let resident = assets.resident_bytes();
            assets.load(&strip.artwork, Some(&strip));
            assert_eq!(assets.resident_bytes(), resident);
            assert!(Arc::ptr_eq(&picture, &assets.pictures[&strip_key(&strip)]));
            let other = UiStrip {
                frames: 2,
                ..strip.clone()
            };
            assert_ne!(strip_key(&strip), strip_key(&other));
        }
        for frames in [0, 3, 4097] {
            let strip = UiStrip {
                artwork: artwork("horizontal.png", false),
                frames,
                horizontal: true,
            };
            assets.load(&strip.artwork, Some(&strip));
            assert!(!assets.pictures.contains_key(&strip_key(&strip)));
        }
        assert!(dimensions(&vec![0; COMPRESSED_LIMIT + 1]).is_none());
        let mut huge = png(1, 1, &[0; 4]);
        huge[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        // Recompute the authored IHDR CRC so rejection tests dimensions, not corruption.
        let crc = crc32fast::hash(&huge[12..29]);
        huge[29..33].copy_from_slice(&crc.to_be_bytes());
        assert!(dimensions(&huge).is_none());
        assets.resident = CACHE_LIMIT;
        assets.load(&artwork("root.png", false), None);
        assert!(!assets.pictures.contains_key("root.png"));
        assert!(assets.diagnostics().limited > 0);
        assets.resident = 0;
        while assets.attempted.len() < REFERENCE_LIMIT {
            assets
                .attempted
                .insert(format!("authored-{}", assets.attempted.len()));
        }
        assets.load(&artwork("relative.png", false), None);
        let limits = assets.diagnostics().limited;
        assets.load(&artwork("relative.png", false), None);
        assert_eq!(assets.diagnostics().limited, limits);
        assert_eq!(assets.attempted.len(), REFERENCE_LIMIT);
        std::fs::remove_file(config.bank).unwrap();
    }
}
