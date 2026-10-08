//! Metadata-only script and selection intent probe. No sample or source dump.
use std::path::Path;
fn main() {
    let arg = std::env::args().nth(1).unwrap();
    if std::env::args().nth(2).as_deref() == Some("--layout") {
        let chunks = sampler_kontakt::read_chunks(Path::new(&arg)).unwrap();
        for chunk in &chunks.0 {
            println!("root id={:#x} length={}", chunk.id, chunk.data.len());
        }
        let program =
            ni_file::kontakt::objects::Program::try_from(chunks.find_first(0x28).unwrap()).unwrap();
        println!(
            "program version={:#x} private_length={} public_length={}",
            program.version(),
            program.0.private_data.len(),
            program.0.public_data.len()
        );
        match sampler_kontakt::program_automation(
            &program.0.private_data,
            program.version(),
            sampler_kontakt::Limits {
                bytes: 64 << 20,
                records: 65536,
            },
        ) {
            Ok(records) => {
                for record in records {
                    println!(
                        "automation offset={} version={:#x} mode={} address={} soft_takeover={} ids={}/{:?} range={}..{} tag={:?}",
                        record.offset,
                        record.version,
                        record.mode,
                        record.address,
                        record.soft_takeover,
                        record.id,
                        record.secondary_id,
                        record.low,
                        record.high,
                        std::str::from_utf8(record.tag.data())
                    );
                }
            }
            Err(error) => println!("automation_layout {error}"),
        }
        for chunk in program.children() {
            println!(
                "program_child id={:#x} length={}",
                chunk.id,
                chunk.data.len()
            );
            if chunk.id == 6 {
                let script = ni_file::kontakt::objects::BParScript::try_from(chunk).unwrap();
                println!(
                    "script version={:#x} private_length={} public_length={} children={:?}",
                    script.0.version,
                    script.0.private_data.len(),
                    script.0.public_data.len(),
                    script
                        .0
                        .children
                        .iter()
                        .map(|c| (c.id, c.data.len()))
                        .collect::<Vec<_>>()
                );
            }
        }
        return;
    }
    let mut library = sampler_kontakt::read(Path::new(&arg)).unwrap();
    if std::env::args().nth(2).as_deref() == Some("--widgets") {
        let (scripts, _, _) = sampler_kontakt::compile_ui(
            &mut library.instrument,
            &sampler_kontakt::Options {
                library: Some(arg.clone().into()),
                ..Default::default()
            },
        );
        for script in scripts {
            let slot = script.view().slot();
            let mut slider = 0;
            for widget in &script.model().interface.widgets {
                let ordinal = (widget.kind == sampler_ksp::model::WidgetKind::Slider).then(|| {
                    let i = slider;
                    slider += 1;
                    i
                });
                if (32808..=32812).contains(&widget.ui_id)
                    || ordinal.is_some_and(|i| (40..=44).contains(&i))
                    || widget.name.contains("controller_dynamics")
                    || widget.name.contains("art_select_tonal")
                {
                    println!(
                        "widget slot={} ui_id={} name={} range={:?} automation_id={:?} slider_ordinal={ordinal:?}",
                        slot,
                        widget.ui_id,
                        widget.name,
                        widget.range,
                        widget.int("CONTROL_PAR_AUTOMATION_ID")
                    );
                }
            }
        }
        return;
    }
    let ir = &library.instrument;
    println!(
        "instrument={} groups={} zones={} default_keyswitch={:?}",
        ir.name,
        ir.groups.len(),
        ir.zones.len(),
        ir.default_keyswitch
    );
    for behavior in &ir.behaviors {
        println!("slot={:?}", behavior.slot);
        for (name, value) in &behavior.state {
            match value {
                sampler_ir::Saved::Int(n) => println!("saved_int {name}={n}"),
                sampler_ir::Saved::Real(n) => println!("saved_real {name}={n}"),
                sampler_ir::Saved::Ints(values)
                    if name.to_ascii_lowercase().contains("cc")
                        || name.to_ascii_lowercase().contains("dyn") =>
                {
                    println!(
                        "saved_ints {name} count={} first={:?}",
                        values.len(),
                        &values[..values.len().min(60)]
                    )
                }
                _ => {}
            }
        }
        // Read every statement in memory; report only identifiers and dependency
        // facts relevant to the instrument's dynamic selection path.
        let source = behavior.source.to_ascii_lowercase();
        for marker in [
            "%cc[1]",
            "%cc[2]",
            "%cc[11]",
            "$event_velocity",
            "set_controller",
            "set_engine_par",
            "allow_group",
            "disallow_group",
        ] {
            println!(
                "dependency={marker} occurrences={}",
                source.matches(marker).count()
            );
        }
        for (i, line) in source.lines().enumerate() {
            if line.contains("cc_num")
                || line.contains("%cc")
                || (line.contains("slider_controller_dynamics")
                    && !line.contains("set_control_par"))
                || line.contains("on controller")
            {
                let tokens: Vec<_> = line
                    .split(|c: char| !(c.is_alphanumeric() || "$%_".contains(c)))
                    .filter(|t| !t.is_empty())
                    .collect();
                println!("dependency_line={} tokens={tokens:?}", i + 1);
            }
        }
    }
    for (g, group) in ir.groups.iter().enumerate() {
        let mut ranges: Vec<_> = ir
            .zones
            .iter()
            .filter(|z| z.group == Some(sampler_ir::GroupRef(g)))
            .map(|z| (z.velocities.low, z.velocities.high))
            .collect();
        ranges.sort();
        ranges.dedup();
        println!(
            "group={g} name={} velocity_ranges={ranges:?} response={:?} start={:?}",
            group.name,
            ir.zones
                .iter()
                .find(|z| z.group == Some(sampler_ir::GroupRef(g)))
                .map(|z| z.velocity),
            group.start
        );
    }
}
