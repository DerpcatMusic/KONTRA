//! Explicit test harness input. This module and its environment variable do not
//! exist in production builds; production always executes the compiled host.
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{borrow::Cow, io::Read, path::Path};

const LIMIT: usize = 256 * 1024;
const ENV: &str = "KONTRA_NATIVE_UI_TEST_HOST_SOURCE";

pub(super) struct Source {
    pub text: Cow<'static, str>,
    path: Option<std::path::PathBuf>,
    sha256: String,
}

fn load(path: Option<&Path>) -> Result<Source> {
    let text = if let Some(path) = path {
        let file = std::fs::File::open(path)
            .with_context(|| format!("Native UI test host open {}", path.display()))?;
        ensure!(
            file.metadata()?.len() <= LIMIT as u64,
            "Native UI test host exceeds {LIMIT} bytes"
        );
        let mut bytes = Vec::new();
        file.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= LIMIT,
            "Native UI test host exceeds {LIMIT} bytes"
        );
        Cow::Owned(String::from_utf8(bytes).context("Native UI test host requires UTF-8")?)
    } else {
        Cow::Borrowed(include_str!("host.lua"))
    };
    Ok(Source {
        sha256: format!("{:x}", Sha256::digest(text.as_bytes())),
        text,
        path: path.map(Path::to_path_buf),
    })
}

pub(super) fn load_from_environment() -> Result<Source> {
    let path = std::env::var_os(ENV).map(std::path::PathBuf::from);
    let source = load(path.as_deref())?;
    // Print before library evaluation: an unsupported frontier must still retain
    // the actual executed host identity, distinct from the frozen Rust source.
    if source.path.is_some() || std::env::var_os("KONTRA_NATIVE_UI_PROBE_INSTRUMENT").is_some() {
        eprintln!(
            "Native UI test host {}",
            serde_json::json!({
                "origin": if source.path.is_some() { "explicit_test_file" } else { "compiled" },
                "path": source.path,
                "bytes": source.text.len(),
                "sha256": source.sha256,
            })
        );
    }
    Ok(source)
}

#[test]
fn test_host_default_override_and_bounded_failures_keep_executed_identity() {
    const CHILD: &str = "KONTRA_NATIVE_UI_TEST_HOST_SOURCE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let selected = load_from_environment().unwrap();
        assert!(selected.path.is_some());
        assert_eq!(
            mlua::Lua::new()
                .load(selected.text.as_ref())
                .eval::<String>()
                .unwrap(),
            "explicit authored override"
        );
        return;
    }
    let compiled = load(None).unwrap();
    assert!(matches!(compiled.text, Cow::Borrowed(_)));
    assert_eq!(compiled.text, include_str!("host.lua"));
    assert_eq!(
        compiled.sha256,
        format!("{:x}", Sha256::digest(include_bytes!("host.lua")))
    );
    assert!(compiled.path.is_none());

    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("host.lua");
    std::fs::write(&path, "return 'explicit authored override'").unwrap();
    let selected = load(Some(&path)).unwrap();
    assert_eq!(selected.path.as_deref(), Some(path.as_path()));
    assert_ne!(selected.sha256, compiled.sha256);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "native_ui::test_host::test_host_default_override_and_bounded_failures_keep_executed_identity", "--nocapture"])
        .env(CHILD, "1")
        .env(ENV, &path)
        .output().unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let identity = String::from_utf8(child.stderr).unwrap();
    assert!(identity.contains("explicit_test_file"));
    assert!(identity.contains(&selected.sha256));
    assert!(identity.contains(&format!("\"bytes\":{}", selected.text.len())));
    let lua = mlua::Lua::new();
    assert_eq!(
        lua.load(selected.text.as_ref()).eval::<String>().unwrap(),
        "explicit authored override"
    );

    std::fs::write(&path, vec![b' '; LIMIT]).unwrap();
    assert_eq!(load(Some(&path)).unwrap().text.len(), LIMIT);
    std::fs::write(&path, vec![b' '; LIMIT + 1]).unwrap();
    assert!(
        load(Some(&path))
            .err()
            .unwrap()
            .to_string()
            .contains("exceeds")
    );
    std::fs::write(&path, [0xff]).unwrap();
    assert!(
        load(Some(&path))
            .err()
            .unwrap()
            .to_string()
            .contains("UTF-8")
    );
    std::fs::remove_file(&path).unwrap();
    assert!(
        load(Some(&path))
            .err()
            .unwrap()
            .to_string()
            .contains("open")
    );
}
