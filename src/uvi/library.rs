//! Local UFS resources: exact virtual paths, private keyed reads and shared PCM.
use super::{
    crypto,
    program::{self, Program},
    sample::{self, Sample},
    ufs::{Directory, Member, Ufs},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Arc,
};

pub(crate) const PCM_LIMIT: usize = 512 << 20;
const MODULE_LIMIT: usize = 16 << 20;

fn normalize(path: &str) -> Result<String> {
    ensure!(
        !path.contains(['\0', ':']) && !path.starts_with('$'),
        "Unresolved UVI volume path"
    );
    let path = path.replace('\\', "/");
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                ensure!(
                    parts.pop().is_some(),
                    "UVI resource path ascends above bank root"
                );
            }
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

/// Exact paths take priority. A bare member name is usable only if unique.
pub fn resolve_member<'a>(directory: &'a Directory, path: &str) -> Result<&'a Member> {
    let path = normalize(path)?;
    let exact: Vec<_> = directory
        .files
        .iter()
        .filter(|m| m.path.as_deref() == Some(&path))
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }
    ensure!(exact.is_empty(), "Ambiguous UVI member path");
    ensure!(
        !path.contains('/'),
        "UVI resource path is absent from this bank"
    );
    let named: Vec<_> = directory.files.iter().filter(|m| m.name == path).collect();
    ensure!(
        named.len() == 1,
        "UVI member name must identify exactly one record (found {})",
        named.len()
    );
    Ok(named[0])
}

fn resource<'a>(directory: &'a Directory, program_path: &str, path: &str) -> Result<&'a Member> {
    let base = program_path.rsplit_once('/').map_or("", |(base, _)| base);
    let path = if path.starts_with('/') {
        normalize(path)?
    } else {
        normalize(&format!("{base}/{path}"))?
    };
    resolve_member(directory, &path)
}

/// A starred filename is an ordered list of synchronized mono channels.
fn resources<'a>(
    directory: &'a Directory,
    program_path: &str,
    path: &str,
) -> Result<Vec<&'a Member>> {
    if let Some((base, names)) = path.split_once('*') {
        ensure!(
            base.ends_with(['/', '\\']),
            "Invalid UVI channel bundle prefix"
        );
        let names: Vec<_> = names.split('*').collect();
        ensure!(
            !names.is_empty() && names.len() <= 12,
            "UVI channel bundle exceeds 12 channels"
        );
        names
            .into_iter()
            .map(|name| {
                ensure!(
                    !name.is_empty() && !name.contains(['/', '\\']),
                    "Invalid UVI channel bundle member"
                );
                resource(directory, program_path, &format!("{base}{name}"))
            })
            .collect()
    } else {
        Ok(vec![resource(directory, program_path, path)?])
    }
}

pub struct Library {
    pub bank: Ufs,
    pub directory: Directory,
    content_key: Option<u64>,
}

pub struct LoadedProgram {
    pub program: Program,
    pub path: String,
}

impl Library {
    pub fn open(path: &Path, namespace: &[u8], content_key: Option<u64>) -> Result<Self> {
        let bank = Ufs::open(path)?;
        let directory = bank.decode_directory(namespace)?;
        Ok(Self {
            bank,
            directory,
            content_key,
        })
    }

    pub fn read(&self, member: &Member) -> Result<Vec<u8>> {
        self.bank
            .read_member(member, self.directory.metadata_key, self.content_key)
    }

    pub fn program(&self, name: &str, namespace: &[u8]) -> Result<LoadedProgram> {
        let member = resolve_member(&self.directory, name)?;
        let bytes = self.read(member)?;
        let text = std::str::from_utf8(&bytes).context("Decoded UVI program is not UTF-8")?;
        let text = crypto::decode_program(text, namespace)?;
        Ok(LoadedProgram {
            program: program::parse_program(&text)?,
            path: member
                .path
                .clone()
                .context("UVI program has no decoded directory path")?,
        })
    }

    /// Resolve and decode an approved bank-local audio resource, including bundles.
    pub fn audio(&self, program_path: &str, path: &str) -> Result<Sample> {
        let members = resources(&self.directory, program_path, path)?;
        let mut operands = Vec::with_capacity(members.len());
        for member in members {
            operands.push(sample::decode(&self.read(member)?)?);
        }
        if operands.len() == 1 {
            Ok(operands.pop().unwrap())
        } else {
            sample::assemble_mono(operands)
        }
    }

    /// Ordered archive records identify audio independently of path spelling.
    pub(crate) fn audio_identity(&self, program_path: &str, path: &str) -> Result<Vec<u64>> {
        Ok(resources(&self.directory, program_path, path)?
            .iter()
            .map(|member| member.record_offset)
            .collect())
    }

    /// Bounded bank-local data, with the same path authority as audio loading.
    pub fn data(&self, program_path: &str, path: &str, limit: u64) -> Result<Vec<u8>> {
        let member = resource(&self.directory, program_path, path)?;
        ensure!(
            member.size <= limit,
            "UVI resource exceeds caller's byte limit"
        );
        self.read(member)
    }

