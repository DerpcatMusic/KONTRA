//! Source-free KSP parity CLI and atomic per-patch cache.
use anyhow::{Context, Result, bail, ensure};
use sampler_parity::{Construct, ScriptReport, Status};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct PatchReport {
    patch: PathBuf,
    scripts: Vec<ScriptReport>,
}
#[derive(Serialize)]
struct Example {
    library: String,
    patch: PathBuf,
    script_id: String,
    line: u32,
}
#[derive(Serialize)]
struct Gap {
    name: String,
    status: Status,
    owner: Option<String>,
    spec: Option<String>,
    detail: String,
    uses: u64,
    libraries: BTreeSet<String>,
    score: u64,
    examples: Vec<Example>,
}
#[derive(Serialize)]
struct Corpus {
    patches: usize,
    gaps: Vec<Gap>,
    failures: Vec<PathBuf>,
}

pub fn run(args: &[String]) -> Result<()> {
    let mut patch = None;
    let mut corpus = None;
    let (mut json, mut lowered) = (false, false);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--lowered" => lowered = true,
            "--corpus" => {
                ensure!(corpus.is_none(), "duplicate --corpus");
                corpus = Some(PathBuf::from(
                    iter.next().context("--corpus requires dir or items.tsv")?,
                ));
            }
            _ if arg.starts_with('-') => bail!("unknown parity option {arg}"),
            _ => {
                ensure!(patch.is_none(), "parity takes one patch");
                patch = Some(PathBuf::from(arg));
            }
        }
    }
    ensure!(
        patch.is_some() != corpus.is_some(),
        "parity <patch> [--json] [--lowered] OR parity --corpus <dir|items.tsv> [--json]"
    );
    if let Some(path) = patch {
        let report = cached_patch(&path)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_patch(&report, lowered);
        }
    } else {
        let root = corpus.unwrap();
        let paths = corpus_paths(&root)?;
        let mut by_name: BTreeMap<(String, Status), Gap> = BTreeMap::new();
        let mut failures = Vec::new();
        let mut patches = 0;
        for path in paths {
            let report = match cached_patch(&path) {
                Ok(r) => r,
                Err(_) => {
                    failures.push(path);
                    continue;
                }
            };
            patches += 1;
            let library = library_name(&path, &root);
            for script in report.scripts {
                for c in script
                    .constructs
                    .into_iter()
                    .filter(|c| c.status != Status::Implemented)
                {
                    let gap = by_name
                        .entry((c.name.clone(), c.status))
                        .or_insert_with(|| Gap {
                            name: c.name,
                            status: c.status,
                            owner: c.owner,
                            spec: c.spec,
                            detail: c.detail,
                            uses: 0,
                            libraries: BTreeSet::new(),
                            score: 0,
                            examples: Vec::new(),
                        });
                    gap.uses += 1;
                    gap.libraries.insert(library.clone());
                    if gap.examples.len() < 5
                        && !gap.examples.iter().any(|e| {
                            e.patch == path && e.script_id == script.id && e.line == c.span.line
                        })
                    {
                        gap.examples.push(Example {
                            library: library.clone(),
                            patch: path.clone(),
                            script_id: script.id.clone(),
                            line: c.span.line,
                        });
                    }
                }
            }
        }
        let mut gaps: Vec<_> = by_name.into_values().collect();
        for gap in &mut gaps {
            gap.score = gap.uses.saturating_mul(gap.libraries.len() as u64);
        }
        gaps.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.status.cmp(&b.status))
        });
        let report = Corpus {
            patches,
            gaps,
            failures,
        };
        if json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!(
                "{} patches; {} failed (source and diagnostic values omitted)",
                report.patches,
                report.failures.len()
            );
            for g in report.gaps {
                println!(
                    "{} {:?}: {} uses × {} libraries = {} | owner={} | spec={} | {}",
                    g.name,
                    g.status,
                    g.uses,
                    g.libraries.len(),
                    g.score,
                    g.owner.as_deref().unwrap_or("-"),
                    g.spec.as_deref().unwrap_or("-"),
                    g.detail
                );
                for e in g.examples {
                    println!(
                        "  {} | {} | {}:{}",
                        e.library,
                        e.patch.display(),
                        e.script_id,
                        e.line
                    );
                }
            }
            for path in report.failures {
                println!("failed: {}", path.display());
            }
        }
    }
    Ok(())
}
fn print_patch(r: &PatchReport, lowered: bool) {
    println!("{} (source omitted)", r.patch.display());
    for s in &r.scripts {
        println!(
            "{}: {} construct uses, {} tokens",
            s.id,
            s.constructs.len(),
            s.tokens.len()
        );
        for status in [
            Status::Implemented,
            Status::Approximate,
            Status::Stub,
            Status::Unsupported,
            Status::Fault,
        ] {
            let n = s.constructs.iter().filter(|c| c.status == status).count();
            let percent = if s.constructs.is_empty() {
                0.0
            } else {
                100.0 * n as f64 / s.constructs.len() as f64
            };
            println!("  {status:?}: {n} ({percent:.2}%)");
        }
        for c in s
            .constructs
            .iter()
            .filter(|c| c.status != Status::Implemented)
        {
            print_construct(c);
        }
        if lowered {
            for l in &s.lowered {
                println!("  lowered line {}: {}", l.span.line, l.ops.join(", "));
            }
        }
    }
}
fn print_construct(c: &Construct) {
    println!(
        "  line {}: {} {:?} | owner={} | spec={} | {}",
        c.span.line,
        c.name,
        c.status,
        c.owner.as_deref().unwrap_or("-"),
        c.spec.as_deref().unwrap_or("-"),
        c.detail
    );
}
fn cached_patch(path: &Path) -> Result<PatchReport> {
    let path = path.canonicalize().context("resolve patch")?;
    let metadata = std::fs::metadata(&path)?;
    let mut hasher = blake3::Hasher::new();
    // Include producer sources, so a parity fix cannot keep an old count alive.
    for source in [
        include_str!("../crates/sampler-ksp/src/parity.rs"),
        include_str!("../crates/sampler-ksp/src/parity_ledger.rs"),
        include_str!("../crates/sampler-ksp/src/lexer.rs"),
        include_str!("../crates/sampler-ksp/src/builtins.rs"),
        include_str!("../crates/sampler-ksp/src/vendor_constants.txt"),
        include_str!("../crates/sampler-ksp/src/parser.rs"),
        include_str!("../crates/sampler-ksp/src/sema.rs"),
        include_str!("../crates/sampler-ksp/src/lower.rs"),
        include_str!("../crates/sampler-kontakt/src/parity.rs"),
        include_str!("../crates/sampler-parity/src/lib.rs"),
    ] {
        hasher.update(source.as_bytes());
    }
    hasher.update(path.as_os_str().as_encoded_bytes());
    hasher.update(&metadata.len().to_le_bytes());
    hasher.update(
        &metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
            .to_le_bytes(),
    );
    let dir = dirs::cache_dir()
        .context("no user cache directory")?
        .join("kontakto-parity");
    std::fs::create_dir_all(&dir)?;
    let cache = dir.join(format!("{}.json", hasher.finalize().to_hex()));
    if let Ok(bytes) = std::fs::read(&cache) {
        if let Ok(report) = serde_json::from_slice(&bytes) {
            return Ok(report);
        }
    }
    let scripts = sampler_kontakt::read_parity_reports(&path).map_err(|_| {
        anyhow::anyhow!("patch could not be inspected (source and diagnostic values omitted)")
    })?;
    let report = PatchReport {
        patch: path,
        scripts,
    };
    let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
    serde_json::to_writer(&mut temp, &report)?;
    temp.as_file().sync_all()?;
    temp.persist(&cache)
        .map_err(|e| e.error)
        .context("write parity cache")?;
    Ok(report)
}
fn corpus_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = BTreeSet::new();
    if root.is_file() {
        for line in std::fs::read_to_string(root)?.lines() {
            let columns: Vec<_> = line.split('\t').collect();
            let path = if columns.len() == 1 {
                columns[0]
            } else if columns[0] == "kontakt" {
                columns[1]
            } else {
                // Also admit library<TAB>/absolute/patch.nki lists; skip UVI rows.
                match columns
                    .iter()
                    .find(|p| p.to_ascii_lowercase().ends_with(".nki"))
                {
                    Some(p) => p,
                    None => continue,
                }
            };
            if Path::new(path)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
            {
                paths.insert(PathBuf::from(path));
            }
        }
    } else {
        ensure!(root.is_dir(), "corpus directory does not exist");
        for item in walkdir::WalkDir::new(root) {
            let item = item?;
            if item.file_type().is_file()
                && item
                    .path()
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
            {
                paths.insert(item.into_path());
            }
        }
    }
    Ok(paths.into_iter().collect())
}
fn library_name(path: &Path, root: &Path) -> String {
    let parts: Vec<_> = path.components().collect();
    if let Some(i) = parts.iter().position(|c| c.as_os_str() == "Kontakt") {
        if let Some(library) = parts.get(i + 1) {
            return library.as_os_str().to_string_lossy().into();
        }
    }
    if root.is_dir() {
        if let Ok(relative) = path.strip_prefix(root) {
            if let Some(first) = relative.components().next() {
                return first.as_os_str().to_string_lossy().into();
            }
        }
    }
    path.parent().unwrap_or(path).to_string_lossy().into()
}
