//! `KONTRA_IMPORT_HASH`: a hash of the sources that decide what an import
//! produces, recorded in load diagnostics.

use std::path::Path;
use std::{
    env,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "src/plugin/automation_ids.rs"]
mod automation_ids;

const SOURCES: &[&str] = &[
    "crates/sampler-ir/src",
    "crates/sampler-kontakt/src",
    "crates/sampler-core/src/lower.rs",
];

fn main() {
    host_parameter_index();
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
    identity(hash, build_hash);
}

// The framework's compile-time preset index cannot inspect a manual Params
// implementation. Emit the same stable slot IDs used by its runtime metadata.
fn host_parameter_index() {
    let out = std::path::PathBuf::from(env::var_os("OUT_DIR").expect("cargo OUT_DIR"));
    let triple = env::var("TARGET").expect("cargo target triple");
    let target = parameter_target_dir(&out, &triple);
    let directory = target.join("param-index").join(env::var("CARGO_PKG_NAME").unwrap());
    std::fs::create_dir_all(&directory).expect("parameter index directory");
    let mut index = String::from("struct = \"HostAutomation\"\nscheme = \"hash\"\n");
    for address in 0..automation_ids::HOST_AUTOMATION_SLOTS {
        use std::fmt::Write;
        writeln!(index, "[[param]]\nid = {}\nfield = \"slot_{address}\"\nname = \"Automation {address}\"\n",
            automation_ids::HOST_AUTOMATION_BASE + u32::from(address)).unwrap();
    }
    std::fs::write(directory.join("HostAutomation.params.toml"), index).expect("host parameter index");
}

fn parameter_target_dir<'a>(out: &'a Path, triple: &str) -> &'a Path {
    let target = out.ancestors().nth(4).expect("cargo build output layout");
    // Explicit --target adds a triple directory even when it equals the host.
    if target.file_name().is_some_and(|name| name == triple) {
        target.parent().expect("cargo target root")
    } else {
        target
    }
}

#[test]
fn parameter_index_uses_the_root_for_host_and_explicit_target_builds() {
    let triple = "x86_64-unknown-linux-gnu";
    let root = Path::new("workspace/target");
    assert_eq!(parameter_target_dir(&root.join("release/build/kontakto-hash/out"), triple), root);
    assert_eq!(parameter_target_dir(&root.join(triple).join("release/build/kontakto-hash/out"), triple), root);
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
        "KONTRA_NIGHTLY_BUILD",
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
    let dirty = if env::var("KONTRA_NIGHTLY_BUILD").as_deref() == Ok("1") {
        // Nightly changes only the root package versions; validate the entire delta.
        let status = Command::new("python3")
            .args(["tools/version.py", "nightly-dirty"])
            .output()
            .expect("Nightly source provenance requires Python 3");
        assert!(status.status.success(), "Nightly provenance check failed: {}",
            String::from_utf8_lossy(&status.stderr));
        match String::from_utf8_lossy(&status.stdout).trim() {
            "false" => Some(false),
            "true" => Some(true),
            _ => panic!("Invalid Nightly provenance result"),
        }
    } else {
        git(&["status", "--porcelain", "--untracked-files=no"]).map(|s| !s.is_empty())
    };
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
