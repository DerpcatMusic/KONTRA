// Standalone evaluation only. No production sampler dependency. See SHARED_IR.md.
#include "lua.h"
#include "lualib.h"
#include "luacode.h"
#include <cassert>
#include <cstdio>
#include <cstdlib>
#include <cstring>

struct Memory {
    size_t live = 0;
    size_t calls = 0;
    size_t frees = 0;
    size_t peak = 0;
};
static void* allocate(void* context, void* pointer, size_t old_size, size_t new_size) {
    auto& memory = *static_cast<Memory*>(context);
    if (!new_size) {
        if (pointer) { ++memory.frees; memory.live -= old_size; }
        std::free(pointer);
        return nullptr;
    }
    const bool replacing = pointer != nullptr;
    void* result = std::realloc(pointer, new_size);
    if (result) {
        ++memory.calls;
        memory.live = memory.live - (replacing ? old_size : 0) + new_size;
        if (memory.live > memory.peak) memory.peak = memory.live;
    }
    return result;
}
static void check(lua_State* state, int status, int expected = LUA_OK) {
    if (status != expected) {
        std::fprintf(stderr, "status %d, expected %d: %s\n", status, expected, lua_tostring(state, -1));
        std::abort();
    }
}
static void load(lua_State* state, const char* source) {
    size_t size = 0;
    char* code = luau_compile(source, std::strlen(source), nullptr, &size);
    check(state, luau_load(state, "sampler-language-probe", code, size, 0));
    std::free(code);
}
static int wait_frames(lua_State* state) {
    luaL_checkinteger(state, 1);
    lua_settop(state, 1);
    return lua_yield(state, 1);
}
struct Budget { size_t remaining; size_t calls; };
static void interrupt(lua_State* state, int gc) {
    if (gc >= 0) return; // -1 denotes bytecode execution; nonnegative values are GC phases.
    auto& budget = *static_cast<Budget*>(lua_callbacks(state)->userdata);
    ++budget.calls;
    if (!budget.remaining) luaL_error(state, "probe execution budget exhausted");
    --budget.remaining;
}
int main() {
    Memory memory;
    lua_State* state = lua_newstate(allocate, &memory);
    assert(state);
    luaL_openlibs(state);
    lua_pushcfunction(state, wait_frames, "waitFrames");
    lua_setglobal(state, "waitFrames");
    luaL_sandbox(state);
    luaL_sandboxthread(state);
    load(state, R"(
        local function makeCounter()
            local state = { count = 0 }
            return function(delta) state.count = state.count + delta; return state.count end
        end
        local counter = makeCounter()
        assert(counter(2) == 2 and counter(3) == 5)
        assert(bit32.band(0xffffffff, 0x80000000) == 2147483648)
        assert(9007199254740992 + 1 == 9007199254740992)
        assert(io == nil and package == nil and loadfile == nil)
        assert(bit == nil and class == nil and table.copy == nil)
        function callback(key)
            local saved = key
            waitFrames(key + 1)
            return saved * 2
        end
        function allocateTables()
            local retained = {}
            for i = 1, 10000 do retained[i] = { value = i } end
            return retained
        end
    )");
    check(state, lua_pcall(state, 0, 0, 0));
    auto* first = lua_newthread(state); // Rooted by the main state's stack.
    auto* second = lua_newthread(state);
    lua_getglobal(first, "callback"); lua_pushinteger(first, 60);
    lua_getglobal(second, "callback"); lua_pushinteger(second, 61);
    check(first, lua_resume(first, nullptr, 1), LUA_YIELD);
    check(second, lua_resume(second, nullptr, 1), LUA_YIELD);
    assert(lua_tointeger(first, -1) == 61 && lua_tointeger(second, -1) == 62);
    lua_settop(second, 0);
    check(second, lua_resume(second, nullptr, 0));
    assert(lua_tointeger(second, -1) == 122);
    lua_settop(first, 0);
    check(first, lua_resume(first, nullptr, 0));
    assert(lua_tointeger(first, -1) == 120);
    // Luau types/extensions can coexist with an ordinary Lua 5.1-style script.
    load(state, R"(
        type Control = { value: number }
        local control: Control = { value = 3 }
        control.value += 4
        assert(control.value == 7)
        local co = coroutine.create(function() coroutine.yield(1); error("must not resume") end)
        assert(coroutine.resume(co))
        assert(coroutine.close(co))
        assert(coroutine.status(co) == "dead")
        local ok = pcall(function() error("expected") end)
        assert(not ok)
    )");
    check(state, lua_pcall(state, 0, 0, 0));
    const auto before = memory.calls;
    lua_getglobal(state, "allocateTables");
    check(state, lua_pcall(state, 0, 1, 0));
    const auto callback_allocations = memory.calls - before;
    assert(callback_allocations > 0);
    lua_pop(state, 1);
    // Test interruption of bytecode loops, not a claim about bounded native calls.
    load(state, "while true do end");
    Budget budget{8, 0};
    lua_callbacks(state)->userdata = &budget;
    lua_callbacks(state)->interrupt = interrupt;
    check(state, lua_pcall(state, 0, 0, 0), LUA_ERRRUN);
    assert(budget.calls == 9);
    lua_callbacks(state)->interrupt = nullptr;
    lua_pop(state, 1);
    // Current pinned source has an explicit int64 C API, distinct from number.
    lua_pushinteger64(state, INT64_C(9007199254740993));
    assert(lua_isinteger64(state, -1));
    int exact = 0;
    assert(lua_tointeger64(state, -1, &exact) == INT64_C(9007199254740993) && exact);
    lua_pop(state, 1);
    lua_close(state);
    assert(memory.live == 0);
    std::printf("Lua-style closures/tables, isolated yielding callbacks, Luau syntax, cancellation, protected errors, loop interruption: PASS\n");
    std::printf("ordinary numbers lose +1 at 2^53; pinned explicit int64 C API roundtrip: PASS\n");
    std::printf("allocating callback: %zu allocator calls; total peak %zu bytes; shutdown live %zu bytes\n", callback_allocations, memory.peak, memory.live);
    std::puts("UVI API / pool allocator / GC deadline / native sampler service integration: NOT IMPLEMENTED BY THIS PROBE");
}
