-- UVIScript on the engine's host calls. `__host(action, ...)` is the only way
-- out; it exists only while a callback runs. `Action` maps names to ids.
-- Units at the host: time in microseconds, volume in millidecibels, tune in
-- millicents, pan in per mille.

-- UVIWorkstation's own prelude ("lua compat", 4.0.9 binary) gives a VM newer
-- than Lua 5.1 back the 5.1 names its scripts use.
string.gfind = string.gfind or string.gmatch

-- Luau's matcher retries from every start, so a pattern like "(.-)/()" over a
-- long subject with no "/" left costs the subject's length squared in one
-- call nothing can interrupt. A literal the pattern requires that the rest of
-- the subject lacks means no match: one plain search says so first. Same
-- results, linear.
do
	local find, match, gmatch, sub = string.find, string.match, string.gmatch, string.sub
	local required_cache = {}
	-- The first character `p` requires (nil if none is certain).
	local function required(p)
		local cached = required_cache[p]
		if cached ~= nil then
			return cached or nil
		end
		local found = false
		local i, n = 1, #p
		if sub(p, 1, 1) == "^" then
			i = n + 1 -- anchored: leave it to the matcher
		end
		while i <= n do
			local c = sub(p, i, i)
			local literal, at = nil, i + 1
			if c == "%" then
				local d = sub(p, i + 1, i + 1)
				if d == "b" then
					at = i + 4
				elseif d == "f" then
					at = i + 2
					if sub(p, at, at) == "[" then
						at = (find(p, "]", at, true) or n) + 1
					end
				elseif d ~= "" and find(d, "^%p") then
					literal, at = d, i + 2
				else
					at = i + 2
				end
			elseif c == "[" then
				local close = i + 1
				if sub(p, close, close) == "^" then
					close = close + 1
				end
				if sub(p, close, close) == "]" then
					close = close + 1
				end
				while close <= n and sub(p, close, close) ~= "]" do
					close = close + (sub(p, close, close) == "%" and 2 or 1)
				end
				at = close + 1
			elseif c == "(" or c == ")" or c == "." or c == "$" then
				at = i + 1
			else
				literal = c
			end
			local q = sub(p, at, at)
			if literal and q ~= "*" and q ~= "-" and q ~= "?" then
				found = literal
				break
			end
			if c == "(" or c == ")" then
				-- zero width: a quantifier cannot follow
			elseif q == "*" or q == "-" or q == "?" or q == "+" then
				at = at + 1
			end
			i = at
		end
		required_cache[p] = found
		return found or nil
	end
	-- Whether a match can start at or after `init` (false: certainly none).
	local function possible(s, p, init)
		local lit = required(p)
		return not lit or find(s, lit, init, true) ~= nil
	end
	string.find = function(s, p, init, plain)
		if plain or type(s) ~= "string" or type(p) ~= "string" or possible(s, p, init) then
			return find(s, p, init, plain)
		end
		return nil
	end
	string.match = function(s, p, init)
		if type(s) ~= "string" or type(p) ~= "string" or possible(s, p, init) then
			return match(s, p, init)
		end
		return nil
	end
	local function gmatch_linear(s, p)
		if type(s) ~= "string" or type(p) ~= "string" or not required(p) then
			return gmatch(s, p)
		end
		local pos, last = 1, nil
		return function()
			while pos <= #s + 1 and possible(s, p, pos) do
				local r = table.pack(find(s, p, pos))
				if r[1] == nil then
					break
				end
				local first, stop = r[1], r[2]
				if stop + 1 == last then
					pos = first + 1 -- an empty match where the last ended: move on
				else
					pos, last = stop + 1, stop + 1
					if r.n == 2 then
						return sub(s, first, stop)
					end
					return table.unpack(r, 3, r.n)
				end
			end
			pos = #s + 2
			return nil
		end
	end
	string.gmatch = gmatch_linear
	string.gfind = gmatch_linear
end
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

-- UVI's event types are MIDI status bytes: scripts build events with
-- `type = 176` (VWinds' hornScript3 logs `type=224` for a bend).
Event = {
	NoteOn = 144,
	NoteOff = 128,
	ControlChange = 176,
	PitchBend = 224,
	AfterTouch = 208,
	PolyAfterTouch = 160,
	ProgramChange = 192,
	-- The spellings this prelude used first.
	MidiAftertouch = 208,
	PolyAftertouch = 160,
}
Event.Type = Event

-- The controller-type event a handler is hearing, and what it posted of
-- it: UVI keeps an event its handler does not post from the instrument,
-- and a posted one reaches it as posted (changed fields and all).
local hearing = nil
local arrived = nil
local posted = nil
local posted_delta = 0
-- Settles the heard event once: when its handler returns, or at the
-- handler's first wait, while the engine still waits on the callback. A
-- postEvent after that sends the event on its own.
local decide

-- Anything a script asks of an object this host does not model answers
-- nothing instead of failing the script.
local inert = {}
inert.__index = function()
	return function() end
end

-- A saved value's text as a value like `value`: a boolean, a number, else
-- the text.
local function parsed(text, value)
	local n = tonumber(text)
	if type(value) == "boolean" then
		return n ~= nil and n ~= 0
	end
	return n or text
end

-- A widget's saved value when the program has one, else `value`.
local function restored(name, value)
	local text = name and __saved[name]
	if text == nil then
		return value
	end
	return parsed(text, value)
end

local kinds = {
	"Knob", "Slider", "NumBox", "Menu", "OnOffButton", "Button", "MultiStateButton",
	"Label", "Image", "Panel", "Viewport", "Table", "WaveView", "AudioMeter", "XY",
	"TextEditor", "MultiSlider", "ModulationEditor", "Waveform", "SVG", "Path", "DnDArea",
	"StepModulation", "ParamKnob", "ParamSlider", "ParamOnOffButton", "ParamMenu", "ParamNumBox",
	"ParameterValue", "FileSelector",
}
-- Per-kind set-up the widgets chunk (widgets.lua) registers, and what
-- watches a program parameter (`ParameterValue`).
__ui_init = {}
local init = __ui_init
local pressed = { Button = true, OnOffButton = true, MultiStateButton = true }
local is_kind = {}
for _, kind in ipairs(kinds) do
	is_kind[kind] = true
end
local widget

-- A container makes children (`panel:Label{...}`); any other method is inert.
-- A capitalised key no widget has (`w.Widget`) is nil: scripts test for it.
-- A widget's value lives in `__value`: assigning `w.value` runs its
-- `changed`, as UVIWorkstation's setter does (VWinds' preset arrows set
-- `menu.value` and rely on it), except from inside its own `changed`.
-- Widget work may suspend inside changed. The machine clears a cancelled
-- thread's scope before reusing it; any widget still holding that scope can
-- then accept a new changed callback.
local route_scopes = setmetatable({}, {__mode = "k"})
local route_generation = 0
__route_scopes = route_scopes
local function enter_route_scope(co)
	local scope = route_scopes[co]
	if not scope then
		scope = {}
		route_scopes[co] = scope
	end
	if not scope[1] then
		-- A cancelled widget can still refer to this reused scope.
		route_generation += 1
		scope[2] = route_generation
	end
	scope[1] = (scope[1] or 0) + 1
	return scope
end
local function leave_route_scope(scope)
	scope[1] -= 1
	if scope[1] == 0 then
		scope[1] = nil
		scope[2] = nil
	end
end

-- Every write counts except one made by this control's routing coroutine,
-- including echoes in changed. A write by another coroutine still counts.
local function wrote(w, v)
	rawset(w, "__value", v)
	local co = coroutine.running()
	local scope = co and route_scopes[co]
	if not (scope and scope[w]) then
		rawset(w, "__writes", (rawget(w, "__writes") or 0) + 1)
	end
end
local function run_changed(w, mods)
	local changed = rawget(w, "changed")
	local owner = rawget(w, "__changing")
	local generation = rawget(w, "__changing_generation")
	if type(changed) ~= "function" or owner == true or (owner and generation and generation == owner[2]) then
		return
	end
	local co = coroutine.running()
	local scope = co and route_scopes[co]
	if scope and scope[2] then
		rawset(w, "__changing_generation", scope[2])
		rawset(w, "__changing", scope)
	else
		rawset(w, "__changing", true)
	end
	local ok, err = pcall(changed, w, mods)
	rawset(w, "__changing", nil)
	if not ok then
		error(err, 0)
	end
end

-- A menu's items: read, renamed (VWinds marks a loaded preset `Name*`),
-- added and cleared. Each change moves the list's revision, which the
-- editor reads to copy the items again.
local function items_changed(w)
	rawset(w, "__items_revision", (rawget(w, "__items_revision") or 0) + 1)
end
local menu_methods = {
	getText = function(w, i)
		return rawget(w, "items")[i] or ""
	end,
	setItem = function(w, i, text)
		rawget(w, "items")[i] = text
		items_changed(w)
	end,
	addItem = function(w, text)
		table.insert(rawget(w, "items"), text)
		items_changed(w)
		return #rawget(w, "items")
	end,
	clear = function(w)
		table.clear(rawget(w, "items"))
		items_changed(w)
	end,
}

-- A widget's place and size: kept as its x, y, width and height, never as
-- the fields themselves, so one moved or sized after it is made
-- (`w.pos = {x, y}`, as V Winds does for its tuning layout) draws there.
local geometry = { pos = true, position = true, size = true, bounds = true }
local alias = { w = "width", h = "height" }
local function place(w, key, v)
	local n = function(i)
		return tonumber(v[i])
	end
	local at = key == "bounds" and 2 or 0
	if key ~= "size" then
		rawset(w, "x", n(1) or w.x)
		rawset(w, "y", n(2) or w.y)
	end
	if key == "size" or key == "bounds" then
		rawset(w, "width", n(at + 1) or w.width)
		rawset(w, "height", n(at + 2) or w.height)
		rawset(w, "width_set", n(at + 1) ~= nil or w.width_set)
		rawset(w, "height_set", n(at + 2) ~= nil or w.height_set)
	end
end

