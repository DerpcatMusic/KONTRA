//! Read-only census: emits metadata and command counts, never library source.
#![cfg(feature = "library-access")]
use sampler_uvi::{
    Bank,
    script::{Command, Config, Files, ScriptHost, Scripts},
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    io::Write,
    rc::Rc,
    time::{Duration, Instant},
};

struct Observed {
    scripts: Rc<Scripts>,
    symbols: RefCell<BTreeSet<String>>,
}
impl Files for Observed {
    fn script(&self, name: &str) -> Option<String> {
        let source = self.scripts.script(name)?;
        let names = [
            "onNote",
            "onRelease",
            "onController",
            "onLoad",
            "onSave",
            "playNote",
            "postEvent",
            "releaseVoice",
            "fadein",
            "fadeout",
            "fade2",
            "changeTune",
            "changeVolume",
            "setSampleOffset",
            "wait",
            "waitBeat",
            "waitForRelease",
            "spawn",
            "run",
            "isNoteHeld",
            "getBeatTime",
            "getRunningBeatTime",
            "getParameter",
            "setParameter",
            "parameterDefinitions",
            "getParameterConnections",
            "loadSample",
            "loadImpulse",
            "loadData",
            "saveState",
            "loadState",
            "persistent",
            "Table",
            "synthChildren",
            "eventProcessors",
            "sampleInfo",
            "uvi.ChordRec",
            "bit.band",
            "getfenv",
            "setfenv",
            "math.pow",
            "table.maxn",
            "newproxy",
        ];
        self.symbols.borrow_mut().extend(
            names
                .into_iter()
                .filter(|n| source.contains(n))
                .map(str::to_owned),
        );
        Some(source)
    }
}

fn plays(commands: &[Command]) -> usize {
    commands
        .iter()
        .filter(|c| matches!(c, Command::Play(_)))
        .count()
}

