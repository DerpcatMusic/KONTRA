//! Port from v1 0cb7a8a0:src/cache.rs, adapted to semantic IR and native KSP state.
//! Only metadata is persisted; sample and impulse PCM remain outside this cache.
use crate::{Kontakt, Resources, Samples};
use std::path::{Path, PathBuf};
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ImpulseRecipe {
    pub source: PathBuf,
    pub params: crate::effects::Convolution,
}
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    collections::BTreeSet,
    fs::{File, OpenOptions},
    hash::{Hash, Hasher},
    io::{Read, Seek, SeekFrom, Write},
    time::SystemTime,
};
const ENTRY_LIMIT: u64 = 256 << 20;
const CACHE_LIMIT: u64 = 2 << 30;
#[derive(Clone, Serialize, Deserialize, PartialEq)]
struct Dependency {
    path: PathBuf,
    version: Option<(u64, u128, Option<(u64, u64)>)>,
}
fn version(path: &Path) -> Option<(u64, u128, Option<(u64, u64)>)> {
    let meta = path.metadata().ok()?;
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        Some((meta.dev(), meta.ino()))
    };
    #[cfg(not(unix))]
    let identity = None;
    Some((
        meta.len(),
        meta.modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos(),
        identity,
    ))
}
fn dependencies(paths: impl IntoIterator<Item = PathBuf>) -> Vec<Dependency> {
    paths
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|path| Dependency {
            version: version(&path),
            path,
        })
        .collect()
}
fn current(dependencies: &[Dependency]) -> bool {
    dependencies.iter().all(|d| version(&d.path) == d.version)
}
fn entry(
    dir: &Path,
    path: &Path,
    program: u32,
    controls: &[(sampler_core::ControlId, i32)],
) -> PathBuf {
    let mut h = std::hash::DefaultHasher::new();
    (path.as_os_str().as_encoded_bytes(), program, controls).hash(&mut h);
    dir.join(format!("{:016x}.instrument", h.finish()))
}
fn stamp(
    path: &Path,
    program: u32,
    controls: &[(sampler_core::ControlId, i32)],
) -> Option<Vec<u8>> {
    let (size, mtime, identity) = version(path)?;
    let mut out = b"KONTRA v2 translated instrument 1 ".to_vec();
    out.extend_from_slice(env!("KONTRA_IMPORT_HASH").as_bytes());
    out.extend_from_slice(&program.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&mtime.to_le_bytes());
    let (dev, ino) = identity.unwrap_or_default();
    out.extend_from_slice(&dev.to_le_bytes());
    out.extend_from_slice(&ino.to_le_bytes());
    let name = path.as_os_str().as_encoded_bytes();
    out.extend_from_slice(&(name.len() as u64).to_le_bytes());
    out.extend_from_slice(name);
    out.extend_from_slice(&(controls.len() as u64).to_le_bytes());
    for (id, value) in controls {
        out.extend_from_slice(&id.0.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    }
    Some(out)
}
#[derive(Serialize, Deserialize)]
struct Head<'a> {
    dependencies: Vec<Dependency>,
    resource_locations: Vec<PathBuf>,
    root: PathBuf,
    instrument: Cow<'a, sampler_ir::Instrument>,
    locations: Cow<'a, [PathBuf]>,
    states: Vec<Option<Result<sampler_ksp::CachedInit, String>>>,
    control_values: Vec<(sampler_core::ControlId, i32)>,
    recipes: Cow<'a, [ImpulseRecipe]>,
}
fn load_from(
    dir: &Path,
    path: &Path,
    program: u32,
    controls: &[(sampler_core::ControlId, i32)],
) -> Option<Kontakt> {
    let stamp = stamp(path, program, controls)?;
    let filename = entry(dir, path, program, controls);
    let mut file = File::open(&filename).ok()?;
    if file.metadata().ok()?.len() > ENTRY_LIMIT {
        return None;
    }
    let mut actual = vec![0; stamp.len()];
    file.read_exact(&mut actual).ok()?;
    if actual != stamp {
        return None;
    }
    let mut checksum = [0; 32];
    file.read_exact(&mut checksum).ok()?;
    let mut bytes = Vec::new();
    file.take(ENTRY_LIMIT + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > ENTRY_LIMIT || Sha256::digest(&bytes).as_slice() != checksum {
        return None;
    }
    let len = usize::try_from(u64::from_le_bytes(bytes.get(..8)?.try_into().ok()?)).ok()?;
    let (head_bytes, tables) = bytes.get(8..)?.split_at_checked(len)?;
    let decode_span = crate::audit::Span::new("preset_cache_decode");
    let head: Head<'static> = serde_json::from_slice(head_bytes).ok()?;
    let (zones, physical) = crate::cache_zones::decode(tables)?;
    drop(bytes);
    drop(decode_span);
    let validation_span = crate::audit::Span::new("preset_cache_validate");
    if !current(&head.dependencies)
        || head.instrument.assets.len() != head.locations.len()
        || head.states.len() != head.instrument.behaviors.len()
        || head.control_values != controls
        || !head.instrument.impulses.is_empty()
        || !head.instrument.zones.is_empty()
        || head
            .instrument
            .kontakt_objects
            .as_ref()
            .is_some_and(|o| !o.zones.is_empty())
    {
        return None;
    }
    let resources = Resources::of(path);
    if dependencies(resources.locations()) != dependencies(head.resource_locations) {
        return None;
    }
    if !head.root.is_dir() || !path.starts_with(&head.root) {
        return None;
    }
    drop(validation_span);
    let mut samples = Samples::new(&head.root);
    let mut instrument = head.instrument.into_owned();
    instrument.zones = zones;
    if let Some(objects) = instrument.kontakt_objects.as_mut() {
        objects.zones = physical;
    } else if !physical.is_empty() {
        return None;
    }
    for recipe in head.recipes.iter() {
        let decoded = samples.decode(&recipe.source).ok()?;
        instrument.impulses.push(
            crate::effects::restore_impulse(&recipe.params, (decoded.rate, decoded.frames)).ok()?,
        );
    }
    instrument.validate().ok()?;
    let restore_span = crate::audit::Span::new("preset_cache_restore_scripts");
    let states = head
        .states
        .into_iter()
        .zip(&instrument.behaviors)
        .map(|(state, behavior)| match state {
            None => Some(None),
            Some(Err(error)) => Some(Some(Err(error))),
            Some(Ok(state)) => sampler_ksp::restore_initialized(
                &behavior.source,
                sampler_ksp::Limits::LIBRARY,
                state,
            )
            .ok()
            .map(|s| Some(Ok(s))),
        })
        .collect::<Option<Vec<_>>>()?;
    drop(restore_span);
    let _ = OpenOptions::new()
        .write(true)
        .open(filename)
        .and_then(|f| f.set_modified(SystemTime::now()));
    Some(Kontakt {
        instrument,
        locations: head.locations.into_owned(),
        samples,
        initialized: Some(crate::load::ScriptInit {
            states,
            control_values: head.control_values,
            resources: Some(resources),
        }),
    })
}
struct CheckedWriter<W> {
    inner: W,
    hash: Sha256,
    size: u64,
}
impl<W: Write> Write for CheckedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self
            .size
            .checked_add(bytes.len() as u64)
            .is_none_or(|n| n > ENTRY_LIMIT)
        {
            return Err(std::io::Error::other("product cache entry limit"));
        }
        let n = self.inner.write(bytes)?;
        self.hash.update(&bytes[..n]);
        self.size += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
fn prune(dir: &Path, budget: u64) {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let m = e.metadata().ok()?;
            (p.extension().is_some_and(|e| e == "instrument") && m.is_file()).then_some((
                m.modified().ok()?,
                m.len(),
                p,
            ))
        })
        .collect();
    files.sort_by_key(|f| f.0);
    let mut size: u64 = files.iter().map(|f| f.1).sum();
    for (_, len, path) in files {
        if size <= budget {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            size = size.saturating_sub(len);
        }
    }
}
fn store_in(
    dir: &Path,
    path: &Path,
    program: u32,
    kontakt: &mut Kontakt,
    recipes: &[ImpulseRecipe],
) {
    if kontakt
        .instrument
        .unsupported
        .iter()
        .any(|f| f.feature == "missing sample")
        || kontakt.instrument.impulses.len() != recipes.len()
    {
        return;
    }
    let Some(stamp) = stamp(
        path,
        program,
        kontakt
            .initialized
            .as_ref()
            .map_or(&[], |s| s.control_values.as_slice()),
    ) else {
        return;
    };
    let Some(initialized) = kontakt.initialized.as_ref() else {
        return;
    };
    let Some(states) = initialized
        .states
        .iter()
        .map(|s| match s {
            None => Some(None),
            Some(Err(e)) => Some(Some(Err(e.clone()))),
            Some(Ok(s)) => s.capture_initialized().map(|s| Some(Ok(s))),
        })
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    let resource_locations = initialized
        .resources
        .as_ref()
        .map_or_else(Vec::new, Resources::locations);
    let dependencies = dependencies(
        kontakt
            .locations
            .iter()
            .chain(recipes.iter().map(|r| &r.source))
            .filter_map(|p| p.ancestors().find(|a| a.is_file()).map(Path::to_path_buf))
            .chain(resource_locations.iter().cloned())
            .chain(std::iter::once(path.to_path_buf())),
    );
    if !current(&dependencies) {
        return;
    }
    let mkdir = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let mut mkdir = {
        use std::os::unix::fs::DirBuilderExt;
        let mut mkdir = mkdir;
        mkdir.mode(0o700);
        mkdir
    };
    #[cfg(not(unix))]
    let mut mkdir = mkdir;
    if mkdir.recursive(true).create(dir).is_err() {
        return;
    }
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let Ok(lock) = options.open(dir.join(".lock")) else {
        return;
    };
    if lock.lock().is_err() {
        return;
    }
    // Holding the directory lock means no other writer can own these leftovers.
    for path in std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
    {
        if path
            .extension()
            .is_some_and(|e| e.to_string_lossy().starts_with("tmp"))
        {
            let _ = std::fs::remove_file(path);
        }
    }
    let filename = entry(dir, path, program, &initialized.control_values);
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = filename.with_extension(format!(
        "tmp{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let impulses = std::mem::take(&mut kontakt.instrument.impulses);
    let zones = std::mem::take(&mut kontakt.instrument.zones);
    let physical = kontakt
        .instrument
        .kontakt_objects
        .as_mut()
        .map(|o| std::mem::take(&mut o.zones))
        .unwrap_or_default();
    let written = (|| -> Option<()> {
        let head = Head {
            dependencies,
            resource_locations,
            root: kontakt.samples.root().into(),
            instrument: Cow::Borrowed(&kontakt.instrument),
            locations: Cow::Borrowed(&kontakt.locations),
            states,
            control_values: initialized.control_values.clone(),
            recipes: Cow::Borrowed(recipes),
        };
        let mut file = options.create_new(true).open(&tmp).ok()?;
        file.write_all(&stamp).ok()?;
        file.write_all(&[0; 32]).ok()?;
        let mut writer = CheckedWriter {
            inner: std::io::BufWriter::new(&mut file),
            hash: Sha256::new(),
            size: stamp.len() as u64 + 32,
        };
        let head = serde_json::to_vec(&head).ok()?;
        writer.write_all(&(head.len() as u64).to_le_bytes()).ok()?;
        writer.write_all(&head).ok()?;
        drop(head);
        let mut tables = Vec::new();
        crate::cache_zones::encode(&zones, &physical, &mut tables).ok()?;
        writer.write_all(&tables).ok()?;
        drop(tables);
        writer.flush().ok()?;
        let checksum = writer.hash.clone().finalize();
        drop(writer);
        file.seek(SeekFrom::Start(stamp.len() as u64)).ok()?;
        file.write_all(&checksum).ok()?;
        drop(file);
        std::fs::rename(&tmp, &filename).ok()?;
        Some(())
    })();
    kontakt.instrument.impulses = impulses;
    kontakt.instrument.zones = zones;
    if let Some(objects) = kontakt.instrument.kontakt_objects.as_mut() {
        objects.zones = physical;
    }
    if written.is_none() {
        let _ = std::fs::remove_file(&tmp);
    } else {
        prune(dir, CACHE_LIMIT);
    }
}
pub(crate) fn load(
    path: &Path,
    program: u32,
    controls: &[(sampler_core::ControlId, i32)],
) -> Option<Kontakt> {
    let _span = crate::audit::Span::new("preset_cache_lookup");
    load_from(
        &dirs::cache_dir()?.join("kontra/v2-instruments"),
        path,
        program,
        controls,
    )
}
pub(crate) fn store(path: &Path, program: u32, kontakt: &mut Kontakt, recipes: &[ImpulseRecipe]) {
    let _span = crate::audit::Span::new("preset_cache_store");
    if let Some(dir) = dirs::cache_dir() {
        store_in(
            &dir.join("kontra/v2-instruments"),
            path,
            program,
            kontakt,
            recipes,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "kontra-v2-cache-fixture-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn kontakt(root: &Path) -> Kontakt {
        Kontakt {
            instrument: Default::default(),
            locations: Vec::new(),
            samples: Samples::new(root),
            initialized: Some(crate::load::ScriptInit {
                control_values: Vec::new(),
                states: Vec::new(),
                resources: None,
            }),
        }
    }
    #[test]
    fn product_cache_roundtrip_and_corruption_fallback() {
        let f = Fixture::new();
        let preset = f.0.join("fixture.nki");
        std::fs::write(&preset, b"numeric fixture").unwrap();
        let dir = f.0.join("cache");
        let mut k = kontakt(&f.0);
        store_in(&dir, &preset, 0, &mut k, &[]);
        assert!(
            load_from(&dir, &preset, 0, &[]).is_some(),
            "translated product cache hit"
        );
        assert!(load_from(&dir, &preset, 1, &[]).is_none());
        let file = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "instrument"))
            .unwrap();
        let mut bytes = std::fs::read(&file).unwrap();
        let at = bytes.len() - 1;
        bytes[at] ^= 1;
        std::fs::write(file, bytes).unwrap();
        assert!(load_from(&dir, &preset, 0, &[]).is_none());
    }
    #[test]
    fn product_cache_restores_host_values_engine_state_and_invalidates_dependencies() {
        let f = Fixture::new();
        let preset = f.0.join("fixture.nki");
        std::fs::write(&preset, b"numeric fixture").unwrap();
        let dir = f.0.join("cache");
        let source = "on init\ndeclare ui_knob $k(0,100,1)\nmake_persistent($k)\nread_persistent_var($k)\nset_engine_par($ENGINE_PAR_VOLUME,12345,-1,-1,-1)\nend on";
        let id = sampler_ksp::derived_control_id(0, "$k");
        let controls = vec![(id, 37)];
        let mut k = kontakt(&f.0);
        k.instrument.behaviors.push(sampler_ir::Behavior {
            name: "fixture".into(),
            language: sampler_ir::Language::Ksp,
            source: source.into(),
            slot: Some(0),
            state: Vec::new(),
            requires: Vec::new(),
        });
        k.initialized = Some(crate::load::initialize_scripts(
            &mut k.instrument,
            Some(&preset),
            Vec::new(),
            &controls,
        ));
        let expected = k.initialized.as_ref().unwrap().states[0]
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .engine_pars();
        store_in(&dir, &preset, 0, &mut k, &[]);
        assert!(
            load_from(&dir, &preset, 0, &[]).is_none(),
            "different restored state is a miss"
        );
        let mut warm = load_from(&dir, &preset, 0, &controls).expect("cached semantic host values");
        let init = warm.initialized.as_mut().unwrap().states[0]
            .take()
            .unwrap()
            .unwrap();
        assert_eq!(init.engine_pars(), expected);
        let script = sampler_ksp::compile_initialized(
            source,
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[],
            init,
        )
        .unwrap();
        assert_eq!(
            script.controls()[0].definition.default,
            sampler_core::ControlValue::Integer(37)
        );
        std::fs::create_dir(f.0.join("Resources")).unwrap();
        std::fs::write(f.0.join("Resources/new.nckp"), b"fixture").unwrap();
        assert!(
            load_from(&dir, &preset, 0, &controls).is_none(),
            "new resource invalidates captured view"
        );
        std::fs::write(&preset, b"changed fixture length").unwrap();
        assert!(load_from(&dir, &preset, 0, &controls).is_none());
    }
    #[test]
    fn metadata_writer_and_cache_eviction_are_bounded() {
        let mut writer = CheckedWriter {
            inner: Vec::new(),
            hash: Sha256::new(),
            size: ENTRY_LIMIT,
        };
        assert!(writer.write_all(&[1]).is_err());
        assert!(writer.inner.is_empty());
        let f = Fixture::new();
        for i in 0..3 {
            std::fs::write(f.0.join(format!("{i}.instrument")), [0; 16]).unwrap();
        }
        prune(&f.0, 16);
        assert_eq!(std::fs::read_dir(&f.0).unwrap().count(), 1);
    }
    #[test]
    fn impulse_cache_contains_only_recipe_and_reconstructs_identical_audio() {
        let f = Fixture::new();
        let preset = f.0.join("fixture.nki");
        std::fs::write(&preset, b"fixture").unwrap();
        let dir = f.0.join("cache");
        let source = f.0.join("impulse.wav");
        let mut wav = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0".to_vec();
        wav.extend(3u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(44100u32.to_le_bytes());
        wav.extend(176400u32.to_le_bytes());
        wav.extend(4u16.to_le_bytes());
        wav.extend(32u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(8u32.to_le_bytes());
        wav.extend(0.25f32.to_le_bytes());
        wav.extend(0.5f32.to_le_bytes());
        std::fs::write(&source, wav).unwrap();
        let params = crate::effects::Convolution {
            decimation: 1.,
            predelay_ms: 0.,
            early: [1., 0., 0.],
            late: [1., 0., 0.],
            xpoint: 0.5,
            flags: [false; 5],
            curve_x: Vec::new(),
            curve_db: Vec::new(),
            ir_index: 0,
        };
        let mut k = kontakt(&f.0);
        let decoded = k.samples.decode(&source).unwrap();
        k.instrument.impulses.push(
            crate::effects::restore_impulse(&params, (decoded.rate, decoded.frames)).unwrap(),
        );
        let expected = k.instrument.impulses.clone();
        let recipes = [ImpulseRecipe { source, params }];
        store_in(&dir, &preset, 0, &mut k, &recipes);
        assert_eq!(
            k.instrument.impulses, expected,
            "capture preserves original audio"
        );
        let encoded = std::fs::read(entry(&dir, &preset, 0, &[])).unwrap();
        let body = stamp(&preset, 0, &[]).unwrap().len() + 32;
        let metadata: serde_json::Value = serde_json::from_slice(
            &encoded[body + 8
                ..body
                    + 8
                    + u64::from_le_bytes(encoded[body..body + 8].try_into().unwrap()) as usize],
        )
        .unwrap();
        assert_eq!(metadata["instrument"]["impulses"], serde_json::json!([]));
        assert_eq!(metadata["recipes"].as_array().unwrap().len(), 1);
        let warm = load_from(&dir, &preset, 0, &[]).expect("recipe cache hit");
        assert_eq!(warm.instrument.impulses, expected);
    }
}
