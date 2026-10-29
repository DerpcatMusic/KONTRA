-- UVIScript on the engine's host calls. `__host(action, ...)` is the only way
-- out; it exists only while a callback runs. `Action` maps names to ids.
-- Units at the host: time in microseconds, volume in millidecibels, tune in
-- millicents, pan in per mille.

-- UVIWorkstation's own prelude ("lua compat", 4.0.9 binary) gives a VM newer
-- than Lua 5.1 back the 5.1 names its scripts use.
string.gfind = string.gfind or string.gmatch
table.getn = table.getn or function(t) return #t end
math.log10 = math.log10 or function(x) return math.log(x, 10) end
math.pow = math.pow or function(a, b) return a ^ b end
math.mod = math.mod or function(a, b) return a % b end
unpack = unpack or table.unpack
loadstring = loadstring or load

-- Calls made while the script loads, before any callback, wait for init.
local pending, pending_routes = {}, {}
local host_call = function(name, ...)
	if not __host then
		table.insert(pending, table.pack(Action[name], ...))
		return
	end
	return __host(Action[name], ...)
end

Event = {
	NoteOn = 1,
	NoteOff = 2,
	ControlChange = 3,
	PitchBend = 4,
	MidiAftertouch = 5,
	PolyAftertouch = 6,
	ProgramChange = 7,
}

-- Anything a script asks of an object this host does not model answers
-- nothing instead of failing the script.
local inert = {}
inert.__index = function()
	return function() end
end

-- A widget's saved value when the program has one, else `value`.
local function restored(name, value)
	local text = name and __saved[name]
	if text == nil then
		return value
	end
	local n = tonumber(text)
	if type(value) == "boolean" then
		return n ~= nil and n ~= 0
	end
	return n or text
end

local kinds = {
	"Knob", "Slider", "NumBox", "Menu", "OnOffButton", "Button", "MultiStateButton",
	"Label", "Image", "Panel", "Viewport", "Table", "WaveView", "AudioMeter", "XY",
	"TextEditor", "MultiSlider", "ModulationEditor", "Waveform", "SVG", "Path", "DnDArea",
	"StepModulation",
}
local is_kind = {}
for _, kind in ipairs(kinds) do
	is_kind[kind] = true
end
local widget

-- A container makes children (`panel:Label{...}`); any other method is inert.
-- A capitalised key no widget has (`w.Widget`) is nil: scripts test for it.
local widget_meta = {
	__index = function(w, key)
		local items = rawget(w, "items")
		if items and (key == "text" or key == "selectedText") then
			return items[rawget(w, "value")] or ""
		end
		if is_kind[key] then
			return function(parent, ...)
				local w = widget(key, ...)
				w.__parent = parent
				return w
			end
		end
		if string.match(key, "^%u") then
			return nil
		end
		return function() end
	end,
}

-- A Table widget: `size` values, each indexed from 1. A saved table is the
-- values separated by spaces, with decimal commas.
local function table_widget(w, name, fields)
	local size = tonumber(fields[2]) or 0
	local values = {}
	local text = name and __saved[name]
	if type(text) == "string" then
		for item in string.gmatch(text, "%S+") do
			table.insert(values, tonumber((string.gsub(item, ",", "."))) or 0)
		end
	end
	for i = #values + 1, size do
		values[i] = tonumber(fields[3]) or 0
	end
	w.values = values
	w.size = size
	w.length = size
	w.min, w.max, w.integer = tonumber(fields[4]) or 0, tonumber(fields[5]) or 1, fields[6] == true
	-- UVIWorkstation 4.0.9: an index outside 1..size reads the widget's own
	-- value, its default clamped to min..max (0x1410c0140, 0x1410ddbd0), and
	-- a write there is dropped (0x1410de650).
	local lo, hi = w.min, w.max
	local own = math.min(math.max(tonumber(fields[3]) or 0, math.min(lo, hi)), math.max(lo, hi))
	local function at(index)
		local i = tonumber(index)
		i = i and (i < 0 and math.ceil(i) or math.floor(i))
		return i and i >= 1 and i <= size and i or nil
	end
	w.setValue = function(self, index, v)
		local i = at(index)
		if i then
			self.values[i] = v
		end
	end
	w.getValue = function(self, index)
		local i = at(index)
		return i and self.values[i] or own
	end
	return w
