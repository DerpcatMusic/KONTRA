//! Bounded local Lua failure context. These types deliberately do not serialize:
//! loaded commercial source may be inspected locally, never copied into reports.
use super::{
    host,
    program::{NodeId, Program},
};
use std::{collections::BTreeMap, ffi::CStr, sync::Arc};

#[derive(Clone)]
pub(crate) struct Context {
    pub processor: Option<NodeId>,
    pub frame: u64,
    pub line: Option<u32>,
    pub chunk: String,
    pub excerpt: Option<serde_json::Value>,
    pub unavailable: &'static str,
    pub display: Arc<str>,
    pub provenance: &'static str,
}
impl Context {
    /// Explicit metadata whitelist; local source bytes/display never serialize.
    pub(crate) fn metadata(&self) -> serde_json::Value {
        serde_json::json!({"source_kind":"lua","processor":self.processor,"frame":self.frame,
            "chunk":self.chunk,"line":self.line,"local_source_excerpt_available":self.excerpt.is_some(),
            "source_provenance":self.provenance,
            "source_excerpt_unavailable":if self.excerpt.is_some(){None}else{Some(self.unavailable)}})
    }
}
impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalLuaContext")
            .field("processor", &self.processor)
            .field("frame", &self.frame)
            .field("line", &self.line)
            .finish_non_exhaustive()
    }
}
#[derive(Debug)]
struct Failure {
    cause: mlua::Error,
    context: Context,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.cause.fmt(f)
    }
}
impl std::error::Error for Failure {}
// Lua 5.1 llex.c::inclinenumber consumes one CR/LF and then an immediately
// opposite byte. Same-byte runs and triples therefore remain separate lines.
fn source_lines(source: &str) -> impl Iterator<Item = &str> {
    let mut remaining = Some(source);
    std::iter::from_fn(move || {
        let source = remaining.take()?;
        if let Some(at) = source.find(['\r', '\n']) {
            let newline = source.as_bytes()[at];
            let mut tail = &source[at + 1..];
            if tail.as_bytes().first().is_some_and(|&next| {
                matches!(next, b'\r' | b'\n') && next != newline
            }) {
                tail = &tail[1..];
            }
            remaining = Some(tail);
            Some(&source[..at])
        } else {
            Some(source)
        }
    })
}

fn excerpt(source: &str, line: u32) -> Option<serde_json::Value> {
    if line == 0 { return None; }
    use std::fmt::Write;
    let first_line = line.saturating_sub(2).max(1);
    let mut last_line = 0;
    let mut truncated = false;
    let mut text = String::new();
    for (index, source_line) in source_lines(source).enumerate() {
        let at = index as u32 + 1;
        if at > line.saturating_add(2) { break; }
        if at < first_line { continue; }
        let mut rendered = String::new();
        let mut clipped = false;
        for ch in source_line.chars() {
            if rendered.len() + ch.len_utf8() > 512 {
                clipped = true;
                break;
            }
            rendered.push(if ch.is_control() && ch != '\t' { '?' } else { ch });
        }
        truncated |= clipped;
        let _ = writeln!(text, "{} {at:>6} | {rendered}{}",
            if at == line { ">" } else { " " }, if clipped { "…" } else { "" });
        last_line = at;
    }
    (last_line >= line).then(|| serde_json::json!({
        "origin":"already loaded local UVI Lua source", "source_kind":"lua",
        "line":line,"column":null,"first_line":first_line,"last_line":last_line,
        "truncated":truncated,"text":text
    }))
}
/// Preserve the known initialization entry without guessing its failing frame.
/// Nested module failures may originate elsewhere; no source line is assigned.
pub(crate) fn initialization(
    cause: mlua::Error,
    processor: Option<NodeId>,
    frame: u64,
    chunk: &str,
) -> mlua::Error {
    if cause.downcast_ref::<Failure>().is_some() {
        return cause;
    }
    let chunk = if chunk.len() <= 512 {
        chunk
    } else {
        "unavailable"
    };
    mlua::Error::ExternalError(Arc::new(Failure {
        cause,
        context: Context {
            processor,
            frame,
            line: None,
            chunk: chunk.into(),
            excerpt: None,
            unavailable: "Initialization entry chunk is known; failing Lua source frame is unavailable",
            display: Arc::from(""),
            provenance: "initialization_entry_chunk",
        },
    }))
}

