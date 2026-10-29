//! Capture the same scenario in an isolated native oracle and compare audio.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

use crate::{RATE, midi, scenario::Input};

fn tool_dir() -> Result<PathBuf> {
    let mut choices = Vec::new();
    if let Some(p) = std::env::var_os("KONTRA_ORACLE_DIR") {
        choices.push(PathBuf::from(p));
    }
    if let Ok(cwd) = std::env::current_dir() {
        choices.extend(cwd.ancestors().map(|p| p.join("tools/oracle")));
    }
    choices.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle"));
    choices.push(PathBuf::from(
        "/data/dev/projects/AudioPlugins/KONTRA/tools/oracle",
    ));
    choices
        .into_iter()
        .find(|p| p.join("capture.py").is_file() && p.join("compare.py").is_file())
        .context("cannot find tools/oracle; set KONTRA_ORACLE_DIR")
}

fn directory() -> Result<PathBuf> {
    let root = std::env::var_os("KONTRA_INSPECT_ORACLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/data/dev/workspace/PluginArtifacts/inspect-oracle"));
    std::fs::create_dir_all(&root).with_context(|| format!("creating {}", root.display()))?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = root.join(format!("inspect-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&path).with_context(|| format!("creating {}", path.display()))?;
    Ok(path)
}

fn wav(path: &Path, audio: &[[f32; 2]]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut out = hound::WavWriter::create(path, spec)
        .with_context(|| format!("creating {}", path.display()))?;
    for sample in audio.iter().flatten() {
        out.write_sample(*sample)?;
    }
    out.finalize()?;
    Ok(())
}

pub fn run(path: &Path, events: &[Input], audio: &[[f32; 2]], first_onset: bool) -> Result<Value> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
    {
        bail!("native oracle supports Kontakt .nki; UVI native capture is unavailable");
    }
    let tools = tool_dir()?;
    let dir = directory()?;
    let midi_path = dir.join("scenario.mid");
    let native = dir.join("native.wav");
    let ours = dir.join("v3.wav");
    let event_path = dir.join("events.json");
    std::fs::write(&midi_path, midi::write(events)?)?;
    std::fs::write(
        &event_path,
        serde_json::to_vec(
            &events
                .iter()
                .map(|e| json!({"frame":e.frame,"event":e.description}))
                .collect::<Vec<_>>(),
        )?,
    )?;
    wav(&ours, audio)?;
    let window = audio.len() as f64 / f64::from(RATE);
    let path = std::fs::canonicalize(path).context("resolving instrument path")?;
    let home =
        std::env::var_os("HOME").context("HOME is not set; cannot locate isolated oracle slots")?;
    let slots = PathBuf::from(home).join(".wine-oracle");
    ensure!(
        slots.is_dir(),
        "isolated oracle slots do not exist at {}",
        slots.display()
    );
    let work_env = format!("ORACLE_WORK={}", dir.join("work").display());
    let capture = Command::new("nice")
        .args(["-n", "19", "oracle-slot", "env", "-u", "KONTAKT_EXE"])
        .arg(work_env)
        .arg("python3")
        .arg(tools.join("capture.py"))
        .arg(&path)
        .arg(&native)
        .arg("--window")
        .arg(format!("{window:.9}"))
        .arg("--midi")
        .arg(&midi_path)
        .env("ORACLE_ROOT", slots)
        .env("ORACLE_SRC_PREFIX", dir.join("no-default-prefix"))
        .env("WINEDEBUG", "-all")
        .env("OPENBLAS_NUM_THREADS", "1")
        .env("OMP_NUM_THREADS", "1")
        .env_remove("KONTAKT_EXE")
        .output()
        .context("starting isolated oracle-slot capture")?;
    std::fs::write(dir.join("capture.stdout"), &capture.stdout)?;
    std::fs::write(dir.join("capture.stderr"), &capture.stderr)?;
    ensure!(
        capture.status.success(),
        "oracle capture failed ({}) in {}: {}",
        capture.status,
        dir.display(),
        String::from_utf8_lossy(&capture.stderr)
    );
    ensure!(
        native.is_file(),
        "oracle produced no WAV in {}",
        dir.display()
    );
    let helper = dir.join("compare_inspect.py");
    std::fs::write(&helper, include_str!("../../oracle/inspect_compare.py"))?;
    let compared = Command::new("nice")
        .args(["-n", "19", "python3"])
        .arg(&helper)
        .arg(tools.join("compare.py"))
        .arg(&native)
        .arg(&ours)
        .arg(&event_path)
        .arg(format!("{window:.9}"))
        .arg(if first_onset {
            "first-onset"
        } else {
            "envelope"
        })
        .env("OPENBLAS_NUM_THREADS", "1")
        .env("OMP_NUM_THREADS", "1")
        .output()
        .context("starting oracle comparison")?;
    std::fs::write(dir.join("compare.stderr"), &compared.stderr)?;
    ensure!(
        compared.status.success(),
        "oracle compare failed ({}) in {}: {}",
        compared.status,
        dir.display(),
        String::from_utf8_lossy(&compared.stderr)
    );
    let mut result: Value =
        serde_json::from_slice(&compared.stdout).context("reading oracle comparison JSON")?;
    if let Some(object) = result.as_object_mut() {
        object.insert("artifacts".into(), json!(dir));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uvi_reference_is_reported_unavailable_before_starting_wine() {
        let err = run(Path::new("example.uvip"), &[], &[], false).unwrap_err();
        assert!(
            err.to_string()
                .contains("UVI native capture is unavailable")
        );
    }
}
