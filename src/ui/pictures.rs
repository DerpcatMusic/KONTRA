//! A script interface's pictures, decoded from the library the loader
//! already read their layouts from.

use super::ir_view::Picture;
use moose::mui::mui::scene::Image;
use sampler_ui_ir as ir;
use sampler_uvi::ResourceError;
use std::{path::Path, sync::Arc};

/// Where an instrument's resources come from.
pub struct Source {
    kontakt: Option<sampler_kontakt::Resources>,
    uvi: Option<sampler_uvi::Resources>,
    strips: Strips,
    resource_failure: Option<ResourceError>,
    #[cfg(any(feature = "shots", test))]
    current_probe: Option<AssetProbe>,
    #[cfg(any(feature = "shots", test))]
    probes: Vec<AssetProbe>,
    #[cfg(any(feature = "shots", test))]
    probe_dropped: usize,
    #[cfg(feature = "shots")]
    pub scan: Scan,
}

/// Whole decoded strips, least recently used out first, bounded by bytes: a
/// turning knob reads and decodes its PNG once, not once per frame.
#[derive(Default)]
struct Strips {
    entries: Vec<(String, Vec<Arc<Image>>, u64)>,
    bytes: usize,
    tick: u64,
}

const STRIP_BUDGET: usize = 48 << 20;
#[cfg(any(feature = "shots", test))]
pub(super) const PROBE_LIMIT: usize = 512;

/// Request evidence from the actual asset worker. No resource bytes or access
/// values are retained. Paths belong in the user's local report only.
#[cfg(any(feature = "shots", test))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct AssetProbe {
    pub index: Option<usize>,
    pub path: String,
    pub path_truncated: bool,
    pub kind: &'static str,
    pub resolver: &'static str,
    pub status: &'static str,
    pub codec: Option<&'static str>,
    pub duration_ms: f64,
    pub read_ms: f64,
    pub decode_ms: f64,
    pub queue_ms: Option<f64>,
    pub permit_wait_ms: Option<f64>,
    pub encoded_bytes: usize,
    pub prepared_bytes: usize,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frames: u32,
    pub frame: usize,
    pub target: [u32; 2],
    pub window: Option<[u32; 4]>,
    pub strip_cache_hit: bool,
}

impl Strips {
    fn get(&mut self, key: &str) -> Option<&Vec<Arc<Image>>> {
        self.tick += 1;
        let tick = self.tick;
        let e = self.entries.iter_mut().find(|e| e.0 == key)?;
        e.2 = tick;
        Some(&e.1)
    }
    fn put(&mut self, key: String, frames: Vec<Arc<Image>>) -> bool {
        let len: usize = frames.iter().map(|f| f.rgba.len()).sum();
        if len > STRIP_BUDGET {
            return false;
        }
        self.bytes += len;
        while self.bytes > STRIP_BUDGET {
            let Some(oldest) = (0..self.entries.len()).min_by_key(|&i| self.entries[i].2) else {
                break;
            };
            let (_, frames, _) = self.entries.swap_remove(oldest);
            self.bytes -= frames.iter().map(|f| f.rgba.len()).sum::<usize>();
        }
        self.tick += 1;
        self.entries.push((key, frames, self.tick));
        true
    }
}

#[cfg(feature = "shots")]
#[derive(Clone, Copy, Default)]
pub struct Scan {
    pub discovery_us: u64,
    pub read_us: u64,
    pub decode_us: u64,
    pub encoded_bytes: usize,
    pub strip_cache_hits: usize,
    pub lookups: usize,
    pub lookup_ok: usize,
    pub lookup_missing: usize,
    pub lookup_invalid: usize,
    pub lookup_ambiguous: usize,
    pub lookup_corrupt: usize,
    pub lookup_limit: usize,
    pub lookup_read: usize,
    pub lookup_unavailable: usize,
    pub decodes: usize,
    pub decode_ok: usize,
    pub fonts: usize,
    pub font_ok: usize,
    pub preparation_completed: usize,
    pub preparation_completed_bytes: usize,
    pub preparation_max_key_bytes: usize,
    pub preparation_wanted_peak_bytes: usize,
    pub preparation_oversized: usize,
    pub preparation_key_budget: usize,
    pub preparation_evicted: usize,
    pub preparation_requeued: usize,
}

impl Source {
    pub(super) fn for_program(instrument: &Path, program: u32) -> Result<Self, ResourceError> {
        if Self::is_uvi(instrument) {
            return Ok(Self::of(instrument));
        }
        #[cfg(feature = "shots")]
        let start = std::time::Instant::now();
        let kontakt =
            sampler_kontakt::Resources::for_program(instrument, program).map_err(kontakt_error)?;
        let mut source = Self::from_resources(Some(kontakt), None);
        #[cfg(feature = "shots")]
        {
            source.scan.discovery_us = start.elapsed().as_micros().min(u64::MAX as u128) as u64;
        }
        Ok(source)
    }

