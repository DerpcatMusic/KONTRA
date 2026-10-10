//! Representative documented Lua 5.1 constructs, not proof of VM equivalence.
use sampler_uvi::script::{Config, ScriptHost};

fn host(source: &str) -> ScriptHost {
    ScriptHost::new(
        &format!(
            "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
        ),
        (),
        Config::default(),
    )
    .unwrap()
}

#[test]
fn lua51_varargs_keep_nil_holes_and_multiple_return_values() {
    let h = host(
        "local function forward(...) return select('#', ...), ... end
         local count, first, hole, last = forward(10, nil, 30)
         result = count == 3 and first == 10 and hole == nil and last == 30",
    );
    assert_eq!(h.global_text("result"), "true");
}

#[test]
fn lua51_function_environments_use_the_assigned_table() {
    let h = host(
        "local function read() return sentinel end
         local environment = {sentinel = 7}
         setfenv(read, environment)
         result = read() == 7 and getfenv(read) == environment",
    );
    assert_eq!(h.global_text("result"), "true");
}

#[test]
fn documented_library_constructs_and_local_extensions_remain_available() {
    let h = host(
        "local a, b = string.match('key=27', '(%a+)=(%d+)')
         local copy = table.copy({value = 3})
         local sum = 0
         for _, value in ipairs({2, 3}) do sum = sum + value end
         math.authoredHelper = function(value) return math.pow(value, 2) end
         result = a == 'key' and tonumber(b) == 27 and copy.value == 3
             and sum == 5 and math.authoredHelper(3) == 9 and (-3 % 2) == 1",
    );
    assert_eq!(h.global_text("result"), "true");
}
