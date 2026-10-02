-- Developer-only runtime boundary probe. Never launched by the plugin.
-- lua5.4 tools/probe_native_ui.lua <resource-root> [module-name]
-- Accepts ordinary Lua .nui modules; extended @component syntax is NOT lowered.
local root, entry = assert(arg[1], "resource root required"), arg[2] or "use_connection"
local loaded, loading, writes = {}, {}, 0
local control = {value = 17}
local function strict(name, value)
    return setmetatable(value, {__index = function(_, key)
        error("Unsupported native UI API: " .. name .. "." .. tostring(key), 2)
    end})
end
local function state(value)
    return function() return value end, function(next_value) value = next_value end
end
local ui = strict("native_ui.global", {
    create_state = state,
    -- Pull evaluation only: dependency tracking/subscriptions are not implemented.
    create_computed = function(compute) return compute end,
})
loaded.native_ui = strict("native_ui", {global = ui})
loaded.kontakt = strict("kontakt", {
create_tmp_midi_file = function() error("Unsupported native UI API: kontakt.create_tmp_midi_file") end,
connect_parameter = function(id)
    assert(id == "fixture", "Probe host only exposes the authored fixture control")
    return strict("parameter", {
        read = function(index)
            assert(index == nil or index == 0, "Probe host exposes one scalar")
            return control.value
        end,
        write = function(value, index)
            assert(index == nil or index == 0, "Probe host exposes one scalar")
            assert(type(value) == "number" and value % 1 == 0 and value >= 0 and value <= 100,
                "Fixture control requires an integer in 0..100")
            control.value, writes = value, writes + 1
        end,
    })
end})
local environment = {
    assert = assert, error = error, pcall = pcall, type = type, tostring = tostring,
    tonumber = tonumber, pairs = pairs, ipairs = ipairs, next = next,
    setmetatable = setmetatable, getmetatable = getmetatable,
    math = math, string = string, table = table, select = select,
}
local function resource_require(name)
    assert(type(name) == "string" and name:match("^[%w_.-]+$")
        and not name:find("..", 1, true) and name:sub(1, 1) ~= ".", "Invalid resource module")
    if loaded[name] then return loaded[name] end
    assert(not loading[name], "Circular native UI module: " .. name)
    local relative = name:gsub("%.", "/")
    local file = io.open(root .. "/" .. relative .. ".nui", "rb")
        or io.open(root .. "/" .. relative .. "/init.nui", "rb")
    assert(file, "Native UI module not found: " .. name)
    local source = file:read(1024 * 1024 + 1); file:close()
    assert(#source <= 1024 * 1024, "Native UI module exceeds probe limit")
    local chunk, reason = load(source, "@native_ui/" .. relative .. ".nui", "t", environment)
    assert(chunk, "Unsupported native UI syntax: " .. tostring(reason))
    loading[name] = true
    local ok, result = pcall(chunk)
    loading[name] = nil
    assert(ok, result)
    loaded[name] = result == nil and true or result
    return loaded[name]
end
environment.require = resource_require
local instructions = 0
debug.sethook(function()
    instructions = instructions + 1000
    assert(instructions <= 100000, "Native UI probe instruction budget exceeded")
end, "", 1000)
local module = resource_require(entry)
local parameter = assert(module.parameter, "Module must export a parameter connection")("fixture")
assert(parameter.read() == 17)
parameter.write(42)
assert(parameter.read() == 42 and control.value == 42 and writes == 1)
local read, write = ui.create_state(3)
local computed = ui.create_computed(function() return read() * 2 end)
assert(computed() == 6); write(7); assert(computed() == 14)
if module.geometry then
    assert(module.geometry.width == 640 and module.geometry.height == 400)
    assert(module.geometry.background == 0x285078)
    assert(module.geometry.control.x == 30 and module.geometry.control.y == 40)
end
debug.sethook()
print("module_load=pass control_host_read_write=pass pull_state=pass")
print("production_runtime=false extended_component_frontend=false rendering=false")
