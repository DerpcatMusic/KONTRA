//! Exhaustive installed-program access/decode census. JSON lines contain status
//! and the first failure only; decoded content is never written to disk.
use serde_json::json;
use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

#[derive(Default)]
struct Counts {
    total: usize,
    opens: usize,
    decodes: usize,
    samples: usize,
}

impl Counts {
    fn record(
        &mut self,
        kind: &str,
        path: &str,
        opens: bool,
        samples: usize,
        failure: Option<String>,
    ) {
        let decodes = opens && samples > 0 && failure.is_none();
        self.total += 1;
        self.opens += usize::from(opens);
        self.decodes += usize::from(decodes);
        self.samples += samples;
        println!(
            "{}",
            json!({"kind": kind, "path": path, "opens": opens,
            "decodes": decodes, "samples": samples, "failure": failure})
        );
    }

    fn summary(&self, kind: &str, path: &Path) {
        println!(
            "{}",
            json!({"kind": kind, "path": path, "total": self.total,
            "opens": self.opens, "decodes": self.decodes, "samples": self.samples})
        );
    }
}

fn roots(args: &[OsString]) -> Vec<PathBuf> {
    if !args.is_empty() {
        return args.iter().map(PathBuf::from).collect();
    }
    let mut roots = Vec::new();
    for variable in ["KONTRA_KONTAKT_LIBRARIES", "KONTRA_UVI_LIBRARIES"] {
        if let Some(paths) = std::env::var_os(variable) {
            roots.extend(std::env::split_paths(&paths));
        }
    }
    if let Some(config) = dirs::config_dir() {
        let settings = std::fs::read(config.join("kontra/settings.json")).unwrap_or_default();
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&settings) {
            roots.extend(
                value["roots"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r["path"].as_str())
                    .map(PathBuf::from),
            );
        }
    }
    if roots.is_empty() {
        roots.extend(
            [
                "/mnt/MAIN_STORAGE/Libraries/Kontakt",
                "/mnt/MAIN_STORAGE/Libraries/UVI",
            ]
            .into_iter()
            .map(PathBuf::from)
            .filter(|r| r.is_dir()),
        );
    }
    roots.sort();
    roots.dedup();
    roots
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    if dir.is_file() {
        files.push(dir.to_owned());
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect(&entry.path(), files)?;
        } else if kind.is_file()
            && entry.path().extension().is_some_and(|ext| {
                ["nki", "nkm", "ufs", "uvip"]
                    .iter()
                    .any(|e| ext.eq_ignore_ascii_case(e))
            })
        {
            files.push(entry.path());
        }
    }
    Ok(())
}

pub fn run(args: &[OsString]) -> io::Result<()> {
    let roots = roots(args);
    if roots.is_empty() {
        return Err(io::Error::other(
            "no installed library roots found; pass census ROOT ...",
        ));
    }
    let mut files = Vec::new();
    for root in &roots {
        collect(root, &mut files)?;
    }
    files.sort();
    files.dedup();
    let mut libraries = BTreeMap::<PathBuf, Counts>::new();
    let mut decoded = HashMap::<PathBuf, Result<(), String>>::new();
    for path in files {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ufs"))
        {
            bank(&path);
            continue;
        }
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("uvip"))
        {
            loose_program(&path);
            continue;
        }
        let library = path
            .ancestors()
            .skip(1)
            .find(|p| p.join("Samples").is_dir())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| {
                let root = roots.iter().find(|r| path.starts_with(r)).unwrap();
                if root.is_file() {
                    return path.parent().unwrap_or(Path::new(".")).into();
                }
                path.strip_prefix(root)
                    .ok()
                    .and_then(|relative| relative.components().next())
                    .map_or_else(|| root.clone(), |component| root.join(component))
            });
        let counts = libraries.entry(library).or_default();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nkm"))
        {
            multi(&path, counts, &mut decoded);
            continue;
        }
        match sampler_kontakt::read(&path) {
            Err(e) => counts.record(
                "kontakt",
                &path.to_string_lossy(),
                false,
                0,
                Some(e.to_string()),
            ),
            Ok(mut source) => {
                let mut failure = source
                    .instrument
                    .unsupported
                    .iter()
                    .find(|u| u.feature == "missing sample")
                    .map(|u| format!("missing sample: {}", u.value));
                let samples = source.locations.len();
                for location in &source.locations {
                    let result = decoded.entry(location.clone()).or_insert_with(|| {
                        source
                            .samples
                            .decode(location)
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    });
                    if let Err(reason) = result {
                        failure.get_or_insert(reason.clone());
                        break;
                    }
                }
                if samples == 0 {
                    failure.get_or_insert("no resolved sample zones".into());
                }
                counts.record("kontakt", &path.to_string_lossy(), true, samples, failure);
            }
        }
    }
    for (library, counts) in &libraries {
        counts.summary("kontakt-library", library);
    }
    Ok(())
}

