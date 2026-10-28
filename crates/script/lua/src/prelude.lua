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

local function widget(kind, name, value, ...)
	value = restored(name, value)
	local w = setmetatable({ kind = kind, name = name, value = value, changed = nil }, inert)
	w.setValue = function(self, v, run)
		self.value = v
		if run ~= false and self.changed then
			self:changed()
		end
	end
	w.getValue = function(self)
		return self.value
	end
	return w
end

Knob = function(name, value, ...) return widget("Knob", name, value or 0, ...) end
Slider = function(name, value, ...) return widget("Slider", name, value or 0, ...) end
NumBox = function(name, value, ...) return widget("NumBox", name, value or 0, ...) end
Menu = function(name, items, ...) return widget("Menu", name, 1, items, ...) end
OnOffButton = function(name, value, ...) return widget("OnOffButton", name, value or false, ...) end
Button = function(name, ...) return widget("Button", name, false, ...) end
Label = function(name, ...) return widget("Label", name, nil, ...) end
Image = function(name, ...) return widget("Image", name, nil, ...) end
Panel = function(name, ...) return widget("Panel", name, nil, ...) end
Viewport = function(name, ...) return widget("Viewport", name, nil, ...) end
Table = function(name, size, value, ...) return widget("Table", name, value or 0, size, ...) end
makePerformanceView = function() end
setSize = function() end

-- The program tree (layers, keygroups, parameters) comes in a later stage.
Program = setmetatable({ layers = {}, modulations = {}, inserts = {}, eventProcessors = {} }, inert)
this = setmetatable({ parent = Program, program = Program }, inert)

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
	local micros = duration and duration > 0 and math.floor(duration * 1000) or 0
	local id = host_call("PlayNote", note, velocity or 100, 0, micros)
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
		if onInit then
			onInit()
		end
	end,
	[1] = function()
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
			value = 0,
			channel = 1,
		})
	end,
}
