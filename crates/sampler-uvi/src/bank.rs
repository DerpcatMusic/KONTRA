//! Programs and samples inside an installed UFS bank. Reader namespaces come
//! from the user's hash-verified official UVI Workstation. Recovered content
//! access and decoded bytes live only in this process; nothing is persisted.

use crate::{
    AccessError,
    access::{self, ReaderNamespaces},
    crypto,
    ufs::{Directory, Member, Protection, Ufs},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// An open bank. Deliberately not Debug: it holds access values.
pub struct Bank {
    ufs: Ufs,
    directory: Directory,
    content_key: Option<u64>,
    program_namespace: Vec<u8>,
    /// Duplicate paths remain ambiguous, as in v1's ResourceIndex.
    paths: HashMap<String, Option<usize>>,
}

/// `uvi_reader` from the player's settings, as v1's catalog passes it.
pub(crate) fn configured_reader() -> Option<PathBuf> {
    let settings = std::fs::read(dirs::config_dir()?.join("kontra/settings.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&settings).ok()?;
    value.get("uvi_reader")?.as_str().map(PathBuf::from)
}

/// Clear and ZIP-wrapped programs need no installed reader; protected ones do.
pub(crate) fn program_text(bytes: &[u8]) -> Result<String, AccessError> {
    let program_error = |e| AccessError::Program(access::failure_reason(&e));
    match crypto::decode_program_bytes(bytes, &[]) {
        Ok(text) => Ok(text),
        Err(error) if error.is::<crypto::NeedsProgramNamespace>() => {
            let reader_error = |e| AccessError::Reader(access::failure_reason(&e));
            let reader =
                access::reader_path(configured_reader().as_deref()).map_err(reader_error)?;
            let namespaces = ReaderNamespaces::open(&reader).map_err(reader_error)?;
            crypto::decode_program_bytes(bytes, &namespaces.program).map_err(program_error)
        }
        Err(error) => Err(program_error(error)),
    }
}

impl Bank {
    /// Open and decode the directory of the bank at `path`.
    pub fn open(path: &Path) -> Result<Self, AccessError> {
        let reader_error = |e| AccessError::Reader(access::failure_reason(&e));
        let bank_error = |e| AccessError::Bank(access::failure_reason(&e));
        let content_error = |e| AccessError::Content(access::failure_reason(&e));
        let reader = access::reader_path(configured_reader().as_deref()).map_err(reader_error)?;
        let namespaces = ReaderNamespaces::open(&reader).map_err(reader_error)?;
        let ufs = Ufs::open(path).map_err(bank_error)?;
        let directory = ufs
            .decode_directory(&namespaces.metadata)
            .map_err(bank_error)?;
        // Only banks with encrypted members need a content state prepared.
        let content_key = if directory
            .files
            .iter()
            .any(|m| m.mode == Protection::Content)
        {
            Some(access::recover_content_key(path, &ufs, &directory).map_err(content_error)?)
        } else {
            None
        };
        let mut paths = HashMap::with_capacity(directory.files.len());
        for (index, member) in directory.files.iter().enumerate() {
            if let Some(path) = &member.path {
                paths
                    .entry(path.clone())
                    .and_modify(|entry| *entry = None)
                    .or_insert(Some(index));
            }
        }
        Ok(Self {
            ufs,
            directory,
            content_key,
            program_namespace: namespaces.program,
            paths,
        })
    }

    /// Program member paths (`*.uvip`), in directory order.
    pub fn programs(&self) -> Vec<String> {
        let mut programs: Vec<String> = self
            .directory
            .files
            .iter()
            .filter_map(|m| m.path.clone())
            .filter(|p| p.to_ascii_lowercase().ends_with(".uvip"))
            .collect();
        programs.sort();
        programs
    }

    fn read(&self, member: &Member) -> Result<Vec<u8>> {
        self.ufs
            .read_member(member, self.directory.metadata_key, self.content_key)
    }

    /// Every member path in the bank, in directory order.
    pub fn members(&self) -> Vec<String> {
        self.directory
            .files
            .iter()
            .filter_map(|m| m.path.clone())
            .collect()
    }

    /// Every Lua member of the bank, for a script's `require`.
    pub fn scripts(&self) -> crate::script::Scripts {
        let mut scripts = crate::script::Scripts::default();
        for path in self.members() {
            if path.to_ascii_lowercase().ends_with(".lua")
                && let Ok(bytes) = self.file(&path)
            {
                scripts.insert(&path, String::from_utf8_lossy(&bytes).into_owned());
            }
        }
        scripts
    }

    /// The bytes of the member at bank-root `path` (a script, say).
    pub fn file(&self, path: &str) -> Result<Vec<u8>, String> {
        let read = || self.read(resolve(&self.directory, path)?);
        read().map_err(|e| access::failure_reason(&e))
    }

    /// Decode the program at member `name` to its clear XML and its path.
    pub fn program(&self, name: &str) -> Result<(String, String), AccessError> {
        self.program_inner(name)
            .map_err(|e| AccessError::Program(access::failure_reason(&e)))
    }

    fn program_inner(&self, name: &str) -> Result<(String, String)> {
        let member = self.resolve(name)?;
        ensure!(
            member.size <= crypto::PROGRAM_XML_LIMIT as u64,
            "UVI program exceeds 32 MiB"
        );
        let bytes = self.read(member)?;
        let text = crypto::decode_program_bytes(&bytes, &self.program_namespace)?;
        let path = member
            .path
            .clone()
            .context("UVI program has no directory path")?;
        Ok((text, path))
    }

    /// Decode a bank-local audio resource relative to `program_path`. A starred
    /// filename is a bundle of mono channels. Returns raw encoded file bytes.
    pub fn resource(&self, program_path: &str, path: &str) -> Result<Vec<Vec<u8>>, AccessError> {
        self.resource_inner(program_path, path)
            .map_err(|e| AccessError::Resource(access::failure_reason(&e)))
    }

    /// Decode a resource or mono-channel bundle without translating the program.
    pub fn decode_resource(
        &self,
        program_path: &str,
        path: &str,
    ) -> Result<sampler_kontakt::Decoded, AccessError> {
        crate::audio::decode(&self.resource(program_path, path)?)
            .map(|(audio, _)| audio)
            .map_err(AccessError::Audio)
    }

    fn resource_inner(&self, program_path: &str, path: &str) -> Result<Vec<Vec<u8>>> {
        let path = path.replace('\\', "/");
        let (program_path, path) = resource_base(program_path, &path, &self.ufs.header.bank_name)?;
        resources(program_path, path, |path| self.resolve(path))?
            .iter()
            .map(|member| self.read(member))
            .collect()
    }

    fn resolve(&self, path: &str) -> Result<&Member> {
        let path = normalize(path)?;
        if let Some(index) = self.paths.get(&path) {
            return index
                .map(|i| &self.directory.files[i])
                .context("Ambiguous UVI member path");
        }
        resolve(&self.directory, &path)
    }
}

/// Falcon's `$Bank.ufs/` volume is rooted in this bank, not the preset folder.
/// Never satisfy a resource explicitly bound to a different bank.
fn resource_base<'a>(program: &'a str, path: &'a str, bank: &str) -> Result<(&'a str, &'a str)> {
    let Some(volume) = path.strip_prefix('$') else {
        return Ok((program, path));
    };
    let (volume, relative) = volume
        .split_once('/')
        .context("UVI bank volume has no resource path")?;
    ensure!(
        volume.eq_ignore_ascii_case(bank) || volume.eq_ignore_ascii_case(&format!("{bank}.ufs")),
        "UVI resource belongs to a different bank"
    );
    Ok(("", relative))
}

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

/// Exact paths take priority; a bare member name is usable only if unique.
fn resolve<'a>(directory: &'a Directory, path: &str) -> Result<&'a Member> {
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
    ensure!(named.len() == 1, "UVI member name is absent or ambiguous");
    Ok(named[0])
}