    pub fn of(instrument: &Path) -> Self {
        #[cfg(feature = "shots")]
        let start = std::time::Instant::now();
        let uvi = Self::is_uvi(instrument).then(|| sampler_uvi::Resources::of(instrument));
        // A UVI program has exactly one authority. Walking Kontakt resource
        // trees here was unrelated work and could scan an entire bank's parent.
        let kontakt = uvi
            .is_none()
            .then(|| sampler_kontakt::Resources::of(instrument));
        let mut source = Self::from_resources(kontakt, uvi);
        #[cfg(feature = "shots")]
        {
            source.scan.discovery_us = start.elapsed().as_micros().min(u64::MAX as u128) as u64;
        }
        source
    }
    fn is_uvi(instrument: &Path) -> bool {
        instrument.ancestors().any(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ufs") || e.eq_ignore_ascii_case("uvip"))
        })
    }
    pub(super) fn failed(error: ResourceError) -> Self {
        let mut source = Self::from_resources(None, None);
        source.resource_failure = Some(error);
        source
    }
    fn from_resources(
        kontakt: Option<sampler_kontakt::Resources>,
        uvi: Option<sampler_uvi::Resources>,
    ) -> Self {
        Self {
            kontakt,
            uvi,
            strips: Strips::default(),
            resource_failure: None,
            #[cfg(any(feature = "shots", test))]
            current_probe: None,
            #[cfg(any(feature = "shots", test))]
            probes: Vec::new(),
            #[cfg(any(feature = "shots", test))]
            probe_dropped: 0,
            #[cfg(feature = "shots")]
            scan: Scan::default(),
        }
    }

    pub(crate) fn read_result(&mut self, path: &str) -> Result<Option<Vec<u8>>, ResourceError> {
        #[cfg(any(feature = "shots", test))]
        let start = std::time::Instant::now();
        let result = if path.is_empty() || path.len() > 4096 || path.contains('\0') {
            Err(ResourceError::InvalidPath)
        } else if let Some(failure) = self.resource_failure {
            Err(failure)
        } else {
            match &self.uvi {
                Some(uvi) => uvi.read_with_origin_result(path),
                None => self
                    .kontakt
                    .as_mut()
                    .ok_or(ResourceError::Unavailable)
                    .and_then(|kontakt| {
                        kontakt.read_with_origin_result(path).map_err(kontakt_error)
                    }),
            }
        };
        #[cfg(any(feature = "shots", test))]
        if let Ok(Some((_, origin))) = &result
            && let Some(probe) = &mut self.current_probe
        {
            probe.resolver = origin;
        }
        let result = result.map(|found| found.map(|(bytes, _)| bytes));
        #[cfg(feature = "shots")]
        {
            self.scan.read_us = self
                .scan
                .read_us
                .saturating_add(start.elapsed().as_micros().min(u64::MAX as u128) as u64);
            self.scan.encoded_bytes = self.scan.encoded_bytes.saturating_add(
                result
                    .as_ref()
                    .ok()
                    .and_then(|b| b.as_ref())
                    .map_or(0, Vec::len),
            );
            self.scan.lookups += 1;
            match &result {
                Ok(Some(_)) => self.scan.lookup_ok += 1,
                Ok(None) => {
                    self.scan.lookup_missing += 1;
                    if std::env::var_os("KONTRA_RESOURCE_PROBE_NAMES").is_some() {
                        eprintln!("RESOURCE_MISS path={path:?}");
                    }
                }
                Err(ResourceError::InvalidPath) => self.scan.lookup_invalid += 1,
                Err(ResourceError::Ambiguous) => self.scan.lookup_ambiguous += 1,
                Err(ResourceError::Corrupt) => self.scan.lookup_corrupt += 1,
                Err(ResourceError::Limit) => self.scan.lookup_limit += 1,
                Err(ResourceError::Read) => self.scan.lookup_read += 1,
                Err(ResourceError::Unavailable) => self.scan.lookup_unavailable += 1,
            }
        }
        #[cfg(any(feature = "shots", test))]
        if let Some(probe) = &mut self.current_probe {
            probe.read_ms += start.elapsed().as_secs_f64() * 1000.;
            probe.status = match &result {
                Ok(Some(bytes)) => {
                    probe.encoded_bytes += bytes.len();
                    "found"
                }
                Ok(None) => "missing",
                Err(error) => resource_category(*error),
            };
        }
        result
    }
    pub(super) fn native_names(&mut self) -> Vec<String> {
        let Some(kontakt) = &mut self.kontakt else {
            return Vec::new();
        };
        let mut names = kontakt.names("resources/native_ui/");
        names.extend(kontakt.names("native_ui/"));
        names.sort();
        names.dedup();
        names
    }
    pub fn font(&mut self, asset: &ir::Asset) -> Option<moose::mui::mui::prelude::Font> {
        #[cfg(any(feature = "shots", test))]
        let start = self.begin_probe(asset, 0, [0; 2], None);
        let bytes = self.read_result(&asset.path).ok().flatten();
        #[cfg(any(feature = "shots", test))]
        let decode_start = std::time::Instant::now();
        let font = bytes.and_then(|bytes| moose::mui::mui::prelude::Font::new(bytes).ok());
        #[cfg(any(feature = "shots", test))]
        self.finish_font_probe(start, decode_start, font.as_ref());
        font
    }
    #[cfg(any(feature = "shots", test))]
    pub(super) fn finish_font_probe(
        &mut self,
        start: std::time::Instant,
        decode_start: std::time::Instant,
        font: Option<&moose::mui::mui::prelude::Font>,
    ) {
        {
            let elapsed = decode_start.elapsed();
            if let Some(probe) = &mut self.current_probe {
                probe.decode_ms = elapsed.as_secs_f64() * 1000.;
                if font.is_some() {
                    probe.status = "found";
                    probe.codec = Some("font");
                    probe.prepared_bytes = font.map_or(0, |f| f.as_ref().len());
                } else if probe.status == "found" {
                    probe.status = "decode_failed";
                }
            }
            #[cfg(feature = "shots")]
            {
                self.scan.decode_us = self
                    .scan
                    .decode_us
                    .saturating_add(elapsed.as_micros().min(u64::MAX as u128) as u64);
            }
            self.finish_probe(start);
        }
        #[cfg(feature = "shots")]
        {
            self.scan.fonts += 1;
            self.scan.font_ok += usize::from(font.is_some());
        }
    }

    /// Compatibility probe: returns one frame. A bounded strip cache can
    /// prepare its neighboring frames; production uses requested device size.
    pub fn load(&mut self, asset: &ir::Asset) -> Option<Arc<Picture>> {
        self.load_frame(asset, 0, [u32::MAX; 2], None, || false)
    }
    pub(super) fn load_frame(
        &mut self,
        asset: &ir::Asset,
        frame: usize,
        target: [u32; 2],
        window: Option<[u32; 4]>,
        canceled: impl Fn() -> bool,
    ) -> Option<Arc<Picture>> {
        #[cfg(any(feature = "shots", test))]
        let start = self.begin_probe(asset, frame, target, window);
        let picture = self.load_frame_inner(asset, frame, target, window, &canceled);
        #[cfg(any(feature = "shots", test))]
        {
            if let Some(probe) = &mut self.current_probe {
                if let Some(picture) = &picture {
                    probe.status = "found";
                    probe.prepared_bytes = picture.frames.iter().map(|i| i.rgba.len()).sum();
                    if let Some(image) = picture.frames.first() {
                        probe.width = Some(image.width);
                        probe.height = Some(image.height);
                    }
                } else if canceled() {
                    probe.status = "canceled";
                } else if probe.status == "found" {
                    probe.status = "decode_failed";
                }
            }
            self.finish_probe(start);
        }
        picture
    }
    fn load_frame_inner(
        &mut self,
        asset: &ir::Asset,
        frame: usize,
        target: [u32; 2],
        window: Option<[u32; 4]>,
        canceled: impl Fn() -> bool,
    ) -> Option<Arc<Picture>> {
        if canceled() {
            return None;
        }
        let meta = match asset.kind {
            ir::AssetKind::Image(m) => m,
            ir::AssetKind::BitmapFont => ir::ImageMeta::default(),
            _ => return None,
        };
        let strip =
            window.is_none() && meta.frames > 1 && matches!(asset.kind, ir::AssetKind::Image(_));
        let key = format!("{}:{:?}:{target:?}", asset.path, meta);
        if strip && let Some(frames) = self.strips.get(&key) {
            let image = frames.get(frame.min(frames.len() - 1))?.clone();
            #[cfg(any(feature = "shots", test))]
            if let Some(probe) = &mut self.current_probe {
                probe.strip_cache_hit = true;
                probe.codec = Some("png");
            }
            #[cfg(feature = "shots")]
            {
                self.scan.strip_cache_hits += 1;
            }
            return Some(Arc::new(Picture::prepared(
                image,
                frame,
                meta.frames as usize,
                window,
            )));
        }
        let bytes = self.read_result(&asset.path).ok().flatten()?;
        if canceled() {
            return None;
        }
        #[cfg(any(feature = "shots", test))]
        let decode_start = std::time::Instant::now();
        #[cfg(any(feature = "shots", test))]
        if let Some(probe) = &mut self.current_probe {
            probe.codec = super::picture_decode::codec(&bytes);
            probe.status = if probe.codec.is_some() {
                "decode_failed"
            } else {
                "unsupported"
            };
        }
        let frames = strip
            .then(|| {
                super::picture_decode::png_strip(&bytes, meta, target, STRIP_BUDGET, &canceled)
            })
            .flatten();
        if let Some(frames) = frames {
            #[cfg(any(feature = "shots", test))]
            self.record_decode(decode_start, true);
            let frames: Vec<_> = frames.into_iter().map(Arc::new).collect();
            let image = frames.get(frame.min(frames.len() - 1))?.clone();
            self.strips.put(key, frames);
            return Some(Arc::new(Picture::prepared(
                image,
                frame,
                meta.frames as usize,
                window,
            )));
        }
        let image = super::picture_decode::decode(&bytes, meta, frame, target, window, canceled);
        #[cfg(any(feature = "shots", test))]
        self.record_decode(decode_start, image.is_some());

        let image = image?;
        if matches!(asset.kind, ir::AssetKind::BitmapFont) {
            #[cfg(any(feature = "shots", test))]
            if let Some(probe) = &mut self.current_probe {
                probe.status = "bitmap_font_invalid";
            }
            let sidecar = format!("{}.txt", asset.path.rsplit_once('.')?.0);
            let text = match self.read_result(&sidecar) {
                Ok(Some(text)) => text,
                Ok(None) => {
                    #[cfg(any(feature = "shots", test))]
                    if let Some(probe) = &mut self.current_probe {
                        probe.status = "bitmap_font_metadata_missing";
                    }
                    return None;
                }
                Err(_) => return None,
            };
            #[cfg(any(feature = "shots", test))]
            if let Some(probe) = &mut self.current_probe {
                probe.status = "bitmap_font_invalid";
            }
            if sampler_ksp::ui::picture_meta(&String::from_utf8_lossy(&text)).frames != 1 {
                return None;
            }
            return Some(Arc::new(Picture::new(font_frames(&image)?)));
        }
        Some(Arc::new(Picture::prepared(
            Arc::new(image),
            frame,
            meta.frames.max(1) as usize,
            window,
        )))
    }
    #[cfg(any(feature = "shots", test))]
    pub(super) fn begin_probe(
        &mut self,
        asset: &ir::Asset,
        frame: usize,
        target: [u32; 2],
        window: Option<[u32; 4]>,
    ) -> std::time::Instant {
        let path_end = (0..=asset.path.len().min(4096))
            .rev()
            .find(|end| asset.path.is_char_boundary(*end))
            .unwrap_or(0);
        self.current_probe = Some(AssetProbe {
            index: None,
            path: asset.path[..path_end].to_owned(),
            path_truncated: path_end < asset.path.len(),
            kind: match asset.kind {
                ir::AssetKind::Image(_) => "image",
                ir::AssetKind::BitmapFont => "bitmap_font",
                ir::AssetKind::TrueTypeFont => "font",
            },
            resolver: self.uvi.as_ref().map_or_else(
                || {
                    self.kontakt
                        .as_ref()
                        .map_or("unavailable", |k| k.authority_for(&asset.path))
                },
                |uvi| uvi.authority(),
            ),
            status: "unsupported",
            codec: None,
            duration_ms: 0.,
            read_ms: 0.,
            decode_ms: 0.,
            queue_ms: None,
            permit_wait_ms: None,
            encoded_bytes: 0,
            prepared_bytes: 0,
            width: None,
            height: None,
            frames: match asset.kind {
                ir::AssetKind::Image(meta) => meta.frames,
                _ => 1,
            },
            frame,
            target,
            window,
            strip_cache_hit: false,
        });
        std::time::Instant::now()
    }
    #[cfg(any(feature = "shots", test))]
    fn record_decode(&mut self, start: std::time::Instant, ok: bool) {
        let elapsed = start.elapsed();
        if let Some(probe) = &mut self.current_probe {
            probe.decode_ms += elapsed.as_secs_f64() * 1000.;
        }
        #[cfg(feature = "shots")]
        {
            self.scan.decodes += 1;
            self.scan.decode_ok += usize::from(ok);
            self.scan.decode_us = self
                .scan
                .decode_us
                .saturating_add(elapsed.as_micros().min(u64::MAX as u128) as u64);
        }
        #[cfg(not(feature = "shots"))]
        let _ = ok;
    }
    #[cfg(any(feature = "shots", test))]
    fn finish_probe(&mut self, start: std::time::Instant) {
        if let Some(mut probe) = self.current_probe.take() {
            probe.duration_ms = start.elapsed().as_secs_f64() * 1000.;
            if self.probes.len() < PROBE_LIMIT {
                self.probes.push(probe);
            } else {
                self.probe_dropped += 1;
            }
        }
    }
    #[cfg(any(feature = "shots", test))]
    pub fn take_probes(&mut self) -> Vec<AssetProbe> {
        std::mem::take(&mut self.probes)
    }
    #[cfg(any(feature = "shots", test))]
    pub fn probe_dropped(&self) -> usize {
        self.probe_dropped
    }
}

