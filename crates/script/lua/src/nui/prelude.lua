-- Kontakt 8 `native_ui` as a renderer of plain trees. A `@F{...}` (rewritten
-- `__nui(F, {...})`) is a node not yet expanded: `__render` expands the
-- entry's tree top-down, calling each component with its props, so the
-- contexts a parent provides reach its children. Hooks (`create_state`) are
-- kept by the node's path in the tree, so a state lives across renders.
-- The result holds primitives only: { kind, props, mods, children }.

-- Kontakt's addition to the table library.
function table.shallow_copy(t)
	local c = {}
	for k, v in pairs(t) do
		c[k] = v
	end
	return c
end

local Node = {}
-- Any method on a node is a modifier: recorded, in order, and chained.
Node.__index = function(_, name)
	return function(self, ...)
		local mods = rawget(self, "mods")
		mods[#mods + 1] = { name, ... }
		return self
	end
end

function __nui(f, props)
	if f == nil then
		error("unknown component", 2)
	end
	return setmetatable({ f = f, props = props or {}, mods = {} }, Node)
end

local function is_node(v)
	return getmetatable(v) == Node
end

-- A node's positional items in order, past the holes a `@{ cond and x }`
-- leaves when it is nil.
local function positional(t)
	local last = 0
	for k in pairs(t) do
		if type(k) == "number" and k > last then
			last = k
		end
	end
	local out = {}
	for i = 1, last do
		if t[i] ~= nil and t[i] ~= false then
			out[#out + 1] = t[i]
		end
	end
	return out
end

-- Colours: 0..255 channels and an alpha 0..1; `ui.Color(0xAARRGGBB)` too.
local Color = {}
Color.__index = Color
local function color(r, g, b, a)
	return setmetatable({ __color = true, r = r, g = g, b = b, a = a or 1 }, Color)
end
function Color:opacity(a)
	return color(self.r, self.g, self.b, self.a * a)
end
local named = {
	white = color(255, 255, 255),
	black = color(0, 0, 0),
	red = color(255, 0, 0),
	blue = color(0, 0, 255),
	grey = color(128, 128, 128),
	none = color(0, 0, 0, 0),
}
local ColorType = setmetatable({}, {
	__index = named,
	__call = function(_, r, g, b, a)
		if g == nil then
			local n = r or 0
			return color(bit32.band(bit32.rshift(n, 16), 255), bit32.band(bit32.rshift(n, 8), 255), bit32.band(n, 255), bit32.band(bit32.rshift(n, 24), 255) / 255)
		end
		return color(r, g, b, a)
	end,
})

-- Rendering state: the node being expanded, its hooks, the states kept.
local states = {}
local current = { path = "", hook = 0, context = {} }
__dirty = false

-- The functions native_ui made to be read (a state's, a computed, a
-- context's): only these are read while drawing; any other function a
-- page passes (a setter, a gesture's) is a callback, never called then.
local getters = setmetatable({}, { __mode = "k" })
local function getter(f)
	getters[f] = true
	return f
end

local function hook_key()
	current.hook = current.hook + 1
	return current.path .. "#" .. current.hook
end

local ui = { Color = ColorType, no_style = {} }
for _, name in ipairs({ "ZStack", "HStack", "VStack", "Text", "Rectangle", "Image", "Spacer", "Ring", "ForEach", "Canvas", "TextInput" }) do
	ui[name] = { __primitive = name }
end
function ui.create_state(initial)
	local key = hook_key()
	if states[key] == nil then
		states[key] = { value = initial }
	end
	local s = states[key]
	return getter(function()
		return s.value
	end), function(v)
		if s.value ~= v then
			s.value = v
			__dirty = true
		end
	end
end
function ui.create_computed(f)
	return getter(f)
end
function ui.create_effect(f) end
function ui.create_context(default)
	return { default = default }
end
-- A getter, as `create_state`'s: the value its nearest provider gave.
function ui.use_context(c)
	local v = current.context[c]
	if v == nil then
		v = c.default
	end
	return getter(function()
		return v
	end)
end
function ui.is_enabled()
	return getter(function()
		return true
	end)
end
function ui.resign_focus() end
setmetatable(ui, {
	__index = function(_, k)
		error("native_ui." .. tostring(k) .. " is not provided", 2)
	end,
})

-- The host's parameters by name (`__params`, set before each render) and
-- what the page wrote (`__writes`, for the host to apply).
__params, __meters, __writes = {}, {}, {}
local Parameter = {}
Parameter.__index = Parameter
function Parameter:value()
	return __params[self.id] or 0
end
function Parameter:set_value(v)
	__params[self.id] = v
	__writes[#__writes + 1] = { self.id, v }
	__dirty = true
end
-- The control's KSP properties, as the page's own uses read them: 0 and 14
-- its label, 16 its default, 17 its entries' count, 18/19/20 entry `i`'s
-- text, value and shown, 21/22 its range. From the host (`__controls`).
__controls = {}
local ENTRY = { [18] = "text", [19] = "value", [20] = "shown" }
function Parameter:ksp_control_property(property, i)
	local c = __controls[self.id] or {}
	if ENTRY[property] then
		local e = (c.entries or {})[(i or 0) + 1]
		return e and e[ENTRY[property]]
	end
	if property == 0 or property == 14 then
		return c.label or ""
	elseif property == 16 then
		return c.default
	elseif property == 17 then
		return #(c.entries or {})
	elseif property == 21 then
		return c.min
	elseif property == 22 then
		return c.max
	end
	return nil
end
function Parameter:update_touch() end
function Parameter:end_touch() end
function Parameter:begin_midi_learn() end
function Parameter:end_midi_learn() end
function Parameter:is_touch_active()
	return false
end
function Parameter:is_midi_learn_active()
	return false
end
local kontakt = {
	connect_parameter = function(id)
		return setmetatable({ id = id }, Parameter)
	end,
	connect_level_meter = function(id)
		return {
			level_value = getter(function()
				return __meters[id] or 0
			end),
		}
	end,
	create_tmp_midi_file = function()
		return ""
	end,
}

local util = {
	clamp = function(v, lo, hi)
		return math.min(math.max(v, lo), hi)
	end,
	map_range = function(v, a, b, c, d)
		if b == a then
			return c
		end
		return c + (v - a) * (d - c) / (b - a)
	end,
	partial = function(f, ...)
		local bound = { ... }
		return function(...)
			local args = { table.unpack(bound) }
			for _, a in ipairs({ ... }) do
				args[#args + 1] = a
			end
			return f(table.unpack(args))
		end
	end,
}
setmetatable(util, {
	__index = function(_, k)
		error("native_ui_util." .. tostring(k) .. " is not provided", 2)
	end,
})
__builtins = { native_ui = ui, native_ui_util = util, kontakt = kontakt }

-- Callbacks the tree holds (gestures), by number for the host.
__callbacks = {}

local expand

-- A value a primitive draws with, its nodes expanded: a computed one
-- (one of `getters`) read now, any other function numbered as a callback.
local function plain(v, path, context, reactive)
	if is_node(v) then
		return expand(v, path, context)
	elseif type(v) == "function" and reactive and getters[v] then
		local ok, value = pcall(v)
		if ok and type(value) ~= "function" then
			return plain(value, path, context)
		end
	end
	if type(v) == "function" then
		__callbacks[#__callbacks + 1] = v
		return { __callback = #__callbacks }
	elseif type(v) == "table" and not v.__color then
		local out = {}
		for k, x in pairs(v) do
			out[k] = plain(x, path .. "." .. tostring(k), context, reactive)
		end
		return out
	end
	return v
end

local function expand_mods(mods, path, context)
	local out = {}
	for i, m in ipairs(mods) do
		local e = { m[1] }
		local reactive = string.sub(m[1], 1, 3) ~= "on_"
		for j = 2, #m do
			e[j] = plain(m[j], path .. "/m" .. i, context, reactive)
		end
		out[i] = e
	end
	return out
end

expand = function(v, path, context)
	if not is_node(v) then
		return nil
	end
	-- `.context(key = k, value = v)` provides to the node's whole subtree.
	for _, m in ipairs(v.mods) do
		if m[1] == "context" and type(m[2]) == "table" then
			context = setmetatable({ [m[2].key] = m[2].value }, { __index = context })
		end
	end
	local f = v.f
	if type(f) == "table" and f.__primitive then
		local out = { kind = f.__primitive, props = {}, children = {}, mods = expand_mods(v.mods, path, context) }
		for k, x in pairs(v.props) do
			if type(k) == "string" and k ~= "content" and k ~= "data" then
				out.props[k] = plain(x, path .. "." .. k, context, true)
			end
		end
		local children = {}
		if f.__primitive == "ForEach" then
			local data = v.props.data
			if type(data) == "function" then
				data = data()
			end
			-- Each item's content runs where the ForEach stands: its
			-- contexts, its own hooks.
			local saved = current
			current = { path = path .. "/each", hook = 0, context = context }
			for i, item in ipairs(data or {}) do
				local ok, child = pcall(v.props.content, item, getter(function()
					return i
				end))
				if not ok then
					current = saved
					error(child, 0)
				end
				children[i] = child
			end
			current = saved
		else
			children = positional(v.props)
		end
		for i, c in ipairs(children) do
			local e = expand(c, path .. "/" .. i, context)
			if e then
				out.children[#out.children + 1] = e
			end
		end
		return out
	end
	-- A component sees its positional items as `props.children`.
	local props = v.props
	if props.children == nil then
		local items = positional(props)
		if #items > 0 then
			props.children = items
		end
	end
	local saved = current
	current = { path = path, hook = 0, context = context }
	local ok, result = pcall(f, props)
	current = saved
	if not ok then
		error(result, 0)
	end
	local out = expand(result, path .. "/", context)
	if out then
		-- A component's own modifiers wrap its root.
		for _, m in ipairs(expand_mods(v.mods, path, context)) do
			out.mods[#out.mods + 1] = m
		end
	end
	return out
end

-- The entry's tree, from `root` (the entry module's value: a function).
function __render(root)
	__callbacks = {}
	__dirty = false
	return expand(__nui(root, {}), "", {})
end