    /// Source-only modules supplied to the sandbox, with no ambient disk search.
    pub fn modules(&self) -> Result<BTreeMap<String, Vec<u8>>> {
        let mut modules = BTreeMap::new();
        let mut total = 0usize;
        let mut stems = HashMap::<String, Vec<&Member>>::new();
        for member in self
            .directory
            .files
            .iter()
            .filter(|m| m.name.to_ascii_lowercase().ends_with(".lua"))
        {
            ensure!(member.size <= 2 << 20, "UVI Lua module exceeds 2 MiB limit");
            total = total
                .checked_add(member.size as usize)
                .context("UVI module size overflow")?;
            ensure!(total <= MODULE_LIMIT, "UVI modules exceed 16 MiB limit");
            let bytes = self.read(member)?;
            std::str::from_utf8(&bytes).context("Decoded UVI module is not UTF-8")?;
            ensure!(
                !bytes.starts_with(b"\x1bLua"),
                "Only Lua source modules are accepted"
            );
            let path = member
                .path
                .as_ref()
                .context("UVI module has no decoded directory path")?;
            let name = path[..path.len() - 4].replace('/', ".");
            ensure!(
                modules.insert(name, bytes).is_none(),
                "Duplicate UVI module path"
            );
            stems
                .entry(member.name[..member.name.len() - 4].into())
                .or_default()
                .push(member);
        }
        for (stem, members) in stems {
            if members.len() == 1 && !modules.contains_key(&stem) {
                let path = members[0].path.as_ref().unwrap();
                let bytes = &modules[&path[..path.len() - 4].replace('/', ".")];
                total = total
                    .checked_add(bytes.len())
                    .context("UVI module size overflow")?;
                ensure!(
                    total <= MODULE_LIMIT,
                    "UVI modules exceed 16 MiB limit including aliases"
                );
                modules.insert(stem, bytes.clone());
            }
        }
        Ok(modules)
    }

    /// Decode each referenced sample once, retaining every source channel.
    /// Bypassed oscillators still need their resources if Lua enables them.
    pub fn samples(&self, loaded: &LoadedProgram) -> Result<HashMap<String, Arc<Sample>>> {
        let mut result = HashMap::new();
        let mut cache = HashMap::<Vec<u64>, Arc<Sample>>::new();
        let mut total = 0usize;
        let paths = loaded
            .program
            .sample_zones
            .iter()
            .map(|z| z.sample_path.as_str())
            .chain(loaded.program.nodes.iter().filter_map(|n| {
                match n.kind.as_str() {
                    "Convolver" | "SampledReverb" => n.attributes.get("SamplePath"),
                    "WaveTableOscillator" => n.attributes.get("WavetablePath"),
                    _ => None,
                }
                .map(String::as_str)
            }));
        for path in paths.filter(|p| !p.is_empty()) {
            if result.contains_key(path) {
                continue;
            }
            let identity = self.audio_identity(&loaded.path, path)?;
            let sample = if let Some(sample) = cache.get(&identity) {
                sample.clone()
            } else {
                let decoded = self.audio(&loaded.path, path)?;
                total = total
                    .checked_add(decoded.interleaved.bytes())
                    .context("UVI sample memory overflow")?;
                ensure!(
                    total <= PCM_LIMIT,
                    "UVI resident sample data exceeds 512 MiB limit"
                );
                let sample = Arc::new(decoded);
                cache.insert(identity, sample.clone());
                sample
            };
            result.insert(path.to_owned(), sample);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_resource_paths_and_ambiguous_names() {
        let member = |path: &str| Member {
            record_offset: 1,
            name: path.rsplit('/').next().unwrap().into(),
            path: Some(path.into()),
            parent: None,
            size: 0,
            offset: 0,
            mode: 0,
            footer: Vec::new(),
        };
        let directory = Directory {
            files: vec![
                member("Samples/a.wav"),
                member("Other/a.wav"),
                member("Scripts/data.lua"),
            ],
            directories: Vec::new(),
            records: Vec::new(),
            warnings: Vec::new(),
            metadata_key: 0,
        };
        assert_eq!(
            resource(&directory, "Programs/p.uvip", ".\\..\\Samples\\a.wav")
                .unwrap()
                .path
                .as_deref(),
            Some("Samples/a.wav")
        );
        assert!(resolve_member(&directory, "a.wav").is_err());
        assert!(resolve_member(&directory, "Missing/a.wav").is_err());
        assert!(resource(&directory, "Programs/p.uvip", "../../a.wav").is_err());
        assert!(resolve_member(&directory, "$Other/a.wav").is_err());
        let bundle = resources(&directory, "Programs/p.uvip", "../Samples/*a.wav*a.wav").unwrap();
        assert_eq!(bundle.len(), 2);
        assert!(resources(&directory, "Programs/p.uvip", "../Samples/*a.wav*").is_err());
        assert!(resources(&directory, "Programs/p.uvip", "../Samples/*../Other/a.wav").is_err());
        assert_eq!(
            resolve_member(&directory, "data.lua").unwrap().name,
            "data.lua"
        );
        let path = std::env::temp_dir().join(format!("uvi-identity-{}.ufs", std::process::id()));
        let mut header = [0u8; 320];
        header[..4].copy_from_slice(b"UFS2");
        header[4..8].copy_from_slice(&3u32.to_le_bytes());
        std::fs::write(&path, header).unwrap();
        let library = Library {
            bank: Ufs::open(&path).unwrap(),
            directory,
            content_key: None,
        };
        assert_eq!(
            library
                .audio_identity("Programs/p.uvip", "../Samples/a.wav")
                .unwrap(),
            library
                .audio_identity("Programs/p.uvip", "/Samples/./a.wav")
                .unwrap()
        );
        assert_ne!(
            library
                .audio_identity("Programs/p.uvip", "../Samples/a.wav")
                .unwrap(),
            library
                .audio_identity("Programs/p.uvip", "../Samples/*a.wav*a.wav")
                .unwrap()
        );
        std::fs::remove_file(path).unwrap();
    }
}