local widget_meta = {
	__newindex = function(w, key, v)
		if key == "length" and rawget(w, "__resize") then
			w:__resize(v)
		elseif alias[key] then
			rawset(w, alias[key], v)
			rawset(w, alias[key] .. "_set", true)
		elseif key == "selected" then
			w.value = v
		elseif key == "value" and rawget(w, "__target") then
			-- A bound widget mirrors its parameter; only a `ParameterValue` writes it.
			if rawget(w, "kind") == "ParameterValue" then
				local target = rawget(w, "__target")
				target[1]:setParameter(target[2], v)
			end
		elseif key == "value" then
			wrote(w, v)
			run_changed(w)
		elseif geometry[key] and type(v) == "table" then
			place(w, key, v)
		else
			rawset(w, key, v)
		end
	end,
	__index = function(w, key)
		if key == "value" or key == "selected" then
			local target = rawget(w, "__target")
			if target then
				return target[1]:getParameter(target[2])
			end
			return rawget(w, "__value")
		elseif alias[key] then
			return rawget(w, alias[key])
		elseif key == "displayName" or key == "tooltip" then
			return rawget(w, "name")
		elseif key == "length" and rawget(w, "items") then
			return #rawget(w, "items")
		elseif key == "length" and rawget(w, "__resize") then
			return #rawget(w, "values")
		elseif key == "length" then
			-- Only lists have one: Augmented Orchestra tells menus from
			-- knobs by `w.length ~= nil` and hides the menus.
			return nil
		elseif key == "bound" then
			return rawget(w, "__target") ~= nil
		elseif key == "pos" or key == "position" then
			return { rawget(w, "x"), rawget(w, "y") }
		elseif key == "size" then
			return { rawget(w, "width"), rawget(w, "height") }
		elseif key == "bounds" then
			return { rawget(w, "x"), rawget(w, "y"), rawget(w, "width"), rawget(w, "height") }
		end
		local items = rawget(w, "items")
		if items and (key == "text" or key == "selectedText") then
			return items[rawget(w, "__value")] or ""
		end
		if items and menu_methods[key] then
			return menu_methods[key]
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

-- A saved table's cells: values separated by spaces, with decimal commas.
local function cells(text)
	local values = {}
	if type(text) == "string" then
		for item in string.gmatch(text, "%S+") do
			table.insert(values, tonumber((string.gsub(item, ",", "."))) or 0)
		end
	end
	return values
end

-- A Table widget: `size` values, each indexed from 1.
local function table_widget(w, name, fields)
	local size = tonumber(fields[2]) or 0
	local values = cells(name and __saved[name])
	for i = #values + 1, size do
		values[i] = tonumber(fields[3]) or 0
	end
	w.values = values
	w.size = size
	w.min, w.max, w.integer = tonumber(fields[4]) or 0, tonumber(fields[5]) or 1, fields[6] == true
	-- UVIWorkstation 4.0.9: an index outside 1..size reads the widget's own
	-- value, its default clamped to min..max, and
	-- a write there is dropped.
	local lo, hi = w.min, w.max
	local own = math.min(math.max(tonumber(fields[3]) or 0, math.min(lo, hi)), math.max(lo, hi))
	local function at(index)
		local i = tonumber(index)
		i = i and (i < 0 and math.ceil(i) or math.floor(i))
		return i and i >= 1 and i <= #values and i or nil
	end
	-- `length = n` resizes it; entries added take the widget's default.
	w.__resize = function(self, n)
		n = math.max(0, math.floor(tonumber(n) or #values))
		for i = #values + 1, n do
			values[i] = tonumber(rawget(self, "default")) or tonumber(fields[3]) or 0
		end
		for i = #values, n + 1, -1 do
			values[i] = nil
		end
	end
	-- ponytail: Table:setRange moves the range the out-of-range read clamps
	-- to; the stored values are not clamped (the AO step table's checkbox
	-- switches between 0..1 and -1..1; unmeasured whether UVI clamps).
	w.setRange = function(self, min, max)
		lo, hi = tonumber(min) or lo, tonumber(max) or hi
		self.min, self.max = lo, hi
		own = math.min(math.max(own, math.min(lo, hi)), math.max(lo, hi))
	end
	w.setValue = function(self, index, v, call)
		local i = at(index)
		if i then
			values[i] = v
			-- `changed(self, index)`, unless the script asked it not to.
			if call ~= false then
				run_changed(self, i)
			end
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
-- `setValue`'s second argument for a move that runs `changed` even when the
-- value stays (a user's edit).
__FORCE = {}
local function force(w)
	return w.kind ~= "Table" and __FORCE or nil
end

-- Control `id` (0-based in `__controls`) moves to `value` and runs its
-- `changed`, as a user's edit would; a button reads nonzero as on.
function __control(id, value)
	local c = __controls[id + 1]
	if c and type(c.value) == "boolean" then
		c:setValue(value ~= 0, __FORCE)
	elseif c then
		c:setValue(value, force(c))
	end
end
-- Control `id` moves to its own value's place in its range times
-- `1 + scale`, plus `m`, held within it (a format's connection onto a knob);
-- its `changed` runs when the value moves. Its own value is what a restore,
-- the player or the script last set: the routes move from it, not from
-- where they last left it, so a preset's value (Augmented Orchestra's
-- `MIDICurve_1=-0.99999899`) holds until a route moves.
function __control_mod(id, scale, m)
	local c = __controls[id + 1]
	local lo, hi = tonumber(c and c.min), tonumber(c and c.max)
	local own = tonumber(c and c.value)
	if not (lo and hi and own) or hi == lo then
		return
	end
	-- Any write since the route's own (a script's, the player's, a
	-- restore's, even of the routed value) is the new base.
	local writes = rawget(c, "__writes") or 0
	local base = rawget(c, "__route_base")
	if base == nil or writes ~= rawget(c, "__routed_at") then
		base = own
	end
	local u = math.min(math.max((base - lo) / (hi - lo) * (1 + scale) + m, 0), 1)
	-- Routes at rest leave it exactly where it was.
	local v = (scale == 0 and m == 0) and base or lo + u * (hi - lo)
	-- Its own flag: an unset one reads through the widget's methods.
	if rawget(c, "integer") == true then
		v = math.floor(v + 0.5)
	end
	-- Kept before the write: a `changed` that fails or waits leaves it.
	rawset(c, "__route_base", base)
	rawset(c, "__routed_at", writes)
	local co = coroutine.running()
	local scope = co and enter_route_scope(co)
	if scope then
		scope[c] = (scope[c] or 0) + 1
	end
	local ok, err = pcall(c.setValue, c, v)
	if scope then
		local n = scope[c] - 1
		scope[c] = n > 0 and n or nil
		leave_route_scope(scope)
	end
	if not ok then
		error(err, 0)
	end
end
-- The UI the script builds, for the editor: every widget in the order the
-- script made it (its index there names it), the page's size, picture and
-- colour, and whether it asked for a performance view.
__ui = { widgets = {}, width = 720, height = 480 }

-- After the script's body: each persistent widget takes its saved value and,
-- when that moves it from what the body left, runs its `changed`, before
-- `onLoad` (lua.uvi.net, Widget::persistent: "Persistent widgets will call
-- their changed function on reload"; onLoad: "after restoring widgets
-- values"). A value the body already set does not run it again, as JUCE's
-- controls under UVI notify only a change: Augmented Orchestra's twenty LFO
-- shape buttons each select themselves in `changed`, so running every one
-- left the last selected (shape 20 over the saved 1, and its LFO preset over
-- the program's), where every factory preset UVI saved keeps the first.
__restore_errors = {}
function __restore()
	if __ui_built then
		__ui_built()
	end
	local list = saved_widgets
	saved_widgets = {}
	for _, w in ipairs(list) do
		-- Each `changed` runs on its own: one that fails (VWinds' `dynamics`
		-- names a Macro 7 its Bb Clarinet lacks) leaves the rest restored.
		if w.persistent ~= false then
			rawset(w, "__restored", true)
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
	-- A bound widget is made from an element and a parameter name, not a field table.
	if type(name) == "table" and getmetatable(name) == nil then
		fields = name
		name, value = fields[1], fields.value or fields[2]
	end
	if type(name) == "table" or type(name) == "userdata" then
		name = nil
	end
	local rest = { fields[3] }
	local items
	if kind == "Menu" or kind == "MultiStateButton" then
		-- Menu(name, items, selected): the value is the item's index.
		items = type(value) == "table" and value or {}
		value = tonumber(rest[1]) or 1
	end
	if type(value) ~= "number" and type(value) ~= "boolean" then
		value = kind ~= "OnOffButton" and kind ~= "Button" and 0
	end
	-- Reserve the restore flag at creation; loading a preset can run in a callback.
	local w = setmetatable({
		kind = kind, name = name, alpha = 1, enabled = true, visible = true, __restored = false,
	}, widget_meta)
	for k, v in pairs(fields) do
		if type(k) == "string" and k ~= "value" and not geometry[k] then
			rawset(w, k, v)
		end
	end
	-- Made with its value: no `changed` runs yet.
	rawset(w, "__value", value)
	w.items = items
	if kind == "Slider" and rawget(w, "vertical") == nil then
		rawset(w, "vertical", fields[6] == true)
	end
	if kind == "Knob" or kind == "Slider" or kind == "NumBox" then
		-- `Knob(name, value, min, max, integer)`: the range by place too.
		for at, key in ipairs({ "min", "max", "integer" }) do
			if rawget(w, key) == nil then
				w[key] = fields[at + 2]
			end
		end
	end
	if kind == "Image" and rawget(w, "image") == nil and type(name) == "string" then
		-- `Image{path, ...}`: its first field is its picture.
		w.image = name
	end
	if kind == "SVG" and rawget(w, "svg") == nil and type(name) == "string" then
		w.svg = name
	end
	table.insert(__ui.widgets, w)
	if init[kind] then
		init[kind](w, fields)
	end
	if name ~= nil and __saved[name] ~= nil and kind ~= "Table" then
		table.insert(saved_widgets, w)
	end
	-- UVI names it `position` (UVIWorkstation's Widget properties: width,
	-- height, position, bounds); `pos` is kept for scripts that use it.
	local pos, size = fields.position or fields.pos or {}, fields.size or {}
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
	-- A script's setValue runs `changed` only when the value changes, as
	-- JUCE's controls under UVI do: W Strings Harmonic's linked arpeggiator
	-- buttons set each other and recursed without end. A user's move always
	-- runs it (`__FORCE`); a restore, when it moves the widget. A `changed`
	-- that failed leaves the value unsettled, so setting it again (the same
	-- preset reloaded) runs `changed` again.
	w.setValue = function(self, v, run)
		local was = rawget(self, "__value")
		wrote(self, v)
		if run ~= false and (run == __FORCE or v ~= was or rawget(self, "__unsettled")) then
			-- A click's modifier keys reach a button's `changed`; a script's
			-- own move has none.
			local click = run == __FORCE and pressed[kind]
			local ok, err = pcall(run_changed, self, click and { altDown = false, commandDown = false, shiftDown = false } or nil)
			rawset(self, "__unsettled", not ok or nil)
			if not ok then
				error(err, 0)
			end
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
-- UVIScript's enumerations, in the order its reference lists them
-- (https://lua.uvi.net/class_mapper.html, class_unit.html); RectanglePlacement
-- takes JUCE's flags, which UVI wraps. The rest only need distinct values.
local function enum(names)
	local next_value = 0
	local t = setmetatable({}, {
		__index = function(t, key)
			next_value = next_value + 1
			rawset(t, key, next_value)
			return next_value
		end,
	})
	for i, name in ipairs(names or {}) do
		rawset(t, name, i - 1)
	end
	return t
end
Mapper = enum({ "Linear", "Exponential", "QuinticRoot", "QuarticRoot", "CubeRoot", "SquareRoot", "Quadratic", "Cubic", "Quartic", "Quintic" })
Mapper.Type = Mapper
Unit = enum({ "Generic", "Percent", "PercentNormalized", "Seconds", "MilliSeconds", "Hertz", "Decibels", "LinearGain", "UviFilter", "Pan", "Megabyte", "SemiTones", "Cents", "MidiKey" })
Unit.Type = Unit
FileFormat = enum({ "All", "Audio", "MIDI" })
FileFormat.Type = FileFormat
RectanglePlacement = {
	Left = 1, Right = 2, HorizontallyCentred = 4, Top = 8, Bottom = 16, VerticallyCentred = 32,
	StretchToFit = 64, FillDestination = 128, OnlyReduceInSize = 256, OnlyIncreaseInSize = 512,
	DoNotResize = 768, Centred = 36,
}
RectanglePlacement.Type = RectanglePlacement
for _, name in ipairs({ "DisplayType", "Justification", "MouseEvent", "Direction" }) do
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
local START_OFFSET = 4096
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
local ROUTE_AMOUNT = Action.RouteAmount
local Connection = { parameterDefinitions = connection_definitions }
Connection.__index = Connection
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
			-- Straight to the host: a program's walk sets thousands at once.
			__host(ROUTE_AMOUNT, c[4], amount)
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

-- A layer's play law, `line` its `{voice group, glide scope, own note
-- limit}`. `PlayMode` (Falcon manual, Layer editor; program.rs
-- `layer_polyphony`): 0 Poly, 1 Poly Portamento (each note slides from the
-- last), 2 Mono Retrigger, 3 Mono Portamento (a legato note takes over and
-- slides), 4 Mono Portamento Slide (a detached note slides too). The glide's
-- numbers: `PortamentoTime` seconds, `PortamentoMode` 1 per semitone,
-- `PortamentoCurve` -1..1.
local GLIDE_KNOBS = { PortamentoTime = 1, PortamentoMode = 2, PortamentoCurve = 3 }
local function set_line(line, name, value)
	local x = tonumber(value)
	if not x then
		return
	end
	if name == "PlayMode" then
		local m = math.floor(x)
		local mono = m >= 2 and m <= 4
		local bit = function(b)
			return b and 1 or 0
		end
		host_call("VoiceLine", line[1], line[2], mono and 1 or line[3], bit(m == 3 or m == 4), bit(mono),
			bit(m == 1 or m == 3 or m == 4), bit(m == 3), bit(m ~= 1))
	elseif GLIDE_KNOBS[name] then
		host_call("GlideKnob", line[2], GLIDE_KNOBS[name], math.floor(x * 1e6 + 0.5))
	end
end

-- An AHD's or DAHDSR's times (seconds, 0..30), levels and curves, as
-- kontra-format-uvi's modulation.rs translates them; `env` is `{tag,
-- {group, envelope}, ...}`, envelope -1 a group's amplitude envelope.
-- A segment curve c passes (1 - c)/2 of its way at half its time: shape
-- 2 ln((1 + c)/(1 - c)), c within +-0.9998 (UVIWorkstation [ref]).
local ENVELOPE_PARAMS = {
	DelayTime = true, AttackTime = true, HoldTime = true, DecayTime = true, SustainLevel = true,
	ReleaseTime = true, AttackCurve = true, DecayCurve = true, ReleaseCurve = true,
}
local function micro(x)
	return math.floor(x * 1e6 + 0.5)
end
local function set_envelope(p, env)
	local function seconds(name, default)
		return math.max(0, math.min(30, tonumber(p[name]) or default))
	end
	local function curve(name)
		local c = math.max(-0.9998, math.min(0.9998, tonumber(p[name]) or 0))
		return 2 * math.log((1 + c) / (1 - c))
	end
	local sustain = math.max(0, math.min(1, tonumber(p.SustainLevel) or 1))
	local points
	if env[1] == "AHD" then
		points = {
			{ seconds("AttackTime", 0), 1, curve("AttackCurve") },
			{ seconds("HoldTime", 1), 1, 0 },
			{ seconds("DecayTime", 0.1), 0, curve("DecayCurve") },
		}
	else
		points = {
			{ seconds("DelayTime", 0), 0, 0 },
			{ seconds("AttackTime", 0), 1, curve("AttackCurve") },
			{ seconds("HoldTime", 0), 1, 0 },
			{ seconds("DecayTime", 0), sustain, curve("DecayCurve") },
			{ seconds("ReleaseTime", 0), 0, curve("ReleaseCurve") },
		}
	end
	for i = 2, #env do
		local group, index = env[i][1], env[i][2]
		if index < 0 then
			-- Decay and release share the release's curve, the decay's when
			-- only it moves.
			local delay = points[1][1]
			local decays = sustain < 1 and points[4][1] > 0
			local fall = curve((decays and points[5][1] == 0) and "DecayCurve" or "ReleaseCurve")
			host_call("AmpEnvelope", group, micro(points[2][1]), micro(points[3][1]),
				micro(points[4][1]), micro(sustain), micro(points[5][1]), micro(points[2][3]),
				micro(fall), micro(delay))
		else
			for k, point in ipairs(points) do
				host_call("EnvelopePoint", group, index, k - 1, micro(point[1]), micro(point[2]),
					micro(point[3]))
			end
		end
	end
end

-- An LFO's, MultiLFO's or StepEnvelope's wave, rate and clock as
-- kontra-format-uvi's modulation.rs translates them; `lfo` is `{tag,
-- {group, index}, ..., fixed}`, `fixed` for a user-table LFO, whose wave
-- is its table. WaveFormType (registry [ref]): its engine wave and
-- that wave's numbers as f32 bits (the chaos constants set at
-- [ref]); 5 AnalogSquare and 9 User keep the wave they have.
local LFO_PARAMS = { Freq = true, SyncToHost = true, WaveFormType = true, Retrigger = true }
local CHAOS_TEN = 1092616192
local LFO_WAVES = {
	[0] = { 0 },
	[1] = { 4, 1056964608 },
	[2] = { 1 },
	[3] = { 3 },
	[4] = { 2 },
	[6] = { 5 },
	[7] = { 6, CHAOS_TEN, 1105199104, 1076535230, 1065353216, 1028730567 },
	[8] = { 7, 1041865114, 1045220557, CHAOS_TEN, 1077558641, 1030475397 },
}
local function set_lfo(p, lfo)
	local tag = lfo[1]
	local wave = { -1 }
	if tag == "LFO" and not lfo.fixed then
		wave = LFO_WAVES[math.floor(tonumber(p.WaveFormType) or 0)] or wave
	end
	local sync = p.SyncToHost == true or (tonumber(p.SyncToHost) or 0) ~= 0
	local steps = tag == "StepEnvelope"
	local freq = tonumber(p.Freq) or (steps and 0.25 or 0.5)
	-- A random wave draws twice a cycle; a step envelope's cycle is its steps.
	local per = steps and (tonumber(p.NumSteps) or 16) or (wave[1] == 5 and 2 or 1)
	local kind, speed = -1, 0
	if freq > 0 then
		kind = sync and 1 or 0
		speed = sync and freq * per or freq / per
	end
	local retrigger = tonumber(p.Retrigger) or (steps and 0 or 1)
	local clock = (retrigger == 0 or retrigger == 1) and retrigger or -1
	for i = 2, #lfo do
		host_call("LfoSet", lfo[i][1], lfo[i][2], wave[1], wave[2] or 0, wave[3] or 0, wave[4] or 0,
			wave[5] or 0, wave[6] or 0, kind, math.floor(speed * 1e6 + 0.5), clock)
	end
end

local Element = {}
function Element.getParameter(self, name)
	return self.params[name]
end
function Element.hasParameter(self, name)
	return self.params[name] ~= nil
end
-- An insert's `Bypass`, a Gain insert's `Volume` and the numbers its format
-- mapped (`knobs`) reach the effect the engine plays for it (`fx`: where,
-- group or bus, slot).
local function write_effect_knobs(at, index, slot, knobs, name, value)
	-- Modelled effects map each attribute name to its integer knob index;
	-- legacy/synthetic entries map to `{ {control, x1, y1, ...}, ... }`.
	local x = type(value) == "boolean" and (value and 1 or 0) or tonumber(value)
	local entry = x and (knobs or EMPTY)[name]
	if not entry then
		return
	end
	if type(entry) == "number" then
		host_call("EffectKnob", at, index, slot, entry, x)
		return
	end
	for _, knob in ipairs(entry) do
		local y = knob[3]
		if x >= knob[#knob - 1] then
			y = knob[#knob]
		else
			for i = 4, #knob - 1, 2 do
				if x < knob[i] then
					local x0, y0, x1, y1 = knob[i - 2], knob[i - 1], knob[i], knob[i + 1]
					y = x <= x0 and y0 or y0 + (y1 - y0) * (x - x0) / (x1 - x0)
					break
				end
			end
		end
		host_call("EffectField", at, index, slot, knob[1], math.floor(y * 1000 + 0.5))
	end
end

local OSC_PARAMS = {
	BaseNote = 0,
	CoarseTune = 1,
	FineTune = 2,
	Pitch = 3,
	Gain = 4,
	Reverse = 5,
	SampleStart = 6,
	LoopStart = 7,
	LoopEnd = 8,
	Bypass = 9,
}

local KG_PARAMS = {
	LowKey = 0,
	HighKey = 1,
	LowVelocity = 2,
	HighVelocity = 3,
	Tune = 4,
	CoarseTune = 5,
	FineTune = 6,
	Gain = 7,
	Pan = 8,
}

local function runtime_connected()
	return __runtime_connected ~= nil and __runtime_connected()
end

local function to_param_number(value)
	if type(value) == "boolean" then
		return value and 1 or 0
	end
	return tonumber(value)
end

local function effect_target_of(el)
	local f = fields_of(el)
	if type(f.group) == "number" then
		return 0, f.group
	elseif type(f.bus) == "number" then
		return 1, f.bus
	elseif el == Program then
		return 2, 0
	end
	return nil, nil
end

function Element.setParameter(self, name, value)
	local was = self.params[name]
	self.params[name] = value
	if was ~= value and __watch then
		__watch(self, name)
	end
	local fields = fields_of(self)
	local env = fields.env
	if env and ENVELOPE_PARAMS[name] and tonumber(value) then
		set_envelope(self.params, env)
		return
	end
	local lfo = fields.lfo
	if lfo and LFO_PARAMS[name] then
		set_lfo(self.params, lfo)
		return
	end
	local line = fields.line
	if line then
		set_line(line, name, value)
		return
	end
	local rack_mix = fields.rack_mix
	if name == "Mix" and rack_mix and tonumber(value) then
		local wet = math.clamp(tonumber(value), 0, 1)
		host_call("EffectGain", 1, rack_mix[1], 0, math.floor(wet * 1e6 + 0.5))
		host_call("EffectGain", 1, rack_mix[2], 0, math.floor((1 - wet) * 1e6 + 0.5))
		return
	end
	local routed = fields.routed_knobs
	if routed then
		for i = 2, #routed do
			write_effect_knobs(1, routed[1], routed[i][1], routed[i][2], name, value)
		end
		return
	end
	local fx = fields.fx
	if not fx then
		if runtime_connected() then
			if type(fields.group) == "number" then
				local code = KG_PARAMS[name]
				local n = code ~= nil and to_param_number(value)
				if n ~= nil then
					__runtime_kg_param(fields.group, code, n)
				end
			else
				local parent = fields.parent
				local pg = parent and fields_of(parent).group
				if type(pg) == "number" then
					if name == "SamplePath" and type(value) == "string" then
						loadSample(self, value)
					else
						local code = OSC_PARAMS[name]
						local n = code ~= nil and to_param_number(value)
						if n ~= nil then
							__runtime_osc_param(pg, (fields.index or 1) - 1, code, n)
						end
					end
				end
			end
		end
		return
	elseif name == "Bypass" then
		local on = value == true or (type(value) == "number" and value ~= 0)
		host_call("EffectBypass", fx[1], fx[2], fx[3], on and 1 or 0)
	elseif name == "Volume" and type(value) == "number" then
		host_call("EffectGain", fx[1], fx[2], fx[3], math.floor(value * 1e6 + 0.5))
	end
	-- A number its response is prepared by: prepared again off the audio thread.
	local knob = (fields.impulse or EMPTY)[name]
	if knob and tonumber(value) then
		host_call("ImpulseKnob", fx[1], fx[2], fx[3], knob, math.floor(tonumber(value) * 1e6 + 0.5))
	end
	write_effect_knobs(fx[1], fx[2], fx[3], fields.knobs, name, value)
	if fields.dynamic and runtime_connected() then
		local n = to_param_number(value)
		if n ~= nil then
			__runtime_effect_param(fx[1], fx[2], fx[3], name, n)
		end
	end
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

-- Async task handle classes: every async operation returns a task table whose
-- `:cancel()` marks it cancelled so deferred callbacks skip cleanly.
local function task_class()
	local cls = {}
	cls.__index = cls
	function cls:cancel()
		self.cancelled = true
	end
	return cls
end

AsyncTask = task_class()
AsyncBrowseForFileTask = task_class()
AsyncDataLoadTask = task_class()
AsyncDataSaveTask = task_class()
AsyncListFilesTask = task_class()
AsyncMidiFileLoadTask = task_class()

local function make_task(cls, fields)
	local t = fields or {}
	t.cancelled = false
	return setmetatable(t, cls or AsyncTask)
end

-- Dynamic effect chain mutations on an element's inserts container.
local function reindex_inserts(list, parent)
	local tk, ti = effect_target_of(parent)
	for i, child in ipairs(list) do
		local f = fields_of(child)
		f.index = i
		f.parent = parent
		f.dynamic = true
		if f.fx and tk ~= nil and f.fx[1] == tk and f.fx[2] == ti then
			f.fx[3] = i - 1
		end
	end
end

function Element.insertEffect(self, effectType, index)
	local list = self.inserts
	local name = tostring(effectType or "Effect")
	local fx = element({ params = { Name = name, Type = name, Bypass = "0" } })
	local pos = tonumber(index)
	if pos then
		pos = math.max(1, math.min(#list + 1, math.floor(pos)))
		table.insert(list, pos, fx)
	else
		pos = #list + 1
		table.insert(list, fx)
	end
	local tk, ti = effect_target_of(self)
	if tk ~= nil then
		local f = fields_of(fx)
		f.fx = { tk, ti, pos - 1 }
		f.dynamic = true
		if runtime_connected() then
			__runtime_effect_op(tk, ti, 0, pos - 1, 0, name)
		end
	end
	reindex_inserts(list, self)
	return fx
end

function Element.removeEffect(self, target)
	local list = self.inserts
	local idx = tonumber(target)
	if idx then
		idx = math.floor(idx)
	elseif type(target) == "userdata" then
		for i, item in ipairs(list) do
			if item == target then
				idx = i
				break
			end
		end
	end
	if not idx or idx < 1 or idx > #list then
		return false
	end
	local removed = table.remove(list, idx)
	local fx = removed and fields_of(removed).fx
	if fx then
		host_call("EffectBypass", fx[1], fx[2], fx[3], 1)
	end
	local tk, ti = effect_target_of(self)
	if tk ~= nil and runtime_connected() then
		__runtime_effect_op(tk, ti, 1, idx - 1, 0, "")
	end
	reindex_inserts(list, self)
	return true
end

function Element.moveEffect(self, from, to)
	local list = self.inserts
	local f = math.floor(tonumber(from) or 0)
	local t = math.floor(tonumber(to) or 0)
	if f < 1 or f > #list or t < 1 or t > #list then
		return false
	end
	if f ~= t then
		local item = table.remove(list, f)
		table.insert(list, t, item)
		local tk, ti = effect_target_of(self)
		if tk ~= nil and runtime_connected() then
			__runtime_effect_op(tk, ti, 2, f - 1, t - 1, "")
		end
		reindex_inserts(list, self)
	end
	return true
end

function Element.loadImpulse(self, path, done)
	return loadImpulse(self, path, done)
end

function Element.getSliceInfo(self)
	local fields = fields_of(self)
	if fields.slices and #fields.slices > 0 then
		local out = {}
		for i, s in ipairs(fields.slices) do
			out[i] = { index = i, start = s.start, length = s.length }
		end
		return out
	end
	local p = fields.params or EMPTY
	local count = math.max(0, math.floor(tonumber(p.NumSlices) or 0))
	local frames = math.max(0, math.floor(tonumber(fields.sampleFrames or p.SampleLength) or 0))
	local slices = {}
	for i = 1, count do
		local start = count > 0 and math.floor((i - 1) * frames / count) or 0
		local stop = count > 0 and math.floor(i * frames / count) or 0
		slices[i] = { index = i, start = start, length = math.max(0, stop - start) }
	end
	return slices
end

function Element.getNumZones(self)
	local fields = fields_of(self)
	if type(fields.numZones) == "number" then
		return fields.numZones
	end
	if fields.zones then
		return #fields.zones
	end
	local path = fields.params and fields.params.SamplePath
	return (type(path) == "string" and path ~= "") and 1 or 0
end

function Element.getLoadingStatus(self)
	return fields_of(self).loadingStatus or 0
end

-- Pending async runtime swaps (loadSample / loadMapping / setPlaybackOptions),
-- pumped by one shared coroutine so burst reloads never exhaust task slots.
local pending_swaps = {}
local pending_swap_count = 0
local swap_pumper_active = false
local poll_slices = {}

local function finish_swap(id, ok, frames, rate, num_zones, num_slices)
	local entry = pending_swaps[id]
	if not entry then
		return
	end
	pending_swaps[id] = nil
	pending_swap_count = math.max(0, pending_swap_count - 1)
	local target = entry.target
	if type(target) == "userdata" then
		local f = fields_of(target)
		f.loadingStatus = math.max(0, (f.loadingStatus or 1) - 1)
		if ok then
			if frames and frames > 0 then
				f.sampleFrames = frames
				f.sampleRate = rate
				f.sampleInfo = nil
			end
			if entry.kind == "mapping" and num_zones and num_zones > 0 then
				f.numZones = num_zones
			end
			if num_slices and num_slices > 0 then
				local slices = {}
				for i = 1, num_slices do
					local st = poll_slices[i] or 0
					local nx = (i < num_slices and poll_slices[i + 1]) or frames or st
					slices[i] = { index = i, start = st, length = math.max(0, nx - st) }
				end
				f.slices = slices
				if f.params then
					f.params.NumSlices = num_slices
				end
			end
		end
	end
	local task = entry.task
	task.success = ok == true
	if not task.cancelled and type(entry.done) == "function" then
		entry.done(task)
	end
end

local function pump_swaps()
	while pending_swap_count > 0 do
		local batch = 0
		while batch < 512 do
			local id, ok, frames, rate, num_zones, num_slices = __runtime_poll(poll_slices)
			if not id then
				break
			end
			batch = batch + 1
			finish_swap(id, ok, frames, rate, num_zones, num_slices)
		end
		if pending_swap_count > 0 then
			wait(1)
		end
	end
	swap_pumper_active = false
end

local function register_swap(id, target, kind, done)
	local task = make_task(AsyncTask, { success = false })
	pending_swaps[id] = { task = task, done = done, target = target, kind = kind }
	pending_swap_count = pending_swap_count + 1
	if not swap_pumper_active then
		swap_pumper_active = true
		spawn(pump_swaps)
	end
	return task
end

function Element.loadMapping(self, path, done)
	local fields = fields_of(self)
	fields.mappingPath = path
	local parent = fields.parent
	local g = fields.group or (parent and fields_of(parent).group)
	if runtime_connected() and type(g) == "number" and type(path) == "string" then
		local slot = (fields.index or 1) - 1
		fields.loadingStatus = (fields.loadingStatus or 0) + 1
		local id = __runtime_load_mapping(g, slot, path)
		if id and id > 0 then
			return register_swap(id, self, "mapping", done)
		end
		fields.loadingStatus = math.max(0, (fields.loadingStatus or 1) - 1)
	end
	fields.loadingStatus = 0
	local task = make_task(AsyncTask, { success = false })
	if type(done) == "function" then
		done(task)
	end
	return task
end

local function set_element_purged(self, purged)
	local fields = fields_of(self)
	fields.purged = purged
	local parent = fields.parent
	local g = fields.group or (parent and fields_of(parent).group)
	if type(g) == "number" then
		host_call("PurgeGroup", g, purged and 0 or 1)
	end
end

function Element.purgeZones(self, _zones)
	set_element_purged(self, true)
end

function Element.unpurgeZones(self, _zones)
	set_element_purged(self, false)
end

function Element.waitForLoading(self)
	while self:getLoadingStatus() ~= 0 do
		wait(1)
	end
end

local function element_index(e, key)
	local fields = fields_of(e)
	local value = fields[key]
	if value ~= nil then
		return value
	end
	value = Element[key]
	if value then
		return value
	end
	if key == "sends" then
		return e.busRouters
	end
	if key == "synthChildren" then
		return fields.layers or fields.keygroups or fields.oscillators or EMPTY
	end
	if is_container[key] then
		local list = {}
		fields[key] = list
		return list
	end
	-- A sample oscillator's sample, as UVIWorkstation 4.0.9 builds it
	-- ([ref]: name, type, channels, ..., basenote, finetune, loops).
	-- ponytail: name, basenote and finetune only; the rest when a script reads them.
	local path = fields.params and fields.params.SamplePath
	if key == "sampleInfo" and type(path) == "string" and path ~= "" then
		local p = fields.params
		local info = {
			name = string.match(path, "[^/\\]*$"),
			basenote = p.BaseNote,
			finetune = p.FineTune,
			length = fields.sampleFrames or 0,
			sampleRate = fields.sampleRate or 48000,
		}
		fields.sampleInfo = info
		return info
	end
	if key == "looplabInfo" then
		local p = fields.params or EMPTY
		local info = {
			numSlices = math.max(0, math.floor(tonumber(p.NumSlices) or 0)),
			tempo = tonumber(p.Tempo) or 120,
			beats = tonumber(p.NumBeats) or 0,
		}
		fields.looplabInfo = info
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
		dry = node.dry or false,
		name = params.Name,
		displayName = params.DisplayName,
		mod = node.mod,
		fx = node.fx,
		knobs = node.knobs,
		routed_knobs = node.routed_knobs,
		rack_mix = node.rack_mix,
		impulse = node.impulse,
		line = node.line,
		env = node.env,
		lfo = node.lfo,
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

-- A layer's fader and an aux's or rack chain's gain are their buses'
-- volumes.
local function push_bus(e)
	local off = e.params.Bypass or e.params.Mute
	local db = off and -200 or decibels(e.params.Gain or 1)
	host_call("BusVolume", e.bus, math.floor(db * 1000))
	-- An effect rack's dry bus passes its input while it is bypassed.
	if e.dry then
		host_call("BusVolume", e.dry, off and 0 or -200000)
	end
end

-- A keygroup's group level and pan, under its layer's mute and pan.
local function push_group(lp, kg)
	local kp = kg.params
	local silent = lp.Mute or lp.Bypass or kp.Bypass or kp.Mute
	local db = tree.db + decibels(kp.Gain or 1)
	host_call("GroupVolume", kg.group, silent and -200000 or math.floor(db * 1000))
	local pan = math.max(-1, math.min(1, tree.pan + (lp.Pan or 0) + (kp.Pan or 0)))
	host_call("GroupPan", kg.group, math.floor(pan * 1000))
end

-- Pushes every group's level when a callback can reach the host.
local function apply_levels()
	if not __host then
		stale = true
		return
	end
	stale = false
	each_bus(Program, push_bus)
	for _, layer in ipairs(Program.layers) do
		for _, kg in ipairs(layer.keygroups) do
			if kg.group then
				push_group(layer.params, kg)
			end
		end
	end
end

-- Pushes what a level change on `e` reaches: its own bus, and the groups
-- under it (a layer's keygroups, or a keygroup itself). Everything else
-- holds the level it was last sent.
local function apply_level(e)
	if stale or not __host then
		apply_levels()
		return
	end
	if e.bus then
		push_bus(e)
	end
	-- Raw fields: a missing key reads as a method or an empty container.
	local fields = fields_of(e)
	local layer = fields.parent
	if layer and (fields_of(layer).keygroups or EMPTY)[fields.index] == e then
		if e.group then
			push_group(layer.params, e)
		end
	else
		for _, kg in ipairs(fields.keygroups or EMPTY) do
			if kg.group then
				push_group(e.params, kg)
			end
		end
	end
end

local levels = { Gain = true, Pan = true, Mute = true, Bypass = true }

local function set_level(self, name, value)
	if name == "Mix" and fields_of(self).rack_mix then
		return Element.setParameter(self, name, value)
	end
	self.params[name] = value
	local f = fields_of(self)
	local line = f.line
	if levels[name] then
		apply_level(self)
	elseif line then
		set_line(line, name, value)
	end
	if runtime_connected() and type(f.group) == "number" then
		local code = KG_PARAMS[name]
		local n = code ~= nil and to_param_number(value)
		if n ~= nil then
			__runtime_kg_param(f.group, code, n)
		end
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
	if hearing ~= nil then decide() end
	coroutine.yield(math.max(0, ms or 0) * 1000)
end

function getTime()
	return host_call("EngineMs") or 0
end

-- ponytail: no engine action reports the rate; the engine's default rate
-- until one does (scripts mostly use it for ms/sample conversions).
local DEFAULT_SAMPLE_RATE = 48000
function getSamplingRate()
	return DEFAULT_SAMPLE_RATE
end

function getTempo()
	local quarter = host_call("DurationQuarter")
	return quarter and quarter > 0 and 60e6 / quarter or 120
end

function getTimeSignature()
	local num = host_call("SignatureNum")
	local den = host_call("SignatureDenom")
	return (num and num > 0) and num or 4, (den and den > 0) and den or 4
end

function beat2ms(beats)
	return beats * 60000 / getTempo()
end

function ms2beat(ms)
	return ms * getTempo() / 60000
end

function ms2samples(ms)
	return (ms or 0) * getSamplingRate() / 1000
end

function samples2ms(samples)
	return (samples or 0) * 1000 / getSamplingRate()
end

-- Beats since the engine started, at the current tempo.
function getRunningBeatTime()
	return getTime() / beat2ms(1)
end

-- The host's song position in beats (960 ticks a beat).
function getBeatTime()
	return (host_call("SongPosition") or 0) / 960
end

function getBeatDuration()
	return beat2ms(1)
end

function getBarDuration()
	local bar = host_call("DurationBar")
	if bar and bar > 0 then
		return bar / 1000
	end
	local num, den = getTimeSignature()
	return getBeatDuration() * num * 4 / den
end

function getCC(cc, _channel, _input)
	return host_call("Cc", cc or 0) or 0
end

function getNoteDuration(note)
	local us = host_call("NoteDuration", note or 0)
	return us and us / 1000 or 0
end

function waitBeat(beats)
	wait(beat2ms(beats))
end

function isKeyDown(note)
	return host_call("KeyDown", note) ~= 0
end

function isOctaveKeyDown(note)
	return host_call("KeyDownOct", math.floor(tonumber(note) or 0) % 12) ~= 0
end

-- UVI's: whether the note that started the callback is still down.
function isNoteHeld()
	return host_call("NoteHeld") ~= 0
end

-- UVI's: returns once the note that started the callback is released
-- (`playNote(.., 0)` then `waitForRelease()` then `releaseVoice`).
-- ponytail: polls every millisecond of engine time; a release wake-up would
-- save the resumes if many notes wait at once.
function waitForRelease()
	while isNoteHeld() do
		wait(1)
	end
end

-- UVI's signature (its embedded api): `slice` comes before `oscIndex`, and
-- a duration > 0 releases after that many ms, 0 waits for releaseVoice,
-- -1 (the default) releases with the note that started the callback.
function playNote(note, velocity, duration, layer, channel, input, vol, pan, tune, slice, oscIndex)
	if type(note) == "table" then
		local t = note
		note, velocity, duration = t.note or t[1], t.velocity or t[2], t.duration or t[3]
		layer, vol, pan, tune = t.layer or t[4], t.vol or t[7], t.pan or t[8], t.tune or t[9]
		oscIndex = t.oscIndex or t[11]
	end
	duration = duration or -1
	local micros = duration > 0 and math.floor(duration * 1000) or (duration < 0 and -1 or 0)
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

-- A controller-type event's value, sent to the instrument `delta` ms on.
local function send(e, delta)
	local later = math.floor(math.max(0, delta or 0) * 1000)
	if e.type == Event.ControlChange then
		host_call("SetController", e.controller, e.value, later)
	elseif e.type == Event.PitchBend then
		host_call("SetController", 128, math.floor((e.bend or 0) * 8192 + 0.5), later)
	elseif e.type == Event.AfterTouch then
		host_call("SetController", 129, e.value, later)
	elseif e.type == Event.PolyAfterTouch then
		host_call("SetPolyAt", e.note, e.value, later)
	elseif e.type == Event.ProgramChange then
		local ch = math.max(0, math.min(63, math.floor((e.channel or 1) - 1)))
		host_call("SetMidi", ch, Event.ProgramChange, math.floor(e.program or e.value or 0), 0)
	end
end

-- Runs `handler` on the controller-type event `e` as it arrived: kept from
-- the instrument unless posted, replaced by the posted one if it changed.
local function hear(handler, e)
	if handler == nil then return end
	arrived, hearing, posted = table.copy(e), e, nil
	handler(e)
	if hearing == e then decide() end
end

decide = function()
	local out, delta = posted, posted_delta
	hearing, posted = nil, nil
	if out == nil then
		host_call("IgnoreController")
		return
	end
	-- Posted later, or changed: the posted one stands in for it.
	if delta > 0 then
		host_call("IgnoreController")
		return send(out, delta)
	end
	for k, v in pairs(out) do
		if arrived[k] ~= v then
			host_call("IgnoreController")
			return send(out)
		end
	end
end

function postEvent(e, delta)
	if e == hearing then
		posted, posted_delta = table.copy(e), delta or 0
		return
	end
	send(e, delta)
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

-- killVoice defaults to false (UVI's own prelude): the voice plays on silent.
function fadeout(id, ms, kill)
	host_call("FadeOut", id, math.floor((ms or 0) * 1000), kill and 1 or 0)
end

function print(...)
	local parts = {}
	for i = 1, select("#", ...) do
		parts[i] = tostring((select(i, ...)))
	end
	host_call("Message", table.concat(parts, " "))
end

-- UVI's `_spawn` (its embedded `uvi.thread`): the task is queued, not run;
-- it starts at the end of the current instant, after the caller, with the
-- arguments it was given.
function spawn(f, ...)
	local args = table.pack(...)
	table.insert(__spawned, coroutine.create(function()
		return f(table.unpack(args, 1, args.n))
	end))
end

-- loadSample(oscillator, path, done): records at load time (`__record`); at
-- runtime dispatches off-audio-thread sample loading & atomic Plan swap via
-- the lock-free runtime channel and invokes `done(task)` once landed.
local loads
function loadSample(oscillator, path, done)
	if loads and type(oscillator) == "userdata" and oscillator.parent then
		table.insert(loads, { oscillator.parent.group, oscillator.index - 1, path, oscillator.params.BaseNote })
	end
	if type(oscillator) == "userdata" then
		local f = fields_of(oscillator)
		if f.params and type(path) == "string" then
			f.params.SamplePath = path
		end
		f.sampleInfo = nil
		f.slices = nil
		if not loads and runtime_connected() and type(path) == "string" then
			local parent = f.parent
			local g = parent and fields_of(parent).group
			if type(g) == "number" then
				local p = f.params or EMPTY
				local slot = (f.index or 1) - 1
				f.loadingStatus = (f.loadingStatus or 0) + 1
				local id = __runtime_load_sample(
					g,
					slot,
					path,
					tonumber(p.BaseNote),
					tonumber(p.SampleStart),
					p.Reverse,
					tonumber(p.CoarseTune),
					tonumber(p.FineTune),
					tonumber(p.Gain)
				)
				if id and id > 0 then
					return register_swap(id, oscillator, "sample", done)
				end
				f.loadingStatus = math.max(0, (f.loadingStatus or 1) - 1)
				local fail = make_task(AsyncTask, { success = false })
				if type(done) == "function" then
					done(fail)
				end
				return fail
			end
		end
	end
	local task = make_task(AsyncTask, { success = true })
	if type(done) == "function" then
		done(task)
	end
	return task
end

-- loadImpulse(insert, path, done): the bank's response `path` (found by its
-- file name, as the program's own responses are listed) into the
-- convolution the engine plays for `insert`. It is decoded off the audio
-- thread and swapped in a few blocks later; the callback reports at once,
-- so a script that mutes while one loads (VWinds' room) unmutes.
function loadImpulse(insert, path, done)
	local fx = type(insert) == "userdata" and fields_of(insert).fx
	local ok = false
	if fx and type(path) == "string" then
		-- Before init the call waits in `pending`: it loads then.
		ok = host_call("LoadImpulse", fx[1], fx[2], fx[3], path) ~= nil or not __host
	end
	local task = make_task(AsyncTask, { success = ok })
	if type(done) == "function" then
		done(task)
	end
	return task
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
			table.insert(runs, type(spawned) == "table" and spawned[1] or spawned)
		end
		table.clear(__spawned)
		i = i + 1
	end
	return loads
end

local function event(kind)
	local id = host_call("EventId")
	return {
		type = kind,
		id = id,
		note = host_call("EventNote"),
		-- A release reports its key-up velocity ($EVENT_PAR_REL_VELOCITY, 27).
		velocity = kind == Event.NoteOff and host_call("GetEventPar", id, 27) or host_call("EventVelocity"),
		channel = 1,
		layer = 0,
	}
end

-- Whether init (and the saved state's onLoad) is running: it runs while
-- the engine is built, off the audio thread.
local initializing = false

-- Entries by the engine's callback number: init, note, release, controller.
__entries = {
	[0] = function()
		-- UVI calls onTransport when the host starts and stops.
		if onTransport then
			host_call("SetListener", 1, 1)
			host_call("SetListener", 2, 1)
		end
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
		initializing = true
		local ok, err = pcall(function()
			if onInit then
				onInit()
			end
			-- The state the program saved (`onSave`) comes back after init.
			local found, state = pcall(require, "__state")
			if onLoad and found and state ~= true then
				onLoad(state)
			end
		end)
		initializing = false
		if not ok then
			error(err, 0)
		end
	end,
	[1] = function()
		if stale then
			apply_levels()
		end
		local e = event(Event.NoteOn)
		-- A UVI note sounds only when the script posts it.
		host_call("IgnoreEvent", e.id)
		local handler = onNote or onEvent
		if handler then
			handler(e)
		end
	end,
	[2] = function()
		local e = event(Event.NoteOff)
		host_call("IgnoreEvent", e.id)
		local handler = onRelease or onEvent
		if handler then
			handler(e)
		end
	end,
	-- The editor moved widget `__widget` (its place in `__ui.widgets`) to
	-- `__widget_value`: its `changed` runs, as a click would. A button's
	-- value stays a boolean (0 is true in Lua), a menu's an index.
	[4] = function()
		local w, v = __ui.widgets[__widget], __widget_value
		if w == nil or rawget(w, "setValue") == nil then
			return
		end
		if w.kind == "Table" then
			-- A table moves one cell; a move that names none changes nothing.
			local index = __widget_cell
			if index == nil then return end
			if w.integer then v = math.floor(v + 0.5) end
			w:setValue(index, v)
			if type(rawget(w, "changed")) == "function" then w:changed(index) end
			return
		end
		if type(w.value) == "boolean" then
			v = v ~= 0
		elseif w.kind == "Menu" or rawget(w, "integer") == true then
			v = math.floor(v + 0.5)
		end
		w:setValue(v, __FORCE)
	end,
	-- Polyphonic aftertouch on key `PolyAtNum`, or a note's own pressure.
	[6] = function()
		local note = host_call("PolyAtNum")
		hear(onPolyAfterTouch or onEvent, {
			type = Event.PolyAfterTouch,
			note = note,
			value = host_call("PolyAt", note),
			channel = 1,
			input = 1,
		})
	end,
	-- The host started (signal 1) or stopped its transport.
	[5] = function()
		onTransport(host_call("SignalType") == 1)
	end,
	[3] = function()
		local cc = host_call("CcNum")
		-- Pitch bend and channel pressure come as controllers 128 and 129.
		if cc == 128 then
			hear(onPitchBend or onEvent, { type = Event.PitchBend, bend = host_call("Cc", 128) / 8192, channel = 1, input = 1 })
		elseif cc == 129 then
			hear(onAfterTouch or onEvent, { type = Event.AfterTouch, value = host_call("Cc", 129), channel = 1, input = 1 })
		else
			hear(onController or onEvent, { type = Event.ControlChange, controller = cc, value = host_call("Cc", cc), channel = 1, input = 1 })
		end
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

function controlChange(cc, value, _channel, _input)
	host_call("SetController", cc, value)
end

function pitchBend(value, _channel, _input)
	host_call("SetController", 128, math.floor((value or 0) * 8192 + 0.5))
end

function afterTouch(value, _channel, _input)
	host_call("SetController", 129, math.floor(tonumber(value) or 0))
end

function polyAfterTouch(note, value, _channel, _input)
	host_call("SetPolyAt", math.floor(tonumber(note) or 0), math.floor(tonumber(value) or 0))
end

function programChange(program, channel, _input)
	local ch = math.max(0, math.min(63, math.floor((tonumber(channel) or 1) - 1)))
	local prog = math.floor(tonumber(program) or 0)
	host_call("SetMidi", ch, Event.ProgramChange, prog, 0)
end

__midi_flags = { data_entry_as_cc = false, nrpn_as_cc = false, rpn_as_cc = false }
function enable_data_entry_as_cc(enable)
	__midi_flags.data_entry_as_cc = enable ~= false
end
function enable_nrpn_as_cc(enable)
	__midi_flags.nrpn_as_cc = enable ~= false
end
function enable_rpn_as_cc(enable)
	__midi_flags.rpn_as_cc = enable ~= false
end

-- MidiEvent & MidiSequence (UVI Falcon MIDI file API).
MidiEvent = {}
MidiEvent.__index = MidiEvent
function MidiEvent.MidiEvent(ev, beat, track)
	local out
	if type(ev) == "table" then
		out = table.copy(ev)
		out.beat = beat ~= nil and beat or (out.beat or 0)
		out.track = track ~= nil and track or (out.track or 1)
	else
		out = { type = ev or Event.NoteOn, note = 60, velocity = 100, channel = 1, beat = beat or 0, track = track or 1, duration = 0 }
	end
	return setmetatable(out, MidiEvent)
end
setmetatable(MidiEvent, {
	__call = function(_, ...)
		return MidiEvent.MidiEvent(...)
	end,
})

function postMidiEvent(ev, delta)
	if type(ev) ~= "table" then
		return
	end
	return postEvent(ev, delta)
end

MidiSequence = {}
MidiSequence.__index = MidiSequence
setmetatable(MidiSequence, {
	__call = function(_)
		return setmetatable({ tracks = {} }, MidiSequence)
	end,
})

local function seq_track(seq, track)
	local idx = math.max(1, math.floor(tonumber(track) or 1))
	local list = seq.tracks[idx]
	if not list then
		list = {}
		seq.tracks[idx] = list
	end
	return list
end

function MidiSequence:insertEvent(ev, beat, track)
	local item = MidiEvent(ev, beat, track)
	local list = seq_track(self, item.track)
	local pos = #list + 1
	for i = 1, #list do
		if (list[i].beat or 0) > (item.beat or 0) then
			pos = i
			break
		end
	end
	table.insert(list, pos, item)
	return pos
end

function MidiSequence:getNumEventsForTrack(track)
	local list = self.tracks[math.max(1, math.floor(tonumber(track) or 1))]
	return list and #list or 0
end

function MidiSequence:getEvent(index, track)
	local list = self.tracks[math.max(1, math.floor(tonumber(track) or 1))]
	return list and list[math.floor(tonumber(index) or 0)] or nil
end

function MidiSequence:findAt(beat, track)
	local list = self.tracks[math.max(1, math.floor(tonumber(track) or 1))]
	if not list then return nil end
	local b = tonumber(beat) or 0
	for i, ev in ipairs(list) do
		if math.abs((ev.beat or 0) - b) <= 1e-6 then
			return i
		end
	end
	return nil
end

function MidiSequence:findNextAt(beat, track)
	local list = self.tracks[math.max(1, math.floor(tonumber(track) or 1))]
	if not list then return nil end
	local b = tonumber(beat) or 0
	for i, ev in ipairs(list) do
		if (ev.beat or 0) >= b - 1e-6 then
			return i
		end
	end
	return nil
end

function MidiSequence:findPreviousAt(beat, track)
	local list = self.tracks[math.max(1, math.floor(tonumber(track) or 1))]
	if not list then return nil end
	local b = tonumber(beat) or 0
	local best = nil
	for i, ev in ipairs(list) do
		if (ev.beat or 0) <= b + 1e-6 then
			best = i
		else
			break
		end
	end
	return best
end

-- Element class parameter constant tables.
Keygroup = {
	Parameters = {
		Gain = "Gain", Pan = "Pan", Mute = "Mute", Bypass = "Bypass",
		LowKey = "LowKey", HighKey = "HighKey", LowVelocity = "LowVelocity", HighVelocity = "HighVelocity",
		Tune = "Tune", ExclusiveGroup = "ExclusiveGroup", TriggerRule = "TriggerRule", TriggerMode = "TriggerMode",
	},
}
Oscillator = {
	Parameters = {
		Gain = "Gain", Pitch = "Pitch", CoarseTune = "CoarseTune", FineTune = "FineTune",
		NoteTracking = "NoteTracking", Bypass = "Bypass", BaseNote = "BaseNote",
	},
}
SampleBasedOscillator = {
	Parameters = {
		SamplePath = "SamplePath", BaseNote = "BaseNote", CoarseTune = "CoarseTune", FineTune = "FineTune",
		Gain = "Gain", Reverse = "Reverse", SampleStart = "SampleStart", StartOffset = "StartOffset",
		LoopStart = "LoopStart", LoopEnd = "LoopEnd",
	},
}
SampleMappingOscillator = {
	Parameters = {
		SamplePath = "SamplePath", Gain = "Gain", Pitch = "Pitch", Bypass = "Bypass", SampleStart = "SampleStart",
	},
}
SamplePlayer = {
	Parameters = {
		SamplePath = "SamplePath", BaseNote = "BaseNote", CoarseTune = "CoarseTune", FineTune = "FineTune",
		Gain = "Gain", Reverse = "Reverse", NoteTracking = "NoteTracking", Bypass = "Bypass", SampleStart = "SampleStart",
	},
}
SampledReverb = {
	loadImpulse = loadImpulse,
}

function fadein(id, ms)
	host_call("FadeIn", id, math.floor((ms or 0) * 1000))
end

-- UVI's fade gain sits on top of changeVolume (its embedded api: `fade`
-- ramps from the current fade value, `fade2` from `from`; the fifth
-- argument is a layer, not a kill). A note not yet started fades in from
-- silence, as `fade2(playNote{...}, 0, 1, ms)` asks.
-- ponytail: the engine fades to full or to silence; a target between
-- holds full, and a started voice's `from` is where it is. Every flagship
-- call is (new note, 0, 1|0, ms).
function fade(id, to, ms)
	local micros = math.floor(math.max(0, ms or 0) * 1000)
	if (to or 0) <= 0 then
		host_call("FadeOut", id, micros, 0)
	else
		host_call("FadeIn", id, micros)
	end
end

function fade2(id, from, to, ms)
	fade(id, to, ms)
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

-- UVI's `_run`: the task runs at once, depth first, and the caller goes
-- on when it ends or first waits; the rest runs after that wait.
function run(f, ...)
	local co = coroutine.create(f)
	local ok, waited = coroutine.resume(co, ...)
	if not ok then
		error(waited, 0)
	elseif coroutine.status(co) == "suspended" then
		table.insert(__spawned, { co, waited or 0 })
	end
end

-- A path a script names, relative to its own folder in the bank (`__dir`),
-- as the bank path its data file is carried under.
local function bank_path(path)
	local found, dir = pcall(require, "__dir")
	local full = (found and type(dir) == "string" and dir .. "/" or "") .. tostring(path):gsub("\\", "/")
	local parts = {}
	for part in string.gmatch(full, "[^/]+") do
		if part == ".." then
			table.remove(parts)
		elseif part ~= "." then
			table.insert(parts, part)
		end
	end
	return table.concat(parts, "/")
end

-- Script presets saved this session (saveState/saveData), by bank path.
-- Each also goes out to the host (`__take_file`, read with the project's
-- state), which writes it under KONTRA's saves for this instrument; the
-- next load carries those files back as the bank's own.
local stored = {}
local outbox = {}

local function keep(path, file)
	local key = bank_path(path)
	stored[key] = file
	table.insert(outbox, key)
end

-- The oldest saved file the host has not written yet: its bank path and
-- its table; nothing once all are out.
function __take_file()
	local key = table.remove(outbox, 1)
	if key ~= nil then
		return key, stored[key]
	end
end

-- loadData(path, done): the JSON file's table. The program carries the
-- bank's data files (the format reader), so this reads no disk; a file it
-- does not carry calls nothing, as a failed load does.
function loadData(path, done)
	local file = stored[bank_path(path)] or __file(bank_path(path))
	local ok = file ~= nil and file.data ~= nil
	local task = make_task(AsyncDataLoadTask, { success = ok, data = ok and file.data or nil })
	if ok and type(done) == "function" then
		done(file.data, task)
	end
	return task
end

function loadTextData(path, done)
	local key = bank_path(path)
	local file = stored[key] or __file(key)
	local text = file and (file.text ~= nil and file.text or (type(file.data) == "string" and file.data or nil)) or nil
	local task = make_task(AsyncDataLoadTask, { success = text ~= nil, data = text })
	if text ~= nil and type(done) == "function" then
		done(text, task)
	end
	return task
end

function saveTextData(text, path, done)
	keep(path, { text = tostring(text or "") })
	local task = make_task(AsyncDataSaveTask, { success = true })
	if type(done) == "function" then
		done(task)
	end
	return task
end

function createMidiFile()
	return MidiSequence()
end

function saveMidi(seq, path, done)
	keep(path, { midi = seq })
	local task = make_task(AsyncDataSaveTask, { success = true })
	if type(done) == "function" then
		done(task)
	end
	return task
end

function loadMidi(path, done)
	local key = bank_path(path)
	local file = stored[key] or __file(key)
	local seq = file and file.midi or nil
	local task = make_task(AsyncMidiFileLoadTask, { success = seq ~= nil, midi = seq })
	if seq ~= nil and type(done) == "function" then
		done(task)
	end
	return task
end

local function glob_matches(path, filter)
	if type(filter) ~= "string" or filter == "" or filter == "*" or filter == "*.*" then
		return true
	end
	local lower = string.lower(path)
	for pat in string.gmatch(filter, "[^;,%s]+") do
		local lua_pat = "^" .. string.gsub(string.lower(pat), "([%.%+%-%^%$%(%)%[%]%%])", "%%%1")
		lua_pat = string.gsub(lua_pat, "%*", ".*") .. "$"
		local name = string.match(lower, "[^/]+$") or lower
		if string.match(name, lua_pat) then
			return true
		end
	end
	return false
end

function listFiles(path, filter, done)
	if type(filter) == "function" and done == nil then
		done, filter = filter, nil
	end
	local prefix = bank_path(path or "")
	local seen, result = {}, {}
	local function add(p)
		local lp = string.lower(p)
		if not seen[lp] and glob_matches(p, filter) then
			seen[lp] = true
			table.insert(result, p)
		end
	end
	if __list_files then
		for _, p in ipairs(__list_files(prefix)) do
			add(p)
		end
	end
	local pref_low = string.lower(prefix)
	for k in pairs(stored) do
		local kl = string.lower(k)
		if pref_low == "" or kl == pref_low or (string.sub(kl, 1, #pref_low) == pref_low and string.sub(kl, #pref_low + 1, #pref_low + 1) == "/") then
			add(k)
		end
	end
	table.sort(result)
	local task = make_task(AsyncListFilesTask, { success = true, result = result })
	if type(done) == "function" then
		done(task)
	end
	return task
end

local function walk_purge(el, purged)
	if type(el) ~= "userdata" then
		return
	end
	local fields = fields_of(el)
	if type(fields.group) == "number" then
		host_call("PurgeGroup", fields.group, purged and 0 or 1)
	end
	for _, key in ipairs(containers) do
		for _, child in ipairs(fields[key] or EMPTY) do
			walk_purge(child, purged)
		end
	end
end

function purge(target, done)
	if type(target) == "function" and done == nil then
		done, target = target, Program
	end
	walk_purge(target or Program, true)
	local task = make_task(AsyncTask, { success = true })
	if type(done) == "function" then
		done(task)
	end
	return task
end

function unpurge(target, done)
	if type(target) == "function" and done == nil then
		done, target = target, Program
	end
	walk_purge(target or Program, false)
	local task = make_task(AsyncTask, { success = true })
	if type(done) == "function" then
		done(task)
	end
	return task
end

function setPlaybackOptions(oscillator, options, done)
	if type(oscillator) == "userdata" and type(options) == "table" then
		local f = fields_of(oscillator)
		local p = f.params
		if p then
			for k, v in pairs(options) do
				p[k] = v
			end
		end
		if runtime_connected() then
			local parent = f.parent
			local g = parent and fields_of(parent).group
			if type(g) == "number" then
				local slot = (f.index or 1) - 1
				local start = tonumber(options.SampleStart or options.StartOffset or options.start)
				local loop_start = tonumber(options.LoopStart or options.loopStart)
				local loop_end = tonumber(options.LoopEnd or options.loopEnd)
				local loop_mode = tonumber(options.LoopMode or options.loopMode)
				local reverse = options.Reverse
				if reverse == nil then
					reverse = options.reverse
				end
				f.loadingStatus = (f.loadingStatus or 0) + 1
				local id = __runtime_playback_opts(g, slot, start, loop_start, loop_end, loop_mode, reverse)
				if id and id > 0 then
					return register_swap(id, oscillator, "playback", done)
				end
				f.loadingStatus = math.max(0, (f.loadingStatus or 1) - 1)
			end
		end
	end
	local task = make_task(AsyncTask, { success = type(oscillator) == "userdata" })
	if type(done) == "function" then
		done(task)
	end
	return task
end

-- loadState(path, done): a saved script preset. Its widgets take their
-- values and run `changed` as on a reload, then `onLoad` gets its state.
-- Milliseconds of audio between the controls a loaded preset sets: longer
-- than a 512-frame block at 48 kHz.
local PRESET_PACE = 12

-- Sets `file`'s values on the widgets, `pace(PRESET_PACE)` after each
-- control with a changed, then hands its state to onLoad and calls `done`.
local function land(file, done, pace, task)
	for _, w in ipairs(__ui.widgets) do
		if task and task.cancelled then
			return
		end
		local text = w.name and file.values[w.name]
		local ok, err = true, nil
		if text ~= nil and w.kind == "Table" and w.persistent ~= false then
			for i, v in ipairs(cells(text)) do
				w:setValue(i, v)
			end
			ok, err = pcall(run_changed, w)
		elseif text ~= nil and w.persistent ~= false then
			rawset(w, "__restored", true)
			ok, err = pcall(w.setValue, w, parsed(text, w.value))
		end
		if not ok then
			print("loadState " .. tostring(w.name) .. ": " .. tostring(err))
		end
		if text ~= nil and type(rawget(w, "changed")) == "function" then
			pace(PRESET_PACE)
		end
	end
	if task and task.cancelled then
		return
	end
	if onLoad and file.state ~= nil then
		onLoad(file.state)
	end
	if task then
		task.success = true
	end
	if type(done) == "function" then
		done(task or make_task(AsyncTask, { success = true }))
	end
end


-- A widget's value as a saved file holds it (what `land` reads back).
local function saved_text(w)
	if w.kind == "Table" then
		return table.concat(w.values or {}, " ")
	elseif type(w.value) == "boolean" then
		return w.value and "1" or "0"
	end
	return w.value
end

function saveState(path, done)
	local values = {}
	for _, w in ipairs(__ui.widgets) do
		if w.name and w.persistent ~= false then
			values[w.name] = saved_text(w)
		end
	end
	keep(path, { values = values, state = onSave and onSave() })
	local task = make_task(AsyncDataSaveTask, { success = true })
	if type(done) == "function" then
		done(task)
	end
	return task
end

function saveData(data, path, done)
	keep(path, { data = data })
	local task = make_task(AsyncDataSaveTask, { success = true })
	if type(done) == "function" then
		done(task)
	end
	return task
end

-- Milliseconds between looks at the editor's answer to a pick.
local PICK_PACE = 50

-- browseForFile(mode, title, initial, filter, done): the editor asks the
-- player for a file of the filter's type among those saved for this
-- instrument; `done` gets its name. A cancelled pick calls nothing.
function browseForFile(mode, _title, _initial, filter, done)
	local task = make_task(AsyncBrowseForFileTask, { success = false, result = nil })
	local ext = tostring(filter or ""):match("%*(%.[%w_]+)") or ""
	local id = __host and __host(Action.PickFile, mode == "save" and 1 or 0, ext)
	if type(id) ~= "number" or id <= 0 or type(done) ~= "function" then
		return task
	end
	spawn(function()
		while not task.cancelled do
			local answer = __host(Action.PickedFile, id)
			if task.cancelled then
				return
			end
			if type(answer) == "string" then
				task.result = answer
				task.success = true
				return done(task)
			elseif answer ~= nil then
				return
			end
			wait(PICK_PACE)
		end
	end)
	return task
end

function loadState(path, done)
	local task = make_task(AsyncTask, { success = false })
	local file = stored[bank_path(path)] or __file(bank_path(path))
	if not (file and file.values) then
		return task
	end
	if initializing then
		-- Init runs off the audio thread, and the project's saved values
		-- and state land after it, so they win over a default preset.
		land(file, done, function() end, task)
		return task
	end
	-- Once running, the preset lands after the call returns, as UVI's async
	-- `_loadState` task does. From a control move it lands at once, all its
	-- values together as UVI's do: the host slices the task to the block's
	-- budget (a VWinds preset is ~130 values; paced, a move made within
	-- 1.5 s of picking it was overwritten). From any other callback, which
	-- runs unsliced, one control's callbacks every PRESET_PACE ms of audio,
	-- so a preset's callbacks (VWinds, up to 4 ms each) never pile into one
	-- block.
	local pace = __editing and function() end or wait
	spawn(function()
		wait(PRESET_PACE)
		if not task.cancelled then
			land(file, done, pace, task)
		end
	end)
	return task
end

-- UVI's own: `setVoiceParameter(SampleStartMillisecond, id, ms)` (its
-- embedded api, "offset in milliseconds in the sample"). It acts on a note
-- before its voices start, as `playNote` returns in the same callback.
function setSampleOffset(id, ms)
	host_call("SetEventPar", id, START_OFFSET, math.floor(math.max(0, ms or 0) * 1000))
end

-- Keyboard colouring and key types forwarded to the host (`SetKeyColor` /
-- `SetKeyType` / `SetKeyPressed`): grey/gray marks out-of-range keys (type 0),
-- red marks keyswitches (type 2), transparent/default or zone colours mark
-- playable keys (type 1) so the host and keyboard display know the real range.
local KEY_COLOURS = {
	grey = 0xFF808080, gray = 0xFF808080, darkgrey = 0xFF404040, darkgray = 0xFF404040,
	red = 0xFFFF0000, darkred = 0xFF8B0000,
	blue = 0xFF0000FF, green = 0xFF00FF00, yellow = 0xFFFFFF00,
	orange = 0xFFFFA500, cyan = 0xFF00FFFF, magenta = 0xFFFF00FF,
	purple = 0xFF800080, white = 0xFFFFFFFF, black = 0xFF000000,
	none = 0, transparent = 0,
}

local function parse_key_colour(colour)
	if type(colour) == "number" then
		return math.floor(colour), 1
	end
	if type(colour) ~= "string" then
		return 0, 1
	end
	local s = string.lower(colour):gsub("^%s+", ""):gsub("%s+$", "")
	if s == "" or s == "none" or s == "transparent" then
		return 0, 1
	end
	if s == "grey" or s == "gray" or s == "darkgrey" or s == "darkgray" then
		return KEY_COLOURS[s], 0
	end
	if s == "red" or s == "darkred" then
		return KEY_COLOURS[s], 2
	end
	if KEY_COLOURS[s] then
		return KEY_COLOURS[s], 1
	end
	local hex = s:gsub("^#", ""):gsub("^0x", "")
	local n = tonumber(hex, 16)
	if not n then
		return 0, 1
	end
	if #hex == 8 then
		local alpha = math.floor(n / 0x1000000) % 256
		if alpha == 0 then
			return 0, 1
		end
	elseif #hex == 6 then
		n = 0xFF000000 + n
	end
	return n, 1
end

function setKeyColour(key, colour)
	local k = tonumber(key)
	if not k then return end
	k = math.floor(k)
	if k < 0 or k > 127 then return end
	local argb, kind = parse_key_colour(colour)
	host_call("SetKeyColor", k, argb)
	host_call("SetKeyType", k, kind)
end

function resetKeyColour(key)
	if key == nil then
		for k = 0, 127 do
			host_call("SetKeyColor", k, 0)
			host_call("SetKeyType", k, 0)
		end
		return
	end
	local k = tonumber(key)
	if not k then return end
	k = math.floor(k)
	if k < 0 or k > 127 then return end
	host_call("SetKeyColor", k, 0)
	host_call("SetKeyType", k, 0)
end

function setKeyType(key, kind)
	local k = tonumber(key)
	if not k then return end
	k = math.floor(k)
	if k < 0 or k > 127 then return end
	host_call("SetKeyType", k, math.floor(tonumber(kind) or 0))
end

function setKeyPressed(key, pressed)
	local k = tonumber(key)
	if not k then return end
	k = math.floor(k)
	if k < 0 or k > 127 then return end
	local on = pressed == true or (type(pressed) == "number" and pressed ~= 0)
	host_call("SetKeyPressed", k, on and 1 or 0)
end
