-- The UVI script environment that the host does not implement natively:
-- stateful user-interface widgets and stand-ins for unmodeled engine parts, the
-- element tree helpers, and the threading helpers built on coroutines.
-- Anything inert is reported once through __report, never silently.
local ENV = _G -- the main environment; a coroutine's own is a proxy
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
stub_mt.__call = function(self, ...)
  local root = rawget(self, "__root")
  if native.scanKey and (root == "setKeyColour" or root == "resetKeyColour") then native.scanKey(root, {...}) end
  return stub(root)
end
stub_mt.__tostring = function(self) return "" end
stub_mt.__concat = function(a, b) return tostring(a) .. tostring(b) end
local function zero() return 0 end
stub_mt.__add, stub_mt.__sub, stub_mt.__mul = zero, zero, zero
stub_mt.__div, stub_mt.__mod, stub_mt.__pow, stub_mt.__unm = zero, zero, zero, zero

-- Under Luau's sandbox the environment reads through to the libraries.
local base_mt = getmetatable(_G)
local base = base_mt and base_mt.__index
setmetatable(_G, {
  __index = function(_, name)
    if base then
      local v = base[name]
      if v ~= nil then return v end
    end
    if type(name) ~= "string" then return nil end
    return nil
  end,
})

-- The API revision the shipped scripts gate on (they require at least 22).
__API_VERSION__ = 22

-- Constants ----------------------------------------------------------------
Event = {
  NoteOn = 1, NoteOff = 2, Controller = 3, PitchBend = 4, AfterTouch = 5,
  PolyAfterTouch = 6, ProgramChange = 7, Transport = 8,
}
Event.ControlChange = Event.Controller
bit = bit32
Unit = setmetatable({}, { __index = function(t, k) local v = k; rawset(t, k, v); return v end })
Engine = setmetatable({}, { __index = function(t, k)
  report("Engine " .. tostring(k), "")
  return stub("Engine")
end })

-- Widgets ------------------------------------------------------------------
local kinds = {
  "Panel", "Viewport", "Knob", "Slider", "OnOffButton", "Button", "Menu",
  "MultiStateButton", "NumBox", "Label", "Image", "SVG", "Table", "AudioMeter",
  "XY", "WaveView", "WaveForm", "FileSelector", "DnDArea", "Text", "Frame",
  "ParamKnob", "ParamSlider", "ParamOnOffButton", "ParamMenu", "ParamNumBox",
  "ParameterValue",
}
Mapper = setmetatable({}, { __index = function(t, k) rawset(t, k, k); return k end })
FileFormat = {Audio="Audio", Midi="Midi", All="All", Data="Data"}
local widget_mt, methods = {}, {}
local registry = {}
local ui = { widgets = registry, revision = 0, keys = {} }
local rawtype = type
local function data(w) return rawget(w, "__data") end
local function notify(w, index)
  local f = data(w).changed
  if rawtype(f) == "function" then f(w, index) end