end

-- Widgets with a saved value, in the order the script made them.
local saved_widgets = {}

-- What a user or host can move, by name: each live macro as the program
-- tree makes it, then each named widget as the script does.
__controls = {}

-- Control `id` (0-based in `__controls`) moves to `value` and runs its
-- `changed`, as a user's edit would; a button reads nonzero as on.
function __control(id, value)
	local c = __controls[id + 1]
	if c and type(c.value) == "boolean" then
		c:setValue(value ~= 0)
	elseif c then
		c:setValue(value)
	end
end
-- The UI the script builds, for the editor: every widget in the order the
-- script made it (its index there names it), the page's size, picture and
-- colour, and whether it asked for a performance view.
__ui = { widgets = {}, width = 720, height = 480 }

-- After the script's body: each persistent widget takes its saved value and
-- runs its `changed`, before `onLoad` (lua.uvi.net, Widget::persistent:
-- "Persistent widgets will call their changed function on reload"; onLoad:
-- "after restoring widgets values").
__restore_errors = {}
function __restore()
	local list = saved_widgets
	saved_widgets = {}
	for _, w in ipairs(list) do
		-- Each `changed` runs on its own: one that fails (VWinds' `dynamics`
		-- names a Macro 7 its Bb Clarinet lacks) leaves the rest restored.
		if w.persistent ~= false then
			local ok, err = pcall(w.setValue, w, restored(w.name, w.value))
			if not ok then
				table.insert(__restore_errors, tostring(w.name) .. ": " .. tostring(err))
			end
		end
	end
end

-- A widget, from `Kind(name, value, ...)` or `Kind{name, value, field=...}`.
widget = function(kind, name, value, ...)
	local fields = { name, value, ... }
	if type(name) == "table" then
		fields = name
		name, value = fields[1], fields.value or fields[2]
	end
	local rest = { fields[3] }
	local items
	if kind == "Menu" then
		-- Menu(name, items, selected): the value is the item's index.
		items = type(value) == "table" and value or {}
		value = tonumber(rest[1]) or 1
	end
	if type(value) ~= "number" and type(value) ~= "boolean" then
		value = kind ~= "OnOffButton" and kind ~= "Button" and 0
	end
	local w = setmetatable({ kind = kind, name = name, alpha = 1, enabled = true, visible = true }, widget_meta)
	for k, v in pairs(fields) do
		if type(k) == "string" then
			w[k] = v
		end
	end
	w.value = value
	w.items = items
	if kind == "Knob" or kind == "Slider" or kind == "NumBox" then
		-- `Knob(name, value, min, max, integer)`: the range by place too.
		for at, key in ipairs({ "min", "max", "integer" }) do
			if rawget(w, key) == nil then
				w[key] = fields[at + 2]
			end
		end
	end
	if kind == "Image" and w.image == nil and type(name) == "string" then
		-- `Image{path, ...}`: its first field is its picture.
		w.image = name
	end
	table.insert(__ui.widgets, w)
	if name ~= nil and __saved[name] ~= nil and kind ~= "Table" then
		table.insert(saved_widgets, w)
	end
	local pos, size = fields.pos or {}, fields.size or {}
	local bounds = fields.bounds or {}
	w.x, w.y = pos[1] or bounds[1] or 0, pos[2] or bounds[2] or 0
	w.width, w.height = size[1] or bounds[3] or 0, size[2] or bounds[4] or 0
	w.width_set, w.height_set = (size[1] or bounds[3]) ~= nil, (size[2] or bounds[4]) ~= nil
	w.setStripImage = function(self, image, steps)
		self.stripImage, self.stripSteps = image, steps
	end
	if kind == "Table" then
		return table_widget(w, name, fields)
	end
	w.setValue = function(self, v, run)
		self.value = v
		if run ~= false and type(rawget(self, "changed")) == "function" then
			self:changed()
		end
	end
	w.getValue = function(self)
		return self.value
	end
	if name ~= nil then
		table.insert(__controls, w)
	end
	return w
end

for _, kind in ipairs(kinds) do
	_G[kind] = function(...)
		return widget(kind, ...)
	end
end
makePerformanceView = function()
	__ui.performance = true
end
setSize = function(width, height)
	__ui.width, __ui.height = tonumber(width) or __ui.width, tonumber(height) or __ui.height