pub(super) fn resource_category(error: ResourceError) -> &'static str {
    match error {
        ResourceError::InvalidPath => "lookup-invalid",
        ResourceError::Ambiguous => "lookup-ambiguous",
        ResourceError::Corrupt => "lookup-corrupt",
        ResourceError::Limit => "lookup-limit",
        ResourceError::Read => "lookup-read",
        ResourceError::Unavailable => "lookup-unavailable",
    }
}

// Only fixed provider reasons are classified; paths and error payloads stay private.
fn kontakt_error(error: sampler_kontakt::LoadError) -> ResourceError {
    use sampler_kontakt::LoadError as E;
    match error {
        E::Io { .. } => ResourceError::Read,
        E::Decode { .. } => ResourceError::Corrupt,
        E::Invalid { reason, .. } => match reason.as_str() {
            "Invalid library-relative resource path" => ResourceError::InvalidPath,
            "Ambiguous loose resource name" | "Ambiguous resource name" => ResourceError::Ambiguous,
            "Resource exceeds 32 MiB" | "Resource namespace exceeds discovery bounds" => {
                ResourceError::Limit
            }
            "Resource namespace could not be read" => ResourceError::Read,
            "Resource container could not be indexed" => ResourceError::Unavailable,
            "authored resource container is unavailable" | "resource program is absent" => {
                ResourceError::Unavailable
            }
            _ => ResourceError::Corrupt,
        },
        E::Staged { source, .. } => kontakt_error(*source),
        E::Access { .. } | E::Lower(_) | E::Canceled => ResourceError::Unavailable,
    }
}

