//! The status ledger. Bindings and catalog names come from the actual host/translator,
//! never from the older corpus inventory's yes/dropped labels.
use sampler_parity::{ConstructKind as K, Status as S};
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub kind: K,
    pub status: S,
    pub owner: String,
    pub spec: String,
    pub detail: String,
    pub binding: String,
}

pub(crate) fn owner(file: &str, anchor: &str) -> String {
    let text = match file {
        "script.rs" => include_str!("../script.rs"),
        "script_prelude.lua" => include_str!("../script_prelude.lua"),
        "engine_parameters.rs" => include_str!("../engine_parameters.rs"),
        "inserts.rs" => include_str!("../inserts.rs"),
        "modulation.rs" => include_str!("../modulation.rs"),
        "script/async_data.rs" => include_str!("../script/async_data.rs"),
        "script/modules.rs" => include_str!("../script/modules.rs"),
        _ => unreachable!(),
    };
    let line = text
        .lines()
        .position(|l| l.contains(anchor))
        .expect("ledger owner anchor")
        + 1;
    format!("crates/sampler-uvi/src/{file}:{line}")
}

fn row(
    name: &str,
    kind: K,
    status: S,
    file: &str,
    anchor: &str,
    detail: &str,
    binding: &str,
) -> Entry {
    Entry {
        name: name.into(),
        kind,
        status,
        owner: owner(file, anchor),
        spec: if matches!(kind, K::LuaModule | K::LuaParameter) {
            "docs/architecture-v2/FALCON_MODULE_COVERAGE.md".into()
        } else {
            "docs/architecture-v2/UVI_LUA.md".into()
        },
        detail: detail.into(),
        binding: binding.into(),
    }
}

pub(crate) const CALLBACKS: &[&str] = &[
    "onInit",
    "onNote",
    "onRelease",
    "onEvent",
    "onController",
    "onPitchBend",
    "onAfterTouch",
    "onPolyAfterTouch",
    "onProgramChange",
    "onTransport",
    "onSave",
    "onLoad",
];
pub(crate) fn is_callback(name: &str) -> bool {
    CALLBACKS.contains(&name)
}

pub(crate) const WIDGETS: &[&str] = &[
    "Panel",
    "Viewport",
    "Knob",
    "Slider",
    "OnOffButton",
    "Button",
    "Menu",
    "MultiStateButton",
    "NumBox",
    "Label",
    "Image",
    "SVG",
    "Table",
    "AudioMeter",
    "XY",
    "WaveView",
    "WaveForm",
    "FileSelector",
    "DnDArea",
    "Text",
    "Frame",
    "ParamKnob",
    "ParamSlider",
    "ParamOnOffButton",
    "ParamMenu",
    "ParamNumBox",
    "ParameterValue",
];

