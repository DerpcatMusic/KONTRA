//! Metadata-only display census. Authored scripts, labels and values stay in RAM.
use sampler_ui_ir::{Interface, Kind, Role};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

fn references(source: &str, counts: &mut BTreeMap<&'static str, usize>) {
    let (mut comment, mut quoted, mut escaped) = (0u32, false, false);
    let mut code = String::new();
    for c in source.chars() {
        if comment > 0 {
            if c == '{' { comment += 1; }
            if c == '}' { comment -= 1; }
        } else if quoted {
            if escaped { escaped = false; }
            else if c == '\\' { escaped = true; }
            else if c == '"' { quoted = false; }
        } else if c == '{' { comment = 1; }
        else if c == '"' { quoted = true; }
        else { code.push(c); continue; }
        code.push(' ');
    }
    for token in code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$')) {
        for name in ["set_knob_label", "set_knob_unit", "$CONTROL_PAR_LABEL", "$CONTROL_PAR_UNIT", "$CONTROL_PAR_DISPLAY", "$CONTROL_PAR_DISPLAY_VALUE"] {
            if token == name { *counts.entry(name).or_default() += 1; }
        }
    }
}

fn stats(faces: &[Interface]) -> serde_json::Value {
    let mut kinds = BTreeMap::<&str, usize>::new();
    let mut units = BTreeMap::<&str, usize>::new();
    let (mut labels, mut empty_labels, mut pictured_labels, mut hidden_values, mut ratios, mut enum_units) = (0,0,0,0,0,0);
    for w in faces.iter().flat_map(|f| &f.widgets) {
        let (kind, display) = match &w.kind {
            Kind::Knob {display,..} => ("knob", Some(display)),
            Kind::ValueEdit {display,..} => ("value_edit", Some(display)),
            Kind::Slider {..} => ("slider", None),
            _ => continue,
        };
        *kinds.entry(kind).or_default() += 1;
        hidden_values += usize::from(w.hide.value);
        if let Some(t) = &w.value_text {
            labels += 1;
            empty_labels += usize::from(t.is_empty());
            pictured_labels += usize::from(w.image(Role::Strip).is_some());
        }
        if let Some(d) = display {
            ratios += usize::from(d.ratio != 1.);
            let unit = match d.unit.as_str() {
                "" => "none", "%" => "%", "Hz" => "Hz", "ms" => "ms", "dB" => "dB", "oct" => "oct", "st" => "st",
                "Percent" => "Percent", "PercentNormalized" => "PercentNormalized", "Seconds" => "Seconds", "MilliSeconds" => "MilliSeconds", "Hertz" => "Hertz", "Decibels" => "Decibels", "LinearGain" => "LinearGain", "Pan" => "Pan", "SemiTones" => "SemiTones", _ => "other",
            };
            *units.entry(unit).or_default() += 1;
            enum_units += usize::from(matches!(unit, "Percent"|"PercentNormalized"|"Seconds"|"MilliSeconds"|"Hertz"|"Decibels"|"LinearGain"|"Pan"|"SemiTones"));
        }
    }
    json!({"numeric_widgets":kinds,"authored_labels":labels,"empty_labels":empty_labels,"pictured_labels":pictured_labels,"hidden_values":hidden_values,"display_ratios":ratios,"units":units,"uvi_enum_unit_readouts":enum_units})
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = std::fs::read_to_string(std::env::args_os().nth(1).ok_or("manifest")?)?;
    for line in manifest.lines() {
        let (kind, id) = line.split_once('\t').ok_or("manifest row")?;
        let mut refs = BTreeMap::new();
        let result = if kind == "kontakt" {
            match sampler_kontakt::read(Path::new(id)) {
                Err(_) => json!({"loaded":false,"stage":"read"}),
                Ok(mut loaded) => {
                    for b in &loaded.instrument.behaviors { references(&b.source,&mut refs); }
                    let (scripts, faces, _) = sampler_kontakt::compile_ui(&mut loaded.instrument, &sampler_kontakt::Options { library:Some(id.into()), ..Default::default() });
                    let lost_slider_units = scripts.iter().flat_map(|s| &s.model().interface.widgets).filter(|w| matches!(w.kind,sampler_ksp::model::WidgetKind::Slider) && w.int("$CONTROL_PAR_UNIT").is_some_and(|u| u != 0)).count();
                    json!({"loaded":true,"compiled_scripts":scripts.len(),"source_scripts":loaded.instrument.behaviors.len(),"native_ui":faces.iter().any(|f| f.native_ui.is_some()),"stats":stats(&faces),"source_refs":refs,"slider_unit_metadata_dropped":lost_slider_units})
                }
            }
        } else {
            let (bank, program) = id.split_once("::").ok_or("bank::program")?;
            match sampler_uvi::Bank::open(Path::new(bank)).and_then(|b| b.program(program).map(|(xml,_)| (b,xml))) {
                Err(_) => json!({"loaded":false,"stage":"bank"}),
                Ok((bank,xml)) => match sampler_uvi::script::ScriptHost::new(&xml,bank.scripts(),sampler_uvi::script::Config::default()) {
                    Err(_) => json!({"loaded":false,"stage":"lua"}),
                    Ok(host) => json!({"loaded":true,"stats":stats(&[host.interface()]),"findings":host.findings().iter().map(|f| f.count).sum::<usize>()}),
                },
            }
        };
        println!("{}",json!({"id":id,"kind":kind,"result":result}));
    }
    Ok(())
}

#[test]
fn display_reference_census_excludes_literals_comments_and_prefixes() {
    let mut counts=BTreeMap::new();
    references("{ set_knob_label { $CONTROL_PAR_DISPLAY } } set_knob_label($k,\"$CONTROL_PAR_LABEL\") other_set_knob_unit $CONTROL_PAR_UNIT",&mut counts);
    assert_eq!(counts, BTreeMap::from([("set_knob_label",1),("$CONTROL_PAR_UNIT",1)]));
}