/// Called only after failed resume, on the sole owning worker, while Thread
/// and its Lua state are alive. Sl reads debug fields without pushing values.
/// No error text/traceback is parsed, and no hook runs on successful callbacks.
pub(crate) fn capture(
    cause: mlua::Error,
    thread: &mlua::Thread,
    processor: Option<NodeId>,
    frame: u64,
    modules: Option<&BTreeMap<String, Vec<u8>>>,
) -> mlua::Error {
    if cause.downcast_ref::<Failure>().is_some() {
        return cause;
    }
    let mut context = Context {
        processor,
        frame,
        line: None,
        chunk: "unavailable".into(),
        excerpt: None,
        unavailable: "Lua coroutine source/line is unavailable",
        display: Arc::from(""),
        provenance: "unavailable",
    };
    if thread.is_error() {
        // SAFETY: Lua/Thread are owned by this worker (no send feature). Calls
        // cannot execute Lua or invoke user code. Debug pointers are read only
        // before Thread drops; the walk is bounded independently of Lua depth.
        unsafe {
            for level in 0..64 {
                let mut debug = std::mem::zeroed::<mlua::ffi::lua_Debug>();
                if mlua::ffi::lua_getstack(thread.state(), level, &mut debug) == 0 {
                    break;
                }
                if mlua::ffi::lua_getinfo(thread.state(), c"Sl".as_ptr(), &mut debug) == 0
                    || debug.currentline <= 0
                {
                    continue;
                }
                context.line = Some(debug.currentline as u32);
                // Unnamed chunks can contain their entire source in Debug.source.
                // Never retain that string or guess an outer caller's source.
                let source =
                    (!debug.source.is_null()).then(|| CStr::from_ptr(debug.source).to_bytes());
                let source = source
                    .filter(|source| source.len() <= 512)
                    .and_then(|source| std::str::from_utf8(source).ok());
                context.provenance = "structured_coroutine_frame";
                registered_source(&mut context, source, modules);
                break;
            }
        }
    }
    mlua::Error::ExternalError(Arc::new(Failure { cause, context }))
}
fn registered_source(
    context: &mut Context,
    source: Option<&str>,
    modules: Option<&BTreeMap<String, Vec<u8>>>,
) {
    match source {
        Some(source)
            if source
                .strip_prefix("UVI ScriptProcessor node ")
                .and_then(|n| n.parse::<NodeId>().ok())
                == context.processor
                && context.processor.is_some() =>
        {
            context.chunk = source.into();
            context.unavailable = "Loaded processor source is unavailable";
        }
        Some(source) if source.starts_with("embedded module ") => {
            let name = &source["embedded module ".len()..];
            if let Some(bytes) =
                modules.and_then(|modules| host::resolve_module(modules, name).ok())
            {
                context.chunk = source.into();
                context.excerpt = std::str::from_utf8(bytes)
                    .ok()
                    .and_then(|source| excerpt(source, context.line.unwrap()));
                context.unavailable = "Loaded module source or reported line is unavailable";
            } else {
                context.unavailable = "Lua module is not in the loaded approved registry";
            }
        }
        _ => context.unavailable = "Lua chunk is not registered for this callback processor",
    }
}

/// Called only in the existing count hook's exhausted-budget branch, before
/// protected module calls unwind. Never executed for ordinary failures.
pub(crate) fn budget(
    cause: mlua::Error,
    debug: &mlua::debug::Debug,
    processor: Option<NodeId>,
    frame: u64,
    modules: Option<&BTreeMap<String, Vec<u8>>>,
) -> mlua::Error {
    let mut context = Context {
        processor,
        frame,
        line: debug
            .current_line()
            .and_then(|line| u32::try_from(line).ok())
            .filter(|line| *line > 0),
        chunk: "unavailable".into(),
        excerpt: None,
        unavailable: "Instruction budget source/line is unavailable",
        display: Arc::from(""),
        provenance: "existing_instruction_budget_hook",
    };
    if context.line.is_some() {
        let source = debug.source();
        registered_source(
            &mut context,
            source
                .source
                .as_deref()
                .filter(|source| source.len() <= 512),
            modules,
        );
    }
    mlua::Error::ExternalError(Arc::new(Failure { cause, context }))
}

