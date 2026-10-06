//! Offset-seeded UVI byte transforms. Namespaces and content keys come from the caller.
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

const MIX: u64 = 0xc6a4_a793_5bd1_e995;
const STEP: u64 = 0x5851_f42d_4c95_7f2d;
pub(crate) const PROGRAM_XML_LIMIT: usize = crate::XML_LIMIT as usize;

#[derive(Debug)]
pub(crate) struct NeedsProgramNamespace;
impl std::fmt::Display for NeedsProgramNamespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PasswordV2 requires a local reader program namespace")
    }
}
impl std::error::Error for NeedsProgramNamespace {}

fn mix(value: u64) -> u64 {
    let value = value.wrapping_mul(MIX);
    (value ^ (value >> 47)).wrapping_mul(MIX)
}

/// SHA-256 a key string and fold the four little-endian digest words.
pub fn key_from_string(value: &[u8]) -> u64 {
    let digest = Sha256::digest(value);
    let mut words = digest
        .as_chunks::<8>()
        .0
        .iter()
        .copied()
        .map(u64::from_le_bytes);
    let mut key = words.next().unwrap();
    for word in words {
        key = (key ^ mix(word)).wrapping_mul(MIX);
    }
    key
}

/// Directory names use the lower-case hexadecimal digest of namespace + bank name.
pub fn metadata_key(namespace: &[u8], bank_name: &str) -> u64 {
    let mut hash = Sha256::new();
    hash.update(namespace);
    hash.update(bank_name.as_bytes());
    let mut hex = [0u8; 64];
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for (i, value) in hash.finalize().iter().enumerate() {
        hex[2 * i] = DIGITS[(value >> 4) as usize];
        hex[2 * i + 1] = DIGITS[(value & 15) as usize];
    }
    key_from_string(&hex)
}

/// Symmetric transform; nonce is the physical file offset for container members.
pub fn transform(data: &mut [u8], key: u64, nonce: u64) {
    let mut state = (mix(nonce) ^ key).wrapping_mul(MIX);
    let (words, remainder) = data.as_chunks_mut::<4>();
    for chunk in words {
        let word = (((state >> 22) ^ state) >> (22 + (state >> 61))) as u32;
        let value = u32::from_le_bytes(*chunk) ^ word;
        *chunk = value.to_le_bytes();
        state = state.wrapping_mul(STEP);
    }
    let word = (((state >> 22) ^ state) >> (22 + (state >> 61))) as u32;
    for (byte, mask) in remainder.iter_mut().zip(word.to_le_bytes()) {
        *byte ^= mask;
    }
}

/// File data restarts the cipher at every 512-byte physical block.
pub fn transform_blocks(data: &mut [u8], key: u64, physical_offset: u64) {
    for (index, block) in data.chunks_mut(512).enumerate() {
        transform(
            block,
            key,
            physical_offset.wrapping_add((index as u64) * 512),
        );
    }
}

fn recovery_base(task: usize, first_word: u32) -> (u64, u32) {
    let mut h = 0u32;
    let mut upper = task as u64;
    while upper >= 1 << (7 - h) {
        upper -= 1 << (7 - h);
        h += 1;
    }
    let shift = 22 + h;
    let y = u64::from(first_word) | (upper << 32) | (u64::from(h) << (61 - shift));
    ((y ^ (y >> 22)) << shift, shift)
}

