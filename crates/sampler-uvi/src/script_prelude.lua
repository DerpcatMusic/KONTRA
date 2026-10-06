-- The UVI script environment that the host does not implement natively:
-- inert stand-ins for the user interface and for unmodeled engine parts, the
-- element tree helpers, and the threading helpers built on coroutines.
-- Anything inert is reported once through __report, never silently.
local report, native = __report, __native
__report, __native = nil, nil

-- Inert values -------------------------------------------------------------
local stub_mt = {}
local function stub(root)
  local s = setmetatable({}, stub_mt)
  rawset(s, "__root", root)
  return s
end
stub_mt.__index = function(t, k)
  if type(k) ~= "string" then return nil end
  local c = stub(rawget(t, "__root"))
  rawset(t, k, c)
  return c
end
stub_mt.__call = function(self) return stub(rawget(self, "__root")) end
stub_mt.__tostring = function(self) return "" end
stub_mt.__concat = function(a, b) return tostring(a) .. tostring(b) end
local function zero() return 0 end
stub_mt.__add, stub_mt.__sub, stub_mt.__mul = zero, zero, zero
stub_mt.__div, stub_mt.__mod, stub_mt.__pow, stub_mt.__unm = zero, zero, zero, zero

setmetatable(_G, {
  __index = function(_, name)
    if type(name) ~= "string" then return nil end
    report("global", name)
    local s = stub(name)
    rawset(_G, name, s)
    return s
  end,
})

-- Constants ----------------------------------------------------------------
Event = {
  NoteOn = 1, NoteOff = 2, Controller = 3, PitchBend = 4, AfterTouch = 5,
  PolyAfterTouch = 6, ProgramChange = 7, Transport = 8,
}
Unit = setmetatable({}, { __index = function(t, k) local v = k; rawset(t, k, v); return v end })
Engine = setmetatable({}, { __index = function(t, k)
  report("Engine", tostring(k))
  return stub("Engine")
end })

-- Widgets ------------------------------------------------------------------
local kinds = {
  "Panel", "Knob", "Slider", "OnOffButton", "Button", "Menu", "NumBox", "Label",
  "Image", "Table", "AudioMeter", "XY", "Multi", "WaveForm", "Envelope", "Display",
  "Text", "Frame",
}
local widget_mt = {}
local function widget(kind, name, value, min, max, integer)
  -- Kind{"name", value, min, max, integer, size=..., changed=...} passes one table.
  local named
  if type(name) == "table" then
    named = name
    name, value, min, max, integer = named[1], named[2], named[3], named[4], named[5]
    if kind == "Table" then integer = named[6] end
  end
  local w = setmetatable({
    kind = kind, name = name, value = value, min = min or 0, max = max or 1,
    integer = integer, x = 0, y = 0, width = 100, height = 100, alpha = 1,
    visible = true, enabled = true, text = "", tooltip = "", displayName = name,
  }, widget_mt)
  if value == nil then
    if kind == "OnOffButton" or kind == "Button" then w.value = false else w.value = 0 end
  end
  if type(value) == "table" then w.items, w.value, w.text = value, 1, value[1] end
  if kind == "Table" then
    w.length, w.values = value or 0, {}
    w.value, w.min, w.max = 0, named and named[4] or 0, named and named[5] or 1
    for i = 1, w.length do w.values[i] = named and named[3] or 0 end
  end
  if named then
    for k, v in pairs(named) do
      if type(k) == "string" then w[k] = v end
    end
    if type(named.size) == "table" then w.width, w.height = named.size[1], named.size[2] end
    if type(named.pos) == "table" then w.x, w.y = named.pos[1], named.pos[2] end
    if type(named.bounds) == "table" then
      w.x, w.y, w.width, w.height = named.bounds[1], named.bounds[2], named.bounds[3], named.bounds[4]
    end
  end
  report("ui", kind)
  return w
end
for _, kind in ipairs(kinds) do
  _G[kind] = function(...)
    local first = ...
    -- Panel("name") or Kind("name", value, min, max, integer).
    return widget(kind, ...)
  end
end
local methods = {}
function methods.setValue(self, v, notify)
  if self.kind == "Table" then
    -- Table:setValue(index, value)
    self.values[v] = notify
    if type(self.changed) == "function" then self:changed(v) end
    return
  end
  local items = rawget(self, "items")
  if items and type(v) == "number" and v < 1 then v = 1 end
  self.value = v
  if items and type(v) == "number" then self.text = items[v] or self.text end
  if notify ~= false and type(self.changed) == "function" then self:changed() end
end
function methods.getValue(self, i)
  if self.kind == "Table" then return self.values[i] or 0 end
  return self.value
end
function methods.setRange(self, lo, hi) self.min, self.max = lo, hi end
function methods.setPosition(self, x, y) self.x, self.y = x, y end
function methods.setSize(self, w, h) self.width, self.height = w, h end
function methods.setItem(self, i, text) end
function methods.setValueNormalized(self, v) self.value = self.min + v * (self.max - self.min) end
function methods.getValueNormalized(self)
  if self.max == self.min then return 0 end
  return (self.value - self.min) / (self.max - self.min)
