//! Owned explicit-file fixtures at the production ScriptHost seam, not native parity receipts.
use sampler_uvi::script::{Command, Config, FaultCategory, ScriptHost};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "kontra-owned-load-data-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn path(path: &Path) -> String {
    serde_json::to_string(path.to_str().unwrap()).unwrap()
}
fn host(script: &str) -> ScriptHost {
    let xml = format!(
        "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    ScriptHost::new(&xml, (), Config::default()).unwrap()
}
fn until(h: &mut ScriptHost, global: &str, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut time = 0.0;
    while h.global_text(global) != expected {
        assert!(
            Instant::now() < deadline,
            "{global}={} findings={:?}",
            h.global_text(global),
            h.findings()
        );
        time += 1.0;
        h.advance(time);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn load_data_returns_task_and_decoded_value_once() {
    let fixture = Fixture::new();
    let file = fixture.file("types.json", br#"{"flag":true,"n":1.5,"s":"hello","array":[7,false,null,9],"dict":{"1":"one","nested":{"x":2}},"null":null}"#);
    let mut h = host(&format!(
        r#"
        calls = 0
        task = loadData({}, function(...)
            calls = calls + 1; argc = select('#', ...); local data = ...
            types = type(data.flag)..','..type(data.n)..','..type(data.s)..','..type(data.array)..','..type(data.null)
            values = tostring(data.flag)..','..data.n..','..data.s..','..data.array[1]..','..tostring(data.array[2])..','..tostring(data.array[3])..','..data.array[4]..','..data.dict['1']..','..data.dict.nested.x
            numericKeyMissing = data.dict[1] == nil
            callbackFinished = task.finished; callbackSuccess = task.success
        end)
        handle = type(task); initiallyFinished = task.finished
        initialProgress = task.progress; idType = type(task.id)
        spawn(function() while not task.finished do wait(1) end; polled = task.success; state = task.state; progress = task.progress end)
    "#,
        path(&file)
    ));
    assert_eq!(h.global_text("handle"), "userdata");
    assert_eq!(h.global_text("initiallyFinished"), "false");
    assert_eq!(h.global_text("initialProgress"), "0");
    assert_eq!(h.global_text("idType"), "number");
    until(&mut h, "polled", "true");
    assert_eq!(h.global_text("calls"), "1");
    assert_eq!(h.global_text("argc"), "1");
    assert_eq!(h.global_text("types"), "boolean,number,string,table,nil");
    assert_eq!(
        h.global_text("values"),
        "true,1.5,hello,7,false,nil,9,one,2"
    );
    for global in ["numericKeyMissing", "callbackFinished", "callbackSuccess"] {
        assert_eq!(h.global_text(global), "true");
    }
    assert_eq!(h.global_text("state"), "finished");
    assert_eq!(h.global_text("progress"), "1");
    for time in 100..110 {
        h.advance(f64::from(time));
    }
    assert_eq!(h.global_text("calls"), "1");
    assert!(h.findings().is_empty(), "{:?}", h.findings());
}

#[test]
fn load_data_missing_and_unreadable_finish_successfully_without_callback() {
    let fixture = Fixture::new();
    for file in [fixture.0.join("missing.json"), fixture.0.clone()] {
        let mut h = host(&format!(
            r#"
            calls = 0; task = loadData({}, function() calls = calls + 1 end)
            function onController(e) done = task.finished; success = task.success; state = task.state end
        "#,
            path(&file)
        ));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            h.advance(0.0);
            h.controller(1, 1, 0);
            if h.global_text("done") == "true" {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(h.global_text("success"), "true");
        assert_eq!(h.global_text("state"), "finished");
        assert_eq!(h.global_text("calls"), "0");
        assert!(h.findings().is_empty(), "{:?}", h.findings());
    }
}

#[test]
fn load_data_invalid_json_errors_in_completion_not_calling_thread() {
    let fixture = Fixture::new();
    for bytes in [
        &b"{broken"[..],
        &b"\xff\xfe"[..],
        &b""[..],
        &b"{} trailing"[..],
        &b"1e999"[..],
    ] {
        let file = fixture.file("invalid.json", bytes);
        let mut h = host(&format!(
            r#"
        calls = 0; submitted = pcall(function() task = loadData({}, function() calls = calls + 1 end) end)
        function onController(e) done = task.finished; success = task.success; alive = true end
    "#,
            path(&file)
        ));
        assert_eq!(h.global_text("submitted"), "true");
        assert!(h.fault_counts().init.is_empty());
        let deadline = Instant::now() + Duration::from_secs(5);
        while h.fault_counts().runtime.is_empty() {
            h.advance(0.0);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        h.controller(1, 1, 0);
        assert_eq!(h.global_text("calls"), "0");
        assert_eq!(h.global_text("done"), "true");
        assert_eq!(h.global_text("success"), "true");
        assert_eq!(h.global_text("alive"), "true");
        assert_eq!(h.fault_counts().runtime[&FaultCategory::Lua], 1);
        assert!(
            h.findings()
                .iter()
                .any(|f| f.value.contains("loadData invalid JSON")),
            "{:?}",
            h.findings()
        );
    }
}

#[test]
fn load_data_optional_callback_and_scalar_payloads() {
    let fixture = Fixture::new();
    for (index, (bytes, expected, kind)) in [
        (&b"false"[..], "false", "boolean"),
        (&b"12.25"[..], "12.25", "number"),
        (&br#""text""#[..], "text", "string"),
        (&b"null"[..], "nil", "nil"),
    ]
    .into_iter()
    .enumerate()
    {
        let file = fixture.file(&format!("scalar{index}.json"), bytes);
        let mut h = host(&format!(
            r#"
            task = loadData({}, function(data) result = tostring(data); kind = type(data); called = true end)
            spawn(function() while not task.finished do wait(1) end; done = task.success end)
        "#,
            path(&file)
        ));
        until(&mut h, "done", "true");
        assert_eq!(h.global_text("called"), "true");
        assert_eq!(h.global_text("result"), expected);
        assert_eq!(h.global_text("kind"), kind);
        assert!(h.findings().is_empty());
        let mut h = host(&format!(
            "task = loadData({}); spawn(function() while not task.finished do wait(1) end; done = task.success end)",
            path(&file)
        ));
        until(&mut h, "done", "true");
        assert!(h.findings().is_empty());
    }
}

#[test]
fn load_data_callback_yields_without_inheriting_note_context() {
    let fixture = Fixture::new();
    let file = fixture.file("note.json", b"65");
    let mut h = host(&format!(
        r#"
        function onNote(e)
            task = loadData({}, function(data)
                playNote(data, 100, 0); wait(10); playNote(data + 1, 100, 0); resumed = true
            end)
        end
    "#,
        path(&file)
    ));
    h.note_on(17, 60, 100, 0);
    until(&mut h, "resumed", "true");
    let plays: Vec<_> = h
        .take_commands()
        .into_iter()
        .filter_map(|c| {
            if let Command::Play(p) = c {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(plays.len(), 2);
    assert_eq!((plays[0].key, plays[1].key), (65, 66));
    assert_eq!(plays[1].at_ms - plays[0].at_ms, 10.0);
    assert!(plays.iter().all(|p| p.parent.is_none()));
    assert!(h.findings().is_empty());
}

#[test]
fn load_data_cancellation_queue_bounds_and_reload_are_isolated() {
    let fixture = Fixture::new();
    let file = fixture.file("cancel.json", b"60");
    let script = format!(
        r#"
        calls = 0; tasks = {{}}
        for i = 1, 64 do tasks[i] = loadData({}, function() calls = calls + 1 end) end
        overflow = pcall(function() loadData({}) end)
        for i = 1, 64 do tasks[i]:cancel() end
        immediatelyFinished = tasks[1].finished
        spawn(function() for i = 1, 64 do while not tasks[i].finished do wait(1) end end; done = true; state = tasks[1].state; success = tasks[1].success end)
    "#,
        path(&file),
        path(&file)
    );
    let mut h = host(&script);
    assert_eq!(h.global_text("overflow"), "false");
    assert_eq!(h.global_text("immediatelyFinished"), "false");
    until(&mut h, "done", "true");
    assert_eq!(h.global_text("calls"), "0");
    assert_eq!(h.global_text("state"), "cancelled");
    assert_eq!(h.global_text("success"), "false");
    drop(h);
    // Drop before consuming any completions; the new owner must not receive old callbacks.
    let old = host(&format!(
        "for i = 1, 64 do loadData({}, function() playNote(99, 100, 0) end) end",
        path(&file)
    ));
    drop(old);
    let mut fresh = host(&format!(
        "task = loadData({}, function(data) playNote(data, 100, 0); done = true end)",
        path(&file)
    ));
    until(&mut fresh, "done", "true");
    let notes: Vec<_> = fresh
        .take_commands()
        .into_iter()
        .filter_map(|c| {
            if let Command::Play(p) = c {
                Some(p.key)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(notes, [60]);
    assert!(fresh.findings().is_empty());
}

#[test]
fn load_data_rejects_implicit_paths_and_reports_oversized_completion() {
    let fixture = Fixture::new();
    let file = fixture.file("large.json", b"{}");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)
        .unwrap()
        .set_len((8 << 20) + 1)
        .unwrap();
    let mut h = host(&format!(
        r#"
        relative = pcall(function() loadData('relative.json') end)
        empty = pcall(function() loadData('') end)
        nul = pcall(function() loadData('/bad\0path') end)
        tooLong = pcall(function() loadData('/'..string.rep('a', 4096)) end)
        submitted = pcall(function() task = loadData({}, function() called = true end) end)
        spawn(function() while not task.finished do wait(1) end; done = true; success = task.success end)
    "#,
        path(&file)
    ));
    for global in ["relative", "empty", "nul", "tooLong"] {
        assert_eq!(h.global_text(global), "false");
    }
    assert_eq!(h.global_text("submitted"), "true");
    until(&mut h, "done", "true");
    assert_eq!(h.global_text("success"), "false");
    assert_eq!(h.global_text("called"), "nil");
    assert!(
        h.findings().iter().any(|f| f.value.contains("8 MiB limit")),
        "{:?}",
        h.findings()
    );
}
