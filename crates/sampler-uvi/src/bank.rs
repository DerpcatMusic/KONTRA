//! Programs and samples inside an installed UFS bank, ported from v1
//! (`src/uvi/library.rs`, `src/library/uvi.rs`). The reader namespaces come
//! from the user's installed official UVI Workstation (`access::reader_path`,
//! hash-verified); a bank's content state is prepared or reloaded only through
//! v1's owner-only store. Neither value is logged, printed or put in an error:
//! every failure crosses this boundary as `access::failure_reason`.

use crate::{
    access::{self, ReaderNamespaces},
    crypto,
    ufs::{Directory, Member, Ufs},
};
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

/// An open bank. Deliberately not Debug: it holds access values.
pub struct Bank {
    ufs: Ufs,
    directory: Directory,
    content_key: Option<u64>,
    program_namespace: Vec<u8>,
}

/// `uvi_reader` from the player's settings, as v1's catalog passes it.
fn configured_reader() -> Option<PathBuf> {
    let settings = std::fs::read(dirs::config_dir()?.join("kontra/settings.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&settings).ok()?;
    value.get("uvi_reader")?.as_str().map(PathBuf::from)
}

/// v1's private store: `KONTRA_UVI_AUTHORITY_DIR`, else `<config>/kontra/uvi-access`.
fn store_dir() -> Result<PathBuf> {
    std::env::var_os("KONTRA_UVI_AUTHORITY_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::config_dir().map(|c| c.join("kontra/uvi-access")))
        .context("The private UVI access store is unavailable")
}

impl Bank {
    /// Open and decode the directory of the bank at `path`.
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::open_inner(path).map_err(|e| access::failure_reason(&e))
    }

    fn open_inner(path: &Path) -> Result<Self> {
        let reader = access::reader_path(configured_reader().as_deref())?;
        let namespaces = ReaderNamespaces::open(&reader)?;
        let ufs = Ufs::open(path)?;
        let directory = ufs.decode_directory(&namespaces.metadata)?;
        // Only banks with encrypted members need a content state prepared.
        let content_key = if directory.files.iter().any(|m| m.mode == 2) {
            let store = store_dir()?;
            access::ensure_content_state(path, &ufs, &directory, &store)?.map(|state| state.key)
        } else {
            None
        };
        Ok(Self {
            ufs,
            directory,
            content_key,
            program_namespace: namespaces.program,
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

    /// The bytes of the member at bank-root `path` (a script, say).
    pub fn file(&self, path: &str) -> Result<Vec<u8>, String> {
        let read = || self.read(resolve(&self.directory, path)?);
        read().map_err(|e| access::failure_reason(&e))
    }

    /// Decode the program at member `name` to its clear XML and its path.
    pub fn program(&self, name: &str) -> Result<(String, String), String> {
        self.program_inner(name)
            .map_err(|e| access::failure_reason(&e))
    }

    fn program_inner(&self, name: &str) -> Result<(String, String)> {
        let member = resolve(&self.directory, name)?;
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
    pub fn resource(&self, program_path: &str, path: &str) -> Result<Vec<Vec<u8>>, String> {
        self.resource_inner(program_path, path)
            .map_err(|e| access::failure_reason(&e))
    }

    fn resource_inner(&self, program_path: &str, path: &str) -> Result<Vec<Vec<u8>>> {
        resources(&self.directory, program_path, path)?
            .iter()
            .map(|member| self.read(member))
            .collect()
    }
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

fn resource<'a>(directory: &'a Directory, program_path: &str, path: &str) -> Result<&'a Member> {
    let base = program_path.rsplit_once('/').map_or("", |(base, _)| base);
    let path = if path.starts_with('/') {
        normalize(path)?
    } else {
        normalize(&format!("{base}/{path}"))?
    };
    resolve(directory, &path)
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
