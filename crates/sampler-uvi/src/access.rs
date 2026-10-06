//! Automatic local UFS content preparation. No account state, reader constants, or bank keys are bundled.
use super::{
    crypto,
    ufs::{Directory, Member, Protection, Ufs},
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub(crate) struct ReaderNamespaces {
    pub(crate) metadata: Vec<u8>,
    pub(crate) program: Vec<u8>,
}

impl ReaderNamespaces {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let mut bytes = Vec::new();
        File::open(path)?
            .take((64 << 20) + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 64 << 20, "UVI reader exceeds size limit");
        let digest = format!("{:x}", Sha256::digest(&bytes));
        ensure!(
            digest == "78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721",
            "Reader namespace layout is verified only for official UVI Workstation 4.0.9 x64"
        );
        Ok(Self {
            metadata: bytes[0x1ea4e58..0x1ea4e58 + 36].to_vec(),
            program: bytes[31_586_936..31_586_936 + 39].to_vec(),
        })
    }
}

/// Verify all PNG CRCs and critical chunk structure without decoding IDAT pixels.
pub(crate) fn validate_png(bytes: &[u8]) -> Result<usize> {
    ensure!(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "Invalid PNG signature"
    );
    let mut offset = 8usize;
    let (mut chunks, mut data, mut data_ended) = (0usize, false, false);
    let (mut depth, mut color, mut palette) = (0u8, 0u8, 0usize);
    while offset < bytes.len() {
        let header = bytes
            .get(offset..offset + 8)
            .context("Truncated PNG chunk")?;
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        ensure!(size <= i32::MAX as usize, "Invalid PNG chunk length");
        let end = offset
            .checked_add(12)
            .and_then(|n| n.checked_add(size))
            .context("PNG chunk overflow")?;
        let chunk = bytes
            .get(offset + 4..end)
            .context("Truncated PNG chunk data")?;
        let crc = u32::from_be_bytes(chunk[chunk.len() - 4..].try_into().unwrap());
        ensure!(
            crc32fast::hash(&chunk[..chunk.len() - 4]) == crc,
            "PNG CRC mismatch"
        );
        let kind = &header[4..8];
        ensure!(
            kind.iter().all(u8::is_ascii_alphabetic),
            "Invalid PNG chunk type"
        );
        if chunks == 0 {
            ensure!(kind == b"IHDR" && size == 13, "PNG must start with IHDR");
            let ihdr = &chunk[4..17];
            ensure!(
                (1..=i32::MAX as u32).contains(&u32::from_be_bytes(ihdr[..4].try_into().unwrap()))
                    && (1..=i32::MAX as u32)
                        .contains(&u32::from_be_bytes(ihdr[4..8].try_into().unwrap())),
                "Invalid PNG dimensions"
            );
            ensure!(
                match ihdr[9] {
                    0 => matches!(ihdr[8], 1 | 2 | 4 | 8 | 16),
                    2 | 4 | 6 => matches!(ihdr[8], 8 | 16),
                    3 => matches!(ihdr[8], 1 | 2 | 4 | 8),
                    _ => false,
                },
                "Invalid PNG pixel format"
            );
            ensure!(
                ihdr[10] == 0 && ihdr[11] == 0 && ihdr[12] <= 1,
                "Invalid PNG encoding"
            );
            (depth, color) = (ihdr[8], ihdr[9]);
        } else {
            match kind {
                b"IHDR" => anyhow::bail!("Repeated PNG IHDR"),
                b"PLTE" => {
                    ensure!(
                        palette == 0 && !data && matches!(color, 2 | 3 | 6),
                        "Invalid PNG palette order or color type"
                    );
                    ensure!(
                        (3..=768).contains(&size) && size.is_multiple_of(3),
                        "Invalid PNG palette size"
                    );
                    palette = size / 3;
                    ensure!(
                        color != 3 || palette <= 1usize << depth,
                        "PNG palette exceeds bit depth"
                    );
                }
                b"IDAT" => {
                    ensure!(!data_ended, "Nonconsecutive PNG IDAT chunks");
                    ensure!(color != 3 || palette > 0, "Indexed PNG lacks palette");
                    data = true;
                }
                b"IEND" => {}
                _ => ensure!(kind[0] & 0x20 != 0, "Unknown critical PNG chunk"),
            }
        }
        data_ended |= data && kind != b"IDAT";
        chunks += 1;
        offset = end;
        if kind == b"IEND" {
            ensure!(
                size == 0 && data && offset == bytes.len(),
                "Invalid PNG end"
            );
            return Ok(chunks);
        }
    }
    anyhow::bail!("PNG has no IEND")
}

