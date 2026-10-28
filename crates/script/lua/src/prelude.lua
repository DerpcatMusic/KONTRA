-- UVIScript on the engine's host calls. `__host(action, ...)` is the only way
-- out; it exists only while a callback runs. `Action` maps names to ids.
-- Units at the host: time in microseconds, volume in millidecibels, tune in
-- millicents, pan in per mille.

local host_call = function(name, ...)
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

-- A widget starts at its saved value when the program has one.
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
local widget_meta = {
	__index = function(w, key)
		local items = rawget(w, "items")
		if items and (key == "text" or key == "selectedText") then
			return items[rawget(w, "value")] or ""
		end
		if is_kind[key] then
			return function(_, ...)
				return widget(key, ...)
			end
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
	w.setValue = function(self, index, v)
		self.values[index] = v
	end
	w.getValue = function(self, index)
		return self.values[index]
	end
	return w
end

-- A widget, from `Kind(name, value, ...)` or `Kind{name, value, field=...}`.
widget = function(kind, name, value, ...)
	local fields = {}
	local rest = { ... }
	if type(name) == "table" then
		fields = name
		name, value = fields[1], fields.value or fields[2]
		rest = { fields[3] }
	end
	local items
	if kind == "Menu" then
		-- Menu(name, items, selected): the value is the item's index.
		items = type(value) == "table" and value or {}
		value = tonumber(rest[1]) or 1
	end
	if type(value) ~= "number" and type(value) ~= "boolean" then
		value = (kind == "OnOffButton" or kind == "Button") and false or 0
	end
	value = restored(name, value)
	local w = setmetatable({ kind = kind, name = name }, widget_meta)
	for k, v in pairs(fields) do
		if type(k) == "string" then
			w[k] = v
		end
	end
	w.value = value
	w.items = items
	local pos, size = fields.pos or {}, fields.size or {}
	local bounds = fields.bounds or {}
	w.x, w.y = pos[1] or bounds[1] or 0, pos[2] or bounds[2] or 0
	w.width, w.height = size[1] or bounds[3] or 0, size[2] or bounds[4] or 0
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
	return w
end

for _, kind in ipairs(kinds) do
	_G[kind] = function(...)
		return widget(kind, ...)
	end
end
makePerformanceView = function() end
setSize = function() end
setBackground = function() end
setBackgroundColour = function() end
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
local ALL_GROUPS = 0x3FFFFFFF
local ALL_EVENTS = 0x3FFFFFFE
local ALLOW_GROUP = 9
local MOD_VALUE = 14
local found, tree = pcall(require, "__program")
if not found or type(tree) ~= "table" then
	tree = { params = {}, db = 0, pan = 0, layers = {} }
end

local flags = { Bypass = true, Mute = true, Solo = true, MidiMute = true, Streaming = true }

local containers = { "layers", "keygroups", "inserts", "auxs", "oscillators", "modulations", "eventProcessors", "busRouters" }

-- An element of the program tree, from the format's node: its parameters
-- and its children by container; modulations also by name.
local function element(node)
	node = node or {}
	local e = setmetatable({
		params = {},
		group = node.group or false,
		sendPre = node.sendPre or 0,
		sendPost = node.sendPost or 0,
	}, inert)
	for _, key in ipairs(containers) do
		local list = {}
		for i, child in ipairs(node[key] or {}) do
			list[i] = element(child)
			list[i].index = i
			list[i].parent = e
		end
		e[key] = list
	end
	for _, m in ipairs(e.modulations) do
		if m.params.Name then
			e.modulations[m.params.Name] = m
		end
	end
	for name, text in pairs(node.params or {}) do
		local n = tonumber(text)
		if flags[name] then
			e.params[name] = n ~= nil and n ~= 0
		else
			e.params[name] = n or text
		end
	end
	e.name = e.params.Name
	e.displayName = e.params.DisplayName
	e.getParameter = function(self, name)
		return self.params[name]
	end
	e.hasParameter = function(self, name)
		return self.params[name] ~= nil
	end
	e.setParameter = function(self, name, value)
		self.params[name] = value
	end
	-- ponytail: modulation connections are not modelled; each one a script
	-- reaches is an element that keeps what it is set to.
	e.getParameterConnections = function()
		return setmetatable({}, {
			__index = function(list, i)
				local c = element()
				rawset(list, i, c)
				return c
			end,
		})
	end
	return e
end

local stale = false

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
	for _, layer in ipairs(Program.layers) do
		local lp = layer.params
		for _, kg in ipairs(layer.keygroups) do
			if kg.group then
				local kp = kg.params
				local silent = lp.Mute or lp.Bypass or kp.Bypass or kp.Mute
				-- The layer's fader plus its aux sends, as the format sums them.
				local fader = lp.Gain or 1
				local gain = fader + layer.sendPre + fader * layer.sendPost
				local db = tree.db + decibels(gain) + decibels(kp.Gain or 1)
				host_call("GroupVolume", kg.group, silent and -200000 or math.floor(db * 1000))
				local pan = math.max(-1, math.min(1, tree.pan + (lp.Pan or 0) + (kp.Pan or 0)))
				host_call("GroupPan", kg.group, math.floor(pan * 1000))
			end
		end
	end
end

local levels = { Gain = true, Pan = true, Mute = true, Bypass = true }

local function leveled(e)
	e.setParameter = function(self, name, value)
		self.params[name] = value
		if levels[name] then
			apply_levels()
		end
	end
	return e
end

Program = element(tree)
-- The part the program sits in.
Program.parent = element({ params = { MidiChannel = "1", MidiInput = "0", Name = "Part 1" } })
for _, layer in ipairs(Program.layers) do
	leveled(layer)
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
	local l = type(layer) == "table" and layer or Program.layers[layer]
	local groups = {}
	for _, kg in ipairs(l and l.keygroups or {}) do
		if kg.group then
			table.insert(groups, kg.group)
		end
	end
	return groups
end

function sendScriptModulation(id, value, ramp, voice)
	host_call("SetEventParArr", voice or ALL_EVENTS, MOD_VALUE, math.floor((value or 0) * 1e6), id)
end

-- ponytail: the ramp from `from` is not drawn; the value lands at `to`.
function sendScriptModulation2(id, from, to, ramp, voice)
	sendScriptModulation(id, to, ramp, voice)
end

function wait(ms)
	coroutine.yield(math.max(0, ms or 0) * 1000)
end

function getTime()
	return host_call("EngineUptime")
end

function getTempo()
	local quarter = host_call("DurationQuarter")
	return quarter > 0 and 60e6 / quarter or 120
end

function beat2ms(beats)
	return beats * 60000 / getTempo()
end

function ms2beat(ms)
	return ms * getTempo() / 60000
end

function waitBeat(beats)
	wait(beat2ms(beats))
end

function isKeyDown(note)
	return host_call("KeyDown", note) ~= 0
end

function playNote(note, velocity, duration, layer, channel, input, vol, pan, tune)
	if type(note) == "table" then
		local t = note
		note, velocity, duration = t[1], t[2], t[3]
		layer, vol, pan, tune = t.layer or t[4], t.vol, t.pan, t.tune
	end
	local micros = duration and duration > 0 and math.floor(duration * 1000) or 0
	local id = host_call("PlayNote", note, velocity or 100, 0, micros)
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

function fadeout(id, ms, kill)
	host_call("FadeOut", id, math.floor((ms or 0) * 1000))
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
	if ok and coroutine.status(co) == "suspended" then
		table.insert(__spawned, { co, waited or 0 })
	end
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
		apply_levels()
		if onInit then
			onInit()
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

function run(f, ...)
	return f(...)
end

for _, name in ipairs({
	"loadData", "saveData", "loadState", "saveState", "loadImpulse", "loadSample",
	"setKeyColour", "setSampleOffset", "browseForFile", "setKeyType", "setKeyPressed",
}) do
	_G[name] = function() end
end