pub(crate) fn crop(image: &Image, x: u32, y: u32, w: u32, h: u32) -> Option<Arc<Image>> {
    if w == 0 || h == 0 || x.checked_add(w)? > image.width || y.checked_add(h)? > image.height {
        return None;
    }
    if (x, y, w, h) == (0, 0, image.width, image.height) {
        return Some(Arc::new(image.clone()));
    }
    let stride = image.width as usize * 4;
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for row in y as usize..(y + h) as usize {
        let at = row * stride + x as usize * 4;
        rgba.extend_from_slice(&image.rgba[at..at + w as usize * 4]);
    }
    Image::rgba(w, h, rgba).map(Arc::new)
}

pub(crate) fn font_frames(image: &Image) -> Option<Vec<Arc<Image>>> {
    if image.height < 2 {
        return None;
    }
    let starts: Vec<u32> = (0..image.width)
        .filter(|&x| {
            let at = x as usize * 4;
            image.rgba[at..at + 3] == [255, 0, 0]
        })
        .collect();
    if starts.len() != 256 || starts[0] != 0 {
        return None;
    }
    starts
        .iter()
        .enumerate()
        .map(|(n, &x)| {
            crop(
                image,
                x,
                1,
                starts.get(n + 1).copied().unwrap_or(image.width) - x,
                image.height - 1,
            )
        })
        .collect()
}