/// One authoritative table for reports and the generated ledger. Every catalog
/// parameter has separate read/write entries: successful readback is not DSP parity.
pub fn entries() -> &'static BTreeMap<String, Entry> {
    static TABLE: OnceLock<BTreeMap<String, Entry>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut out = BTreeMap::new();
        let mut add = |e: Entry| { out.insert(e.name.clone(), e); };
        for name in CALLBACKS {
            add(row(name, K::Callback, S::Implemented, "script.rs", "fn call(&self", "Dispatched by the host; onEvent takes precedence for MIDI events.", "host callback coroutine"));
        }
        for name in WIDGETS {
            add(row(name, K::LuaApi, S::Approximate, "script_prelude.lua", "widget = function", "Authored widget model exported to UI IR; native rendering/interaction parity unverified.", &format!("UI widget {name} -> registry / sampler-ui-ir")));
        }
        for name in ["Event", "Unit", "Mapper", "FileFormat", "__API_VERSION__", "bit", "Program"] {
            let anchor = if name == "Program" { "fn build_program" } else { name };
            add(row(name, K::LuaApi, S::Approximate, if name == "Program" {"script.rs"} else {"script_prelude.lua"}, anchor, "Host constants / retained element tree; complete native surface unverified.", "host global"));
        }
        add(row("Engine", K::LuaApi, S::Stub, "script_prelude.lua", "Engine =", "Engine members return inert stand-ins.", "unbound Engine stand-in"));
        for name in ["setParameter", "getParameter", "hasParameter", "getParameterConnections"] {
            add(row(name, K::LuaApi, S::Approximate, "script_prelude.lua", &format!("function element.{name}"), "Catalog validation/readback; write status depends on the concrete module and DSP binding.", "element catalog / native parameter service"));
        }
        for name in ["setValue", "getValue", "push", "setRange", "setPosition", "setSize", "setItem", "addItem", "clear", "getText", "setStripImage", "setViewPosition", "loadFont", "toString", "setValueNormalized", "getValueNormalized"] {
            add(row(name, K::LuaApi, S::Approximate, "script_prelude.lua", &format!("function methods.{name}"), "Mutates/reads the authored widget model; native UI semantics unverified.", "UI widget method"));
        }
        for name in ["setBackground", "setHeight", "setBackgroundColour", "setKeyColour", "resetKeyColour", "makePerformanceView", "findLayer", "type", "require", "postEvent", "postMidiEvent"] {
            let anchor = if name == "postMidiEvent" { "postMidiEvent =" } else { name };
            add(row(name, K::LuaApi, S::Approximate, "script_prelude.lua", anchor, "Host prelude implementation; native host equivalence not measured.", "prelude host binding"));
        }
        for (name, anchor) in [("wait", "function wait("), ("waitBeat", "function waitBeat("), ("waitForRelease", "function waitForRelease(")] {
            add(row(name, K::LuaApi, S::Implemented, "script_prelude.lua", anchor, "Coroutine suspension/resumption follows the host clock and note release; focused host tests.", "coroutine yield -> host waiting queue"));
        }
        add(row("print", K::LuaApi, S::Stub, "script_prelude.lua", "function print()", "Printing is intentionally discarded.", "no output"));
        for name in ["getTime", "playNote", "releaseVoice", "sendScriptModulation", "spawn", "run", "getTempo", "getBeatDuration", "getSamplingRate", "getBeatTime", "getRunningBeatTime", "getNoteDuration", "isKeyDown", "isOctaveKeyDown", "isNoteHeld", "getCC", "beat2ms", "ms2beat", "ms2samples", "samples2ms", "changeVolume", "changeVolumedB", "changePan", "changeTune", "fadein", "fadeout", "fade", "fade2", "sendScriptModulation2", "controlChange", "programChange", "pitchBend", "afterTouch", "polyAfterTouch"] {
            add(row(name, K::LuaApi, S::Approximate, "script.rs", &format!("\"{name}\""), "Bound to host state/command queue; complete native timing and edge semantics unverified.", &format!("host {name} -> state / command queue")));
        }
        for name in ["getTimeSignature", "getBarDuration"] {
            add(row(name, K::LuaApi, S::Approximate, "script.rs", &format!("\"{name}\""), "Fixed 4/4 time signature.", "fixed meter / tempo conversion"));
        }
        add(row("loadData", K::LuaApi, S::Approximate, "script/async_data.rs", "pub(super) fn install", "Bounded asynchronous filesystem JSON loading; other native formats unavailable.", "loadData worker -> JSON decode -> completion coroutine"));
        add(row("loadImpulse", K::LuaApi, S::Approximate, "script.rs", "\"loadImpulse\"", "Requires a supplied impulse resolver and a translated bus convolution slot; task can fail.", "impulse resolver -> convolution upload"));
        add(row("loadSample", K::LuaApi, S::Unsupported, "script.rs", "for name in [\"loadSample\"]", "Resource task unavailable; callback receives nil.", "failed resource task"));
        for name in ["class", "AsyncUpdater"] {
            add(row(name, K::LuaApi, S::Approximate, "script/modules.rs", if name == "class" { "fn install_class" } else { "struct AsyncUpdater" }, "Host class/module implementation; full native behavior unverified.", "host class / module"));
        }
        add(row("table.copy", K::LuaApi, S::Approximate, "script_prelude.lua", "_G.table.copy", "Shallow table copy.", "table shallow copy"));
        add(row("table.print", K::LuaApi, S::Stub, "script_prelude.lua", "_G.table.print", "Printing is discarded.", "no output"));
        add(row("Part", K::LuaModule, S::Stub, "script.rs", "let part_xml", "Retained synthetic parent context; no live Part DSP writes.", "synthetic Part element"));
        for name in ["Name", "Gain", "Pan", "Bypass"] {
            add(row(&format!("getParameter Part.{name}"), K::LuaParameter, S::Approximate, "script.rs", "let part_xml", "Reads the retained synthetic Part context.", "synthetic Part readback"));
            add(row(&format!("setParameter Part.{name}"), K::LuaParameter, S::Stub, "script.rs", "\"setParam\"", "Part fields have no DSP write binding.", "unbound write"));
        }
        let modules: std::collections::BTreeSet<_> = include_str!("../parameters.tsv").lines().filter_map(|l| l.split('\t').next()).collect();
        for module in modules {
            let (file, anchor, supported) = match module {
                "Program" | "Layer" | "Keygroup" | "SamplePlayer" | "AuxEffect" | "ScriptProcessor" => ("script.rs", "fn build_program", true),
                "Gain" | "GainMatrix" | "DigitalEq" | "ThreeBandShelves" | "WaveShaper" | "CompExp" | "OnePole" | "TrackDelay" | "EffectRack" | "Convolver" | "SampledReverb" => ("inserts.rs", "fn inserts", true),
                "ConstantModulation" | "ScriptEventModulation" | "DAHDSR" | "AHD" | "LFO" | "MultiLFO" | "StepEnvelope" => ("modulation.rs", "fn signal", true),
                _ => ("inserts.rs", "fn inserts", false),
            };
            add(row(module, K::LuaModule, if supported {S::Approximate} else {S::Unsupported}, file, anchor,
                if supported { "Partial translation; concrete scope/mode/connection can still be dropped. Native laws unverified." } else { "Catalog exists; module has no modeled DSP implementation." },
                if supported { "host element / partial IR translation" } else { "unbound module" }));
            for p in crate::script::parameters::definitions(module) {
                let live = !crate::engine_parameters::lanes(module, p.name).is_empty()
                    || p.name == "Bypass" && matches!(module, "Gain" | "GainMatrix" | "DigitalEq" | "ThreeBandShelves" | "CompExp" | "OnePole" | "TrackDelay" | "EffectRack" | "WaveShaper" | "Convolver" | "SampledReverb" | "AuxEffect")
                    || matches!((module, p.name), ("Program" | "Layer" | "Keygroup" | "SamplePlayer", "Gain" | "Pan") | ("SamplePlayer", "Pitch") | ("Program", "Polyphony") | ("ConstantModulation", "Value" | "Style"))
                    || matches!(module, "Convolver" | "SampledReverb") && crate::script::IMPULSE_FIELDS.contains(&p.name);
                add(row(&format!("setParameter {module}.{}", p.name), K::LuaParameter, if live {S::Approximate} else {S::Stub}, "script.rs", "\"setParam\"",
                    if live { "Potential DSP binding; concrete node scope/layout and resource availability must be checked. Native law unverified." } else { "No live DSP write; catalog/retained readback is not an implementation." },
                    if live { "conditional DSP command" } else { "unbound write" }));
                add(row(&format!("getParameter {module}.{}", p.name), K::LuaParameter, S::Approximate, "script.rs", "\"param\"", "Returns overlay, retained XML or catalog default; no DSP readback.", "parameter overlay / XML / catalog default"));
            }
        }
        out
    })
}