end
setBackground = function(path)
	__ui.background = path
end
setBackgroundColour = function(colour)
	__ui.colour = colour
end
-- Enumerations whose values only need to be distinct (`Unit.Seconds`).
local function enum()
	local next_value = 0
	return setmetatable({}, {
		__index = function(t, key)
			next_value = next_value + 1
			rawset(t, key, next_value)
			return next_value
		end,
	})
end
for _, name in ipairs({ "Unit", "Mapper", "DisplayType", "Justification", "MouseEvent", "Direction" }) do
	_G[name] = enum()
end
__API_VERSION__ = 23
__VERSION__ = "3.1.1"
__PLAYER__ = "Falcon"

-- The program tree from the format's `__program` module: parameters by
-- name; a layer's or keygroup's gain, pan, mute and bypass set the levels of
-- the engine groups its keygroups made.
-- The engine's event ids (kontra_vm::action::event).
local ALL_GROUPS = -1
local ALL_EVENTS = -1
local ALLOW_GROUP = 10
local MOD_VALUE = 30
local ZONE_SLOT = 15
local found, tree = pcall(require, "__program")
if not found or type(tree) ~= "table" then
	tree = { params = {}, db = 0, pan = 0, layers = {} }
end

local flags = { Bypass = true, Mute = true, Solo = true, MidiMute = true, Streaming = true }

local containers = { "layers", "keygroups", "inserts", "auxs", "oscillators", "modulations", "eventProcessors", "busRouters" }