/// Unicode text is indexed by the font's documented Windows-1252 byte order.
/// Characters outside that alphabet use its authored question-mark glyph.
pub(crate) fn font_glyph(c: char) -> usize {
    const EXTENDED: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match c as u32 {
        0..=127 | 160..=255 => c as usize,
        _ => EXTENDED
            .iter()
            .position(|&glyph| glyph == c)
            .map_or(b'?' as usize, |n| n + 128),
    }
}

/// Wake signature for authored resource preparation, independent of Logs visibility.
pub(crate) fn revision() -> u64 {
    super::picture_worker::revision()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires locally owned libraries; numeric asset inventory only"]
    fn kontakt_missing_image_inventory() {
        let manifest =
            std::fs::read_to_string(std::env::var_os("KONTRA_RESOURCE_PROBE_LIST").unwrap())
                .unwrap();
        for (item, line) in manifest.lines().enumerate() {
            let path = Path::new(line.split_once('\t').unwrap().1);
            let mut instrument = sampler_kontakt::read(path).unwrap().instrument;
            instrument.retain_zones(|_| false);
            instrument.assets.clear();
            let loaded = sampler_kontakt::prepare(
                instrument,
                vec![],
                &sampler_kontakt::Options {
                    library: Some(path.to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
            let mut source = Source::of(path);
            let root = source.kontakt.as_ref().unwrap().root().to_owned();
            let names = source.kontakt.as_mut().unwrap().names("");
            for (container, path) in source
                .kontakt
                .as_ref()
                .unwrap()
                .locations()
                .iter()
                .filter(|p| {
                    p.extension().is_some_and(|e| {
                        e.eq_ignore_ascii_case("nkr") || e.eq_ignore_ascii_case("nicnt")
                    })
                })
                .enumerate()
            {
                match sampler_kontakt::ResourceContainer::open(path) {
                    Ok(c) => println!(
                        "RESOURCE_PROBE item={item} container={container} indexed=true members={}",
                        c.names().len()
                    ),
                    Err(e) => println!(
                        "RESOURCE_PROBE item={item} container={container} indexed=false kind={:?} stage={:?}",
                        e.kind(),
                        e.stage()
                    ),
                }
            }
            let loose: Vec<_> = std::fs::read_dir(&root)
                .unwrap()
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.file_name().to_string_lossy().to_lowercase())
                .collect();
            let mut inventory = Vec::new();
            let mut indexed_containers = 0;
            let mut rejected_containers = 0;
            let mut metadata_only_members = 0;
            if let Ok(list) = std::env::var("KONTRA_RESOURCE_GLOBAL_LIST") {
                for line in std::fs::read_to_string(list).unwrap().lines() {
                    let path = Path::new(line);
                    if path.extension().is_some_and(|e| {
                        e.eq_ignore_ascii_case("nkr") || e.eq_ignore_ascii_case("nicnt")
                    }) {
                        match sampler_kontakt::ResourceContainer::open(path) {
                            Ok(c) => {
                                indexed_containers += 1;
                                inventory.extend(c.names().into_iter().map(str::to_owned));
                            }
                            Err(_) => {
                                rejected_containers += 1;
                                // Inspect bounded names only; truncated payloads remain unreadable.
                                if path
                                    .extension()
                                    .is_some_and(|e| e.eq_ignore_ascii_case("nicnt"))
                                {
                                    use std::io::Read;
                                    let mut head = Vec::new();
                                    std::fs::File::open(path)
                                        .unwrap()
                                        .take(4 << 20)
                                        .read_to_end(&mut head)
                                        .unwrap();
                                    let marker = b"/\\ NI FC MTD  /\\";
                                    if let Some(start) = head
                                        .windows(marker.len())
                                        .enumerate()
                                        .skip(1)
                                        .find_map(|(n, b)| (b == marker).then_some(n))
                                    {
                                        if let Some(raw) = head.get(start + 272..start + 280) {
                                            let count = u64::from_le_bytes(raw.try_into().unwrap());
                                            let first = start + 904;
                                            if count <= 100_000
                                                && count
                                                    <= head.len().saturating_sub(first) as u64 / 640
                                            {
                                                for n in 0..count as usize {
                                                    let raw = &head[first + n * 640 + 24
                                                        ..first + n * 640 + 624];
                                                    let units: Vec<_> = raw
                                                        .chunks_exact(2)
                                                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                                                        .take_while(|&u| u != 0)
                                                        .collect();
                                                    inventory
                                                        .push(String::from_utf16_lossy(&units));
                                                    metadata_only_members += 1;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        inventory.push(path.to_string_lossy().into_owned());
                    }
                }
            }
            let mut requested = 0;
            let mut resolved = 0;
            let mut failures = 0;
            for (face_index, face) in loaded.interfaces.iter().enumerate() {
                for (asset_index, asset) in face.assets.iter().enumerate() {
                    if !matches!(asset.kind, ir::AssetKind::Image(_)) {
                        continue;
                    }
                    requested += 1;
                    let result = source.read_result(&asset.path);
                    if result.as_ref().is_ok_and(|b| b.is_some()) {
                        resolved += 1;
                        continue;
                    }
                    failures += 1;
                    // Opt in only when the resource request names are needed for lookup triage.
                    if std::env::var_os("KONTRA_RESOURCE_PROBE_NAMES").is_some() {
                        println!(
                            "RESOURCE_REQUEST item={item} face={face_index} asset={asset_index} path={:?} identity_hash={}",
                            asset.path,
                            blake3::hash(format!("{}:{:?}", asset.path, asset.kind).as_bytes())
                        );
                    }
                    let basename = asset
                        .path
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap()
                        .to_lowercase();
                    let stem = Path::new(&basename).file_stem().unwrap().to_string_lossy();
                    let global_basename = inventory
                        .iter()
                        .filter(|n| {
                            n.rsplit(['/', '\\', '|'])
                                .next()
                                .is_some_and(|n| n.eq_ignore_ascii_case(&basename))
                        })
                        .count();
                    let global_stem = inventory
                        .iter()
                        .filter(|n| {
                            Path::new(n)
                                .file_stem()
                                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&stem))
                        })
                        .count();
                    let numeric_name = stem.parse::<f64>().is_ok();
                    let blank_name = ["empty", "blank", "none", "off", "no"]
                        .iter()
                        .any(|n| stem.eq_ignore_ascii_case(n));
                    let literal_occurrences = loaded
                        .instrument
                        .behaviors
                        .iter()
                        .map(|b| b.source.matches(&format!("\"{stem}\"")).count())
                        .sum::<usize>();
                    let mut kinds = [0usize; 7];
                    for w in face
                        .widgets
                        .iter()
                        .filter(|w| w.images.iter().any(|i| i.asset.0 == asset_index))
                    {
                        kinds[match w.kind {
                            ir::Kind::Knob { .. } => 0,
                            ir::Kind::Slider { .. } => 1,
                            ir::Kind::Button { .. } => 2,
                            ir::Kind::Switch => 3,
                            ir::Kind::Menu { .. } => 4,
                            ir::Kind::Label => 5,
                            _ => 6,
                        }] += 1;
                    }
                    let nil_name = stem.eq_ignore_ascii_case("nil");
                    let dropdown_name = stem.eq_ignore_ascii_case("dropdown");
                    let label_refs = face
                        .widgets
                        .iter()
                        .filter(|w| {
                            matches!(w.kind, ir::Kind::Label)
                                && w.images.iter().any(|i| i.asset.0 == asset_index)
                        })
                        .count();
                    let menu_refs = face
                        .widgets
                        .iter()
                        .filter(|w| {
                            matches!(w.kind, ir::Kind::Menu { .. })
                                && w.images.iter().any(|i| i.asset.0 == asset_index)
                        })
                        .count();
                    let namespace_exact = names
                        .iter()
                        .filter(|n| n.eq_ignore_ascii_case(&asset.path))
                        .count();
                    let namespace_basename = names
                        .iter()
                        .filter(|n| {
                            n.rsplit(['/', '\\', '|'])
                                .next()
                                .is_some_and(|n| n.eq_ignore_ascii_case(&basename))
                        })
                        .count();
                    let namespace_stem = names
                        .iter()
                        .filter(|n| {
                            Path::new(n)
                                .file_stem()
                                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&stem))
                        })
                        .count();
                    let root_basename = loose.iter().filter(|n| *n == &basename).count();
                    let twice_stem = Path::new(stem.as_ref())
                        .file_stem()
                        .map(|n| n.to_string_lossy().to_lowercase());
                    let namespace_twice_stem = twice_stem.as_ref().map_or(0, |stem| {
                        names
                            .iter()
                            .filter(|n| {
                                Path::new(n)
                                    .file_stem()
                                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(stem))
                            })
                            .count()
                    });
                    let root_twice_stem = twice_stem.as_ref().map_or(0, |stem| {
                        loose
                            .iter()
                            .filter(|n| {
                                Path::new(n)
                                    .file_stem()
                                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(stem))
                            })
                            .count()
                    });
                    let image_refs: Vec<_> = face
                        .widgets
                        .iter()
                        .enumerate()
                        .filter(|(_, w)| w.images.iter().any(|i| i.asset.0 == asset_index))
                        .collect();
                    let visible_refs = image_refs
                        .iter()
                        .filter(|(i, _)| face.visible(ir::WidgetRef(*i)))
                        .count();
                    let background_hidden_refs =
                        image_refs.iter().filter(|(_, w)| w.hide.background).count();
                    let title_hidden_refs = image_refs.iter().filter(|(_, w)| w.hide.title).count();
                    let text_empty_refs =
                        image_refs.iter().filter(|(_, w)| w.text.is_empty()).count();
                    let resolved = super::super::ir_view::resolved(face);
                    let geometry_refs: Vec<_> = image_refs
                        .iter()
                        .map(|(i, _)| {
                            let r = resolved.page_rect(ir::WidgetRef(*i));
                            [r.x as i64, r.y as i64, r.width as i64, r.height as i64]
                        })
                        .collect();
                    let background_refs = face
                        .pages
                        .iter()
                        .filter(|p| p.background.image.is_some_and(|a| a.0 == asset_index))
                        .count();
                    let root_stem = loose
                        .iter()
                        .filter(|n| {
                            Path::new(n)
                                .file_stem()
                                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&stem))
                        })
                        .count();
                    println!(
                        "RESOURCE_PROBE item={item} face={face_index} asset={asset_index} requested_hash={} error={:?} namespace_exact={namespace_exact} namespace_basename={namespace_basename} namespace_stem={namespace_stem} root_basename={root_basename} root_stem={root_stem} namespace_twice_stem={namespace_twice_stem} root_twice_stem={root_twice_stem} indexed_containers={indexed_containers} rejected_containers={rejected_containers} metadata_only_members={metadata_only_members} numeric_name={numeric_name} blank_name={blank_name} literal_occurrences={literal_occurrences} kind_counts={kinds:?} global_basename={global_basename} global_stem={global_stem} nil_name={nil_name} dropdown_name={dropdown_name} label_refs={label_refs} menu_refs={menu_refs} name_bytes={} png_suffixes={} refs={} visible_refs={visible_refs} background_refs={background_refs} background_hidden_refs={background_hidden_refs} title_hidden_refs={title_hidden_refs} text_empty_refs={text_empty_refs} geometry_refs={geometry_refs:?}",
                        &blake3::hash(asset.path.as_bytes()).to_hex()[..16],
                        result.err(),
                        asset.path.len(),
                        basename.matches(".png").count(),
                        image_refs.len()
                    );
                }
            }
            println!(
                "RESOURCE_PROBE item={item} namespace_members={} requested={requested} resolved={resolved} failed={failures}",
                names.len()
            );
        }
    }

    #[test]
    fn shared_lookup_distinguishes_absence_invalid_path_and_failed_authority() {
        let dir = std::env::temp_dir().join(format!(
            "kontra-shared-resource-result-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join("Resources")).unwrap();
        std::fs::write(dir.join("Resources/fixture.bin"), b"synthetic").unwrap();
        for instrument in ["fixture.nki", "fixture.uvip"] {
            let mut source = Source::of(&dir.join(instrument));
            assert_eq!(
                source.read_result("Resources/fixture.bin").unwrap(),
                Some(b"synthetic".to_vec())
            );
            assert_eq!(source.read_result("Resources/absent.bin").unwrap(), None);
            assert_eq!(
                source.read_result("../foreign.bin"),
                Err(ResourceError::InvalidPath)
            );
            assert_eq!(
                source.read_result("Resources/\0fixture.bin"),
                Err(ResourceError::InvalidPath)
            );
            assert_eq!(
                source.read_result(&"x".repeat(4097)),
                Err(ResourceError::InvalidPath)
            );
            #[cfg(feature = "shots")]
            assert_eq!(
                (
                    source.scan.lookups,
                    source.scan.lookup_ok,
                    source.scan.lookup_missing,
                    source.scan.lookup_invalid
                ),
                (5, 1, 1, 3)
            );
        }
        std::fs::write(dir.join("broken.nkr"), b"not an archive").unwrap();
        let mut source = Source::of(&dir.join("fixture.nki"));
        assert_eq!(
            source.read_result("Resources/absent.bin"),
            Err(ResourceError::Unavailable)
        );
        #[cfg(feature = "shots")]
        assert_eq!(
            (source.scan.lookup_missing, source.scan.lookup_unavailable),
            (0, 1)
        );
        let mut source = Source::of(&dir.join("broken.ufs/fixture.uvip"));
        assert_eq!(
            source.read_result("Resources/absent.bin"),
            Err(ResourceError::Unavailable)
        );
        #[cfg(feature = "shots")]
        assert_eq!(
            (source.scan.lookup_missing, source.scan.lookup_unavailable),
            (0, 1)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn asset_probes_distinguish_missing_unsupported_corrupt_geometry_and_cache_hits() {
        let dir = tempfile::tempdir().unwrap();
        let pictures = dir.path().join("Resources/pictures");
        std::fs::create_dir_all(&pictures).unwrap();
        std::fs::write(pictures.join("unsupported.bin"), b"unknown authored format").unwrap();
        std::fs::write(pictures.join("corrupt.png"), b"\x89PNGtruncated").unwrap();
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 1, 4);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[1, 2, 3, 255, 4, 5, 6, 127, 7, 8, 9, 255, 10, 11, 12, 127])
                .unwrap();
        }
        std::fs::write(pictures.join("strip.png"), &png).unwrap();
        let mut source = Source::of(&dir.path().join("fixture.nki"));
        let asset = |name: &str, frames| ir::Asset {
            path: format!("Resources/pictures/{name}"),
            kind: ir::AssetKind::Image(ir::ImageMeta {
                frames,
                ..Default::default()
            }),
        };
        for (name, frames, status, codec) in [
            ("absent.png", 1, "missing", None),
            ("unsupported.bin", 1, "unsupported", None),
            ("corrupt.png", 1, "decode_failed", Some("png")),
            ("strip.png", 3, "decode_failed", Some("png")),
        ] {
            assert!(source.load(&asset(name, frames)).is_none());
            let probe = source.take_probes().pop().unwrap();
            assert_eq!((probe.status, probe.codec), (status, codec));
            assert!(probe.duration_ms >= probe.read_ms);
            assert!(!probe.strip_cache_hit);
        }
        for frame in [0, 1] {
            let image = source
                .load_frame(&asset("strip.png", 2), frame, [1, 2], None, || false)
                .unwrap();
            assert_eq!(image.frames[0].rgba[0], if frame == 0 { 1 } else { 7 });
            let probe = source.take_probes().pop().unwrap();
            assert_eq!(
                (probe.status, probe.codec, probe.frames, probe.frame),
                ("found", Some("png"), 2, frame)
            );
            assert_eq!(probe.strip_cache_hit, frame == 1);
            assert_eq!(
                (probe.width, probe.height, probe.prepared_bytes),
                (Some(1), Some(2), 8)
            );
            assert_eq!(probe.encoded_bytes, if frame == 0 { png.len() } else { 0 });
        }
        #[cfg(feature = "shots")]
        assert_eq!(
            (
                source.scan.decodes,
                source.scan.decode_ok,
                source.scan.strip_cache_hits
            ),
            (4, 1, 1)
        );
        let bitmap = ir::Asset {
            path: "Resources/pictures/strip.png".into(),
            kind: ir::AssetKind::BitmapFont,
        };
        assert!(source.load(&bitmap).is_none());
        assert_eq!(
            source.take_probes()[0].status,
            "bitmap_font_metadata_missing"
        );
        std::fs::write(pictures.join("strip.txt"), b"Number of Animations: 1").unwrap();
        std::fs::write(pictures.join("valid.ttf"), super::super::theme::NOTO_SANS).unwrap();
        // Namespace discovery is a source snapshot; reload after adding files.
        let mut source = Source::of(&dir.path().join("fixture.nki"));
        assert!(source.load(&bitmap).is_none());
        assert_eq!(source.take_probes()[0].status, "bitmap_font_invalid");
        let font = |name| ir::Asset {
            path: format!("Resources/pictures/{name}"),
            kind: ir::AssetKind::TrueTypeFont,
        };
        assert!(source.font(&font("valid.ttf")).is_some());
        let probe = source.take_probes().pop().unwrap();
        assert_eq!(
            (probe.kind, probe.status, probe.codec),
            ("font", "found", Some("font"))
        );
        assert_eq!(probe.prepared_bytes, super::super::theme::NOTO_SANS.len());
        assert!(source.font(&font("absent.ttf")).is_none());
        assert_eq!(source.take_probes()[0].status, "missing");
        assert!(source.font(&font("unsupported.bin")).is_none());
        assert_eq!(source.take_probes()[0].status, "decode_failed");
        for _ in 0..PROBE_LIMIT + 3 {
            source.load(&asset("absent.png", 1));
        }
        assert_eq!(source.take_probes().len(), PROBE_LIMIT);
        assert_eq!(source.probe_dropped(), 3);
        let mut uvi = Source::of(&dir.path().join("fixture.uvip"));
        assert!(
            uvi.kontakt.is_none(),
            "UVI must not discover a Kontakt authority"
        );
        assert!(uvi.uvi.is_some());
        uvi.load(&asset("absent.png", 1));
        assert_eq!(uvi.take_probes()[0].resolver, "uvi-loose");
    }

    #[test]
    fn a_dragged_strip_decodes_once_not_once_per_frame() {
        let dir = std::env::temp_dir().join(format!("strip-cache-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Samples")).unwrap();
        std::fs::create_dir_all(dir.join("Resources/pictures")).unwrap();
        let path = dir.join("test.nki");
        std::fs::write(&path, b"synthetic").unwrap();
        let mut bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut bytes, 100, 100 * 128);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            let mut w = e.write_header().unwrap();
            let pixels = (0..128u32)
                .flat_map(|n| [n as u8, (n * 7) as u8, 90, 255].repeat(100 * 100))
                .collect::<Vec<_>>();
            w.write_image_data(&pixels).unwrap();
        }
        std::fs::write(dir.join("Resources/pictures/strip.png"), &bytes).unwrap();
        let meta = ir::ImageMeta {
            frames: 128,
            size: Some(ir::Size {
                width: 100.0,
                height: 100.0,
            }),
            ..Default::default()
        };
        let asset = ir::Asset {
            path: "Resources/pictures/strip.png".into(),
            kind: ir::AssetKind::Image(meta),
        };
        let mut source = Source::of(&path);
        let time = |f: &mut dyn FnMut(usize)| {
            let start = std::time::Instant::now();
            for frame in 0..128 {
                f(frame);
            }
            start.elapsed() / 128
        };
        // Before: every frame read and decoded the strip up to its own rows.
        let per_frame = time(&mut |frame| {
            let image =
                super::super::picture_decode::decode(&bytes, meta, frame, [100, 100], None, || {
                    false
                });
            assert_eq!(image.unwrap().rgba[0], frame as u8);
        });
        // After: one pass, then cache hits.
        let first = time(&mut |frame| {
            if frame == 0 {
                source
                    .load_frame(&asset, 0, [100, 100], None, || false)
                    .unwrap();
            }
        }) * 128;
        let cached = time(&mut |frame| {
            let p = source
                .load_frame(&asset, frame, [100, 100], None, || false)
                .unwrap();
            assert_eq!(p.frames[0].rgba[0], frame as u8);
        });
        println!("STRIP per_frame={per_frame:?} first_whole_strip={first:?} cached={cached:?}");
        assert!(
            cached * 10 < per_frame,
            "cached {cached:?} vs per-frame {per_frame:?}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