fn candidates(directory: &Directory) -> Vec<&Member> {
    let mut found: Vec<_> = directory
        .files
        .iter()
        .filter(|m| {
            m.mode == Protection::Content
                && (45..=16 << 20).contains(&m.size)
                && m.name.to_ascii_lowercase().ends_with(".png")
        })
        .collect();
    found.sort_by_key(|m| m.size);
    found.truncate(8);
    found
}

/// Recover only from the verified v3 cipher and standard PNG header; verify every CRC.
/// At most eight candidate members, each bounded to 16 MiB, are examined.
pub(crate) fn recover_content_key(path: &Path, bank: &Ufs, directory: &Directory) -> Result<u64> {
    let found = candidates(directory);
    ensure!(
        !found.is_empty(),
        "This UVI bank has no supported encrypted PNG for automatic local setup"
    );
    let mut file = File::open(path)?;
    ensure!(
        file.metadata()?.len() == bank.header.physical_size,
        "UVI bank changed during local setup"
    );
    for member in found {
        ensure!(
            member
                .offset
                .checked_add(member.size)
                .is_some_and(|end| end <= bank.header.physical_size),
            "UVI PNG range exceeds container"
        );
        file.seek(SeekFrom::Start(member.offset))?;
        let mut cipher = [0; 16];
        file.read_exact(&mut cipher)?;
        let plain = *b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
        if let Ok(key) = crypto::recover_key(&cipher, &plain, member.offset)
            && let Ok(bytes) = bank.read_member(member, directory.metadata_key, Some(key))
            && validate_png(&bytes).is_ok()
        {
            return Ok(key);
        }
    }
    anyhow::bail!("This UVI bank could not pass automatic local content verification")
}

/// Bounded installed-reader discovery; configured paths and environment overrides remain authoritative.
pub(crate) fn reader_path(configured: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = configured {
        return Ok(path.to_owned());
    }
    if let Some(path) = std::env::var_os("KONTRA_UVI_READER") {
        return Ok(path.into());
    }
    let relative = Path::new("drive_c/Program Files/UVI Workstation/UVIWorkstationx64.exe");
    let mut candidates = Vec::new();
    if let Some(prefix) = std::env::var_os("WINEPREFIX") {
        candidates.push(PathBuf::from(prefix).join(relative));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".wine").join(relative));
    }
    for variable in ["PROGRAMFILES", "ProgramW6432"] {
        if let Some(path) = std::env::var_os(variable) {
            candidates.push(PathBuf::from(path).join("UVI Workstation/UVIWorkstationx64.exe"));
        }
    }
    candidates.extend(
        [
            "/mnt/c/Program Files/UVI Workstation/UVIWorkstationx64.exe",
            "/mnt/Windows11/Program Files/UVI Workstation/UVIWorkstationx64.exe",
        ]
        .map(PathBuf::from),
    );
    candidates.into_iter().find(|path| path.is_file()).context(
        "No local UVI reader was found; select a verified UVI Workstation reader in settings",
    )
}

