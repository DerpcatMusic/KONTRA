//! Stamps the build with its source revision (`KONTRA_REVISION`): the
//! checkout's short SHA, else CI's `GITHUB_SHA`, else "unknown"; and with
//! its version (`KONTRA_VERSION`): the release's, when the release build
//! sets it, else the crate's. About also shows when it was built
//! (`KONTRA_BUILT`, UTC; `SOURCE_DATE_EPOCH` when set, for reproducible
//! builds), for which target (`KONTRA_TARGET`) and in which formats
//! (`KONTRA_FORMATS`).

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    (out.status.success() && !text.trim().is_empty()).then(|| text.trim().to_owned())
}

fn main() {
    let revision = git(&["rev-parse", "--short=9", "HEAD"])
        .or_else(|| {
            std::env::var("GITHUB_SHA")
                .ok()?
                .get(..9)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=KONTRA_REVISION={revision}");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    let version_override = std::env::var("KONTRA_VERSION")
        .ok()
        .filter(|v| !v.is_empty());
    let overridden = if version_override.is_some() { "1" } else { "0" };
    let version = version_override
        .or_else(|| std::env::var("CARGO_PKG_VERSION").ok())
        .unwrap_or_default();
    println!("cargo:rustc-env=KONTRA_VERSION={version}");
    println!("cargo:rustc-env=KONTRA_VERSION_OVERRIDDEN={overridden}");
    println!("cargo:rerun-if-env-changed=KONTRA_VERSION");
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse().ok())
        .or_else(|| Some(SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs()))
        .unwrap_or(0);
    println!("cargo:rustc-env=KONTRA_BUILT={}", utc(secs));
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!(
        "cargo:rustc-env=KONTRA_TARGET={}",
        std::env::var("TARGET").unwrap_or_default()
    );
    let formats: Vec<&str> = [
        ("CLAP", "CLAP"),
        ("VST3", "VST3"),
        ("AU", "AU"),
        ("AAX", "AAX"),
        ("STANDALONE", "Standalone"),
    ]
    .into_iter()
    .filter(|(f, _)| std::env::var_os(format!("CARGO_FEATURE_{f}")).is_some())
    .map(|(_, name)| name)
    .collect();
    println!("cargo:rustc-env=KONTRA_FORMATS={}", formats.join(" · "));
    // An edit to the plugin or its crates is a new build: stamp its time.
    for dir in ["src", "../crates", "../CHANGELOG.md"] {
        println!("cargo:rerun-if-changed={dir}");
    }
    // A commit or a checkout appends to HEAD's log.
    for path in ["HEAD", "logs/HEAD"] {
        if let Some(p) = git(&["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={p}");
        }
    }
}

/// `secs` since the Unix epoch as `YYYY-MM-DD HH:MM UTC` (Howard Hinnant's
/// days-to-civil).
fn utc(secs: u64) -> String {
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        rest / 3_600,
        rest % 3_600 / 60
    )
}
