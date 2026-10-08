//! Diagnose UI resource identities with counts only; never print paths or bytes.
#![cfg(feature = "library-access")]
#[test]
#[ignore = "one installed-program resource probe; approved reader and kontakto-heavy required"]
fn resource_failure_categories() {
    use std::io::Read;
    let item = std::env::var("UVI_AUDIT_ITEM").unwrap();
    let (path, member) = item.split_once("::").unwrap();
    let bank = sampler_uvi::Bank::open(std::path::Path::new(path)).unwrap();
    let (xml, program) = bank.program(member).unwrap();
    let host =
        sampler_uvi::script::ScriptHost::new(&xml, bank.scripts(), Default::default()).unwrap();
    let face = host.interface();
    let members = bank.members();
    let mut header = [0u8; 328];
    std::fs::File::open(path)
        .unwrap()
        .read_exact(&mut header)
        .unwrap();
    let end = header[48..].iter().position(|b| *b == 0).unwrap();
    let name = std::str::from_utf8(&header[48..48 + end]).unwrap();
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for asset in &face.assets {
        *counts.entry("assets".into()).or_default() += 1;
        let path = asset.path.replace('\\', "/");
        for (label, yes) in [
            ("parent_segments", path.split('/').any(|s| s == "..")),
            ("colon", path.contains(':')),
            ("dollar", path.contains('$')),
            ("rooted", path.starts_with('/')),
            ("png", path.to_ascii_lowercase().ends_with(".png")),
            ("svg", path.to_ascii_lowercase().ends_with(".svg")),
            ("empty", path.is_empty()),
        ] {
            if yes {
                *counts.entry(label.into()).or_default() += 1;
            }
        }
        let basename = path.rsplit('/').next().unwrap();
        let name_matches = members
            .iter()
            .filter(|m| m.rsplit('/').next().unwrap().eq_ignore_ascii_case(basename))
            .count();
        *counts
            .entry(format!("basename_matches={name_matches}"))
            .or_default() += 1;
        let folder = program.rsplit_once('/').map_or("", |(a, _)| a);
        let combined = format!("{folder}/{path}");
        let mut parts = Vec::new();
        for part in combined.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                other => parts.push(other),
            }
        }
        let normalized = parts.join("/");
        let full_matches = members
            .iter()
            .filter(|m| m.eq_ignore_ascii_case(&normalized))
            .count();
        *counts
            .entry(format!("preset_relative_matches={full_matches}"))
            .or_default() += 1;
        let (relative, volume_bad) = if let Some(v) = path.strip_prefix('$') {
            if let Some((volume, r)) = v.split_once('/') {
                *counts.entry("qualified".into()).or_default() += 1;
                (
                    r,
                    !volume.eq_ignore_ascii_case(name)
                        && !volume.eq_ignore_ascii_case(&format!("{name}.ufs")),
                )
            } else {
                (path.as_str(), true)
            }
        } else {
            (path.as_str(), false)
        };
        *counts
            .entry(format!("volume_bad={volume_bad}"))
            .or_default() += 1;
        let suffix = format!("/{}", relative.trim_start_matches('/').to_ascii_lowercase());
        let matches = members
            .iter()
            .filter(|p| {
                p.eq_ignore_ascii_case(relative) || p.to_ascii_lowercase().ends_with(&suffix)
            })
            .count();
        *counts
            .entry(format!("suffix_matches={matches}"))
            .or_default() += 1;
        if let Some(p) = members
            .iter()
            .find(|p| p.eq_ignore_ascii_case(relative) || p.to_ascii_lowercase().ends_with(&suffix))
        {
            *counts
                .entry(format!("direct_member_read={}", bank.file(p).is_ok()))
                .or_default() += 1;
        }
        if bank.resource(&program, &asset.path).is_ok() {
            *counts.entry("baseline_resolved".into()).or_default() += 1;
        }
    }
    println!(
        "RESOURCE-CATEGORIES {}",
        serde_json::to_string(&counts).unwrap()
    );
}
