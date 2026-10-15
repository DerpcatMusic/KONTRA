//! Independently authored wire fixtures. No installed libraries, binary
//! fixtures, legacy writers, sample payloads, initialization or DSP execution.
use std::path::{Path, PathBuf};

pub fn directory() -> tempfile::TempDir {
    // Keep authored disk probes on the development drive in the local workspace;
    // the fallback permits this fixture helper on unrelated CI machines.
    let drive = Path::new("/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts");
    let root = std::env::var_os("KONTRA_TEST_SCRATCH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if drive.is_dir() {
                drive.into()
            } else {
                std::env::temp_dir()
            }
        });
    std::fs::create_dir_all(&root).unwrap();
    let directory = tempfile::Builder::new()
        .prefix("snapshot-reuse-authored-")
        .tempdir_in(root)
        .unwrap();
    // Anchor namespace discovery inside the authored fixture rather than a
    // parent workspace that might contain unrelated resources or samples.
    std::fs::create_dir(directory.path().join("Samples")).unwrap();
    std::fs::create_dir(directory.path().join("Resources")).unwrap();
    directory
}

fn sized(bytes: &[u8]) -> Vec<u8> {
    let mut out = (bytes.len() as u32).to_le_bytes().to_vec();
    out.extend(bytes);
    out
}
fn wide(text: &str) -> Vec<u8> {
    let mut out = (text.encode_utf16().count() as u32).to_le_bytes().to_vec();
    out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    out
}
pub fn chunk(id: u16, bytes: &[u8]) -> Vec<u8> {
    let mut out = id.to_le_bytes().to_vec();
    out.extend(sized(bytes));
    out
}
fn object(version: u16, public: &[u8], children: &[u8]) -> Vec<u8> {
    let mut out = vec![1];
    out.extend(version.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(sized(public));
    out.extend(sized(children));
    out
}

/// Literal-only FastLZ level 1 inside an authored NKS42 container.
pub fn container(payload: &[u8]) -> Vec<u8> {
    let packed: Vec<_> = payload
        .chunks(32)
        .flat_map(|bytes| std::iter::once((bytes.len() - 1) as u8).chain(bytes.iter().copied()))
        .collect();
    let mut out = vec![0; 222];
    out[..4].copy_from_slice(&0x7fa89012u32.to_le_bytes());
    out[4..8].copy_from_slice(&(packed.len() as u32).to_le_bytes());
    out[8..10].copy_from_slice(&0x110u16.to_le_bytes());
    out[10..14].copy_from_slice(&0xea37631au32.to_le_bytes());
    out[186..190].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend(packed);
    out.extend(0xb00ee1aeu32.to_le_bytes());
    out.extend([1, 1, 12, 0]);
    out
}

/// Snapshot v3/flags3 has no native group or effect payload. Script policy
/// modes 1/3 belong to the base's KSP and are independent of these wire flags.
pub fn script_only_snapshot(primary: &str, secondary: &str, slots: &[&[&str]; 5]) -> Vec<u8> {
    let mut public = 3u32.to_le_bytes().to_vec();
    for rows in slots {
        public.extend((rows.len() as u32).to_le_bytes());
        for row in *rows {
            public.extend(sized(row.as_bytes()));
        }
    }
    let mut payload = chunk(0x4f, &object(3, &public, &[]));
    let mut metadata = 0u32.to_le_bytes().to_vec();
    metadata.extend(wide(primary));
    metadata.extend(wide(secondary));
    payload.extend(chunk(0x51, &object(1, &metadata, &[])));
    container(&payload)
}

/// Complete version-0x90 public metadata and empty group/zone/file tables.
/// Every physical script position is serialized, including empty slots.
/// The result admits Resources::for_program and the normal library reader.
pub fn base_instrument(name: &str, scripts: &[Option<&str>; 5]) -> Vec<u8> {
    let mut public = wide(name);
    public.extend(0f64.to_le_bytes()); // Total sample bytes.
    public.push(0); // Transpose.
    for value in [1f32, 0., 1.] {
        public.extend(value.to_le_bytes());
    }
    public.extend([1, 127, 0, 127]); // Velocity/key domain.
    public.extend((-1i16).to_le_bytes());
    public.extend([0; 16]); // Preload/library/fingerprint/loading flags.
    public.push(0); // Solo.
    public.extend(0i32.to_le_bytes()); // Category icon.
    for _ in 0..3 {
        public.extend(wide(""));
    }
    public.extend([0; 6]); // Categories.
    let mut children = chunk(0x33, &0u32.to_le_bytes());
    children.extend(chunk(0x34, &0u32.to_le_bytes()));
    for script in scripts {
        let mut script_public = script.map_or_else(
            || u32::MAX.to_le_bytes().to_vec(),
            |text| sized(text.as_bytes()),
        );
        script_public.extend([0, 0, 0]); // Editor/touched/bypass.
        script_public.extend(sized(&[])); // Password hash.
        script_public.extend(sized(&[])); // Description.
        script_public.extend(u32::MAX.to_le_bytes()); // No linked source.
        script_public.extend(0u32.to_le_bytes()); // No saved init state.
        children.extend(chunk(6, &object(0x60, &script_public, &[])));
    }
    let mut payload = chunk(0x28, &object(0x90, &public, &children));
    let mut files = 2u16.to_le_bytes().to_vec();
    files.extend([0; 12]); // Empty special, sample and other filename tables.
    payload.extend(chunk(0x4b, &files));
    container(&payload)
}