end
local function clamp(w, v)
  local d = data(w)
  if d.kind == "ParameterValue" then return v end
  if d.kind == "OnOffButton" or d.kind == "Button" or d.kind == "ParamOnOffButton" then
    return v == true or (rawtype(v) == "number" and v >= 0.5)
  end
  if rawtype(v) ~= "number" or v ~= v or math.abs(v) == math.huge then error("invalid widget value") end
  if d.items then return math.max(1, math.min(math.max(1, #d.items), math.floor(v + 0.5))) end
  v = math.max(d.min, math.min(d.max, v))
  if d.integer then v = math.floor(v + 0.5) end
  return v
end
function methods.setValue(w, v, arg, callChanged)
  local d = data(w)
  if d.kind == "Table" then
    if rawtype(v) ~= 'number' or v ~= v or math.abs(v) == math.huge then error('invalid table index') end
    v = v < 0 and math.ceil(v) or math.floor(v)
    -- v1/Workstation-observed, Falcon unverified: boundary writes are
    -- ignored; programmatic values are independent of the display range.
    if v < 1 or v > d.length then report('widget_index_out_of_range',''); return end
    if rawtype(arg) ~= 'number' or arg ~= arg or math.abs(arg) == math.huge then error('invalid table value') end
    if d.integer then arg = arg < 0 and math.ceil(arg) or math.floor(arg) end
    local old = d.values[v]
    d.values[v] = arg
    if old ~= arg then ui.revision = ui.revision + 1 end
    if callChanged ~= false and old ~= arg then notify(w, v) end
    return
  end
  if d.kind ~= "ParameterValue" then v = clamp(w, v) end
  local old = w.value
  if d.element then d.element:setParameter(d.parameter, v) else d.value = v end
  -- v1/Workstation-observed, Falcon unverified: identical writes/restoration
  -- do not call changed, including a parameter setter that ignored its write.
  if old ~= w.value then
    ui.revision = ui.revision + 1
    if arg ~= false then notify(w) end
  end
end
function methods.getValue(w, i)
  if w.kind == "Table" then
    if rawtype(i) ~= 'number' or i ~= i or math.abs(i) == math.huge then error('invalid table index') end
    i = i < 0 and math.ceil(i) or math.floor(i)
    return data(w).values[i] or data(w).default
  end
  return w.value
end
function methods.setRange(w, lo, hi) w.min, w.max = lo, hi end
function methods.setPosition(w, x, y) w.position = {x, y} end
function methods.setSize(w, width, height) w.size = {width, height} end
function methods.setItem(w, i, text) data(w).items[i] = text; ui.revision = ui.revision + 1 end
function methods.addItem(w, text)
  local items = data(w).items
  items[#items+1] = text; ui.revision = ui.revision + 1
  return #items
end
function methods.clear(w) data(w).items = {}; ui.revision = ui.revision + 1 end
function methods.getText(w, i) return data(w).items[i] end
function methods.setStripImage(w, path, frames) w.stripImage, w.frames = path, frames end
function methods.setViewPosition(w, x, y) w.viewPosition = {x, y} end
function methods.loadFont(w, path) w.font = path end
function methods.toString(w) return tostring(w.value) end
local function mapped(w, x, inverse)
  local m = w.mapper or "Linear"
  if m == "Exponential" and w.min > 0 and w.max > w.min then
    if inverse then return math.log(x/w.min)/math.log(w.max/w.min) end
    return w.min * (w.max/w.min)^x
  end
  local powers = {Quadratic=2,Cubic=3,Quartic=4,Quintic=5,SquareRoot=0.5,CubeRoot=1/3,QuarticRoot=0.25,QuinticRoot=0.2}
  local power = powers[m] or 1
  if inverse then return ((x-w.min)/(w.max-w.min))^(1/power) end
  return w.min + (w.max-w.min)*x^power
end
function methods.setValueNormalized(w, v, notify) w:setValue(mapped(w, math.max(0,math.min(1,v)), false), notify) end
function methods.getValueNormalized(w)
  if w.min == w.max then return 0 end
  return mapped(w, w.value, true)
end
local widget
widget_mt.__index = function(w, k)
  if methods[k] then return methods[k] end
  local d = data(w)
  if k == "size" then return {d.width, d.height} end
  if k == "position" or k == "pos" then return {d.x, d.y} end
  if k == "bounds" then return {d.x, d.y, d.width, d.height} end
  if k == "selected" then return d.value end
  if k == "selectedText" then return d.items and d.items[d.value] or "" end
  if k == "length" and d.items then return #d.items end
  if k == "value" and d.element then return d.element:getParameter(d.parameter) end
  if d[k] ~= nil then return d[k] end
  for _, kind in ipairs(kinds) do
    if kind == k then return function(self, ...)
      local child = widget(k, ...)
      data(child).parent_id = rawget(self, "id")
      return child
    end end
  end
  -- Optional properties are nil; unknown method calls fail visibly instead of
  -- inventing truthy values that change a script's control flow.
  return nil
end
widget_mt.__newindex = function(w, k, v)
  local d = data(w)
  if type(v) == 'string' and (k == 'font' or string.match(k, 'Image$') or k == 'image') then
    v = native.resourcePath(v)
  end
  if k == "value" or k == "selected" then
    if v ~= w.value then methods.setValue(w, v) end
    return
  elseif k == "bounds" then d.x,d.y,d.width,d.height = v[1],v[2],v[3],v[4]
  elseif k == "size" then d.width,d.height = v[1],v[2]
  elseif k == "position" or k == "pos" then d.x,d.y = v[1],v[2]
  else d[k] = v end
  ui.revision = ui.revision + 1
end
local sizes = {Knob={80,80},Slider={120,20},NumBox={80,20},Button={100,25},OnOffButton={100,25},
  MultiStateButton={100,25},Menu={100,25},Label={100,20},Panel={720,100},Viewport={200,200}}
widget = function(kind, ...)
  local args = {...}
  local named = rawtype(args[1]) == "table" and rawget(args[1],"__id") == nil and args[1] or {}
  if next(named) then args = named end
  local name, value, lo, hi, integer = args[1],args[2],args[3],args[4],args[5]
  local basekind = string.gsub(kind, "^Param", "")
  local size = sizes[basekind] or {100,100}
  local d = {kind=kind,name=name or named.name or kind,value=value or 0,min=lo or 0,max=hi or 1,
    default=value or 0,integer=integer==true,x=0,y=0,width=size[1],height=size[2],alpha=1,visible=true,enabled=true,
    persistent=true,exported=false,text="",tooltip=name or "",displayName=name or "",showLabel=true,
    interceptsMouseClicks=true, mapper="Linear",unit="Generic"}
  local w = setmetatable({__data=d}, widget_mt)
  if kind == "Table" then
    d.length,d.value,d.min,d.max,d.integer = value or 0,0,args[4] or 0,args[5] or 1,args[6]==true
    d.default = args[3] or 0
    d.values = {}; for i=1,d.length do d.values[i] = args[3] or 0 end
  elseif kind == "Menu" or kind == "MultiStateButton" then
    d.items,d.value,d.integer = value or named.items or {},lo or 1,true
    d.min,d.max = 1,math.max(1,#d.items)
  elseif kind == "Button" or kind == "OnOffButton" then
    d.value = value == true; if kind == "Button" then d.persistent=false end
  elseif kind == "AudioMeter" then
    d.meter_element,d.stereo,d.channel,d.vertical,d.value,d.persistent = value,args[3]~=false,args[4] or 0,args[5]~=false,0,false
  elseif kind == "XY" then
    d.paramX,d.paramY,d.value = name,value,0
  elseif kind == "Image" or kind == "SVG" then d.image = native.resourcePath(name)
  end
  if string.sub(kind,1,5) == "Param" or kind == "ParameterValue" then
    d.element,d.parameter = name,value
    d.name,d.displayName,d.value = tostring(value),tostring(value),nil
    d.bound = rawtype(name) == "table" and rawget(name,"__id") ~= nil
    if d.bound then
      for _, def in ipairs(name.parameterDefinitions) do
        if def.id == value or def.name == value then
          d.parameter = def.name
          d.min,d.max,d.integer,d.unit,d.mapper = def.min or 0,def.max or 1,def.type=="int",def.unit or "Generic",def.mapper or "Linear"
          break
        end
      end
    else report("ui parameter binding", "unresolved target") end
    if kind == "ParameterValue" then d.visible=false end
  end
  -- Named options use the same setters as later property writes.
  for k,v in pairs(named) do
    if rawtype(k)=="string" and k~="changed" then
      if k=="value" then methods.setValue(w,v,false) else w[k]=v end
    end
  end
  d.changed=named.changed
  if kind=="Slider" and args[6]~=nil then d.vertical=args[6] end
  registry[#registry+1] = w; rawset(w,"id",#registry)
  d.id = #registry
  -- A default grid per parent; explicit bounds/position override it.
  if not named.bounds and not named.position and not named.pos and named.x == nil and named.y == nil then
    d.x,d.y = ((#registry-1)%6)*120,math.floor((#registry-1)/6)*90
  end
  return w
end
for _, kind in ipairs(kinds) do _G[kind] = function(...) return widget(kind, ...) end end
function __restore()
  for _, w in ipairs(registry) do
    local d = data(w)
    local saved = d.persistent and native.saved(d.name)
    if saved then
      if d.kind == "Table" then
        local i=0
        for number in string.gmatch(string.gsub(saved,",","."),"%S+") do
          i=i+1; if i<=d.length then d.values[i] = clamp(w,tonumber(number) or 0) end
        end
        for j=1,d.length do
          local ok,err=pcall(notify,w,j); if not ok then report("lua error",tostring(err)) end
        end
      else
        local v = tonumber((string.gsub(saved,",",".")))
        if d.kind == "OnOffButton" then v = saved=="1" or saved=="true" end
        if v ~= nil then
          local ok,err=pcall(methods.setValue,w,v); if not ok then report("lua error",tostring(err)) end
        end
      end
    end
  end
end
function __ui_edit(id, component, value)
  local w = registry[id]; if not w or not w.enabled then error("invalid UI control") end
  if w.kind == "Table" then
    if component < 1 or component > w.length then error('invalid UI table cell') end
    w:setValue(component,value)
  elseif w.kind == "XY" then
    if component ~= 1 and component ~= 2 then error('invalid UI axis') end
    local name = component==1 and w.paramX or w.paramY
    for _, target in ipairs(registry) do if target.name==name then target:setValue(value); return end end
    error("unbound XY axis")
  elseif w.kind == "Button" then
    if component ~= 0 then error('invalid UI component') end
    if value >= 0.5 then notify(w) end
  else
    if component ~= 0 then error('invalid UI component') end
    w:setValue(value)
  end
end

-- Elements -----------------------------------------------------------------
local element = {}
local collections = {layers=true,keygroups=true,oscillators=true,inserts=true,auxs=true,
  sends=true,modulations=true,eventProcessors=true,connections=true}
element.__index = function(t, k)
  local m = rawget(element, k)
  if m then return m end
  if k == "parameterDefinitions" then
    local defs = native.definitions(rawget(t, "__id"))
    rawset(t, k, defs)
    return defs
  end
  if k == "numParams" then
    local defs=rawget(t,'parameterDefinitions')
    return defs and #defs or native.paramCount(rawget(t,'__id'))
  end
  if k == 'mods' then
    local list = t.modulations; rawset(t,k,list); return list
  end
  if collections[k] or k == 'synthChildren' then
    local list = {}; if collections[k] then setmetatable(list,__list_mt) end
    rawset(t,k,list); return list
  end
  return nil
end
local function parameter_name(self,id)
  local defs=rawget(self,'parameterDefinitions')
  if defs then local def=defs[id]; return def and def.name end
  return native.paramName(rawget(self,'__id'),id)
end
function element.getParameter(self, name)
  if type(name) == "number" then
    name = parameter_name(self,name); if not name then error("invalid parameter id") end
  end
  local overlay = rawget(self, "__set")
  if overlay and overlay[name] ~= nil then return overlay[name] end
  local v = native.param(rawget(self, "__id"), name)
  if v == nil then
    report("getParameter " .. tostring(name), "")
    return 0
  end
  return v
end
function element.hasParameter(self, name)
  if type(name)=='number' then return parameter_name(self,name) ~= nil end
  local defs=rawget(self,'parameterDefinitions')
  return (defs and defs[name] ~= nil) or native.hasParameter(rawget(self,'__id'),name)
end
__touched = {}
function element.setParameter(self, name, value)
  if name == nil then report("setParameter", "nil name"); return end
  if type(name) == "number" then
    name = parameter_name(self,name); if not name then error("invalid parameter id") end
  end
  local defs = rawget(self, 'parameterDefinitions')
  local expected, min, max
  if defs then
    local def = defs[name]
    if def then expected,min,max=def.type,def.min,def.max end
  else expected,min,max=native.definition(rawget(self,'__id'),name) end
  if expected then
    -- v1/Workstation-observed, Falcon unverified: mismatched scalar writes
    -- are ignored. Only lossless int-to-float widening is accepted.
    local actual = type(value)
    if actual == 'boolean' then actual = 'bool'
    elseif actual == 'number' then actual = value == math.floor(value) and 'int' or 'float' end
    if actual ~= expected and not (expected == 'float' and actual == 'int') then
      native.setterMismatch(expected,actual)
      return
    end
    if actual == 'int' or actual == 'float' then
      if value ~= value or math.abs(value) == math.huge then error('expected finite parameter') end
      -- Keep reversed documented bounds intact; native semantics need a measurement.
      if min ~= nil and max ~= nil and min <= max then value = math.max(min, math.min(max, value)) end
    end
  end
  local overlay = rawget(self, "__set")
  if not overlay then
    overlay = {}; rawset(self, "__set", overlay)
    __touched[#__touched + 1] = self
  end
  overlay[name] = value
  if type(value) == "number" and native.setParam(rawget(self, "__id"), name, value) then return end
  report("setParameter " .. rawget(self, "type") .. "." .. tostring(name), "")
end
function element.getParameterConnections(self, name)
  if type(name) == 'number' then
    name = parameter_name(self,name); if not name then error('invalid parameter id') end
  end
  local result = {}
  for _, c in ipairs(self.connections or {}) do
    if c:getParameter('Destination') == name then result[#result+1] = c end
  end
  return result
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
function waitBeat(beats) return coroutine.yield(beat2ms(beats or 0)) end
function waitForRelease() return coroutine.yield("release") end

function postEvent(e, delta)
  if delta and delta > 0 then
    local id = e.id or e.voiceId
    if not id then id = native.nextId(); e.id = id end
    spawn(function() wait(delta); postEvent(e) end)
    return id
  end
  local t = e.type
  if t == Event.NoteOn then
    return native.postNote(e)
  elseif t == Event.NoteOff then
    if e.id or e.voiceId then releaseVoice(e.id or e.voiceId) end
  elseif t == Event.Controller then
    controlChange(e.controller or e.number or 0, e.value or 0, e.channel)
  elseif t == Event.PitchBend then
    pitchBend(e.bend or e.value or 0, e.channel)
  elseif t == Event.AfterTouch then
    afterTouch(e.value or 0, e.channel)
  elseif t == Event.PolyAfterTouch then
    polyAfterTouch(e.value or 0, e.note or 0, e.channel)
  elseif t == Event.ProgramChange then
    programChange(e.program or e.value or 0, e.channel)
  else
    report("postEvent", tostring(t))
  end
end
postMidiEvent = postEvent

-- Scripts extend the libraries; Luau's are read-only, so they get copies.
for _, lib in ipairs({ "table", "string", "math" }) do
  local c = {}
  for k, v in pairs(_G[lib]) do c[k] = v end
  _G[lib] = c
end

-- Helpers UVI provides ------------------------------------------------------
_G.table.copy = function(t)
  local c = {}
  for k, v in pairs(t) do c[k] = v end
  return c
end
_G.table.print = function() end
function print() end

-- Loading ------------------------------------------------------------------
dofile, loadfile, load, loadstring, module = nil, nil, nil, nil, nil
local loaded = {}
function require(name)
  if loaded[name] ~= nil then return loaded[name] end
  if string.sub(name, 1, 4) == "uvi." then
    report("module " .. name, "")
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
  ENV[name] = cls
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

-- What the interface export reads (see ScriptHost::interface).
function setBackground(path) ui.background = native.resourcePath(path) end
function setSize(w, h) ui.width, ui.height = w, h end
function setBackgroundColour(c) ui.backgroundColour = c end
function setKeyColour(key, c)
  ui.keys[key+1] = c; ui.revision = ui.revision + 1
  if native.scanKey then native.scanKey("setKeyColour", {key, c}) end
end
function resetKeyColour(key)
  ui.keys[key+1] = nil; ui.revision = ui.revision + 1
  if native.scanKey then native.scanKey("resetKeyColour", {key}) end
end
function makePerformanceView() ui.performance = true end
__ui = ui