pub(crate) fn entry(name: &str) -> Option<&'static Entry> {
    entries().get(name)
}

/// Deterministic markdown, generated from `entries()`; no library content.
pub fn markdown() -> String {
    let mut out = String::from(
        "# UVI parity ledger\n\nGenerated by `cargo run -p sampler-uvi --example parity_summary -- --ledger` from `sampler_uvi::parity::ledger::entries()`. Do not edit rows by hand.\n\nStatuses describe host bindings, not a claim of measured native DSP equivalence. Conditional writes are downgraded to Stub in a report when no concrete DSP binding exists. The older Falcon module coverage document supplies the catalog/spec inventory, not current implementation verdicts.\n\nRuntime inspection: `ScriptHost::new_inspected` compiles Luau with real statement-line coverage (level 1). Ordinary constructors compile without coverage instructions or retained coverage functions. Inspection lasts for that host lifetime; create an ordinary host to turn it off. `coverage()` returns cumulative snapshots keyed by script id and line-start byte offsets. Luau counters saturate at their VM instruction limit; faults retain at most 256 messages of 1024 characters per script.\n\nReports: `sampler_uvi::parity_reports(&xml)` (valid loaded program XML), `parity::bank_reports(&bank, program_path)` and `Translated::parity_reports()`. Required embedded modules are included by the bank/translated entry points. Reports never hold source text or literal argument values.\n\nStatic resolution covers literal element collections, parent links, findLayer and straight-line aliases. Dynamic indices, control flow and computed parameter names remain explicitly unresolved. A lowering describes the binding, not proof that the call executed; real hits live in Coverage.\n\n| Construct | Status | Owner | Spec | Detail | Binding |\n|---|---|---|---|---|---|\n",
    );
    for e in entries().values() {
        let clean = |s: &str| s.replace('|', "\\|").replace('\n', " ");
        out.push_str(&format!(
            "| {} | {:?} | {} | {} | {} | {} |\n",
            clean(&e.name),
            e.status,
            e.owner,
            e.spec,
            clean(&e.detail),
            clean(&e.binding)
        ));
    }
    out
}

