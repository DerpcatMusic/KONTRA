//! Read-only census: emits metadata and command counts, never library source.
#![cfg(feature = "library-access")]
use sampler_uvi::{
    Bank,
    script::{Command, Config, Files, ScriptHost, Scripts},
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

struct Observed {
    scripts: Scripts,
    symbols: RefCell<BTreeSet<String>>,
}
impl Files for Observed {
    fn script(&self, name: &str) -> Option<String> {
        let source = self.scripts.script(name)?;
        let names = [
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
    let mut open: Option<(String, Bank)> = None;
    for (id, r) in rows {
        if !id.contains(&only) {
            continue;
        }
        let (file, member) = id.split_once("::").unwrap();
        if open.as_ref().is_none_or(|(p, _)| p != file) {
            open = Some((file.to_owned(), Bank::open(file.as_ref()).unwrap()));
        }
        let bank = &open.as_ref().unwrap().1;
        let (xml, _) = bank.program(member).unwrap();
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let state_keys: usize = doc
            .descendants()
            .filter(|n| n.has_tag_name("ScriptData"))
            .map(|n| n.attributes().len())
            .sum();
        let observed = Rc::new(Observed {
            scripts: bank.scripts(),
            symbols: RefCell::new(BTreeSet::new()),
        });
        let mut h = ScriptHost::new(&xml, observed.clone(), Config::default()).unwrap();
        let load_errors: Vec<_> = h
            .findings()
            .into_iter()
            .filter(|f| f.feature.contains("error"))
            .map(|f| f.value)
            .collect();
        let load_commands = h.take_commands();
        let key = r["load"]["key"].as_u64().unwrap_or(60) as u8;
        let vel = r["load"]["velocity"].as_u64().unwrap_or(64) as u8;
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
        let metadata = serde_json::json!({"id":id,"zero":r["sound"]["perf"]["peak_voices"]==0,"key":key,"velocity":vel,"state_keys":state_keys,"load_plays":plays(&load_commands),"note_plays":plays(&note_commands),"cc_plays":plays(&cc_commands),"load_errors":load_errors,"errors":kinds,"globals":globals,"symbols":*observed.symbols.borrow(),"plays":note_commands.iter().filter_map(|c| if let Command::Play(p)=c {Some(serde_json::json!({"id":p.id,"parent":p.parent,"key":p.key,"osc":p.osc,"layers":p.layers.0,"duration":p.duration_ms}))} else {None}).collect::<Vec<_>>()});
        println!("UVI-AUDIT {metadata}");
    }
}