/// Recover a candidate stream key from four known plaintext/ciphertext words.
/// The caller must verify the complete member before trusting the candidate.
/// Search is bounded to 255 tasks (2^32 candidates) and at most eight workers.
pub fn recover_key(cipher: &[u8; 16], plain: &[u8; 16], physical_offset: u64) -> Result<u64> {
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
    let mut expected = [0u32; 4];
    for (i, word) in expected.iter_mut().enumerate() {
        let at = i * 4;
        *word = u32::from_le_bytes(cipher[at..at + 4].try_into().unwrap())
            ^ u32::from_le_bytes(plain[at..at + 4].try_into().unwrap());
    }
    let inverse = (0..6).fold(1u64, |v, _| {
        v.wrapping_mul(2u64.wrapping_sub(MIX.wrapping_mul(v)))
    });
    let next = AtomicUsize::new(0);
    let done = AtomicBool::new(false);
    let found_key = AtomicU64::new(0);
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
    let output = |state: u64| (((state >> 22) ^ state) >> (22 + (state >> 61))) as u32;
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let task = next.fetch_add(1, Relaxed);
                    if task >= 255 || done.load(Relaxed) {
                        break;
                    }
                    let (base, shift) = recovery_base(task, expected[0]);
                    let mut state1 = base.wrapping_mul(STEP);
                    for low in 0..1u64 << shift {
                        if done.load(Relaxed) {
                            break;
                        }
                        if output(state1) == expected[1] {
                            let state2 = state1.wrapping_mul(STEP);
                            if output(state2) == expected[2]
                                && output(state2.wrapping_mul(STEP)) == expected[3]
                                && output(base | low) == expected[0]
                            {
                                let key = (base | low).wrapping_mul(inverse) ^ mix(physical_offset);
                                if !done.swap(true, Relaxed) {
                                    found_key.store(key, Relaxed);
                                }
                                break;
                            }
                        }
                        state1 = state1.wrapping_add(STEP);
                    }
                }
            });
        }
    });
    ensure!(done.load(Relaxed), "No matching UVI stream state found");
    Ok(found_key.load(Relaxed))
}

/// One plain ZIP entry is an observed UVIP wrapper; no archive files are extracted.
fn unpack_program_zip(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    ensure!(
        bytes.len() <= PROGRAM_XML_LIMIT,
        "UVI ZIP program exceeds resource limit"
    );
    ensure!(
        bytes.len() >= 30 && bytes.starts_with(b"PK\x03\x04"),
        "Invalid UVI ZIP local header"
    );
    let word = |at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
    let dword = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let (version, flags, method) = (word(4), word(6), word(8));
    ensure!(
        matches!(version, 10 | 20) && flags & !0x0800 == 0,
        "Unsupported UVI ZIP version, encryption or flags"
    );
    ensure!(
        matches!(method, 0 | 8) && (method == 0 || version == 20),
        "Unsupported UVI ZIP compression method"
    );
    let (crc, compressed, uncompressed) = (dword(14), dword(18) as usize, dword(22) as usize);
    ensure!(
        compressed <= PROGRAM_XML_LIMIT && uncompressed <= PROGRAM_XML_LIMIT,
        "UVI ZIP program exceeds resource limit"
    );
    let name_end = 30 + word(26) as usize;
    let data_at = name_end + word(28) as usize;
    let data_end = data_at + compressed;
    let name = bytes
        .get(30..name_end)
        .context("Truncated UVI ZIP filename")?;
    let name_text = std::str::from_utf8(name).context("Invalid UVI ZIP filename")?;
    ensure!(
        !name_text.is_empty()
            && !name_text.contains(['\0', '/', '\\', ':'])
            && name_text.to_ascii_lowercase().ends_with(".uvip"),
        "Unsafe or unsupported UVI ZIP program filename"
    );
    let encoded = bytes
        .get(data_at..data_end)
        .context("Truncated UVI ZIP program data")?;
    let central = bytes
        .get(data_end..data_end + 46)
        .context("Truncated UVI ZIP central header")?;
    ensure!(
        central.starts_with(b"PK\x01\x02"),
        "UVI ZIP must contain exactly one program entry"
    );
    let cw = |at| u16::from_le_bytes(central[at..at + 2].try_into().unwrap());
    let cd = |at| u32::from_le_bytes(central[at..at + 4].try_into().unwrap());
    ensure!(
        cw(6) == version
            && cw(8) == flags
            && cw(10) == method
            && cd(16) == crc
            && cd(20) as usize == compressed
            && cd(24) as usize == uncompressed
            && cw(34) == 0
            && cd(42) == 0,
        "UVI ZIP local and central headers disagree"
    );
    let attributes = cd(38);
    ensure!(
        attributes & 0x10 == 0 && matches!((attributes >> 16) & 0xf000, 0 | 0x8000),
        "UVI ZIP program is not a regular file"
    );
    let central_name_end = data_end + 46 + cw(28) as usize;
    ensure!(
        bytes.get(data_end + 46..central_name_end) == Some(name),
        "UVI ZIP filenames disagree"
    );
    let end_at = central_name_end + cw(30) as usize + cw(32) as usize;
    let end = bytes
        .get(end_at..end_at + 22)
        .context("Truncated UVI ZIP end record")?;
    let ew = |at| u16::from_le_bytes(end[at..at + 2].try_into().unwrap());
    let ed = |at| u32::from_le_bytes(end[at..at + 4].try_into().unwrap());
    ensure!(
        end.starts_with(b"PK\x05\x06")
            && ew(4) == 0
            && ew(6) == 0
            && ew(8) == 1
            && ew(10) == 1
            && ed(12) as usize == end_at - data_end
            && ed(16) as usize == data_end
            && end_at + 22 + ew(20) as usize == bytes.len(),
        "Unsupported or inconsistent UVI ZIP directory"
    );
    let decoded = if method == 0 {
        ensure!(compressed == uncompressed, "Stored UVI ZIP size mismatch");
        encoded.to_vec()
    } else {
        let mut decoder = flate2::bufread::DeflateDecoder::new(encoded);
        let mut decoded = Vec::new();
        (&mut decoder)
            .take(uncompressed as u64 + 1)
            .read_to_end(&mut decoded)
            .context("Invalid UVI ZIP deflate stream")?;
        ensure!(
            decoder.total_in() == compressed as u64,
            "UVI ZIP deflate span mismatch"
        );
        decoded
    };
    ensure!(
        decoded.len() == uncompressed && crc32fast::hash(&decoded) == crc,
        "UVI ZIP program size or CRC mismatch"
    );
    Ok(decoded)
}

