//! Read-only source-intent probe. Emits summaries, never library script text or payloads.
use std::path::Path;

#[test]
#[ignore = "requires installed survey libraries; source-only, no sample render"]
fn survey_switch_and_conflux_intent() {
    let picks = std::fs::read_to_string("/home/derpcat/.cache/kontakto-survey/picks.tsv").unwrap();
    for (index, row) in picks
        .lines()
        .enumerate()
        .filter(|(i, _)| [2, 5, 6, 10].contains(i))
    {
        let path = Path::new(row.split_once('\t').unwrap().1);
        let kontakt = if index == 6 {
            sampler_kontakt::read_program(path, 0)
        } else {
            sampler_kontakt::read(path)
        }
        .unwrap();
        let mut instrument = kontakt.instrument;
        println!(
            "PATCH {index}: groups={} articulations={} owner={:?}",
            instrument.groups.len(),
            instrument.articulations.len(),
            instrument.switching.owner
        );
        for behavior in &instrument.behaviors {
            let s = &behavior.source;
            println!(
                "SCRIPT {:?} {} bytes={} note={} allow={} disallow={} ignore={} keyrange={} controlkeys={} nativeview={}",
                behavior.slot,
                behavior.name,
                s.len(),
                s.matches("on note").count(),
                s.matches("allow_group(").count(),
                s.matches("disallow_group(").count(),
                s.matches("ignore_event(").count(),
                s.matches("set_keyrange(").count(),
                s.matches("$NI_KEY_TYPE_CONTROL").count(),
                s.matches("make_perfview").count()
            );
        }
        let (_, interfaces, _) = sampler_kontakt::compile_ui(
            &mut instrument,
            &sampler_kontakt::Options {
                library: Some(path.into()),
                ..Default::default()
            },
        );
        for face in interfaces {
            println!(
                "UI {:?} performance={} widgets={} pages={}",
                face.source,
                face.performance,
                face.widgets.len(),
                face.pages.len()
            );
        }
        let mut summaries = std::collections::BTreeMap::new();
        for n in &instrument.unsupported {
            let kind = if n.feature == "modulation target"
                || n.feature == "modulation of a module parameter"
            {
                format!(
                    "{} {}",
                    n.feature,
                    n.value.split_whitespace().next().unwrap_or("")
                )
            } else {
                n.feature.clone()
            };
            *summaries.entry(kind).or_insert(0usize) += 1;
        }
        for (kind, count) in summaries {
            println!("ISSUE {kind} count={count}");
        }
    }
}