fn multi(path: &Path, counts: &mut Counts, decoded: &mut HashMap<PathBuf, Result<(), String>>) {
    let result = sampler_kontakt::read_multi(path);
    let opens = result.is_ok();
    let mut samples = 0;
    let result = result.map_err(|e| e.to_string()).and_then(|multi| {
        let parent = path.parent().unwrap_or(Path::new("."));
        let root = parent
            .ancestors()
            .find(|p| p.join("Samples").is_dir())
            .unwrap_or(parent);
        let mut resolver = sampler_kontakt::Samples::new(root);
        samples = multi.sample_names.len();
        for name in &multi.sample_names {
            let location = resolver
                .resolve(parent, name)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("missing sample: {name}"))?;
            decoded
                .entry(location.clone())
                .or_insert_with(|| {
                    resolver
                        .decode(&location)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                })
                .clone()?;
        }
        if samples == 0 {
            return Err("no sample references in multi".into());
        }
        Ok(())
    });
    counts.record(
        "kontakt-multi",
        &path.to_string_lossy(),
        opens,
        samples,
        result.err(),
    );
}

fn loose_program(path: &Path) {
    let mut counts = Counts::default();
    match sampler_uvi::read(path) {
        Err(error) => counts.record(
            "uvi-program",
            &path.to_string_lossy(),
            false,
            0,
            Some(error.to_string()),
        ),
        Ok(program) => {
            let missing = program
                .instrument
                .unsupported
                .iter()
                .find(|u| u.feature == "missing sample")
                .map(|u| format!("missing sample: {}", u.value));
            let failure = program.locations.iter().find_map(|sample| {
                sampler_uvi::decode_sample(sample)
                    .err()
                    .map(|e| e.to_string())
            });
            counts.record(
                "uvi-program",
                &path.to_string_lossy(),
                true,
                program.locations.len(),
                missing.or(failure),
            );
        }
    }
}

#[cfg(feature = "library-access")]
fn resource_cache_key(program: &str, path: &str) -> String {
    let path = path.replace('\\', "/");
    if path.starts_with(['/', '$']) {
        path
    } else {
        format!("{}/{path}", program.rsplit_once('/').map_or("", |p| p.0))
    }
}

#[cfg(feature = "library-access")]
fn bank(path: &Path) {
    let bank = match sampler_uvi::Bank::open(path) {
        Ok(bank) => bank,
        Err(error) => {
            println!(
                "{}",
                json!({"kind": "uvi-bank", "path": path, "opens": false, "failure": error.to_string()})
            );
            return;
        }
    };
    let mut counts = Counts::default();
    let mut decoded = HashMap::<String, Result<(), String>>::new();
    for program in bank.programs() {
        let opened = bank.program(&program).map_err(|e| e.to_string());
        let opens = opened.is_ok();
        let mut samples = 0;
        let result = (|| -> Result<(), String> {
            let (text, member) = opened?;
            let document = sampler_uvi::parse_program_xml(&text).map_err(|e| e.to_string())?;
            for node in document.descendants() {
                let Some(sample) = node.attribute("SamplePath").filter(|s| !s.is_empty()) else {
                    continue;
                };
                samples += 1;
                let key = resource_cache_key(&member, sample);
                decoded
                    .entry(key)
                    .or_insert_with(|| {
                        bank.decode_resource(&member, sample)
                            .map(|_| ())
                            .map_err(|e| format!("{sample}: {e}"))
                    })
                    .clone()?;
            }
            if samples == 0 {
                return Err("no sample references (synthesis or unsupported oscillator)".into());
            }
            Ok(())
        })();
        counts.record(
            "uvi-program",
            &format!("{}::{program}", path.display()),
            opens,
            samples,
            result.err(),
        );
    }
    counts.summary("uvi-bank", path);
}

#[cfg(not(feature = "library-access"))]
fn bank(path: &Path) {
    println!(
        "{}",
        json!({"kind": "uvi-bank", "path": path, "opens": false,
        "failure": "UVI banks need the library-access feature"})
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "library-access")]
    #[test]
    fn rooted_resources_share_status_across_presets_but_relative_resources_do_not() {
        for sample in ["/Samples/a.wav", "$Authored.ufs/Samples/*L.wav*R.wav"] {
            assert_eq!(
                resource_cache_key("Presets/A/p.uvip", sample),
                resource_cache_key("Presets/B/p.uvip", sample)
            );
        }
        assert_eq!(
            resource_cache_key("Presets/A/p.uvip", "\\Samples\\a.wav"),
            "/Samples/a.wav"
        );
        assert_ne!(
            resource_cache_key("Presets/A/p.uvip", "a.wav"),
            resource_cache_key("Presets/B/p.uvip", "a.wav")
        );
    }

    #[test]
    fn a_program_must_open_and_decode_at_least_one_sample() {
        let mut counts = Counts::default();
        counts.record("fixture", "clear", true, 1, None);
        counts.record("fixture", "empty", true, 0, None);
        counts.record("fixture", "broken", false, 0, Some("damaged".into()));
        assert_eq!((counts.total, counts.opens, counts.decodes), (3, 2, 1));
    }
}
