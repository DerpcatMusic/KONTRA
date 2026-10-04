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
}
impl Context {
    /// Explicit metadata whitelist; local source bytes/display never serialize.
    pub(crate) fn metadata(&self) -> serde_json::Value {
        serde_json::json!({"source_kind":"lua","processor":self.processor,"frame":self.frame,
            "chunk":self.chunk,"line":self.line,"local_source_excerpt_available":self.excerpt.is_some(),
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
fn excerpt(source: &str, line: u32) -> Option<serde_json::Value> {
    let mut excerpt = crate::diagnostics::script_excerpt(source, 1, line, None)?;
    excerpt.as_object_mut()?.remove("script_slot");
    excerpt["origin"] = serde_json::json!("already loaded local UVI Lua source");
    excerpt["source_kind"] = serde_json::json!("lua");
    Some(excerpt)
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
    let mut context = Context {
        processor,
        frame,
        line: None,
        chunk: "unavailable".into(),
        excerpt: None,
        unavailable: "Lua coroutine source/line is unavailable",
        display: Arc::from(""),
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
                match source {
                    Some(source)
                        if source
                            .strip_prefix("UVI ScriptProcessor node ")
                            .and_then(|n| n.parse::<NodeId>().ok())
                            == processor
                            && processor.is_some() =>
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
                                .and_then(|source| excerpt(source, debug.currentline as u32));
                            context.unavailable =
                                "Loaded module source or reported line is unavailable";
                        } else {
                            context.unavailable =
                                "Lua module is not in the loaded approved registry";
                        }
                    }
                    _ => {
                        context.unavailable =
                            "Lua chunk is not registered for this callback processor"
                    }
                }
                break;
            }
        }
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
    fn original_exec_without_coroutine_has_explicit_unavailable_context() {
        let program=crate::uvi::program::parse_program("<Program><EventProcessors><ScriptProcessor><script><![CDATA[local constructor_private_marker=1\nerror('owned constructor failure')]]></script></ScriptProcessor></EventProcessors></Program>").unwrap();
        let error = Session::new_program_chain(&program, BTreeMap::new(), None, 48000)
            .err()
            .unwrap();
        let context = from_error(&error, &program).unwrap();
        assert!(context.excerpt.is_none() && context.line.is_none());
        assert!(context.unavailable.contains("initialization"));
        assert!(
            !context
                .metadata()
                .to_string()
                .contains("constructor_private_marker")
        );
    }
}
