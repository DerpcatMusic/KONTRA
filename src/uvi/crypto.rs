//! Offset-seeded UVI byte transforms. Namespaces and content keys come from the caller.
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use roxmltree::{Document, ParsingOptions};
use sha2::{Digest, Sha256};

const MIX: u64 = 0xc6a4_a793_5bd1_e995;
const STEP: u64 = 0x5851_f42d_4c95_7f2d;
pub const PROGRAM_LIMIT: usize = 16 << 20;

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
    for chunk in data.chunks_mut(4) {
        let word = (((state >> 22) ^ state) >> (22 + (state >> 61))) as u32;
        for (byte, mask) in chunk.iter_mut().zip(word.to_le_bytes()) {
            *byte ^= mask;
        }
        state = state.wrapping_mul(STEP);
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
    ensure!(
        text.len() <= PROGRAM_LIMIT * 2,
        "UVI Program XML exceeds resource limit"
    );
    let doc = Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: 100_000,
            ..Default::default()
        },
    )
    .context("Invalid UVI Program XML")?;
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
    ensure!(
        !namespace.is_empty(),
        "PasswordV2 requires a local reader program namespace"
    );
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
    let mut decoded = decode_base64(&encoded, PROGRAM_LIMIT)?;
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
    let inner = Document::parse_with_options(
        &decoded,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: 100_000,
            ..Default::default()
        },
    )
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
}
