//! `KONTRA_IMPORT_HASH`: a hash of the sources that decide what an import
//! produces, so the instrument cache (`src/cache.rs`) drops entries written
//! by any other importer.

use std::path::Path;
use std::{
    env,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

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
    for source in [
        "src",
        "Cargo.toml",
        "Cargo.lock",
        "vendor/ni-file/src",
        "vendor/moose-mui/src",
        "vendor/mui-baseview/src",
    ] {
        println!("cargo:rerun-if-changed={source}");
        visit(Path::new(source), &mut build_hash);
    }
    println!("cargo:rustc-env=KONTRA_BUILD_HASH={build_hash:016x}");
    // Digests only: installed binaries never embed these complete sources.
    use sha2::{Digest, Sha256};
    for (source, name) in [("src/plugin.rs", "KONTRA_PLUGIN_SOURCE_SHA256"),
                           ("src/plugin/uvi.rs", "KONTRA_UVI_SLOT_SOURCE_SHA256")] {
        println!("cargo:rerun-if-changed={source}");
        let digest = Sha256::digest(std::fs::read(source).expect("native diagnostic source"));
        println!("cargo:rustc-env={name}={digest:x}");
    }
    identity(hash, build_hash);
}

fn git(args: &[&str]) -> Option<String> {
    // A source archive nested inside another checkout does not inherit its identity.
    let prefix = Command::new("git")
        .args(["rev-parse", "--show-prefix"])
        .output()
        .ok()?;
    if !prefix.status.success() || !prefix.stdout.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn revision(value: String) -> String {
    assert!(
        [40, 64].contains(&value.len()) && value.bytes().all(|b| b.is_ascii_hexdigit()),
        "Build revisions must be full Git object IDs"
    );
    value.to_ascii_lowercase()
}

fn utc(epoch: u64) -> String {
    assert!(
        epoch <= 253_402_300_799,
        "SOURCE_DATE_EPOCH exceeds year 9999"
    );
    let (mut days, seconds) = (epoch / 86400, epoch % 86400);
    let mut year = 1970;
    let leap = |y| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    loop {
        let length = if leap(year) { 366 } else { 365 };
        if days < length {
            break;
        }
        days -= length;
        year += 1;
    }
    let months = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0;
    while days >= months[month] {
        days -= months[month];
        month += 1;
    }
    format!(
        "{year:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        month + 1,
        days + 1,
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn identity(import_hash: u64, build_hash: u64) {
    for name in [
        "SOURCE_DATE_EPOCH",
        "KONTRA_BUILD_REVISION",
        "KONTRA_SOURCE_REVISION",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    for path in [
        "build.rs",
        "moose.toml",
        "CHANGELOG.md",
        "CONTRIBUTING.md",
        "tools",
        ".github",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    for name in ["HEAD", "index"] {
        if let Some(path) = git(&["rev-parse", "--git-path", name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"])
        && let Some(path) = git(&["rev-parse", "--git-path", &branch])
    {
        println!("cargo:rerun-if-changed={path}");
    }
    let checkout = git(&["rev-parse", "--verify", "HEAD"]).map(revision);
    let supplied = env::var("KONTRA_BUILD_REVISION").ok().map(revision);
    if let (Some(actual), Some(supplied)) = (&checkout, &supplied) {
        assert_eq!(
            actual, supplied,
            "KONTRA_BUILD_REVISION must identify the actual checkout"
        );
    }
    let rev = checkout.or(supplied).unwrap_or_else(|| "unknown".into());
    let source = env::var("KONTRA_SOURCE_REVISION")
        .ok()
        .map(revision)
        .unwrap_or_else(|| rev.clone());
    // Untracked and ignored output is not a source modification.
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).map(|s| !s.is_empty());
    let epoch = env::var("SOURCE_DATE_EPOCH")
        .map(|v| {
            v.parse()
                .expect("SOURCE_DATE_EPOCH must be a nonnegative integer")
        })
        .unwrap_or_else(|_| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Clock precedes Unix epoch")
                .as_secs()
        });
    let built = utc(epoch);
    let out = env::var("OUT_DIR").unwrap();
    // PROFILE collapses custom profiles to debug/release; the output path retains their names.
    let profile = Path::new(&out)
        .ancestors()
        .nth(3)
        .and_then(Path::file_name)
        .and_then(|v| v.to_str())
        .unwrap_or("unknown");
    let profile = if profile == "debug" { "dev" } else { profile };
    let mut features: Vec<_> = env::vars()
        .filter_map(|(k, _)| {
            k.strip_prefix("CARGO_FEATURE_")
                .map(|v| v.to_ascii_lowercase().replace('_', "-"))
        })
        .collect();
    features.sort();
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let target = env::var("TARGET").unwrap();
    let values = [
        ("version", version.clone()),
        ("revision", rev.clone()),
        ("source_revision", source.clone()),
        ("built_at_utc", built.clone()),
        ("target", target.clone()),
        ("profile", profile.to_owned()),
        ("build_hash", format!("{build_hash:016x}")),
        ("import_hash", format!("{import_hash:016x}")),
    ];
    let state = match dirty {
        Some(true) => "modified",
        Some(false) => "clean",
        None => "source status unknown",
    };
    let short = if rev == "unknown" {
        rev.clone()
    } else {
        format!("g{}", &rev[..12])
    };
    let label = format!("KONTRA {version} · {short} · {state}");
    let summary = format!(
        "{label}\nRevision: {rev}\nSource revision: {source}\nBuilt: {built}\nTarget: {target}\nProfile: {profile}\nFeatures: {}",
        features.join(",")
    );
    let mut rust = String::from("pub const BUILD: BuildInfo = BuildInfo {\n");
    let mut json = String::from("{\n");
    for (key, value) in &values {
        rust.push_str(&format!("{key}: {value:?},\n"));
        json.push_str(&format!("{}: {},\n", json_string(key), json_string(value)));
    }
    rust.push_str(&format!("source_date_epoch: {epoch}, dirty: {dirty:?}, features: &{features:?},\n}};\npub const LABEL: &str = {label:?};\npub const SUMMARY: &str = {summary:?};\n"));
    let dirty_json = dirty.map_or("null", |v| if v { "true" } else { "false" });
    json.push_str(&format!(
        "\"source_date_epoch\": {epoch},\n\"dirty\": {dirty_json},\n\"features\": [{}]\n}}\n",
        features
            .iter()
            .map(|v| json_string(v))
            .collect::<Vec<_>>()
            .join(",")
    ));
    std::fs::write(Path::new(&out).join("build_identity.rs"), rust).unwrap();
    let manifest = Path::new(&out).join("kontra-build.json");
    std::fs::write(&manifest, json).unwrap();
    println!(
        "cargo:rustc-env=KONTRA_BUILD_MANIFEST={}",
        manifest.display()
    );
}

/// FNV-1a over every file's path and bytes, in name order.
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

#[cfg(test)]
mod tests {
    #[test]
    fn reproducible_utc_identity_handles_calendar_boundaries() {
        assert_eq!(super::utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(super::utc(1_709_164_800), "2024-02-29T00:00:00Z");
        assert_eq!(super::utc(4_107_542_400), "2100-03-01T00:00:00Z");
        assert_eq!(super::utc(253_402_300_799), "9999-12-31T23:59:59Z");
        assert_eq!(super::json_string("a\\b\"\n"), "\"a\\\\b\\\"\\u000a\"");
    }
}