fn resource<'a>(
    program_path: &str,
    path: &str,
    resolve: impl Fn(&str) -> Result<&'a Member>,
) -> Result<&'a Member> {
    let base = program_path.rsplit_once('/').map_or("", |(base, _)| base);
    let path = if path.starts_with('/') {
        normalize(path)?
    } else {
        normalize(&format!("{base}/{path}"))?
    };
    resolve(&path)
}

/// A starred filename is an ordered list of synchronized mono channels.
fn resources<'a>(
    program_path: &str,
    path: &str,
    resolve: impl Fn(&str) -> Result<&'a Member>,
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
                resource(program_path, &format!("{base}{name}"), &resolve)
            })
            .collect()
    } else {
        Ok(vec![resource(program_path, path, resolve)?])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_bank_volumes_are_rooted_and_cannot_cross_banks() {
        let (program, path) = resource_base(
            "Presets/Category/p.uvip",
            "$Authored.ufs/Scripts/./../IRs/space.aif",
            "Authored",
        )
        .unwrap();
        assert_eq!(program, "");
        assert_eq!(normalize(path).unwrap(), "IRs/space.aif");
        assert!(resource_base("Presets/p.uvip", "$Other.ufs/IRs/space.aif", "Authored").is_err());
        assert!(resource_base("Presets/p.uvip", "$Authored.ufs", "Authored").is_err());
        let (_, path) =
            resource_base("Presets/p.uvip", "$Authored.ufs/../escape.aif", "Authored").unwrap();
        assert!(normalize(path).is_err());
    }
}
