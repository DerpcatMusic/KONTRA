//! `kontakto audit-dsp`: every filter type, effect, modulator and modulation
//! target the presets under some roots use, statically and through their
//! scripts, ranked by how many instruments use each and marked by whether
//! the engine plays it. Names and counts only; no script source is printed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::filter;
use super::params::Address;
use crate::fx::{Params, params::Value};
use crate::import::{self, Instrument};
use crate::ksp::EnginePar;
use crate::modulation::ModTarget;

/// Item -> (implemented, instruments, occurrences).
type Tally = BTreeMap<(&'static str, String), (bool, BTreeSet<String>, usize)>;

/// Script identifiers that name DSP: filter and effect types, engine parameters.
const SCRIPT_PREFIXES: &[&str] = &["$FILTER_TYPE_", "$EFFECT_TYPE_", "$ENGINE_PAR_"];

/// Module parameters group modulation reaches in the engine.
fn target_applied(target: &ModTarget) -> bool {
    match target {
        ModTarget::Module { param, .. } => filter::Knob::parse(param).is_some(),
        _ => true,
    }
}

fn target_name(target: &ModTarget) -> String {
    match target {
        ModTarget::Volume => "volume".into(),
        ModTarget::Pitch => "pitch".into(),
        ModTarget::SampleStart => "playPos".into(),
        ModTarget::Attack => "ahdsr_attack (volume env)".into(),
        ModTarget::Release => "ahdsr_release (volume env)".into(),
        ModTarget::Module { param, .. } => param.clone(),
    }
}

/// `$NAME_123` identifiers starting with one of [`SCRIPT_PREFIXES`].
fn script_names(source: &str) -> BTreeSet<&str> {
    let mut out = BTreeSet::new();
    for prefix in SCRIPT_PREFIXES {
        for (at, _) in source.match_indices(prefix) {
            let len = source[at + 1..].bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
            out.insert(&source[at..at + 1 + len]);
        }
    }
    out
}

/// Whether the engine plays what a script names.
fn script_supported(name: &str, i: &Instrument) -> bool {
    if name.starts_with("$FILTER_TYPE_") {
        return filter::ksp_filter_type(name).is_some_and(|t| filter::filter_type(t).is_some());
    }
    if name.starts_with("$EFFECT_TYPE_") {
        return crate::fx::ksp_effect_type(name)
            .and_then(|id| u16::try_from(id).ok())
            .is_some_and(|id| id == 0 || crate::fx::Kind::from_ser_id(id).has_dsp());
    }
    let base = crate::ksp::ENGINE_PAR_BASE;
    let Some(id) = (base..base + 1000).find(|&id| crate::ksp::engine_par_name(id) == Some(name)) else {
        return false;
    };
    // Mapped onto a group insert or modulator, an instrument rack slot or a bus.
    let groups = 0..i.groups.len().min(4) as i32;
    let at = groups.flat_map(|g| (0..8).map(move |s| (g, s, -1)));
    let racks = [0, 1, 2, 1000].into_iter().flat_map(|generic| (0..8).map(move |s| (-1, s, generic)));
    at.chain(racks).any(|(group, slot, generic)| {
        Address::resolve(EnginePar { id, group, slot, generic }, &i.groups).is_some()
    })
}

/// Generalize a warning: digits become `N`, a leading group name goes.
fn general(w: &str, groups: &BTreeSet<&str>) -> String {
    let w = w.split_once(": ").filter(|(head, _)| groups.contains(head)).map_or(w, |(_, rest)| rest);
    let mut out = String::new();
    for c in w.chars() {
        if c.is_ascii_digit() {
            if !out.ends_with('N') {
                out.push('N');
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn tally_instrument(i: &Instrument, name: &str, tally: &mut Tally) {
    let mut add = |kind: &'static str, item: String, ok: bool| {
        let row = tally.entry((kind, item)).or_insert((ok, BTreeSet::new(), 0));
        row.1.insert(name.to_owned());
        row.2 += 1;
    };
    let state = |bypass: bool| if bypass { "bypassed" } else { "active" };
    for g in &i.groups {
        for fx in &g.fx.slots {
            match &fx.params {
                Params::Filter(f) => add(
                    "group filter",
                    format!("type {} ({})", f.filter_type, state(fx.bypass)),
                    filter::filter_type(f.filter_type).is_some(),
                ),
                Params::Eq(eq) => add("group filter", format!("EQ {} band ({})", eq.bands.len(), state(fx.bypass)), true),
                p => {
                    let ok = matches!(p, Params::StereoModeller(_))
                        || (fx.kind == crate::fx::Kind::Inverter
                            && !matches!(p, Params::Fields(f) if f.iter().any(|f| matches!(f.value, Value::Flag(true)))));
                    add("group fx", format!("{} ({})", fx.kind.name(), state(fx.bypass)), ok);
                }
            }
        }
        for m in &g.modulators {
            if m.kind != "external" {
                let ok = matches!(m.kind.as_str(), "ahdsr" | "flex");
                add("internal mod", format!("{} [{}]", m.kind, m.name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '_')), ok);
            }
        }
        for e in &g.envelopes {
            for t in &e.targets {
                add("env target", target_name(&t.target), target_applied(&t.target));
            }
        }
        for m in &g.mods {
            let source = format!("{:?}", m.source);
            let source = source.split('(').next().unwrap_or(&source).to_owned();
            add("ext source", source, true);
            add("ext target", target_name(&m.target), target_applied(&m.target));
        }
    }
    for (location, fx) in i.fx.effects() {
        let rack = if location.starts_with("bus") { "bus" } else { location.trim_start_matches("instrument ") };
        let ok = fx.is_implemented() || fx.kind.has_dsp();
        add("rack fx", format!("{} [{rack}, {}]", fx.kind.name(), state(fx.bypass)), ok);
    }
    for source in &i.scripts {
        for n in script_names(source) {
            let kind = if n.starts_with("$ENGINE_PAR_") { "script par" } else { "script type" };
            add(kind, n.to_owned(), script_supported(n, i));
        }
    }
    let groups: BTreeSet<&str> = i.groups.iter().map(|g| g.name.as_str()).collect();
    for w in &i.warnings {
        add("import warning", general(w, &groups), false);
    }
}

/// Every instrument under `roots`, tallied and ranked; the report as text.
pub fn audit_dsp(roots: &[PathBuf]) -> Result<String> {
    let mut tally = Tally::new();
    let mut instruments = 0;
    for root in roots {
        let Ok(presets) = import::presets(root) else {
            eprintln!("skipped {}: not a folder", root.display());
            continue;
        };
        for path in presets {
            let programs = if import::is_multi(&path) {
                import::read_multi(&path).map(|m| m.parts.into_iter().map(|p| p.program).collect::<Vec<_>>())
            } else {
                Ok(vec![0])
            };
            for program in programs.unwrap_or_default() {
                let name = format!("{}#{program}", path.display());
                match import::read_program(&path, program) {
                    Ok(i) => {
                        instruments += 1;
                        tally_instrument(&i, &name, &mut tally);
                    }
                    Err(e) => eprintln!("{name}: {e:#}"),
                }
            }
        }
    }
    Ok(report(instruments, roots, tally))
}

fn report(instruments: usize, roots: &[PathBuf], tally: Tally) -> String {
    let mut rows: Vec<_> = tally.into_iter().collect();
    // Gaps first, then by instruments.
    rows.sort_by(|a, b| a.1.0.cmp(&b.1.0).then(b.1.1.len().cmp(&a.1.1.len())).then(a.0.cmp(&b.0)));
    let roots: Vec<_> = roots.iter().map(|r| r.display().to_string()).collect();
    let missing = rows.iter().filter(|r| !r.1.0).count();
    let mut out = format!(
        "DSP audit: {instruments} instruments under {}; {missing} of {} items not played\n{:>4} {:>11} {:>11}  {:<14} item\n",
        roots.join(", "),
        rows.len(),
        "ok",
        "instruments",
        "occurrences",
        "kind"
    );
    for ((kind, item), (ok, who, n)) in rows {
        let libs: BTreeSet<_> = who.iter().filter_map(|w| library_of(w)).collect();
        let example = who.iter().next().and_then(|w| Path::new(w).file_name()).map(|f| f.to_string_lossy().into_owned());
        out += &format!(
            "{:>4} {:>11} {:>11}  {:<14} {item}  [{}] e.g. {}\n",
            if ok { "ok" } else { "GAP" },
            who.len(),
            n,
            kind,
            libs.into_iter().collect::<Vec<_>>().join(", "),
            example.unwrap_or_default()
        );
    }
    out
}

/// The library folder under the Kontakt root an instrument path lies in.
fn library_of(path: &str) -> Option<String> {
    let rest = Path::new(path).strip_prefix(import::LIBRARY_ROOT).ok()?;
    Some(rest.components().next()?.as_os_str().to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_script_names_without_source() {
        let names = script_names("set_engine_par($ENGINE_PAR_EFFECT_SUBTYPE, $FILTER_TYPE_AR_LP4, 0, 0, -1)");
        assert_eq!(names.into_iter().collect::<Vec<_>>(), ["$ENGINE_PAR_EFFECT_SUBTYPE", "$FILTER_TYPE_AR_LP4"]);
        let groups = BTreeSet::from(["Vln 1"]);
        assert_eq!(general("Vln 1: slot 12 ignored", &groups), "slot N ignored");
    }
}
