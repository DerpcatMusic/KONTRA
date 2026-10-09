//! Programs and samples inside an installed UFS bank. Namespaces use the native
//! format tables; recovered content access and decoded bytes stay in memory.

use crate::{
    AccessError,
    access::{self, Namespaces},
    crypto,
    ufs::{Directory, Member, Protection, Ufs},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::HashMap,
    path::Path,
    sync::Arc,
};

/// An open bank. Deliberately not Debug: it holds access values.
pub struct Bank {
    ufs: Arc<Ufs>,
    directory: Directory,
    content_key: Option<u64>,
    program_namespace: Vec<u8>,
    /// Duplicate paths remain ambiguous, as in v1's ResourceIndex.
    paths: HashMap<String, Option<usize>>,
}

/// Decode clear and protected programs through the native PasswordV2 path.
pub(crate) fn program_text(bytes: &[u8]) -> Result<String, AccessError> {
    let namespaces = Namespaces::native();
    crypto::decode_program_bytes(bytes, &namespaces.program)
        .map_err(|e| AccessError::Program(access::failure_reason(&e)))
}

impl Bank {
    /// Open and decode the directory of the bank at `path`.
    pub fn open(path: &Path) -> Result<Self, AccessError> {
        let bank_error = |e| AccessError::Bank(access::failure_reason(&e));
        let content_error = |e| AccessError::Content(access::failure_reason(&e));
        let span = sampler_kontakt::audit::Span::new("uvi_ufs_header");
        let ufs = Ufs::open(path).map_err(bank_error)?;
        drop(span);
        let span = sampler_kontakt::audit::Span::new("uvi_directory_namespace");
        let namespaces = Namespaces::native();
        let directory = ufs
            .decode_directory(&namespaces.metadata)
            .map_err(bank_error)?;
        let program_namespace = namespaces.program;
        drop(span);
        let span = sampler_kontakt::audit::Span::new("uvi_content_setup");
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
        drop(span);
        let mut paths = HashMap::with_capacity(directory.files.len());
        for (index, member) in directory.files.iter().enumerate() {
            if let Some(path) = &member.path {
                paths
                    .entry(path.to_ascii_lowercase())
                    .and_modify(|entry| *entry = None)
                    .or_insert(Some(index));
            }
        }
        Ok(Self {
            ufs: Arc::new(ufs),
            directory,
            content_key,
            program_namespace,
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

    /// Read a UI asset relative to its preset or the bank resource root. UVI
    /// scripts commonly name Resources/... from a shared Scripts folder.
    /// Ambiguous suffixes and references to another bank are never accepted.
    pub fn ui_resource(&self, program: &str, path: &str) -> Result<Vec<u8>, String> {
        self.ui_resource_result(program, path)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "UI resource missing".into())
    }

    /// Same bank/script-origin authority as ui_resource, without erasing failure categories.
    pub fn ui_resource_result(&self, program: &str, path: &str) -> std::result::Result<Option<Vec<u8>>, crate::ResourceError> {
        use crate::ResourceError as E;
        crate::resources::validate_path(path)?;
        let path = path.replace('\\', "/");
        let (program, path) = resource_base(program, &path, &self.ufs.header.bank_name).map_err(|_| E::InvalidPath)?;
        let candidates = ui_candidates(program, path).map_err(|_| E::InvalidPath)?;
        let mut member = None;
        for candidate in &candidates {
            if let Some(index) = self.paths.get(&candidate.to_ascii_lowercase()) {
                member = Some(&self.directory.files[index.ok_or(E::Ambiguous)?]);
                break;
            }
        }
        let member = if let Some(member) = member { member } else {
            let normalized = normalize(path).map_err(|_| E::InvalidPath)?;
            let suffix = format!("/{}", normalized.to_ascii_lowercase());
            let mut matches = self.directory.files.iter().filter(|m| {
                m.path.as_ref().is_some_and(|p| p.to_ascii_lowercase().ends_with(&suffix))
            });
            let Some(first) = matches.next() else {return Ok(None)};
            if matches.next().is_some() {return Err(E::Ambiguous)};
            first
        };
        if member.size > 32 << 20 {return Err(E::Limit)};
        self.read(member).map(Some).map_err(|e| {
            if e.downcast_ref::<std::io::Error>().is_some() {E::Read} else {E::Corrupt}
        })
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

    /// A bank-local audio resource for streaming: read in pieces and decrypted
    /// in memory as it is read, never written out. Checked by opening it once.
    pub fn stream_source(
        &self,
        program_path: &str,
        path: &str,
    ) -> Result<std::sync::Arc<dyn sampler_kontakt::AssetSource>, String> {
        self.stream_inner(program_path, path)
            .map_err(|e| access::failure_reason(&e))
    }

    fn stream_inner(
        &self,
        program_path: &str,
        path: &str,
    ) -> Result<std::sync::Arc<dyn sampler_kontakt::AssetSource>> {
        let path = path.replace('\\', "/");
        let (program_path, path) = resource_base(program_path, &path, &self.ufs.header.bank_name)?;
        let parts = resources(program_path, path, |path| self.resolve(path))?
            .into_iter()
            .map(|member| {
                let (offset, size, key) = self.ufs.locate(
                    member,
                    self.directory.metadata_key,
                    self.content_key,
                )?;
                Ok(crate::stream::Origin::Member { ufs: self.ufs.clone(), offset, size, key })
            })
            .collect::<Result<Vec<_>>>()?;
        crate::stream::source(parts).map_err(anyhow::Error::msg)
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
        if let Some(index) = self.paths.get(&path.to_ascii_lowercase()) {
            return index
                .map(|i| &self.directory.files[i])
                .context("Ambiguous UVI member path");
        }
        resolve(&self.directory, &path)
    }
}

/// Preserve the actual module directory first, then search its package roots.
/// Candidate order never turns an ambiguous exact identity into a suffix hit.
fn ui_candidates(program: &str, path: &str) -> Result<Vec<String>> {
    let base = program.rsplit_once('/').map_or("", |(base, _)| base);
    let rooted = normalize(path)?;
    let mut out = Vec::new();
    if !path.starts_with('/') { out.push(normalize(&format!("{base}/{path}"))?); }
    if !out.contains(&rooted) { out.push(rooted.clone()); }
    let parts: Vec<_> = rooted.split('/').collect();
    if let Some(scripts) = parts.iter().position(|p| p.eq_ignore_ascii_case("Scripts")) {
        // A module can live below nested package folders. The bank index is
        // authoritative; no filesystem or another bank participates.
        for start in scripts + 1..parts.len() {
            let candidate = parts[start..].join("/");
            if !out.contains(&candidate) { out.push(candidate); }
        }
    }
    Ok(out)
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
        .filter(|m| m.path.as_ref().is_some_and(|p| p.eq_ignore_ascii_case(&path)))
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }
    ensure!(exact.is_empty(), "Ambiguous UVI member path");
    ensure!(
        !path.contains('/'),
        "UVI resource path is absent from this bank"
    );
    let named: Vec<_> = directory.files.iter().filter(|m| m.name.eq_ignore_ascii_case(&path)).collect();
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
    fn standalone_passwordv2_program_text_uses_native_namespace() {
        use base64::Engine as _;

        let namespace = Namespaces::native().program;
        let mut password = b"native standalone fixture password\0".to_vec();
        let mut program = b"<Program Name=\"Native standalone fixture\"/>".to_vec();
        program.resize(program.len().div_ceil(8) * 8, 0);
        crypto::transform(
            &mut program,
            crypto::key_from_string(&password[..password.len() - 1]),
            0,
        );
        crypto::transform(&mut password, crypto::key_from_string(&namespace), 0);
        let wrapper = format!(
            "<UVI4><Program PasswordV2=\"{}\">{}</Program></UVI4>",
            base64::engine::general_purpose::STANDARD.encode(password),
            base64::engine::general_purpose::STANDARD.encode(program),
        );
        assert_eq!(
            program_text(wrapper.as_bytes()).unwrap(),
            "<Program Name=\"Native standalone fixture\"/>"
        );
    }

    #[test]
    fn clear_bank_and_program_load_with_native_namespaces() {
        fn append(bytes: &mut Vec<u8>, payload: &[u8]) -> u64 {
            let pointer = bytes.len() as u64 + 8;
            bytes.extend((payload.len() as u64).to_le_bytes());
            bytes.extend(payload);
            pointer
        }
        fn point(bytes: &mut [u8], at: u64, value: u64) {
            bytes[at as usize..at as usize + 8].copy_from_slice(&value.to_le_bytes());
        }
        let xml = b"<UVI4><Program Name=\"Our clear fixture\"/></UVI4>";
        let mut bytes = vec![0; 320];
        bytes[..4].copy_from_slice(b"UFS2");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        bytes[48..56].copy_from_slice(b"Authored");
        // +32 stays opaque, not an asserted physical size.
        point(&mut bytes, 32, 123);
        let mut folder = vec![0; 272];
        folder[..4].copy_from_slice(&0x2fba3632u32.to_le_bytes());
        folder[4..8].copy_from_slice(b"Root");
        let root = append(&mut bytes, &folder);
        point(&mut bytes, 40, root);
        let mut file = vec![0; 289];
        file[..4].copy_from_slice(&0x675850e4u32.to_le_bytes());
        file[4..15].copy_from_slice(b"preset.uvip");
        let member = append(&mut bytes, &file);
        let mut descriptor = vec![0; 34];
        descriptor[..4].copy_from_slice(&0x1847b398u32.to_le_bytes());
        let tree = append(&mut bytes, &descriptor);
        point(&mut bytes, root + 260, tree);
        let mut leaf = vec![0; 288];
        leaf[..4].copy_from_slice(&0x3ca86aafu32.to_le_bytes());
        leaf[4..8].copy_from_slice(&1u32.to_le_bytes());
        leaf[8..19].copy_from_slice(b"preset.uvip");
        point(&mut leaf, 264, member);
        leaf[272..288].fill(255);
        let table = append(&mut bytes, &leaf);
        for offset in [4, 12, 20] { point(&mut bytes, tree + offset, table); }
        let payload = append(&mut bytes, xml);
        point(&mut bytes, member + 260, xml.len() as u64);
        point(&mut bytes, member + 268, payload);
        let path = std::env::temp_dir().join(format!("kontra-clear-bank-{}.ufs", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let mut bank = Bank::open(&path).unwrap();
        assert_eq!(bank.program_namespace.len(), 39);
        assert_eq!(bank.programs(), ["preset.uvip"]);
        assert_eq!(bank.program("preset.uvip").unwrap(),
            (std::str::from_utf8(xml).unwrap().to_owned(), "preset.uvip".to_owned()));
        assert!(bank.directory.warnings.is_empty());
        use crate::ResourceError as E;
        assert_eq!(bank.ui_resource_result("preset.uvip", "preset.uvip").unwrap(), Some(xml.to_vec()));
        assert_eq!(bank.ui_resource_result("preset.uvip", "missing.png").unwrap(), None);
        assert_eq!(bank.ui_resource_result("preset.uvip", "$Other.ufs/preset.uvip"), Err(E::InvalidPath));
        let index=bank.paths["preset.uvip"].unwrap();
        bank.paths.insert("preset.uvip".into(),None);
        assert_eq!(bank.ui_resource_result("preset.uvip", "preset.uvip"), Err(E::Ambiguous));
        bank.paths.insert("preset.uvip".into(),Some(index));
        let member=&mut bank.directory.files[index];
        let size=member.size;
        member.size=(32<<20)+1;
        assert_eq!(bank.ui_resource_result("preset.uvip", "preset.uvip"), Err(E::Limit));
        bank.directory.files[index].size=size;
        let offset=bank.directory.files[index].offset;
        bank.directory.files[index].offset=u64::MAX;
        assert_eq!(bank.ui_resource_result("preset.uvip", "preset.uvip"), Err(E::Corrupt));
        bank.directory.files[index].offset=offset;
        std::fs::remove_file(path).unwrap();
        assert_eq!(bank.ui_resource_result("preset.uvip", "preset.uvip"), Err(E::Read));
    }

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

    #[test]
    fn ui_candidates_keep_module_and_package_roots_in_order() {
        assert_eq!(ui_candidates("Presets/Strings/p.uvip", "/Scripts/_ui/../Resources/knob.png").unwrap(),
            ["Scripts/Resources/knob.png", "Resources/knob.png", "knob.png"]);
        assert_eq!(ui_candidates("Presets/Strings/p.uvip", "Resources/knob.png").unwrap(),
            ["Presets/Strings/Resources/knob.png", "Resources/knob.png"]);
        assert!(ui_candidates("Presets/p.uvip", "/Scripts/../../escape.png").is_err());
    }
}
