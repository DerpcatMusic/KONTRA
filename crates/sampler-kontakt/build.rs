//! Port from v1 0cb7a8a0:build.rs: invalidate product metadata on importer changes.
use std::path::Path;
fn main() {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for source in [
        "src",
        "../sampler-ir/src",
        "../sampler-ksp/src",
        "../sampler-core/src",
        "../../vendor/ni-file/src",
    ] {
        println!("cargo:rerun-if-changed={source}");
        visit(Path::new(source), &mut hash);
    }
    println!("cargo:rustc-env=KONTRA_IMPORT_HASH={hash:016x}");
}
fn visit(path: &Path, hash: &mut u64) {
    let mut fnv = |bytes: &[u8]| {
        for &b in bytes {
            *hash = (*hash ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
    };
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for entry in entries {
            visit(&entry, hash);
        }
    } else {
        fnv(path.as_os_str().as_encoded_bytes());
        fnv(&std::fs::read(path).unwrap());
    }
}