/// Decode either UTF-8 XML or the observed single-entry plain ZIP UVIP wrapper.
pub fn decode_program_bytes(bytes: &[u8], namespace: &[u8]) -> Result<String> {
    ensure!(
        bytes.len() <= PROGRAM_XML_LIMIT,
        "UVI Program bytes exceed resource limit"
    );
    let decoded;
    let bytes = if bytes.starts_with(b"PK\x03\x04") {
        decoded = unpack_program_zip(bytes)?;
        decoded.as_slice()
    } else {
        bytes
    };
    let text = std::str::from_utf8(bytes).context("Decoded UVI program is not UTF-8")?;
    decode_program(text, namespace)
}

fn decode_base64(text: &str, limit: usize) -> Result<Vec<u8>> {
    ensure!(
        text.len() <= limit.div_ceil(3) * 4 + 4096,
        "UVI base64 exceeds resource limit"
    );
    let compact: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let bytes = STANDARD.decode(compact).context("Invalid UVI base64")?;
    ensure!(
        bytes.len() <= limit,
        "Decoded UVI data exceeds resource limit"
    );
    Ok(bytes)
}

/// Decode a PasswordV2 Program wrapper using the caller's local reader namespace.
/// Clear Program/UVI4 XML passes through; unsupported legacy protection is explicit.
pub fn decode_program(text: &str, namespace: &[u8]) -> Result<String> {
    let doc = crate::parse_program_xml(text).context("Invalid UVI Program XML")?;
    let root = doc.root_element();
    let program = if root.has_tag_name("Program") {
        root
    } else {
        root.children()
            .find(|n| n.has_tag_name("Program"))
            .context("Missing UVI Program")?
    };
    ensure!(
        program.attribute("Password").is_none(),
        "Legacy UVI Password protection is unsupported"
    );
    let Some(encoded_password) = program.attribute("PasswordV2") else {
        return Ok(text.to_owned());
    };
    if namespace.is_empty() {
        return Err(NeedsProgramNamespace.into());
    }
    let mut password = decode_base64(encoded_password, 4096)?;
    transform(&mut password, key_from_string(namespace), 0);
    let end = password
        .iter()
        .position(|&b| b == 0)
        .context("Invalid PasswordV2: missing terminator")?;
    ensure!(
        end > 0 && password[end..].iter().all(|&b| b == 0),
        "Invalid PasswordV2 padding or namespace"
    );
    let key = key_from_string(&password[..end]);
    let encoded: String = program.children().filter_map(|n| n.text()).collect();
    let mut decoded = decode_base64(&encoded, PROGRAM_XML_LIMIT)?;
    ensure!(
        decoded.len().is_multiple_of(8),
        "Encrypted UVI Program is not 8-byte padded"
    );
    transform(&mut decoded, key, 0);
    while decoded.last() == Some(&0) {
        decoded.pop();
    }
    let decoded =
        String::from_utf8(decoded).context("Decoded UVI Program is not UTF-8; check namespace")?;
    let inner = crate::parse_program_xml(&decoded)
        .context("Decoded UVI Program is not XML; check namespace")?;
    ensure!(
        inner.root_element().has_tag_name("Program"),
        "Decoded UVI XML has no Program root"
    );
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_and_protected_programs_share_large_bounded_xml_admission() {
        let xml = format!(
            "<Program><!--{}-->{}</Program>",
            "a".repeat(17 << 20),
            "<p/>".repeat(260_000)
        );
        assert_eq!(decode_program(&xml, &[]).unwrap(), xml);
        let namespace = b"authored large-program namespace";
        let mut password = b"authored large-program password\0".to_vec();
        let mut payload = xml.as_bytes().to_vec();
        payload.resize(payload.len().div_ceil(8) * 8, 0);
        transform(
            &mut payload,
            key_from_string(&password[..password.len() - 1]),
            0,
        );
        transform(&mut password, key_from_string(namespace), 0);
        let wrapper = format!(
            "<Program PasswordV2=\"{}\">{}</Program>",
            STANDARD.encode(password),
            STANDARD.encode(payload)
        );
        assert_eq!(decode_program(&wrapper, namespace).unwrap(), xml);
        assert!(decode_program("<!DOCTYPE Program><Program/>", namespace).is_err());
    }

    #[test]
    fn authored_cipher_and_password_wrapper() {
        let key = key_from_string(b"authored UVI regression key");
        assert_eq!(key, 0xc6f3_57b6_0ad0_ec26);
        let mut known = [0u8; 16];
        transform(&mut known, key, 12345);
        assert_eq!(
            known,
            [
                0x8e, 0xf8, 0xb8, 0xa4, 0x72, 0x10, 0xb6, 0x93, 0x08, 0x93, 0x15, 0xf6, 0xab, 0x88,
                0xa4, 0x00
            ]
        );
        let plain: Vec<u8> = (0..1031).map(|i| i as u8).collect();
        let mut encrypted = plain.clone();
        transform_blocks(&mut encrypted, key, 1024);
        let mut second = plain[512..1024].to_vec();
        transform(&mut second, key, 1536);
        assert_eq!(&encrypted[512..1024], second);
        transform_blocks(&mut encrypted, key, 1024);
        assert_eq!(encrypted, plain);

        let namespace = b"authored program namespace";
        let mut password = b"authored password\0".to_vec();
        let mut program = b"<Program Name=\"Synthetic\"><Layer/></Program>".to_vec();
        program.resize(program.len().div_ceil(8) * 8, 0);
        transform(
            &mut program,
            key_from_string(&password[..password.len() - 1]),
            0,
        );
        transform(&mut password, key_from_string(namespace), 0);
        let wrapper = format!(
            "<UVI4><Program PasswordV2=\"{}\">{}</Program></UVI4>",
            STANDARD.encode(password),
            STANDARD.encode(program)
        );
        assert_eq!(
            decode_program(&wrapper, namespace).unwrap(),
            "<Program Name=\"Synthetic\"><Layer/></Program>"
        );
        assert!(decode_program(&wrapper, b"wrong namespace").is_err());
        assert!(decode_program("<Program Password=\"legacy\"/>", namespace).is_err());
    }
    #[test]
    fn wordwise_transform_preserves_bytes_tails_and_physical_block_restarts() {
        fn reference(data: &mut [u8], key: u64, offset: u64) {
            let mut state = (mix(offset) ^ key).wrapping_mul(MIX);
            for (index, byte) in data.iter_mut().enumerate() {
                let word = (((state >> 22) ^ state) >> (22 + (state >> 61))) as u32;
                *byte ^= (word >> ((index % 4) * 8)) as u8;
                if index % 4 == 3 {
                    state = state.wrapping_mul(STEP);
                }
            }
        }
        for length in [
            0, 1, 2, 3, 4, 5, 7, 8, 15, 16, 511, 512, 513, 1023, 1024, 1031,
        ] {
            for offset in [0, 1, 511, 512, 12345, u64::MAX - 513, u64::MAX] {
                for key in [0, 0x1234_5678_9abc_def0, u64::MAX] {
                    // Exercise unaligned input and protect both adjacent bytes.
                    let plain: Vec<u8> = (0..length + 2).map(|index| index as u8).collect();
                    let mut expected = plain.clone();
                    let mut actual = plain.clone();
                    reference(&mut expected[1..length + 1], key, offset);
                    transform(&mut actual[1..length + 1], key, offset);
                    assert_eq!(actual, expected);
                    transform(&mut actual[1..length + 1], key, offset);
                    assert_eq!(actual, plain);
                    let mut expected = plain.clone();
                    for (index, block) in expected[1..length + 1].chunks_mut(512).enumerate() {
                        reference(block, key, offset.wrapping_add(index as u64 * 512));
                    }
                    transform_blocks(&mut actual[1..length + 1], key, offset);
                    assert_eq!(actual, expected);
                    transform_blocks(&mut actual[1..length + 1], key, offset);
                    assert_eq!(actual, plain);
                }
            }
        }
    }

    #[test]
    fn authored_known_plaintext_recovers_key() {
        for (task, base, shift) in [
            (0, 0x002a_aef3_9dc0_0000, 22),
            (127, 0x1fea_ae8c_9dc0_0000, 22),
            (128, 0x2055_5d67_3b80_0000, 23),
            (191, 0x3fd5_5d19_3b80_0000, 23),
            (254, 0xf557_7a4e_e000_0000, 29),
        ] {
            assert_eq!(recovery_base(task, 0xaabb_ccdd), (base, shift));
        }
        let offset = 12345;
        let inverse = (0..6).fold(1u64, |v, _| {
            v.wrapping_mul(2u64.wrapping_sub(MIX.wrapping_mul(v)))
        });
        assert_eq!(MIX.wrapping_mul(inverse), 1);
        // A small authored initial state keeps the debug regression in the first task.
        let key = 0x123u64.wrapping_mul(inverse) ^ mix(offset);
        let plain = *b"Authored fixture";
        let mut cipher = plain;
        transform(&mut cipher, key, offset);
        let recovered = recover_key(&cipher, &plain, offset).unwrap();
        assert_eq!(recovered, key);
        transform(&mut cipher, recovered, offset);
        assert_eq!(cipher, plain);
    }
    #[test]
    fn authored_zip_program_requires_bounded_verified_single_regular_entry() {
        use std::io::Write;
        fn archive(plain: &[u8], method: u16, name: &[u8]) -> Vec<u8> {
            let data = if method == 8 {
                let mut encoder =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(plain).unwrap();
                encoder.finish().unwrap()
            } else {
                plain.to_vec()
            };
            let crc = crc32fast::hash(plain);
            let mut local = vec![0u8; 30];
            local[..4].copy_from_slice(b"PK\x03\x04");
            local[4..6].copy_from_slice(&20u16.to_le_bytes());
            local[8..10].copy_from_slice(&method.to_le_bytes());
            local[14..18].copy_from_slice(&crc.to_le_bytes());
            local[18..22].copy_from_slice(&(data.len() as u32).to_le_bytes());
            local[22..26].copy_from_slice(&(plain.len() as u32).to_le_bytes());
            local[26..28].copy_from_slice(&(name.len() as u16).to_le_bytes());
            local.extend_from_slice(name);
            local.extend_from_slice(&data);
            let central_at = local.len();
            let mut central = vec![0u8; 46];
            central[..4].copy_from_slice(b"PK\x01\x02");
            central[4..6].copy_from_slice(&20u16.to_le_bytes());
            central[6..8].copy_from_slice(&20u16.to_le_bytes());
            central[10..12].copy_from_slice(&method.to_le_bytes());
            central[16..20].copy_from_slice(&crc.to_le_bytes());
            central[20..24].copy_from_slice(&(data.len() as u32).to_le_bytes());
            central[24..28].copy_from_slice(&(plain.len() as u32).to_le_bytes());
            central[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(name);
            let central_len = central.len();
            local.extend_from_slice(&central);
            let mut end = vec![0u8; 22];
            end[..4].copy_from_slice(b"PK\x05\x06");
            end[8..10].copy_from_slice(&1u16.to_le_bytes());
            end[10..12].copy_from_slice(&1u16.to_le_bytes());
            end[12..16].copy_from_slice(&(central_len as u32).to_le_bytes());
            end[16..20].copy_from_slice(&(central_at as u32).to_le_bytes());
            local.extend_from_slice(&end);
            local
        }
        let plain = b"<UVI4><Program Name=\"Authored ZIP\"/></UVI4>";
        let namespace = b"authored metadata namespace";
        assert_eq!(
            decode_program_bytes(plain, namespace).unwrap(),
            std::str::from_utf8(plain).unwrap()
        );
        for method in [0, 8] {
            let zip = archive(plain, method, b"authored.uvip");
            assert_eq!(
                decode_program_bytes(&zip, namespace).unwrap(),
                std::str::from_utf8(plain).unwrap()
            );
            for cut in 0..zip.len() {
                assert!(decode_program_bytes(&zip[..cut], namespace).is_err());
            }
        }
        let zip = archive(plain, 0, b"authored.uvip");
        let central = 30 + b"authored.uvip".len() + plain.len();
        let end = zip.len() - 22;
        let mut bad = zip.clone();
        bad[30 + b"authored.uvip".len() + 10] ^= 1;
        assert_eq!(
            decode_program_bytes(&bad, namespace)
                .unwrap_err()
                .to_string(),
            "UVI ZIP program size or CRC mismatch"
        );
        let mut bad = zip.clone();
        bad[22..26].copy_from_slice(&((PROGRAM_XML_LIMIT + 1) as u32).to_le_bytes());
        bad[central + 24..central + 28]
            .copy_from_slice(&((PROGRAM_XML_LIMIT + 1) as u32).to_le_bytes());
        assert!(decode_program_bytes(&bad, namespace).is_err());
        let mut bad = zip.clone();
        bad[end + 8..end + 10].copy_from_slice(&2u16.to_le_bytes());
        bad[end + 10..end + 12].copy_from_slice(&2u16.to_le_bytes());
        assert!(decode_program_bytes(&bad, namespace).is_err());
        let mut bad = zip.clone();
        bad[6] = 1;
        bad[central + 8] = 1;
        assert!(decode_program_bytes(&bad, namespace).is_err());
        let mut bad = zip.clone();
        bad[central + 38..central + 42].copy_from_slice(&0xa000_0000u32.to_le_bytes());
        assert!(decode_program_bytes(&bad, namespace).is_err());
        let mut bad = zip.clone();
        bad[central + 16] ^= 1;
        assert!(decode_program_bytes(&bad, namespace).is_err());
        for name in [
            b"../unsafe.uvip".as_slice(),
            b"dir/unsafe.uvip",
            b"C:unsafe.uvip",
        ] {
            assert!(decode_program_bytes(&archive(plain, 8, name), namespace).is_err());
        }
        let mut bad = archive(plain, 8, b"authored.uvip");
        let central = bad.len() - 22 - 46 - b"authored.uvip".len();
        bad[22..26].copy_from_slice(&4u32.to_le_bytes());
        bad[central + 24..central + 28].copy_from_slice(&4u32.to_le_bytes());
        assert!(decode_program_bytes(&bad, namespace).is_err());
    }
}