-- A connection the engine carries, from the format's `{Source, Destination,
-- Ratio, route key, amount per unit of Ratio, Bypass}`: setting its Ratio or
-- Bypass sets its route's amount (none while bypassed). One metatable for
-- all keeps a big program's thousands light.
local fields_at = { Source = 1, Destination = 2, Ratio = 3, Bypass = 6 }
local connection_definitions = {
	{ name = "Ratio", id = "Ratio", default = 0, min = -1, max = 1 },
	{ name = "Source", id = "Source", default = 0, min = 0, max = 1 },
	{ name = "Destination", id = "Destination", default = 0, min = 0, max = 1 },
	{ name = "Bypass", id = "Bypass", default = 0, min = 0, max = 1 },
}
local Connection = {}
Connection.__index = function(c, key)
	if key == "parameterDefinitions" then
		return connection_definitions
	end
	return Connection[key]
end
function Connection.getParameter(c, name)
	local at = fields_at[name]
	return at and c[at]
end
function Connection.hasParameter(c, name)
	return fields_at[name] ~= nil
end
function Connection.setParameter(c, name, value)
	if name == "Ratio" and type(value) == "number" then
		c[3] = value
	elseif name == "Bypass" then
		c[6] = value == true or (type(value) == "number" and value ~= 0)
	else
		return
	end
	if c[4] >= 0 then
		local amount = c[6] and 0 or math.floor(c[3] * c[5] * 1e6 + 0.5)
		if __host then
			host_call("RouteAmount", c[4], amount)
		else
			-- Before init only a route's last amount is kept: AO's knobs set
			-- the same routes hundreds of times while they load.
			pending_routes[c[4]] = amount
		end
	end
end

-- An element of the program tree, from the format's node: its parameters
-- and its children by container; modulations also by name.
-- ponytail: a definition's range is 0..max(1, value); the XML carries no
-- ranges, and only editor widgets read them.
local function definitions(e)
	local list = {}
	for name, value in pairs(e.params) do
		local n = type(value) == "number" and value or 0
		table.insert(list, { name = name, id = name, default = n, min = 0, max = math.max(1, n) })
	end
	return list
end

-- Falcon elements are userdata: scripts tell one element from a list of
-- elements by type(). Each proxy's metatable holds only the shared `__index`
-- and its own fields as `__newindex`; methods are shared, containers made on
-- first use, so a big program's ten thousand elements stay light.
local EMPTY = {}
local LINKS, CONNECTIONS = {}, {}
local is_container = {}
for _, key in ipairs(containers) do
	is_container[key] = true
end
local function noop() end
local function fields_of(e)
	return getmetatable(e).__newindex
end

local Element = {}
function Element.getParameter(self, name)
	return self.params[name]
end
function Element.hasParameter(self, name)
	return self.params[name] ~= nil
end
function Element.setParameter(self, name, value)
	self.params[name] = value
end

local element
-- One past an element's connections on a parameter is an element that keeps
-- what it is set to.
local blank_connections = {
	__index = function(l, i)
		local c = element()
		c.params.Ratio = 0
		rawset(l, i, c)
		return c
	end,
}
-- The connections on parameter `name`, in order; one list per name.
function Element.getParameterConnections(self, name)
	local fields = fields_of(self)
	local cache = fields[CONNECTIONS]
	if not cache then
		cache = {}
		fields[CONNECTIONS] = cache
	end
	local list = cache[name]
	if list then
		return list
	end
	list = {}
	for _, i in ipairs(fields[LINKS] or EMPTY) do
		local c = tree.connections and tree.connections[i]
		if c and c[2] == name then
			table.insert(list, setmetatable(c, Connection))
		end
	end
	cache[name] = setmetatable(list, blank_connections)
	return list
end

local function element_index(e, key)
	local fields = fields_of(e)
	local value = fields[key]
	if value ~= nil then
		return value
	end
	if key == "sends" then
		return e.busRouters
	end
	if is_container[key] then
		local list = {}
		fields[key] = list
		return list
	end
	-- A sample oscillator's sample, as UVIWorkstation 4.0.9 builds it
	-- (0x140f3f2a0: name, type, channels, ..., basenote, finetune, loops).
	-- ponytail: name, basenote and finetune only; the rest when a script reads them.
	local path = fields.params and fields.params.SamplePath
	if key == "sampleInfo" and type(path) == "string" and path ~= "" then
		local p = fields.params
		local info = { name = string.match(path, "[^/\\]*$"), basenote = p.BaseNote, finetune = p.FineTune }
		fields.sampleInfo = info
		return info
	end
	if key == "parameterDefinitions" then
		-- Built once: AO asks one element for them on every knob it makes.
		local list = definitions(fields)
		fields.parameterDefinitions = list
		return list
	end
	return Element[key] or noop
end

-- Macros the engine reads live, and their setter ([`set_macro`]).
local macros, set_macro = {}, nil
function element(node)
	node = node or {}
	-- The format's params, converted in place.
	local params = node.params or {}
	for name, text in pairs(params) do
		local n = tonumber(text)
		if flags[name] then
			params[name] = n ~= nil and n ~= 0
		else
			params[name] = n or text
		end
	end
	local fields = {
		params = params,
		group = node.group or false,
		bus = node.bus or false,
		name = params.Name,
		displayName = params.DisplayName,
		mod = node.mod,
		[LINKS] = node.links,
	}
	local e = newproxy(true)
	local meta = getmetatable(e)
	meta.__index = element_index
	meta.__newindex = fields
	for _, key in ipairs(containers) do
		local children = node[key]
		if children and #children > 0 then
			local list = {}
			for i, child in ipairs(children) do
				local c = element(child)
				c.index = i
				c.parent = e
				list[i] = c
			end
			fields[key] = list
		end
	end
	for _, m in ipairs(fields.modulations or EMPTY) do
		if m.params.Name then
			fields.modulations[m.params.Name] = m
		end
	end
	if node.mod then
		fields.setParameter = set_macro
		table.insert(macros, e)
		table.insert(__controls, {
			name = params.Name,
			setValue = function(_, v)
				e:setParameter("Value", v)
			end,
		})
	end
	return e
end

local stale = false

-- Calls `f` on `e` and every element under it that has a bus.
local function each_bus(e, f)
	if e.bus then
		f(e)
	end
	local fields = fields_of(e)
	for _, key in ipairs(containers) do
		for _, child in ipairs(fields[key] or EMPTY) do
			each_bus(child, f)
		end
	end
end

local function decibels(gain)
	return gain and gain > 0 and 20 * math.log10(gain) or -200
end

-- Pushes every group's level when a callback can reach the host.
local function apply_levels()
	if not __host then
		stale = true
		return
	end
	stale = false
	-- A layer's fader and an aux's or rack chain's gain are their buses'
	-- volumes.
	each_bus(Program, function(e)
		local off = e.params.Bypass or e.params.Mute
		local db = off and -200 or decibels(e.params.Gain or 1)
		host_call("BusVolume", e.bus, math.floor(db * 1000))
	end)
	for _, layer in ipairs(Program.layers) do
		local lp = layer.params
		for _, kg in ipairs(layer.keygroups) do
			if kg.group then
				local kp = kg.params
				local silent = lp.Mute or lp.Bypass or kp.Bypass or kp.Mute
				local db = tree.db + decibels(kp.Gain or 1)
				host_call("GroupVolume", kg.group, silent and -200000 or math.floor(db * 1000))
				local pan = math.max(-1, math.min(1, tree.pan + (lp.Pan or 0) + (kp.Pan or 0)))
				host_call("GroupPan", kg.group, math.floor(pan * 1000))
			end
		end
	end
end

local levels = { Gain = true, Pan = true, Mute = true, Bypass = true }

local function set_level(self, name, value)
	self.params[name] = value
	if levels[name] then
		apply_levels()
	end
end

-- A live macro's `Value` goes to the engine as script modulation `mod`:
-- its position, bipolar or not (kontra-format-uvi matrix.rs).
local function send_macro(e)
	local v = tonumber(e.params.Value) or 0
	host_call("SetEventParArr", ALL_EVENTS, MOD_VALUE, math.floor(v * 1e6 + 0.5), e.mod, 0)
end
function set_macro(self, name, value)
	self.params[name] = value
	-- Before init only the last value counts: init sends every macro's.
	if name == "Value" and __host then
		send_macro(self)
	end
end

local function leveled(e)
	e.setParameter = set_level
	return e
end

Program = element(tree)
-- The part the program sits in.
Program.parent = element({ params = { MidiChannel = "1", MidiInput = "0", Name = "Part 1" } })
each_bus(Program, leveled)
for _, layer in ipairs(Program.layers) do
	for _, kg in ipairs(layer.keygroups) do
		leveled(kg)
	end
end
this = setmetatable({ parent = Program, program = Program }, inert)

-- The index of the layer named `name` (its display name or its own).
function findLayer(name)
	for i, layer in ipairs(Program.layers) do
		if layer.params.DisplayName == name or layer.params.Name == name then
			return i
		end
	end
	return nil
end

-- Engine groups a `layer` (index or layer) plays.
local function layer_groups(layer)
	local l = type(layer) == "userdata" and layer or Program.layers[layer]
	local groups = {}
	for _, kg in ipairs(l and l.keygroups or {}) do
		if kg.group then
			table.insert(groups, kg.group)
		end
	end
	return groups
end

-- Moves script modulation `id` to `value` over `ramp` ms, for one voice or all.
function sendScriptModulation(id, value, ramp, voice)
	local micros = math.floor(math.max(0, ramp or 0) * 1000)
	host_call("SetEventParArr", voice or ALL_EVENTS, MOD_VALUE, math.floor((value or 0) * 1e6), id, micros)
end

function sendScriptModulation2(id, from, to, ramp, voice)
	sendScriptModulation(id, from, 0, voice)
	sendScriptModulation(id, to, ramp, voice)
end

function wait(ms)
	coroutine.yield(math.max(0, ms or 0) * 1000)
end

function getTime()
	return host_call("EngineUptime") or 0
end

function getTempo()
	local quarter = host_call("DurationQuarter")
	return quarter and quarter > 0 and 60e6 / quarter or 120
end

function beat2ms(beats)
	return beats * 60000 / getTempo()
end

function ms2beat(ms)
	return ms * getTempo() / 60000
end

-- Beats since the engine started, at the current tempo.
-- ponytail: the host's song position is not passed in; song time is running time.
function getRunningBeatTime()
	return getTime() / beat2ms(1)
end
getBeatTime = getRunningBeatTime

function getBeatDuration()
	return beat2ms(1)
end

function waitBeat(beats)
	wait(beat2ms(beats))
end

function isKeyDown(note)
	return host_call("KeyDown", note) ~= 0
end

function playNote(note, velocity, duration, layer, channel, input, vol, pan, tune, oscIndex)
	if type(note) == "table" then
		local t = note
		note, velocity, duration = t.note or t[1], t.velocity or t[2], t.duration or t[3]
		layer, vol, pan, tune, oscIndex = t.layer or t[4], t.vol, t.pan, t.tune, t.oscIndex
	end
	local micros = duration and duration > 0 and math.floor(duration * 1000) or 0
	local id = host_call("PlayNote", note, velocity or 100, 0, micros)
	if oscIndex and oscIndex >= 0 then
		host_call("SetEventPar", id, ZONE_SLOT, oscIndex)
	end
	if layer ~= nil and layer ~= -1 then
		host_call("SetEventParArr", id, ALLOW_GROUP, 0, ALL_GROUPS)
		for _, g in ipairs(layer_groups(layer)) do
			host_call("SetEventParArr", id, ALLOW_GROUP, 1, g)
		end
	end
	if vol and vol ~= 1 then
		changeVolume(id, vol, false, true)
	end
	if pan and pan ~= 0 then
		changePan(id, pan, false, true)
	end
	if tune and tune ~= 0 then
		changeTune(id, tune, false, true)
	end
	return id
end

function releaseVoice(id)
	host_call("NoteOff", id)
end

function postEvent(e)
	if e.type == Event.NoteOn then
		-- The incoming note was held back; this plays it, ending with it.
		return host_call("PlayNote", e.note, e.velocity, 0, -1)
	elseif e.type == Event.NoteOff then
		host_call("NoteOff", e.id)
	end
end

local function millidecibels(gain)
	return gain > 0 and math.floor(20000 * math.log10(gain)) or -100000
end

function changeVolume(id, gain, relative, immediate)
	host_call("ChangeVol", id, millidecibels(gain), relative and 1 or 0)
end

function changeVolumedB(id, db, relative, immediate)
	host_call("ChangeVol", id, math.floor(db * 1000), relative and 1 or 0)
end

function changeTune(id, semitones, relative, immediate)
	host_call("ChangeTune", id, math.floor(semitones * 100000), relative and 1 or 0)
end

function changePan(id, pan, relative, immediate)
	host_call("ChangePan", id, math.floor(pan * 1000), relative and 1 or 0)
end

-- ponytail: always ends the voice (kill); holding at silence waits for UVI's default, unread.
function fadeout(id, ms, kill)
	host_call("FadeOut", id, math.floor((ms or 0) * 1000), 1)
end

function print(...)
	local parts = {}
	for i = 1, select("#", ...) do
		parts[i] = tostring((select(i, ...)))
	end
	host_call("Message", table.concat(parts, " "))
end

function spawn(f, ...)
	local co = coroutine.create(f)
	local ok, waited = coroutine.resume(co, ...)
	-- A coroutine that failed is queued too: resuming it counts the fault.
	if not ok or coroutine.status(co) == "suspended" then
		table.insert(__spawned, { co, ok and waited or 0 })
	end
end

-- loadSample(oscillator, path, done): the samples a script loads were put
-- on their oscillators when the program loaded (`__record`); here it only
-- reports success.
local loads
function loadSample(oscillator, path, done)
	if loads and type(oscillator) == "userdata" and oscillator.parent then
		table.insert(loads, { oscillator.parent.group, oscillator.index - 1, path, oscillator.params.BaseNote })
	end
	if type(done) == "function" then
		done({ success = true })
	end
end

-- ponytail: impulses are not loaded; the callback reports success so a
-- script that mutes while one loads (VWinds' room) unmutes.
function loadImpulse(insert, path, done)
	if type(done) == "function" then
		done({ success = true })
	end
end

-- The program as it loads, run once off the audio thread: init and onLoad,
-- then what they spawned, every host call answered 0 and every wait
-- skipped. Returns the samples it loaded: {group, slot, path, base note}.
function __record()
	loads = {}
	__host = function()
		return 0
	end
	local runs = { coroutine.create(__entries[0]) }
	local i = 1
	while runs[i] and i <= 256 do
		local co, steps = runs[i], 0
		while coroutine.status(co) == "suspended" and steps < 1000 do
			coroutine.resume(co)
			steps = steps + 1
		end
		for _, spawned in ipairs(__spawned) do
			table.insert(runs, spawned[1])
		end
		table.clear(__spawned)
		i = i + 1
	end
	return loads
end

local function event(kind)
	return {
		type = kind,
		id = host_call("EventId"),
		note = host_call("EventNote"),
		velocity = host_call("EventVelocity"),
		channel = 1,
		layer = 0,
	}
end

-- Entries by the engine's callback number: init, note, release, controller.
__entries = {
	[0] = function()
		for _, call in ipairs(pending) do
			__host(table.unpack(call, 1, call.n))
		end
		pending = {}
		for route, amount in pairs(pending_routes) do
			__host(Action.RouteAmount, route, amount)
		end
		pending_routes = {}
		apply_levels()
		for _, m in ipairs(macros) do
			send_macro(m)
		end
		if onInit then
			onInit()
		end
		-- The state the program saved (`onSave`) comes back after init.
		local found, state = pcall(require, "__state")
		if onLoad and found and state ~= true then
			onLoad(state)
		end
	end,
	[1] = function()
		if stale then
			apply_levels()
		end
		local e = event(Event.NoteOn)
		-- A UVI note sounds only when the script posts it.
		host_call("IgnoreEvent", e.id)
		onNote(e)
	end,
	[2] = function()
		local e = event(Event.NoteOff)
		host_call("IgnoreEvent", e.id)
		onRelease(e)
	end,
	-- The editor moved widget `__widget` (its place in `__ui.widgets`) to
	-- `__widget_value`: its `changed` runs, as a click would. A button's
	-- value stays a boolean (0 is true in Lua), a menu's an index.
	[4] = function()
		local w, v = __ui.widgets[__widget], __widget_value
		if w == nil or rawget(w, "setValue") == nil then
			return
		end
		if type(w.value) == "boolean" then
			v = v ~= 0
		elseif w.kind == "Menu" or rawget(w, "integer") == true then
			v = math.floor(v + 0.5)
		end
		w:setValue(v)
	end,
	[3] = function()
		onController({
			type = Event.ControlChange,
			controller = host_call("CcNum"),
			value = host_call("Cc", host_call("CcNum")),
			channel = 1,
		})
	end,
}

-- Builtins the engine answers with what it has; files, state and key colours
-- belong to the editor and are inert here.
function table.copy(t)
	local out = {}
	for k, v in pairs(t) do
		out[k] = type(v) == "table" and table.copy(v) or v
	end
	return out
end

function controlChange(cc, value)
	host_call("SetController", cc, value)
end

function fadein(id, ms)
	host_call("FadeIn", id, math.floor((ms or 0) * 1000))
end

-- ponytail: the fade is drawn as a jump to its end level.
function fade2(id, from, to, ms, kill)
	if (to or 0) <= 0 then
		fadeout(id, ms, kill)
	else
		changeVolume(id, to, false, true)
	end
end

-- class 'Name' or class 'Name' (Base): luabind-style classes. Calling the
-- class makes an instance and runs its __init.
local function construct(cls, ...)
	local object = setmetatable({}, cls)
	if cls.__init then
		cls.__init(object, ...)
	end
	return object
end

function class(name)
	local cls = {}
	cls.__index = cls
	setmetatable(cls, { __call = construct })
	_G[name] = cls
	return function(base)
		setmetatable(cls, { __index = base, __call = construct })
	end
end

-- uvi.ChordRec: names the chord of sorted notes by pitch-class templates.
-- Returns root, kind ("" major, "m" minor, ...) and bass pitch classes.
ChordRec = {}
local CHORDS = {
	{ "", { 0, 4, 7 } }, { "m", { 0, 3, 7 } }, { "7", { 0, 4, 7, 10 } },
	{ "maj7", { 0, 4, 7, 11 } }, { "m7", { 0, 3, 7, 10 } }, { "dim", { 0, 3, 6 } },
	{ "aug", { 0, 4, 8 } }, { "sus4", { 0, 5, 7 } }, { "sus2", { 0, 2, 7 } },
	{ "5", { 0, 7 } },
}
function ChordRec.chordKind(notes)
	local classes = {}
	for _, note in ipairs(notes) do
		classes[note % 12] = true
	end
	local count = 0
	for _ in pairs(classes) do
		count = count + 1
	end
	for _, chord in ipairs(CHORDS) do
		local kind, intervals = chord[1], chord[2]
		if #intervals == count then
			for root in pairs(classes) do
				local all = true
				for _, interval in ipairs(intervals) do
					all = all and classes[(root + interval) % 12] == true
				end
				if all then
					return root, kind, notes[1] % 12
				end
			end
		end
	end
end

function run(f, ...)
	return f(...)
end

for _, name in ipairs({
	"loadData", "saveData", "loadState", "saveState",
	"setKeyColour", "setSampleOffset", "browseForFile", "setKeyType", "setKeyPressed",
}) do
	_G[name] = function() end
end
