//! A script interface's pictures, decoded from the library the loader
//! already read their layouts from.

use super::ir_view::Picture;
use moose::mui::mui::scene::Image;
use sampler_ui_ir as ir;
use sampler_uvi::ResourceError;
use std::{path::Path, sync::Arc};

/// Where an instrument's resources come from.
pub struct Source {
    kontakt: sampler_kontakt::Resources,
    uvi: Option<sampler_uvi::Resources>,
    strips: Strips,
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
    pub fn of(instrument: &Path) -> Self {
        let uvi = instrument
            .ancestors()
            .any(|p| {
                p.extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("ufs") || e.eq_ignore_ascii_case("uvip")
                })
            })
            .then(|| sampler_uvi::Resources::of(instrument));
        Self {
            kontakt: sampler_kontakt::Resources::of(instrument),
            uvi,
            strips: Strips::default(),
            #[cfg(feature = "shots")]
            scan: Scan::default(),
        }
    }

    pub(crate) fn read_result(&mut self, path: &str) -> Result<Option<Vec<u8>>, ResourceError> {
        let result = if path.is_empty() || path.len() > 4096 || path.contains('\0') {
            Err(ResourceError::InvalidPath)
        } else {
            match &self.uvi {
                Some(uvi) => uvi.read_result(path),
                None => self.kontakt.read_result(path).map_err(kontakt_error),
            }
        };
        #[cfg(feature = "shots")]
        {
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
        result
    }
    pub(super) fn native_names(&mut self) -> Vec<String> {
        let mut names = self.kontakt.names("resources/native_ui/");
        names.extend(self.kontakt.names("native_ui/"));
        names.sort();
        names.dedup();
        names
    }
    pub fn font(&mut self, asset: &ir::Asset) -> Option<moose::mui::mui::prelude::Font> {
        let font = self
            .read_result(&asset.path)
            .ok()
            .flatten()
            .and_then(|bytes| moose::mui::mui::prelude::Font::new(bytes).ok());
        #[cfg(feature = "shots")]
        {
            self.scan.fonts += 1;
            self.scan.font_ok += usize::from(font.is_some());
        }
        font
    }

    /// Compatibility probe: one frame, never an eagerly decoded strip.
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
        if canceled() {
            return None;
        }
        let meta = match asset.kind {
            ir::AssetKind::Image(m) => m,
            ir::AssetKind::BitmapFont => ir::ImageMeta::default(),
            _ => return None,
        };
        let strip = window.is_none() && meta.frames > 1 && matches!(asset.kind, ir::AssetKind::Image(_));
        let key = format!("{}:{:?}:{target:?}", asset.path, meta);
        if strip && let Some(frames) = self.strips.get(&key) {
            let image = frames.get(frame.min(frames.len() - 1))?.clone();
            return Some(Arc::new(Picture::prepared(image, frame, meta.frames as usize, window)));
        }
        let bytes = self.read_result(&asset.path).ok().flatten()?;
        if canceled() {
            return None;
        }
        if strip
            && let Some(frames) = super::picture_decode::png_strip(&bytes, meta, target, &canceled)
        {
            let frames: Vec<_> = frames.into_iter().map(Arc::new).collect();
            let image = frames.get(frame.min(frames.len() - 1))?.clone();
            self.strips.put(key, frames);
            return Some(Arc::new(Picture::prepared(image, frame, meta.frames as usize, window)));
        }
        let image = super::picture_decode::decode(&bytes, meta, frame, target, window, canceled);

        #[cfg(feature = "shots")]
        {
            self.scan.decodes += 1;
            self.scan.decode_ok += usize::from(image.is_some());
        }

        let image = image?;
        if matches!(asset.kind, ir::AssetKind::BitmapFont) {
            let sidecar = format!("{}.txt", asset.path.rsplit_once('.')?.0);
            let text = self.read_result(&sidecar).ok().flatten()?;
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
            "Resource exceeds 32 MiB" => ResourceError::Limit,
            "Resource container could not be indexed" => ResourceError::Unavailable,
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
            let root = source.kontakt.root().to_owned();
            let names = source.kontakt.names("");
            for (container, path) in source
                .kontakt
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
            size: Some(ir::Size { width: 100, height: 100 }),
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
            let image = super::super::picture_decode::decode(&bytes, meta, frame, [100, 100], None, || false);
            assert_eq!(image.unwrap().rgba[0], frame as u8);
        });
        // After: one pass, then cache hits.
        let first = time(&mut |frame| {
            if frame == 0 {
                source.load_frame(&asset, 0, [100, 100], None, || false).unwrap();
            }
        }) * 128;
        let cached = time(&mut |frame| {
            let p = source.load_frame(&asset, frame, [100, 100], None, || false).unwrap();
            assert_eq!(p.frames[0].rgba[0], frame as u8);
        });
        println!("STRIP per_frame={per_frame:?} first_whole_strip={first:?} cached={cached:?}");
        assert!(cached * 10 < per_frame, "cached {cached:?} vs per-frame {per_frame:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