end
widget_mt.__index = function(t, k)
  if methods[k] then return methods[k] end
  for _, kind in ipairs(kinds) do
    if kind == k then
      return function(self, ...) return widget(k, ...) end
    end
  end
  if type(k) ~= "string" then return nil end
  local c = stub("ui")
  rawset(t, k, c)
  return c
end

-- Elements -----------------------------------------------------------------
local element = {}
element.__index = function(t, k)
  local m = rawget(element, k)
  if m then return m end
  if k == "parameterDefinitions" then
    local defs = {}
    for _, n in ipairs(native.paramNames(rawget(t, "__id"))) do defs[#defs + 1] = { name = n } end
    return defs
  end
  return nil
end
function element.getParameter(self, name)
  local overlay = rawget(self, "__set")
  if overlay and overlay[name] ~= nil then return overlay[name] end
  local v = native.param(rawget(self, "__id"), name)
  if v == nil then
    report("getParameter", name)
    return 0
  end
  return v
end
function element.setParameter(self, name, value)
  if name == nil then report("setParameter", "nil name"); return end
  local overlay = rawget(self, "__set")
  if not overlay then overlay = {}; rawset(self, "__set", overlay) end
  overlay[name] = value
  report("setParameter", rawget(self, "type") .. "." .. tostring(name))
end
-- Connections are not modeled: any index answers with one inert element.
local inert, connections = {}, nil
connections = setmetatable({}, { __index = function(_, k)
  if type(k) == "number" then return inert end
end })
function inert.getParameterConnections() return connections end
function inert.setParameter(_, n) report("setParameter", "connection." .. tostring(n)) end
function inert.getParameter(_, n) report("getParameter", "connection." .. tostring(n)); return 0 end
function element.getParameterConnections(self, name)
  report("getParameterConnections", tostring(name))
  return connections
end
function element.sendScriptModulation(self, ...) report("sendScriptModulation", "") end
element.__element = true
-- element lists also answer to an element name (Program.modulations["LFO 1"]).
__list_mt = { __index = function(t, k)
  if type(k) ~= "string" then return nil end
  for _, e in ipairs(t) do
    if e.name == k or native.param(rawget(e, "__id"), "DisplayName") == k then return e end
  end
end }
__element_mt = element

-- Threads ------------------------------------------------------------------
function wait(ms) return coroutine.yield(ms or 0) end
function waitBeat(beats) return coroutine.yield((beats or 0) * 500) end
function waitForRelease() return coroutine.yield("release") end

function postEvent(e, delta)
  if delta and delta > 0 then
    local id = native.nextId()
    spawn(function() wait(delta); postEvent(e) end)
    return id
  end
  if e.type == Event.NoteOn then
    return playNote(e)
  elseif e.type == Event.NoteOff then
    if e.id then releaseVoice(e.id) end
  else
    report("postEvent", tostring(e.type))
  end
end

-- Helpers UVI provides ------------------------------------------------------
function table.copy(t)
  local c = {}
  for k, v in pairs(t) do c[k] = v end
  return c
end
function table.print() end
function print() end

-- Loading ------------------------------------------------------------------
dofile, loadfile, load, loadstring, module = nil, nil, nil, nil, nil
local loaded = {}
function require(name)
  if loaded[name] ~= nil then return loaded[name] end
  if string.sub(name, 1, 4) == "uvi." then
    report("module", name)
    loaded[name] = stub(name)
    return loaded[name]
  end
  local source = native.source(name)
  if source == nil then error("module '" .. tostring(name) .. "' not found", 2) end
  local chunk, err = native.compile(source, name)
  if not chunk then error(err, 2) end
  local result = chunk(name)
  if result == nil then result = true end
  loaded[name] = result
  return result
end

-- class 'Name' / class 'Name'(Base), as the shipped scripts use it.
local function instantiate(cls, ...)
  local o = setmetatable({}, { __index = cls })
  if cls.__init then cls.__init(o, ...) end
  return o
end
function class(name)
  local cls = { __name = name }
  setmetatable(cls, { __call = instantiate })
  _G[name] = cls
  return function(base)
    if type(base) == "table" then
      setmetatable(cls, { __index = base, __call = instantiate })
    end
    return cls
  end
end

-- Program lookups the scripts expect; a layer is addressed by its ordinal.
function findLayer(name)
  for i, l in ipairs(Program.layers) do
    if native.param(rawget(l, "__id"), "DisplayName") == name or l.name == name then return i end
  end
  return nil
end


-- Elements and widgets are userdata in the engine; scripts test type().
local rawtype, getmt = type, getmetatable
function type(v)
  local t = rawtype(v)
  if t == "table" then
    local m = getmt(v)
    if m == element or m == widget_mt then return "userdata" end
  end
  return t
end