/// JSON type errors can echo private field values; journal only their category.
pub(crate) fn failure_reason(error: &anyhow::Error) -> String {
    if error.chain().any(|cause| cause.is::<serde_json::Error>()) {
        "Invalid private UVI access record".into()
    } else {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_failure_reason_does_not_echo_private_json_values() {
        let error = serde_json::from_slice::<u64>(br#""private-value-marker""#)
            .err()
            .unwrap();
        let error = anyhow::Error::new(error).context("Invalid local content-state file");
        assert_eq!(failure_reason(&error), "Invalid private UVI access record");
        let error = anyhow::anyhow!("Content-state file must be private (chmod 600)");
        assert_eq!(failure_reason(&error), error.to_string());
    }

    fn authored_png() -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut chunk = |kind: &[u8; 4], data: &[u8]| {
            png.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let start = png.len();
            png.extend_from_slice(kind);
            png.extend_from_slice(data);
            let crc = crc32fast::hash(&png[start..]);
            png.extend_from_slice(&crc.to_be_bytes());
        };
        chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]);
        chunk(b"IDAT", &[0x78, 1, 1, 2, 0, 0xfd, 0xff, 0, 0, 0, 2, 0, 1]);
        chunk(b"IEND", &[]);
        png
    }

    #[test]
    fn content_recovery_is_verified_and_creates_no_access_files() {
        let root =
            std::env::temp_dir().join(format!("kontra-authored-access-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("authored.ufs");
        let plain = authored_png();
        let mixed = 328u64.wrapping_mul(0xc6a4_a793_5bd1_e995);
        let key = (mixed ^ (mixed >> 47)).wrapping_mul(0xc6a4_a793_5bd1_e995);
        // Authored zero initial stream state keeps debug recovery bounded to task 0.
        let mut encrypted = plain.clone();
        crypto::transform_blocks(&mut encrypted, key, 328);
        let mut bytes = vec![0; 320];
        bytes[..4].copy_from_slice(b"UFS2");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        bytes[8..24].fill(1);
        bytes[48..57].copy_from_slice(b"Synthetic");
        bytes.extend_from_slice(&(encrypted.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&encrypted);
        let size = bytes.len() as u64;
        bytes[32..40].copy_from_slice(&size.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        let bank = Ufs::open(&path).unwrap();
        let mut directory = Directory {
            files: vec![Member {
                record_offset: 999,
                name: "authored.png".into(),
                path: Some("authored.png".into()),
                parent: None,
                size: plain.len() as u64,
                offset: 328,
                mode: Protection::Content,
                footer: Vec::new(),
            }],
            directories: Vec::new(),
            records: Vec::new(),
            warnings: Vec::new(),
            metadata_key: 0,
        };
        let recovered = recover_content_key(&path, &bank, &directory).unwrap();
        let decoded = bank
            .read_member(&directory.files[0], directory.metadata_key, Some(recovered))
            .unwrap();
        assert_eq!(decoded, plain);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        // Complete CRC validation rejects a damaged encrypted member.
        bytes[328 + 45] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        assert!(recover_content_key(&path, &bank, &directory).is_err());
        directory.files[0].name = "unsupported.bin".into();
        assert!(recover_content_key(&path, &bank, &directory).is_err());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        assert_eq!(
            reader_path(Some(Path::new("configured-reader"))).unwrap(),
            Path::new("configured-reader")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn complete_png_requires_all_crcs_and_end_marker() {
        let png = authored_png();
        assert_eq!(validate_png(&png).unwrap(), 3);
        let mut corrupt = png.clone();
        corrupt[45] ^= 1;
        assert!(validate_png(&corrupt).is_err());
        assert!(validate_png(&png[..png.len() - 1]).is_err());
        let mut extra = png;
        extra.push(0);
        assert!(validate_png(&extra).is_err());
    }
    #[test]
    fn complete_png_rejects_crc_correct_invalid_pixel_formats() {
        for (depth, color) in [(7, 0), (8, 1), (4, 2), (16, 3), (1, 4), (4, 6)] {
            let mut png = authored_png();
            png[24] = depth;
            png[25] = color;
            let crc = crc32fast::hash(&png[12..29]);
            png[29..33].copy_from_slice(&crc.to_be_bytes());
            assert!(
                validate_png(&png).is_err(),
                "Accepted depth {depth}, color {color}"
            );
        }
    }
    #[test]
    fn complete_png_requires_critical_chunk_structure() {
        fn png(depth: u8, color: u8, chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
            let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
            let mut append = |kind: &[u8; 4], data: &[u8]| {
                png.extend_from_slice(&(data.len() as u32).to_be_bytes());
                let start = png.len();
                png.extend_from_slice(kind);
                png.extend_from_slice(data);
                let crc = crc32fast::hash(&png[start..]);
                png.extend_from_slice(&crc.to_be_bytes());
            };
            append(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, depth, color, 0, 0, 0]);
            for &(kind, data) in chunks {
                append(kind, data);
            }
            png
        }
        let palette = [0; 3];
        for chunks in [
            vec![(b"IDAT", &[][..]), (b"IEND", &[][..])],
            vec![
                (b"PLTE", &palette[..]),
                (b"PLTE", &palette[..]),
                (b"IDAT", &[][..]),
                (b"IEND", &[][..]),
            ],
            vec![
                (b"IDAT", &[][..]),
                (b"PLTE", &palette[..]),
                (b"IEND", &[][..]),
            ],
            vec![(b"PLTE", &[][..]), (b"IDAT", &[][..]), (b"IEND", &[][..])],
            vec![
                (b"PLTE", &[0; 4][..]),
                (b"IDAT", &[][..]),
                (b"IEND", &[][..]),
            ],
            vec![
                (b"PLTE", &[0; 771][..]),
                (b"IDAT", &[][..]),
                (b"IEND", &[][..]),
            ],
        ] {
            assert!(validate_png(&png(8, 3, &chunks)).is_err());
        }
        assert!(
            validate_png(&png(
                1,
                3,
                &[(b"PLTE", &[0; 9]), (b"IDAT", &[]), (b"IEND", &[])]
            ))
            .is_err()
        );
        for color in [0, 4] {
            assert!(
                validate_png(&png(
                    8,
                    color,
                    &[(b"PLTE", &palette), (b"IDAT", &[]), (b"IEND", &[])]
                ))
                .is_err()
            );
        }
        for kind in [b"IHDR", b"ABCD", b"x1XX"] {
            assert!(
                validate_png(&png(8, 0, &[(kind, &[]), (b"IDAT", &[]), (b"IEND", &[])])).is_err()
            );
        }
        assert!(
            validate_png(&png(
                8,
                0,
                &[
                    (b"IDAT", &[]),
                    (b"tEXt", &[]),
                    (b"IDAT", &[]),
                    (b"IEND", &[])
                ]
            ))
            .is_err()
        );
        assert!(validate_png(&png(8, 0, &[(b"IEND", &[])])).is_err());
        assert!(validate_png(&png(8, 0, &[(b"IDAT", &[]), (b"IEND", &[0])])).is_err());
        assert!(
            validate_png(&png(
                8,
                0,
                &[(b"IDAT", &[]), (b"IEND", &[]), (b"IEND", &[])]
            ))
            .is_err()
        );
        for (depth, color) in [(1, 3), (8, 2), (16, 6)] {
            assert_eq!(
                validate_png(&png(
                    depth,
                    color,
                    &[
                        (b"PLTE", &palette),
                        (b"IDAT", &[]),
                        (b"IDAT", &[]),
                        (b"IEND", &[])
                    ]
                ))
                .unwrap(),
                5
            );
        }
        // Unknown ancillary and future reserved-bit chunks are ignored by decoder policy.
        assert_eq!(
            validate_png(&png(
                8,
                0,
                &[
                    (b"zzzz", &[]),
                    (b"IDAT", &[]),
                    (b"IDAT", &[]),
                    (b"tEXt", &[]),
                    (b"IEND", &[])
                ]
            ))
            .unwrap(),
            6
        );
        let mut oversized = png(8, 0, &[(b"IDAT", &[]), (b"IEND", &[])]);
        oversized[16..20].copy_from_slice(&0x8000_0000u32.to_be_bytes());
        let crc = crc32fast::hash(&oversized[12..29]);
        oversized[29..33].copy_from_slice(&crc.to_be_bytes());
        assert!(validate_png(&oversized).is_err());
    }
}
