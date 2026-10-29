//! Stamps the build with its source revision (`KONTRA_REVISION`): the
//! checkout's short SHA, else CI's `GITHUB_SHA`, else "unknown".

use std::process::Command;

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
    // A commit or a checkout appends to HEAD's log.
    for path in ["HEAD", "logs/HEAD"] {
        if let Some(p) = git(&["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={p}");
        }
    }
}
