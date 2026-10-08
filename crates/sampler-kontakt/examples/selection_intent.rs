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
        automation_candidates("program_private", &program.0.private_data);
        automation_candidates("program_public", &program.0.public_data);
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

// Bounded RE inventory only: meanings and array framing remain unverified.
// Report decoded metadata, never retain the underlying library bytes.
fn automation_candidates(name: &str, bytes: &[u8]) {
    let mut count = 0;
    for offset in 0..bytes.len().saturating_sub(26) {
        if bytes[offset] > 1 || bytes[offset + 2] != 0 {
            continue;
        }
        let version = bytes[offset + 1];
        let fields = &bytes[offset + 3..];
        let (id_at, second_at, floats_at, tag_at) = match version {
            0x70 => (8, None, 12, 20),
            0x71 => (7, Some(11), 15, 23),
            _ => continue,
        };
        let u32_at = |at: usize| {
            fields
                .get(at..at + 4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        };
        let Some(mode @ 0..=2) = u32_at(0) else {
            continue;
        };
        let Some(length) = u32_at(tag_at).filter(|n| *n <= 256) else {
            continue;
        };
        let Some(tag) = fields.get(tag_at + 4..tag_at + 4 + length as usize) else {
            continue;
        };
        let Ok(tag) = std::str::from_utf8(tag) else {
            continue;
        };
        if !tag.bytes().all(|b| b.is_ascii_graphic() || b == b' ') {
            continue;
        }
        let Some(a) = u32_at(floats_at)
            .map(f32::from_bits)
            .filter(|f| f.is_finite())
        else {
            continue;
        };
        let Some(b) = u32_at(floats_at + 4)
            .map(f32::from_bits)
            .filter(|f| f.is_finite())
        else {
            continue;
        };
        let word_at = if version == 0x70 { 6 } else { 5 };
        let word = u16::from_le_bytes(fields[word_at..word_at + 2].try_into().unwrap());
        println!(
            "automation_candidate block={name} offset={offset} version={version:#x} mode={mode} byte={} word={word} id={:?} second={:?} range={a},{b} tag={tag:?}",
            fields[4],
            u32_at(id_at).map(|v| v as i32),
            second_at.and_then(u32_at).map(|v| v as i32)
        );
        count += 1;
    }
    println!("automation_inventory block={name} candidates={count}");
}
