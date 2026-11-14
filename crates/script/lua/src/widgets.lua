-- UVIScript's UI classes beyond what the prelude's generic widget does
-- (https://lua.uvi.net/group___u_i.html): the buttons' push, the ranges,
-- the widgets bound to a program parameter, the non-visual
-- `ParameterValue`, and the classes this host has no surface for, which
-- keep what a script sets. Each kind registers what its widget gets as it
-- is made (`__ui_init`, the prelude's hook).

local init = __ui_init
__ui_init = nil

local function range(w)
	w.setRange = function(self, min, max)
		rawset(self, "min", tonumber(min) or self.min)
		rawset(self, "max", tonumber(max) or self.max)
	end
end

local function lottie(w, needs_mode)
	w.setLottieAnimation = function(self, path, mode)
		if needs_mode and mode == nil then
			error("setLottieAnimation: the mode is required")
		end
		rawset(self, "lottieAnimation", path)
		rawset(self, "lottieMode", mode == "trigger" and "trigger" or "toggle")
	end
end

-- Parameter ids are assigned in the order widgets are made.
local next_param = 0
local function param(w)
	next_param = next_param + 1
	rawset(w, "paramId", next_param)
	if rawget(w, "mapper") == nil then
		rawset(w, "mapper", Mapper.Linear)
	end
	if rawget(w, "unit") == nil then
		rawset(w, "unit", Unit.Generic)
	end
	if rawget(w, "exported") == nil then
		rawset(w, "exported", false)
	end
end

for _, kind in ipairs({ "Knob", "Slider", "NumBox" }) do
	init[kind] = function(w)
		param(w)
		range(w)
		lottie(w, false)
	end
end
init.Menu = function(w)
	param(w)
end
init.MultiStateButton = init.Menu
init.OnOffButton = function(w)
	param(w)
	lottie(w, true)
end
init.Button = function(w)
	lottie(w, true)
	-- A simulated press runs `changed` with no modifier keys.
	w.push = function(self, call)
		if call ~= false then
			local changed = rawget(self, "changed")
			if type(changed) == "function" then
				changed(self, nil)
			end
		end
	end
end

-- A widget bound to a program parameter mirrors it: `value` reads the
-- parameter, and the widget never runs `changed`.
local watchers = setmetatable({}, { __mode = "k" })
-- What the editor reads of a bound widget: the parameter's value as a
-- number (a boolean as 0 or 1), over a range from 0 to the larger of 1 and
-- the value, as the prelude's parameter definitions have it.
local function sync(w)
	local target = rawget(w, "__target")
	local value = target and target[1]:getParameter(target[2])
	if type(value) == "boolean" then
		value = value and 1 or 0
	end
	if type(value) == "number" then
		rawset(w, "__value", value)
		if rawget(w, "min") == nil then
			rawset(w, "min", 0)
			rawset(w, "max", math.max(1, value))
		end
	end
end
local function resolve(element, name)
	if type(element) == "string" then
		error("bind: no element at '" .. element .. "'")
	end
	if (type(element) ~= "table" and type(element) ~= "userdata") or not element.hasParameter or not element:hasParameter(name) then
		error("bind: missing parameter '" .. tostring(name) .. "'")
	end
	return { element, name }
end
local function bindable(w, fields)
	-- Unbound: no value until it is bound.
	rawset(w, "__value", nil)
	w.bind = function(self, element, name)
		local target = resolve(element, name)
		local was = rawget(self, "__target")
		if was and was[1] == target[1] and was[2] == target[2] then
			return
		end
		rawset(self, "__target", target)
		sync(self)
		local by = watchers[target[1]] or {}
		watchers[target[1]] = by
		by[name] = by[name] or {}
		table.insert(by[name], self)
	end
	local first, second = fields[1], fields[2]
	if (type(first) == "table" or type(first) == "userdata") and type(second) == "string" then
		w:bind(first, second)
	end
end
for _, kind in ipairs({ "ParamKnob", "ParamSlider", "ParamOnOffButton", "ParamMenu", "ParamNumBox" }) do
	init[kind] = function(w, fields)
		bindable(w, fields)
		if kind == "ParamKnob" or kind == "ParamSlider" then
			range(w)
			lottie(w, false)
		end
		if kind == "ParamOnOffButton" then
			lottie(w, true)
			w.setNormalImage = function(self, p) rawset(self, "normalImage", p) end
			w.setPressedImage = function(self, p) rawset(self, "pressedImage", p) end
			w.setOverImage = function(self, p) rawset(self, "overImage", p) end
		end
	end
end
init.ParameterValue = function(w, fields)
	bindable(w, fields)
	w.toString = function(self)
		return tostring(self.value)
	end
end
-- A parameter that changed runs the `changed` of each `ParameterValue` on it.
__watch = function(element, name)
	local list = watchers[element] and watchers[element][name]
	for _, w in ipairs(list or {}) do
		sync(w)
		if rawget(w, "kind") == "ParameterValue" then
			local changed = rawget(w, "changed")
			if type(changed) == "function" then
				changed(w)
			end
		end
	end
end
-- Once when the interface is built.
__ui_built = function()
	for _, by in pairs(watchers) do
		for _, list in pairs(by) do
			for _, w in ipairs(list) do
				local changed = rawget(w, "kind") == "ParameterValue" and rawget(w, "changed")
				if type(changed) == "function" then
					pcall(changed, w)
				end
			end
		end
	end
end

-- Classes with no surface here keep what a script sets and answer what
-- their methods return.
init.WaveView = function(w)
	w.bind = function(self, oscillator)
		rawset(self, "__oscillator", oscillator)
	end
end
init.Viewport = function(w)
	w.setViewPosition = function(self, x, y)
		rawset(self, "viewX", x)
		rawset(self, "viewY", y)
	end
end
init.AudioMeter = function(w, fields)
	-- AudioMeter(name, element, isVertical, busId, useOutput)
	rawset(w, "element", fields[2])
	rawset(w, "isVertical", fields[3] == true)
	rawset(w, "busId", fields[4])
	rawset(w, "useOutput", fields[5] == true)
	w.setStripImage = function(self, image, steps)
		self.stripImage, self.stripSteps = image, steps
	end
	lottie(w, false)
end
init.FileSelector = function(w)
	w.prev = function() end
	w.next = function() end
end

init.XY = function(w, fields)
	rawset(w, "paramX", fields[1])
	rawset(w, "paramY", fields[2])
end

-- The page's functions.
function setHeight(h)
	__ui.height = tonumber(h) or __ui.height
end
-- A widget moved to a place on the automatic grid (1-based).
function moveControl(w, column, row)
	rawset(w, "gridColumn", column)
	rawset(w, "gridRow", row)
end
function resetKeyColour() end
