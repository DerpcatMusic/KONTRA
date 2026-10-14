use sampler_uvi::script::{Config, ScriptHost};

fn host(script: &str) -> ScriptHost {
    let xml = format!("<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>");
    ScriptHost::new(&xml, (), Config::default()).unwrap()
}

#[test]
fn unavailable_resource_callbacks_receive_the_returned_failed_task_once() {
    let mut h = host(r#"
        calls = 0
        function onNote(e)
            task = loadSample({}, 'missing.wav', function(t)
                calls = calls + 1
                identity = rawequal(t, task)
                success = t.success
                finished = t.finished
                state = t.state
                progress = t.progress
                identifier = type(t.id)
                t:cancel()
                wait(1)
                resumed = true
            end)
            noCallback = loadSample({}, 'missing.wav') == nil
        end
    "#);
    h.note_on(1, 60, 100, 0);
    h.advance(0.0);
    assert_eq!(h.global_text("calls"), "1");
    assert_eq!(h.global_text("identity"), "true");
    assert_eq!(h.global_text("success"), "false");
    assert_eq!(h.global_text("finished"), "true");
    assert_eq!(h.global_text("state"), "finished");
    assert_eq!(h.global_text("progress"), "1");
    assert_eq!(h.global_text("identifier"), "number");
    assert_eq!(h.global_text("noCallback"), "true");
    h.advance(2.0);
    assert_eq!(h.global_text("resumed"), "true");
    assert_eq!(h.global_text("calls"), "1");
    assert!(h.fault_counts().runtime.is_empty(), "{:?}", h.fault_counts());
    assert!(h.findings().iter().any(|f| f.feature.contains("loadSample failed")));
}

#[test]
fn impulse_failure_uses_the_same_read_only_task_contract() {
    let mut h = host(r#"
        calls = 0
        function onNote(e)
            task = loadImpulse({}, 'missing.wav', function(t)
                calls = calls + 1
                identity = rawequal(t, task)
                success = t.success
                finished = t.finished
            end)
            independent = loadImpulse({}, 'missing.wav')
            different = task.id ~= independent.id
        end
    "#);
    h.note_on(1, 60, 100, 0);
    h.advance(1.0);
    assert_eq!(h.global_text("calls"), "1");
    assert_eq!(h.global_text("identity"), "true");
    assert_eq!(h.global_text("success"), "false");
    assert_eq!(h.global_text("finished"), "true");
    assert_eq!(h.global_text("different"), "true");
    assert!(h.fault_counts().runtime.is_empty());
}