#[test]
#[ignore = "read-only installed library survey; run serially through kontakto-heavy"]
fn corpus_script_commands_and_faults() {
    let path = std::env::var("UVI_AUDIT_CORPUS").unwrap();
    let mut rows = BTreeMap::new();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let r: serde_json::Value = serde_json::from_str(line).unwrap();
        if r["kind"] == "uvi-program" && r["status"] == "done" {
            rows.insert(r["id"].as_str().unwrap().to_owned(), r);
        }
    }
    let only = std::env::var("UVI_AUDIT_ONLY").unwrap_or_default();
    let cache = std::env::var("UVI_AUDIT_CACHE").ok();
    let completed: BTreeSet<String> = cache
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|r| r["id"].as_str().map(str::to_owned))
        .collect();
    let started = Instant::now();
    let budget = Duration::from_secs(
        std::env::var("UVI_AUDIT_SECONDS")
            .unwrap_or_else(|_| "240".into())
            .parse()
            .unwrap(),
    );
    let zero_only = std::env::var_os("UVI_AUDIT_ZERO_ONLY").is_some();
    let structure = std::env::var_os("UVI_AUDIT_STRUCTURE").is_some();
    let mut open: Option<(String, Bank, Option<Rc<Scripts>>)> = None;
    for (id, r) in rows {
        if !id.contains(&only) {
            continue;
        }
        if completed.contains(&id) || (zero_only && r["sound"]["perf"]["peak_voices"] != 0) {
            continue;
        }
        if started.elapsed() >= budget {
            break;
        }
        let (file, member) = id.split_once("::").unwrap();
        if open.as_ref().is_none_or(|(p, _, _)| p != file) {
            let bank = Bank::open(std::path::Path::new(file)).unwrap();
            let scripts = (!structure).then(|| Rc::new(bank.scripts()));
            open = Some((file.to_owned(), bank, scripts));
        }
        let bank = &open.as_ref().unwrap().1;
        let (xml, _) = bank.program(member).unwrap();
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let state_keys: usize = doc
            .descendants()
            .filter(|n| n.has_tag_name("ScriptData"))
            .map(|n| n.attributes().len())
            .sum();
        let zero_gains: usize = doc
            .descendants()
            .filter(|n| n.attribute("Gain").and_then(|v| v.parse::<f64>().ok()) == Some(0.0))
            .count();
        let processor_count = doc
            .descendants()
            .filter(|n| n.has_tag_name("ScriptProcessor"))
            .count();
        let key = r["load"]["key"].as_u64().unwrap_or(60) as u8;
        let vel = r["load"]["velocity"].as_u64().unwrap_or(64) as u8;
        if structure {
            let ir = sampler_uvi::translate_program(bank, member).unwrap();
            let matching: Vec<_> = ir
                .zones
                .iter()
                .filter(|z| {
                    z.keys.low <= key
                        && key <= z.keys.high
                        && z.velocities.low <= vel
                        && vel <= z.velocities.high
                })
                .filter_map(|z| z.group.map(|g| g.0))
                .collect();
            let metadata = serde_json::json!({"id":id,"zero":r["sound"]["perf"]["peak_voices"]==0,"key":key,"velocity":vel,"state_keys":state_keys,"zero_gains":zero_gains,"processor_count":processor_count,"ir_zones":ir.zones.len(),"ir_matching":matching.len(),"ir_groups":matching.iter().map(|g|&ir.groups[*g].name).collect::<BTreeSet<_>>(),"layers":doc.descendants().filter(|n|n.has_tag_name("Layer")).take(8).map(|l|serde_json::json!({"name":l.attribute("Name"),"mute":l.attribute("Mute"),"gain":l.attribute("Gain"),"low":l.attribute("LowKey"),"high":l.attribute("HighKey")})).collect::<Vec<_>>()});
            if let Some(path) = &cache {
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .unwrap();
                writeln!(file, "{metadata}").unwrap();
            }
            println!("UVI-AUDIT {metadata}");
            continue;
        }
        let observed = Rc::new(Observed {
            scripts: open.as_ref().unwrap().2.as_ref().unwrap().clone(),
            symbols: RefCell::new(BTreeSet::new()),
        });
        // Authored instrumentation wraps only initialization, to read back the
        // layer overlays after the original callback. Vendor text stays in RAM.
        let mut probed_xml = xml.clone();
        if let Some(script) = doc.descendants().find(|n| n.has_tag_name("script")) {
            let source = script.text().unwrap_or_default();
            let probe = "\nlocal audit_original_init = onInit; function onInit() if type(audit_original_init) == 'function' then audit_original_init() end; audit_layer_state = ''; for i,l in ipairs(Program.layers) do audit_layer_state = audit_layer_state .. i .. ':' .. tostring(l:getParameter('Mute')) .. ':' .. tostring(l:getParameter('Gain')) .. ';' end end";
            probed_xml.replace_range(
                script.range(),
                &format!("<script><![CDATA[{source}{probe}]]></script>"),
            );
        }
        let mut h = ScriptHost::new(&probed_xml, observed.clone(), Config::default()).unwrap();
        let load_errors: Vec<_> = h
            .findings()
            .into_iter()
            .filter(|f| f.feature.contains("error"))
            .map(|f| f.value)
            .collect();
        let load_commands = h.take_commands();
        h.note_on(1, key, vel, 0);
        h.advance(1000.0);
        let note_commands = h.take_commands();
        let mut kinds = BTreeMap::new();
        for f in h
            .findings()
            .into_iter()
            .filter(|f| f.feature.contains("error"))
        {
            // Errors contain messages/stack locations, never print Lua source.
            kinds.insert(f.feature, f.value);
        }
        let mut globals = BTreeMap::new();
        for n in [
            "minNote",
            "maxNote",
            "ccVel",
            "hornModel",
            "lastNote",
            "lastKeyboardNote",
            "isNoteOn",
        ] {
            let v = h.global_text(n);
            if v == "nil" || v == "true" || v == "false" || v.parse::<f64>().is_ok() {
                globals.insert(n, v);
            }
        }
        h.note_off(1, key, 64, 0);
        h.advance(2000.0);
        h.take_commands();
        for (cc, value) in [(1, 100), (2, 100), (11, 127)] {
            h.controller(cc, value, 0);
        }
        h.note_on(2, key, vel, 0);
        h.advance(3000.0);
        let cc_commands = h.take_commands();
        let matching = |p: &sampler_uvi::script::Play| {
            let bound = |n: roxmltree::Node, a: &str, default: u8| {
                n.attribute(a)
                    .and_then(|v| v.parse::<u8>().ok())
                    .unwrap_or(default)
            };
            doc.descendants()
                .filter(|n| n.has_tag_name("Layer"))
                .enumerate()
                .filter(|(i, l)| {
                    (p.layers.is_empty() || p.layers.contains(*i as u32 + 1))
                        && p.key >= bound(*l, "LowKey", 0)
                        && p.key <= bound(*l, "HighKey", 127)
                })
                .flat_map(|(_, l)| l.descendants().filter(|n| n.has_tag_name("Keygroup")))
                .filter(|k| {
                    p.key >= bound(*k, "LowKey", 0)
                        && p.key <= bound(*k, "HighKey", 127)
                        && p.velocity >= bound(*k, "LowVelocity", 1)
                        && p.velocity <= bound(*k, "HighVelocity", 127)
                })
                .flat_map(|k| {
                    k.descendants()
                        .filter(|n| n.has_tag_name("SamplePlayer"))
                        .enumerate()
                        .filter(|(i, n)| {
                            p.osc.is_none_or(|o| o == *i as u32) && bound(*n, "Bypass", 0) == 0
                        })
                })
                .count()
        };
        let metadata = serde_json::json!({"id":id,"zero":r["sound"]["perf"]["peak_voices"]==0,"key":key,"velocity":vel,"state_keys":state_keys,"zero_gains":zero_gains,"processor_count":processor_count,"layer_count":doc.descendants().filter(|n| n.has_tag_name("Layer")).count(),"loaded_layers":h.global_text("audit_layer_state"),"load_plays":plays(&load_commands),"note_plays":plays(&note_commands),"cc_plays":plays(&cc_commands),"load_errors":load_errors,"errors":kinds,"globals":globals,"symbols":*observed.symbols.borrow(),"plays":note_commands.iter().filter_map(|c| if let Command::Play(p)=c {Some(serde_json::json!({"id":p.id,"parent":p.parent,"key":p.key,"velocity":p.velocity,"osc":p.osc,"layers":p.layers.0,"duration":p.duration_ms,"xml_candidates":matching(p)}))} else {None}).collect::<Vec<_>>()});
        if let Some(path) = &cache {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap();
            writeln!(file, "{metadata}").unwrap();
        }
        println!("UVI-AUDIT {metadata}");
    }
}
