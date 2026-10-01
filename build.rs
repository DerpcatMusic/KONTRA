//! `KONTRA_IMPORT_HASH`: a hash of the sources that decide what an import
//! produces, so the instrument cache (`src/cache.rs`) drops entries written
//! by any other importer.

use std::path::Path;

const SOURCES: &[&str] = &[
    "src/import.rs",
    "src/modulation.rs",
    "src/cache.rs",
    "src/audio.rs",
    "src/fx",
    "src/ksp/mod.rs",
    "src/engine/filter.rs",
    "vendor/ni-file/src",
];

fn main() {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for source in SOURCES {
        println!("cargo:rerun-if-changed={source}");
        visit(Path::new(source), &mut hash);
    }
    println!("cargo:rustc-env=KONTRA_IMPORT_HASH={hash:016x}");
    let mut build_hash = 0xcbf2_9ce4_8422_2325_u64;
    for source in ["src", "Cargo.toml", "Cargo.lock", "vendor/ni-file/src", "vendor/moose-mui/src", "vendor/mui-baseview/src"] {
        println!("cargo:rerun-if-changed={source}");
        visit(Path::new(source), &mut build_hash);
    }
    println!("cargo:rustc-env=KONTRA_BUILD_HASH={build_hash:016x}");
}

/// FNV-1a over every file's path and bytes, in name order.
fn visit(path: &Path, hash: &mut u64) {
    let mut fnv = |bytes: &[u8]| {
        for &b in bytes {
            *hash = (*hash ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
    };
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path).unwrap().map(|e| e.unwrap().path()).collect();
        entries.sort();
        for entry in entries {
            visit(&entry, hash);
        }
    } else {
        fnv(path.as_os_str().as_encoded_bytes());
        fnv(&std::fs::read(path).unwrap());
    }
}