use super::{Target, construct, kind_name};
use sampler_parity::{Construct, Span};

pub(super) fn parameter(
    target: Option<Target>,
    operation: &str,
    name: Option<&str>,
    span: Span,
    lowered: Option<&crate::FullTranslation>,
) -> (Construct, String) {
    let Some((target, name)) = target.zip(name) else {
        let mut c = construct(span, entry(operation).unwrap());
        c.name = format!(
            "{operation} {}.{}",
            target.map(kind_name).unwrap_or("<dynamic module>"),
            name.unwrap_or("<dynamic parameter>")
        );
        c.kind = K::LuaParameter;
        c.status = if operation == "setParameter" {
            S::Stub
        } else {
            S::Approximate
        };
        c.detail =
            "Static binding unresolved; inspect runtime target before claiming a DSP write.".into();
        return (c, "binding unresolved (dynamic target or parameter)".into());
    };
    let kind = kind_name(target);
    let canonical = format!("{operation} {kind}.{name}");
    let mut c = entry(&canonical)
        .map(|e| construct(span, e))
        .unwrap_or_else(|| {
            let mut c = construct(span, entry(operation).unwrap());
            c.name = canonical;
            c.kind = K::LuaParameter;
            c.status = if operation == "setParameter" {
                S::Stub
            } else {
                S::Approximate
            };
            c.detail = "Retained XML field or unknown parameter; no live DSP write.".into();
            c
        });
    if operation != "setParameter" {
        let known = crate::script::parameters::definitions(kind)
            .iter()
            .any(|p| p.name == name)
            || matches!(target, Target::Node(n) if n.attribute(name).is_some())
            || matches!(target, Target::Part) && matches!(name, "Name" | "Gain" | "Pan" | "Bypass");
        if !known {
            c.status = S::Stub;
            c.detail = "Unknown parameter returns a reported zero.".into();
        }
        return (
            c,
            format!("{operation} {kind}.{name} -> overlay / XML / catalog read"),
        );
    }
    let live = match target {
        Target::Node(node) => crate::engine_parameters::live_names(node),
        Target::Part => Vec::new(),
    };
    let mut binding = match target {
        Target::Node(node) if live.contains(&"Bypass") && name == "Bypass" => Some(format!(
            "core slot-bypass write node {} (group -1, slot -1)",
            node.id().get_usize()
        )),
        Target::Node(node)
            if kind == "AuxEffect" && name == "Bypass" && live.contains(&"BypassGain") =>
        {
            Some(format!(
                "rack chain gain write node {}",
                node.id().get_usize()
            ))
        }
        Target::Node(node) if kind == "ConstantModulation" && live.contains(&name) => {
            Some(format!(
                "ScriptLive modulation command node {}",
                node.id().get_usize()
            ))
        }
        Target::Node(node)
            if matches!(kind, "Convolver" | "SampledReverb")
                && crate::script::IMPULSE_FIELDS.contains(&name)
                && crate::engine_parameters::bus_scope(node) =>
        {
            Some(format!(
                "convolution rebuild node {} (requires supplied slot/resolver)",
                node.id().get_usize()
            ))
        }
        Target::Node(node) if live.contains(&name) => {
            crate::engine_parameters::binding(node.id().get_usize(), kind, name)
                .map(|b| format!("core engine-parameter write {:?}", b.address))
        }
        _ if matches!(
            (kind, name),
            (
                "Program" | "Layer" | "Keygroup" | "SamplePlayer",
                "Gain" | "Pan"
            ) | ("SamplePlayer", "Pitch")
                | ("Program", "Polyphony")
        ) =>
        {
            Some(format!("scoped Command::Parameter {kind}.{name}"))
        }
        _ => None,
    };
    if let (Target::Node(node), Some((ir, _, _, inserts))) = (target, lowered) {
        let id = node.id().get_usize();
        if node
            .parent()
            .is_some_and(|p| p.has_tag_name("Inserts") || p.has_tag_name("Chains"))
        {
            let placed = inserts.iter().any(|i| i.node == id && i.count > 0);
            let actual = if name == "Bypass" && live.contains(&"Bypass") {
                ir.chains.iter().flat_map(|c| c.pre_amplitude.iter().chain(&c.post_amplitude))
                    .any(|p| matches!(p, sampler_ir::Processor::Mix {address,..} if address.generic == id as i32))
            } else if matches!(kind, "Convolver" | "SampledReverb")
                && crate::script::IMPULSE_FIELDS.contains(&name)
            {
                placed
            } else {
                // Bandwidth writes through the Q lane, and GainScale through OverallGain.
                let (effective, _) = crate::engine_parameters::effective(kind, name, 0., |_| None);
                ir.controls
                    .iter()
                    .any(|c| c.key == format!("uvi:insert:{id}:{kind}:{effective}"))
            };
            if !placed || !actual {
                binding = None;
            }
        }
    }
    if let Some(binding) = binding {
        // Live host controls are still approximations until native laws are measured.
        c.status = S::Approximate;
        (c, format!("{operation} {kind}.{name} -> {binding}"))
    } else {
        c.status = S::Stub;
        c.detail = if kind == "GainMatrix" {
            "Unbound write: the core processes only the first stereo pair."
        } else {
            "No live DSP binding for this concrete node/scope; readback is not DSP acceptance."
        }
        .into();
        (
            c,
            format!(
                "{operation} {kind}.{name} -> unbound{}",
                if kind == "GainMatrix" {
                    " (stereo core)"
                } else {
                    ""
                }
            ),
        )
    }
}

pub(super) fn unknown(span: Span, name: &str, engine: bool) -> Construct {
    let mut c = construct(span, entry("Engine").unwrap());
    c.name = if engine {
        format!("Engine.{name}")
    } else {
        name.into()
    };
    c.status = if engine { S::Stub } else { S::Unsupported };
    if !engine {
        c.owner = Some(owner("script_prelude.lua", "setmetatable(_G"));
    }
    c.detail = if engine {
        "Inert Engine stand-in."
    } else {
        "Unknown host API; global lookup returns nil."
    }
    .into();
    c
}

pub(super) fn module_reference(span: Span, kind: &str) -> Construct {
    let mut c = entry(kind)
        .map(|e| construct(span, e))
        .unwrap_or_else(|| unknown(span, kind, false));
    c.kind = K::LuaModule;
    c
}