pub(crate) fn from_error(error: &anyhow::Error, program: &Program) -> Option<Context> {
    let retained = error.chain().find_map(|cause| {
        cause.downcast_ref::<Failure>().or_else(|| {
            cause
                .downcast_ref::<mlua::Error>()?
                .downcast_ref::<Failure>()
        })
    });
    let mut context = if let Some(failure) = retained {
        failure.context.clone()
    } else if error
        .chain()
        .any(|cause| cause.downcast_ref::<mlua::Error>().is_some())
    {
        Context {
            processor: None,
            frame: 0,
            line: None,
            chunk: "unavailable".into(),
            excerpt: None,
            unavailable: "Lua initialization did not retain a coroutine source frame",
            display: Arc::from(""),
            provenance: "unavailable",
        }
    } else {
        return None;
    };
    if context.excerpt.is_none()
        && let (Some(processor), Some(line)) = (context.processor, context.line)
        && context.chunk == format!("UVI ScriptProcessor node {processor}")
    {
        let mut sources = program
            .nodes
            .iter()
            .filter(|node| node.parent == Some(processor) && node.kind == "script");
        if program
            .nodes
            .get(processor)
            .is_some_and(|node| node.kind == "ScriptProcessor")
            && let Some(source) = sources.next()
            && sources.next().is_none()
        {
            context.excerpt = excerpt(&source.text, line);
        }
    }
    let text = context
        .excerpt
        .as_ref()
        .and_then(|excerpt| excerpt["text"].as_str())
        .map_or_else(
            || format!("Source context unavailable: {}.", context.unavailable),
            str::to_owned,
        );
    let processor = context.processor.map_or_else(
        || "Processor unavailable".into(),
        |id| format!("ScriptProcessor {id}"),
    );
    let line = context
        .line
        .map_or_else(|| "line unavailable".into(), |line| format!("line {line}"));
    context.display=format!("Local Lua source context (excluded from copy and export)\n{processor} · {} · {line} · frame {}\n{text}",
        context.chunk,context.frame).into();
    Some(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uvi::script::{Input, InputKind, Session};
    #[test]
    fn lua_logical_newlines_preserve_pairs_runs_and_empty_lines() {
        for newline in ["\n", "\r", "\r\n", "\n\r"] {
            assert_eq!(source_lines(&format!("one{newline}two{newline}")).collect::<Vec<_>>(),
                ["one", "two", ""]);
        }
        for source in ["one\r\rtwo", "one\n\ntwo", "one\r\n\rtwo", "one\n\r\ntwo"] {
            assert_eq!(source_lines(source).collect::<Vec<_>>(), ["one", "", "two"]);
        }
        assert_eq!(source_lines("").collect::<Vec<_>>(), [""]);
        assert_eq!(source_lines("界\r\n\n\rfin\r\r").collect::<Vec<_>>(), ["界", "", "fin", "", ""]);
        assert!(excerpt("one\r\ntwo", 0).is_none());
        assert!(excerpt("one\r\ntwo", 3).is_none());
        assert_eq!(excerpt("one\r\ntwo\r\n", 3).unwrap()["line"], 3);
        // The shared KSP helper retains its original LF-only interpretation.
        assert_eq!(crate::diagnostics::script_excerpt("one\rtwo", 1, 1, None).unwrap()["last_line"], 1);
        assert_eq!(excerpt("one\rtwo", 2).unwrap()["last_line"], 2);
    }

    #[test]
    fn lua_excerpt_keeps_bounded_utf8_lines_and_sanitizes_controls() {
        for newline in ["\n", "\r", "\r\n", "\n\r"] {
            let long_line = format!("-- {}", "界".repeat(1000));
            let lines = ["-- first", "-- before", "error('failed')", "-- \tcontrol\u{1b}",
                long_line.as_str(), "-- excluded sixth"];
            let value = excerpt(&lines.join(newline), 3).unwrap();
            assert_eq!((value["first_line"].as_u64(), value["last_line"].as_u64()), (Some(1), Some(5)));
            assert_eq!(value["truncated"], true);
            let text = value["text"].as_str().unwrap();
            assert_eq!(text.lines().count(), 5);
            assert!(text.len() < 3000 && text.contains(">      3 | error('failed')"));
            assert!(text.contains("\tcontrol?") && !text.contains("excluded sixth"));
            assert!(!text.contains('\r') && !text.contains('\u{1b}'));
            assert!(value.get("script_slot").is_none());
            assert_eq!(value["text"], crate::diagnostics::script_excerpt(&lines.join("\n"), 1, 3, None).unwrap()["text"]);
        }
    }

    #[test]
    fn logical_newlines_use_exact_loaded_processor_and_module_source() {
        for newline in ["\n", "\r", "\r\n", "\n\r"] {
            for module in [false, true] {
                let mut program=crate::uvi::program::parse_program("<Program><EventProcessors><ScriptProcessor><script/></ScriptProcessor></EventProcessors><Layers><Layer/></Layers></Program>").unwrap();
                let marker = "-- source-only-logical-newline-marker";
                let source = if module {
                    [marker, "local module={}", "function module.fail() error('owned newline failure') end", "return module"].join(newline)
                } else {
                    [marker, "function onNote(e)", " error('owned newline failure')", "end"].join(newline)
                };
                // Set authored loaded bytes after XML parsing: XML newline
                // normalization must not hide the Lua-local line rule under test.
                program.nodes.iter_mut().find(|node| node.kind == "script").unwrap().text = if module {
                    "local module=require('_Folder/Newline')\nfunction onNote(e) module.fail() end".into()
                } else { source.clone() };
                let modules = if module {
                    BTreeMap::from([("Scripts._Folder.Newline".into(), source.into_bytes())])
                } else { BTreeMap::new() };
                let mut session = Session::new_program_chain(&program, modules, None, 48000).unwrap();
                let error = session.process(&[Input { frame:256, kind:InputKind::NoteOn {
                    channel:0, note:60, velocity:100,
                }}],512).unwrap_err();
                let context = from_error(&error,&program).unwrap();
                assert_eq!((context.processor,context.frame,context.line),(Some(2),256,Some(3)));
                assert_eq!(context.provenance,"structured_coroutine_frame");
                assert_eq!(context.chunk,if module {"embedded module _Folder/Newline"} else {"UVI ScriptProcessor node 2"});
                let text = context.excerpt.as_ref().unwrap()["text"].as_str().unwrap();
                assert!(text.contains(">      3 |") && text.contains("error('owned newline failure')"));
                assert!(text.contains(marker) && !text.contains('\r'));
                assert!(!context.metadata().to_string().contains(marker));
                assert!(!format!("{context:?}").contains(marker));
            }
        }
    }

    fn fault(
        source: &str,
        modules: BTreeMap<String, Vec<u8>>,
    ) -> (Program, anyhow::Error, Context) {
        let program=crate::uvi::program::parse_program(&format!("<Program><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors><Layers><Layer/></Layers></Program>")).unwrap();
        let mut session = Session::new_program_chain(&program, modules, None, 48000).unwrap();
        let error = session
            .process(
                &[Input {
                    frame: 256,
                    kind: InputKind::NoteOn {
                        channel: 0,
                        note: 60,
                        velocity: 100,
                    },
                }],
                512,
            )
            .unwrap_err();
        let context = from_error(&error, &program).unwrap();
        (program, error, context)
    }
    #[test]
    fn sandbox_cannot_compile_a_forged_registered_chunk_name() {
        let source = "function onInit() for _,name in ipairs({'load','loadstring','loadfile','dofile','package','debug'}) do assert(_G[name]==nil and getfenv(0)[name]==nil,name) end end";
        let program=crate::uvi::program::parse_program(&format!("<Program><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors></Program>")).unwrap();
        Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
    }
    #[test]
    fn failed_callback_uses_structured_source_not_forged_error_text() {
        let source = "-- private-source-only-marker\nfunction onNote(e)\n error([=[[string \"UVI ScriptProcessor node 999\"]:77: fake\nstack traceback:\n\t[string \"UVI ScriptProcessor node 999\"]:77: fake]=])\nend";
        let (program, error, context) = fault(source, BTreeMap::new());
        assert_eq!(
            (context.processor, context.frame, context.line),
            (Some(2), 256, Some(3))
        );
        assert_eq!(context.chunk, "UVI ScriptProcessor node 2");
        assert!(
            context.excerpt.as_ref().unwrap()["text"]
                .as_str()
                .unwrap()
                .contains(">      3 |  error(")
        );
        assert!(context.display.contains("private-source-only-marker"));
        assert!(
            !context
                .metadata()
                .to_string()
                .contains("private-source-only-marker")
        );
        assert!(!format!("{context:?}").contains("private-source-only-marker"));
        assert!(
            !serde_json::to_string(&program)
                .unwrap()
                .contains("private-source-only-marker")
        );
        assert_eq!(
            format!("{error:#}").matches("runtime error:").count(),
            1,
            "typed local context does not duplicate the original cause"
        );
    }
    #[test]
    fn module_alias_and_long_chunk_name_resolve_exact_loaded_bytes() {
        for name in [
            "_Folder/Failure".to_owned(),
            "LongOwnedModule".repeat(15),
            "UVI ScriptProcessor node 2".into(),
        ] {
            let key = if name == "_Folder/Failure" {
                "Scripts._Folder.Failure".to_owned()
            } else {
                name.clone()
            };
            let source =
                format!("local module=require({name:?})\nfunction onNote(e) module.fail() end");
            let (_,_,context)=fault(&source,BTreeMap::from([(key,b"local module={}\nfunction module.fail() error('owned module error') end\nreturn module".to_vec())]));
            assert_eq!(context.chunk, format!("embedded module {name}"));
            assert_eq!((context.processor, context.line), (Some(2), Some(2)));
            assert!(
                context.excerpt.unwrap()["text"]
                    .as_str()
                    .unwrap()
                    .contains("function module.fail()")
            );
        }
    }
    #[test]
    fn yielded_callback_failure_retains_resumed_original_line() {
        let (_, _, context) = fault(
            "function onNote(e)\n wait(1)\n error('after owned yield')\nend",
            BTreeMap::new(),
        );
        assert_eq!(
            (context.processor, context.frame, context.line),
            (Some(2), 304, Some(3))
        );
        assert!(
            context.excerpt.unwrap()["text"]
                .as_str()
                .unwrap()
                .contains(">      3 |  error(")
        );
    }
    #[test]
    fn local_context_caps_lines_and_bytes_without_exporting_source() {
        let source = format!(
            "-- {}\nfunction onNote(e)\n error('bounded error')\nend\n-- {}\n-- excluded sixth line",
            "界".repeat(1000),
            "界".repeat(1000)
        );
        let (_, _, context) = fault(&source, BTreeMap::new());
        let excerpt = context.excerpt.as_ref().unwrap();
        let text = excerpt["text"].as_str().unwrap();
        assert_eq!(text.lines().count(), 5);
        assert!(text.len() < 3000 && excerpt["truncated"] == true);
        assert!(!text.contains("excluded sixth line"));
        assert!(!context.metadata().to_string().contains("bounded error"));
    }
    #[test]
    fn initialization_entry_does_not_claim_a_nested_modules_failed_line() {
        let program=crate::uvi::program::parse_program("<Program><EventProcessors><ScriptProcessor><script><![CDATA[local module=require('_Folder/Failure')]]></script></ScriptProcessor></EventProcessors></Program>").unwrap();
        let modules=BTreeMap::from([("Scripts._Folder.Failure".into(),b"-- source-only-constructor-module-marker\nerror('[string \"UVI ScriptProcessor node 999\"]:77: forged initialization')".to_vec())]);
        let error = Session::new_program_chain(&program, modules, None, 48000)
            .err()
            .unwrap();
        let context = from_error(&error, &program).unwrap();
        assert_eq!(context.processor, Some(2));
        assert_eq!(context.chunk, "UVI ScriptProcessor node 2");
        assert!(context.line.is_none() && context.excerpt.is_none());
        assert_eq!(
            context.metadata()["source_provenance"],
            "initialization_entry_chunk"
        );
        assert!(
            !context
                .display
                .contains("source-only-constructor-module-marker")
        );
        assert_eq!(format!("{error:#}").matches("runtime error:").count(), 1);
    }
    #[test]
    fn save_budget_failure_does_not_leak_into_later_processors_budget() {
        let save = "function onSave()\n for i=1,10000000 do local x=i+i end\nend\nfunction onNote(e) postEvent(e) end";
        let later = "function onNote(e)\n if e.note==61 then\n  local x=0\n  for i=1,10000000 do x=x+i end\n end\n postEvent(e)\nend";
        let program=crate::uvi::program::parse_program(&format!("<Program><EventProcessors><ScriptProcessor><script><![CDATA[{save}]]></script></ScriptProcessor><ScriptProcessor><script><![CDATA[{later}]]></script></ScriptProcessor></EventProcessors><Layers><Layer/></Layers></Program>")).unwrap();
        let mut session=Session::new_program_chain(&program,BTreeMap::new(),None,48000).unwrap();
        let save_error=session.saved_state(0).err().unwrap();
        let first=from_error(&save_error,&program).unwrap();
        assert_eq!((first.processor,first.frame,first.line),(Some(2),0,Some(2)));
        assert_eq!(first.provenance,"existing_instruction_budget_hook");
        assert!(format!("{save_error:#}").contains("Lua line Some(2)"));
        let input=|frame,note| Input{frame,kind:InputKind::NoteOn{channel:0,note,velocity:100}};
        let commands=session.process(&[input(128,60)],256).unwrap();
        assert!(!commands.commands.is_empty(),"successful callback still forwards the note after a failed save");
        let later_error=session.process(&[input(512,61)],768).unwrap_err();
        let second=from_error(&later_error,&program).unwrap();
        assert_eq!((second.processor,second.frame,second.line),(Some(4),512,Some(4)));
        assert_eq!(second.chunk,"UVI ScriptProcessor node 4");
        assert_eq!(second.provenance,"existing_instruction_budget_hook");
        assert!(format!("{later_error:#}").contains("Lua line Some(4)"));
        let text=second.excerpt.as_ref().unwrap()["text"].as_str().unwrap();
        assert!(text.contains(">      4 |   for i=1,10000000"));
        assert!(!text.contains("function onSave"));
        // The original returned error remains owned by its caller and accurate.
        let retained=from_error(&save_error,&program).unwrap();
        assert_eq!((retained.processor,retained.frame,retained.line),(Some(2),0,Some(2)));
    }

    #[test]
    fn initialization_budget_retains_exact_direct_or_caught_module_source() {
        for module in [false, true] {
            let source = if module {
                "local module=require('_Folder/Budget')"
            } else {
                "local x=0\nfor i=1,10000000 do x=x+i end"
            };
            let program=crate::uvi::program::parse_program(&format!("<Program><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors></Program>")).unwrap();
            let modules = if module {
                BTreeMap::from([("Scripts._Folder.Budget".into(),b"-- private-budget-module-source-marker\nlocal ok=pcall(function() for i=1,10000000 do local x=i+i end end)\nreturn {}".to_vec())])
            } else {
                BTreeMap::new()
            };
            let error = Session::new_program_chain(&program, modules, None, 48000)
                .err()
                .unwrap();
            let context = from_error(&error, &program).unwrap();
            assert_eq!(
                (context.processor, context.frame, context.line),
                (Some(2), 0, Some(2))
            );
            assert_eq!(context.provenance, "existing_instruction_budget_hook");
            assert_eq!(
                context.chunk,
                if module {
                    "embedded module _Folder/Budget"
                } else {
                    "UVI ScriptProcessor node 2"
                }
            );
            let text = context.excerpt.as_ref().unwrap()["text"].as_str().unwrap();
            assert!(text.contains(">      2 |") && text.contains("for i=1,10000000"));
            assert!(
                !context
                    .metadata()
                    .to_string()
                    .contains("private-budget-module-source-marker")
            );
            assert!(
                format!("{error:#}")
                    .contains("UVI instruction budget exceeded at Lua line Some(2)")
            );
        }
    }
    #[test]
    fn original_exec_without_coroutine_has_explicit_unavailable_context() {
        let program=crate::uvi::program::parse_program("<Program><EventProcessors><ScriptProcessor><script><![CDATA[local constructor_private_marker=1\nerror('owned constructor failure')]]></script></ScriptProcessor></EventProcessors></Program>").unwrap();
        let error = Session::new_program_chain(&program, BTreeMap::new(), None, 48000)
            .err()
            .unwrap();
        let context = from_error(&error, &program).unwrap();
        assert!(context.excerpt.is_none() && context.line.is_none());
        assert_eq!(context.processor, Some(2));
        assert_eq!(context.chunk, "UVI ScriptProcessor node 2");
        assert_eq!(
            context.metadata()["source_provenance"],
            "initialization_entry_chunk"
        );
        assert!(context.unavailable.contains("Initialization"));
        assert!(
            !context
                .metadata()
                .to_string()
                .contains("constructor_private_marker")
        );
    }
}
