-- UVIScript UI probe: every widget class, attribute, method and callback of
-- the UVI script reference (https://lua.uvi.net/group___u_i.html), each
-- checked against what the reference says, one line per member:
--   `Class.member: ok` or `Class.member: FAIL <what came back>`.
-- Run it in KONTRA (crates/script/lua/tests/uvi_ui.rs diffs it with
-- probe.expected) and in UVI Workstation (print goes to its console) and
-- diff the two. A member that fails is a gap, listed in docs/uvi/UI-WIDGETS.md.

local function check(id, fn)
	local ok, got = pcall(fn)
	if ok and got == true then
		print(id .. ": ok")
	else
		print(id .. ": FAIL " .. tostring(got))
	end
end

-- Every attribute a class documents reads back what a script set on it.
local function attributes(class, make, values)
	local names = {}
	for name in pairs(values) do
		table.insert(names, name)
	end
	table.sort(names)
	for _, name in ipairs(names) do
		check(class .. "." .. name, function()
			local w = make()
			w[name] = values[name]
			local got = w[name]
			if type(got) == "table" then
				for i, v in ipairs(values[name]) do
					if got[i] ~= v then
						return "table " .. tostring(got[i]) .. " ~= " .. tostring(v)
					end
				end
				return true
			end
			return got == values[name] or tostring(got)
		end)
	end
end

local colour = "#FF00CC"
local widget_attributes = {
	displayName = "shown", displayText = "text", tooltip = "tip", x = 3, y = 4, width = 50, height = 20,
	alpha = 0.5, enabled = false, visible = false, interceptsMouseClicks = false, showLabel = false,
	persistent = false,
}
local function with(base, extra)
	local out = {}
	for k, v in pairs(base) do
		out[k] = v
	end
	for k, v in pairs(extra or {}) do
		out[k] = v
	end
	return out
end
local param_attributes = with(widget_attributes, { mapper = Mapper.Exponential, unit = Unit.Hertz, exported = true })

-- Widget: geometry aliases and tables.
check("Widget.w", function()
	local w = Knob("a", 0, 0, 1)
	w.w = 40
	return w.width == 40 or w.width
end)
check("Widget.h", function()
	local w = Knob("a", 0, 0, 1)
	w.h = 30
	return w.height == 30 or w.height
end)
check("Widget.size", function()
	local w = Knob("a", 0, 0, 1)
	w.size = { 10, 20 }
	return (w.width == 10 and w.height == 20) or "no"
end)
check("Widget.position", function()
	local w = Knob("a", 0, 0, 1)
	w.position = { 5, 6 }
	return (w.x == 5 and w.y == 6) or "no"
end)
check("Widget.pos", function()
	local w = Knob("a", 0, 0, 1)
	w.pos = { 7, 8 }
	return (w.x == 7 and w.y == 8) or "no"
end)
check("Widget.bounds", function()
	local w = Knob("a", 0, 0, 1)
	w.bounds = { 1, 2, 30, 40 }
	local b = w.bounds
	return (b[1] == 1 and b[2] == 2 and b[3] == 30 and b[4] == 40) or "no"
end)
check("Widget.name", function()
	return Knob("nm", 0, 0, 1).name == "nm"
end)
check("Widget.displayName.default", function()
	return Knob("nm", 0, 0, 1).displayName == "nm"
end)
check("Widget.tooltip.default", function()
	return Knob("nm", 0, 0, 1).tooltip == "nm"
end)
attributes("Widget", function() return Label("l") end, widget_attributes)

-- ParamWidget.
check("ParamWidget.paramId", function()
	local a, b = Knob("a", 0, 0, 1), Slider("b", 0, 0, 1)
	return (type(a.paramId) == "number" and b.paramId ~= a.paramId) or "no"
end)
check("ParamWidget.mapper.default", function()
	return Knob("a", 0, 0, 1).mapper == Mapper.Linear or "not linear"
end)
check("Mapper.values", function()
	for _, n in ipairs({ "Linear", "Exponential", "QuinticRoot", "QuarticRoot", "CubeRoot", "SquareRoot", "Quadratic", "Cubic", "Quartic", "Quintic" }) do
		if Mapper[n] == nil then
			return n
		end
	end
	return true
end)
check("Unit.values", function()
	for _, n in ipairs({ "Generic", "Percent", "PercentNormalized", "Seconds", "MilliSeconds", "Hertz", "Decibels", "LinearGain", "UviFilter", "Pan", "Megabyte", "SemiTones", "Cents", "MidiKey" }) do
		if Unit[n] == nil then
			return n
		end
	end
	return true
end)
check("RectanglePlacement.values", function()
	for _, n in ipairs({ "Left", "Right", "HorizontallyCentred", "Top", "Bottom", "VerticallyCentred", "StretchToFit", "FillDestination", "OnlyReduceInSize", "OnlyIncreaseInSize", "DoNotResize", "Centred" }) do
		if RectanglePlacement[n] == nil then
			return n
		end
	end
	return true
end)
check("FileFormat.values", function()
	return (FileFormat.All and FileFormat.Audio and FileFormat.MIDI) and true or "missing"
end)

-- Knob.
local knob_own = { showPopupDisplay = false, showValue = false, fillColour = colour, outlineColour = colour, stripImage = "k.png", lottieAnimation = "k.json", default = 0.25 }
attributes("Knob", function() return Knob("k", 0.5, 0, 1) end, with(with(widget_attributes, param_attributes), knob_own))
check("Knob.constructor", function()
	local k = Knob("k", 0.5, -1, 2, true)
	return (k.value == 0.5 and k.min == -1 and k.max == 2 and k.integer == true) or "no"
end)
check("Knob.setValue.changed", function()
	local k, n = Knob("k", 0.5, 0, 1), 0
	k.changed = function() n = n + 1 end
	k:setValue(0.7)
	k:setValue(0.8, false)
	return (n == 1 and k.value == 0.8) or n
end)
check("Knob.setRange", function()
	local k = Knob("k", 0.5, 0, 1)
	k:setRange(-2, 3)
	return (k.min == -2 and k.max == 3) or "no"
end)
check("Knob.setStripImage", function()
	local k = Knob("k", 0.5, 0, 1)
	k:setStripImage("s.png", 64)
	return (k.stripImage == "s.png") or "no"
end)
check("Knob.setLottieAnimation", function()
	local k = Knob("k", 0.5, 0, 1)
	k:setLottieAnimation("k.json")
	return k.lottieAnimation == "k.json" or "no"
end)

-- Slider.
local slider_own = { backgroundColour = colour, thumbColour = colour, trackColour = colour, fillColour = colour, backgroundImage = "b.png", handleImage = "h.png", showPopupDisplay = true, lottieAnimation = "s.json", default = 0.1 }
attributes("Slider", function() return Slider("s", 0.5, 0, 1) end, with(with(widget_attributes, param_attributes), slider_own))
check("Slider.constructor", function()
	local s = Slider("s", 3, 0, 10, true, true)
	return (s.value == 3 and s.integer == true and s.vertical == true) or "no"
end)
check("Slider.setValue", function()
	local s = Slider("s", 3, 0, 10)
	s:setValue(5)
	return s.value == 5 or s.value
end)
check("Slider.setRange", function()
	local s = Slider("s", 3, 0, 10)
	s:setRange(1, 2)
	return (s.min == 1 and s.max == 2) or "no"
end)

-- Button.
local button_own = { normalImage = "n.png", pressedImage = "p.png", overImage = "o.png", backgroundColourOff = colour, textColourOff = colour, backgroundColourOn = colour, textColourOn = colour, triggeredOnMouseDown = true, dragAndDropFilepath = "f", dragAndDropText = "t", fontSize = 12, font = "f.ttf", stripImage = "s.png", lottieAnimation = "b.json" }
attributes("Button", function() return Button("b") end, with(widget_attributes, button_own))
check("Button.push.changed", function()
	local b, got = Button("b"), "unset"
	b.changed = function(self, mods) got = mods end
	b:push(true)
	return got == nil or "mods"
end)
check("Button.push.silent", function()
	local b, n = Button("b"), 0
	b.changed = function() n = n + 1 end
	b:push(false)
	return n == 0 or n
end)
check("Button.setStripImage", function()
	local b = Button("b")
	b:setStripImage("s.png", 6)
	return b.stripImage == "s.png" or "no"
end)
check("Button.setLottieAnimation.mode", function()
	local b = Button("b")
	local ok = pcall(b.setLottieAnimation, b, "a.json")
	return (not ok) or "no error without a mode"
end)
check("Button.setLottieAnimation", function()
	local b = Button("b")
	b:setLottieAnimation("a.json", "trigger")
	return b.lottieAnimation == "a.json" or "no"
end)

-- OnOffButton.
local onoff_own = with(button_own, { overPressedImage = "op.png", showPopupDisplay = true, default = true })
onoff_own.dragAndDropFilepath, onoff_own.dragAndDropText = nil, nil
attributes("OnOffButton", function() return OnOffButton("o", false) end, with(with(widget_attributes, param_attributes), onoff_own))
check("OnOffButton.setValue", function()
	local b, n = OnOffButton("o", false), 0
	b.changed = function() n = n + 1 end
	b:setValue(true)
	b:setValue(false, false)
	return (n == 1 and b.value == false) or n
end)
check("OnOffButton.changed.mods", function()
	local b, mods = OnOffButton("o", false), "none"
	b.changed = function(self, m) mods = m end
	b:setValue(true)
	return mods == nil or "mods on a script move"
end)
check("OnOffButton.setLottieAnimation.mode", function()
	local b = OnOffButton("o", false)
	return (not pcall(b.setLottieAnimation, b, "a.json")) or "no error without a mode"
end)

-- Label.
local label_own = { text = "hi", align = "left", fontSize = 11, font = "f.ttf", backgroundColour = colour, textColour = colour, backgroundColourWhenEditing = colour, textColourWhenEditing = colour, editable = true, minimumHorizontalScale = 0.5, url = "https://example.com" }
attributes("Label", function() return Label("l") end, with(widget_attributes, label_own))
check("Label.text.silent", function()
	local l, n = Label("l"), 0
	l.changed = function() n = n + 1 end
	l.text = "x"
	return n == 0 or n
end)

-- Menu and MultiStateButton.
local menu_own = { showPopupDisplay = true, fontSize = 11, font = "f.ttf", popupFont = "p.ttf", popupFontSize = 12, align = "left", backgroundImage = "b.png", backgroundColour = colour, textColour = colour, outlineColour = colour, arrowColour = colour, displayArrow = false, popupBackgroundColour = colour, popupTextColour = colour, popupHighlightedBackgroundColour = colour, popupHighlightedTextColour = colour, menuHeight = 18, hierarchical = true, minimumHorizontalScale = 0.5, default = 2 }
attributes("Menu", function() return Menu("m", { "a", "b", "c" }) end, with(with(widget_attributes, param_attributes), menu_own))
local function items_class(class, make)
	check(class .. ".constructor", function()
		local m = make()
		return (m.value == 2 and m.length == 3) or ("value " .. tostring(m.value) .. " length " .. tostring(m.length))
	end)
	check(class .. ".addItem", function()
		local m = make()
		return m:addItem("d") == 4 or "index"
	end)
	check(class .. ".setItem", function()
		local m = make()
		m:setItem(2, "B")
		return m:getText(2) == "B" or "no"
	end)
	check(class .. ".getText", function()
		local m = make()
		return (m:getText(1) == "a" and m:getText(9) == "") or "no"
	end)
	check(class .. ".clear", function()
		local m = make()
		m:clear()
		return m.length == 0 or m.length
	end)
	check(class .. ".selected", function()
		local m = make()
		m.selected = 3
		return (m.value == 3 and m.selected == 3 and m.selectedText == "c" and m.text == "c") or "no"
	end)
	check(class .. ".items", function()
		return #make().items == 3
	end)
	check(class .. ".setValue", function()
		local m, n = make(), 0
		m.changed = function() n = n + 1 end
		m:setValue(3)
		m:setValue(1, false)
		return (n == 1 and m.value == 1) or n
	end)
end
items_class("Menu", function() return Menu("m", { "a", "b", "c" }, 2) end)
local state_own = { showPopupDisplay = true, backgroundImage = "b.png", backgroundColour = colour, textColour = colour, default = 2 }
attributes("MultiStateButton", function() return MultiStateButton("m", { "a", "b", "c" }) end, with(with(widget_attributes, param_attributes), state_own))
items_class("MultiStateButton", function() return MultiStateButton("m", { "a", "b", "c" }, 2) end)
check("MultiStateButton.changed.mods", function()
	local b, mods = MultiStateButton("m", { "a", "b" }), "none"
	b.changed = function(self, m) mods = m end
	b:setValue(2)
	return mods == nil or "mods on a script move"
end)

-- Table.
local table_own = { default = 0.5, backgroundColour = colour, sliderColour = colour, showPopupDisplay = true, fillStyle = "solid", drawInnerEdge = false, innerEdgeColour = colour }
attributes("Table", function() return Table("t", 4, 0, 0, 1) end, with(widget_attributes, table_own))
check("Table.constructor", function()
	local t = Table("t", 4, 0.5, -1, 1, true)
	return (t.length == 4 and t.min == -1 and t.max == 1 and t:getValue(1) == 0.5) or "no"
end)
check("Table.setValue", function()
	local t = Table("t", 4, 0, 0, 1)
	t:setValue(2, 0.75)
	return t:getValue(2) == 0.75 or "no"
end)
check("Table.setValue.changed", function()
	local t, at = Table("t", 4, 0, 0, 1), nil
	t.changed = function(self, index) at = index end
	t:setValue(3, 0.5)
	return at == 3 or tostring(at)
end)
check("Table.setValue.silent", function()
	local t, n = Table("t", 4, 0, 0, 1), 0
	t.changed = function() n = n + 1 end
	t:setValue(3, 0.5, false)
	return n == 0 or n
end)
check("Table.length.resize", function()
	local t = Table("t", 2, 0.25, 0, 1)
	t.length = 4
	return (#t.values == 4 and t:getValue(4) == 0.25) or "no"
end)
check("Table.setRange", function()
	local t = Table("t", 2, 0, 0, 1)
	t:setRange(-1, 2)
	return (t.min == -1 and t.max == 2) or "no"
end)

-- NumBox.
local numbox_own = { showPopupDisplay = true, fontSize = 11, font = "f.ttf", align = "left", minimumHorizontalScale = 0.5, hitTestOnlyOnText = false, backgroundImage = "b.png", backgroundColour = colour, textColour = colour, default = 2 }
attributes("NumBox", function() return NumBox("n", 1, 0, 9) end, with(with(widget_attributes, param_attributes), numbox_own))
check("NumBox.setValue", function()
	local n, c = NumBox("n", 1, 0, 9), 0
	n.changed = function() c = c + 1 end
	n:setValue(5)
	n:setValue(6, false)
	return (c == 1 and n.value == 6) or c
end)
check("NumBox.setRange", function()
	local n = NumBox("n", 1, 0, 9)
	n:setRange(2, 3)
	return (n.min == 2 and n.max == 3) or "no"
end)

-- Image, SVG, DnDArea.
attributes("Image", function() return Image("a.png") end, with(widget_attributes, { overImage = "o.png", rectanglePlacement = RectanglePlacement.Centred }))
check("Image.constructor", function()
	return Image("a.png").image == "a.png" or "no"
end)
attributes("SVG", function() return SVG("a.svg") end, with(widget_attributes, { rectanglePlacement = RectanglePlacement.Centred }))
check("SVG.constructor", function()
	return SVG("a.svg").svg == "a.svg" or "no"
end)
attributes("DnDArea", function() return DnDArea("d") end, with(widget_attributes, { acceptedFileFormat = FileFormat.Audio, image = "i.png", droppingImage = "d.png", backgroundColour = colour, droppingColour = colour, acceptedText = "x", fileDropped = function() end, textDropped = function() end }))

-- Panel, Viewport.
attributes("Panel", function() return Panel("p") end, with(widget_attributes, { backgroundImage = "b.png", backgroundColour = colour }))
check("Panel.children", function()
	local p = Panel("p")
	local k = p:Knob("k", 0, 0, 1)
	local q = p:Panel("q")
	return (k.name == "k" and q.name == "q") or "no"
end)
attributes("Viewport", function() return Viewport("v") end, with(widget_attributes, { scrollBarThickness = 9, scrollBarThumbColour = colour, scrollBarTrackColour = colour, scrollBarBackgroundColour = colour }))
check("Viewport.setViewPosition", function()
	local v = Viewport("v")
	v:setViewPosition(3, 4)
	return true
end)

-- WaveView, XY, AudioMeter, FileSelector.
attributes("WaveView", function() return WaveView("w") end, with(widget_attributes, { sample = "s.wav", playbackCursorColour = colour, hiWaveColour = colour, lowWaveColour = colour, showPeak = false, showRms = false, axisColour = colour, channelSeparatorColour = colour, sampleNameColour = colour, sampleNameBackgroundColour = colour }))
check("WaveView.bind", function()
	WaveView("w"):bind(Program.layers[1].keygroups[1].oscillators[1])
	return true
end)
attributes("XY", function() return XY("a", "b") end, with(widget_attributes, { handleImage = "h.png", backgroundColour = colour, handleColour = colour, canOnlyDragHandle = true }))
check("XY.constructor", function()
	local xy = XY("a", "b")
	return (xy.paramX == "a" and xy.paramY == "b") or "no"
end)
attributes("AudioMeter", function() return AudioMeter("m", Program, true, 0, false) end, with(widget_attributes, { backgroundColour = colour, foregroundColour = colour, textColour = colour, overColour = colour, ["0dBColour"] = colour, ["3dBColour"] = colour, ["6dBColour"] = colour, ["10dBColour"] = colour, displayScale = true, displayAsMono = true, lottieAnimation = "m.json" }))
check("AudioMeter.setStripImage", function()
	local m = AudioMeter("m", Program, true, 0, false)
	m:setStripImage("s.png", 8)
	return m.stripImage == "s.png" or "no"
end)
attributes("FileSelector", function() return FileSelector("f") end, with(widget_attributes, { basepath = "/b", filepath = "/b/f", filetype = "*.wav", textColour = colour, backgroundColour = colour, highlightColour = colour, arrowColour = colour, scrollBarThumbColour = colour, scrollBarTrackColour = colour, scrollBarBackgroundColour = colour, fillStyle = "solid", fontSize = 12, font = "f.ttf", scrollBarThickness = 8, columnWidth = 100, rowHeight = 18, displayExtension = false, allowDnD = true, invertColourOnSelection = true, pathSelected = function() end, pathActivated = function() end }))
check("FileSelector.prev", function()
	FileSelector("f"):prev()
	return true
end)
check("FileSelector.next", function()
	FileSelector("f"):next()
	return true
end)

-- The widgets bound to a program parameter, and the non-visual one.
local element = Program.layers[1]
local function bound(class)
	local w = _G[class](element, "Gain")
	return w
end
for _, class in ipairs({ "ParamKnob", "ParamSlider", "ParamOnOffButton", "ParamMenu", "ParamNumBox" }) do
	check(class .. ".bind.value", function()
		local w = bound(class)
		return w.value == element:getParameter("Gain") or tostring(w.value)
	end)
	check(class .. ".unbound.value", function()
		return _G[class]("u").value == nil or "not nil"
	end)
	check(class .. ".rebind", function()
		local w = _G[class]("u")
		w:bind(element, "Gain")
		return w.value == element:getParameter("Gain") or tostring(w.value)
	end)
	check(class .. ".bind.missing", function()
		local w = _G[class]("u")
		return (not pcall(w.bind, w, element, "NoSuchParameter")) or "no error"
	end)
	check(class .. ".readonly", function()
		local w = bound(class)
		local was = w.value
		w.value = 99
		return w.value == was or "written"
	end)
	check(class .. ".never.changed", function()
		local w, n = bound(class), 0
		w.changed = function() n = n + 1 end
		element:setParameter("Gain", 0.123)
		return n == 0 or n
	end)
end
check("ParamKnob.attributes", function()
	local k = bound("ParamKnob")
	k.unit, k.default, k.stripImage, k.showValue, k.showPopupDisplay = Unit.Hertz, 1, "s.png", false, false
	k.fillColour, k.outlineColour, k.lottieAnimation = colour, colour, "a.json"
	return (k.default == 1 and k.fillColour == colour) or "no"
end)
check("ParamKnob.setRange", function()
	local k = bound("ParamKnob")
	k:setRange(0, 2)
	return true
end)
check("ParamSlider.attributes", function()
	local s = bound("ParamSlider")
	s.horizontal, s.thumbColour, s.trackColour = true, colour, colour
	return (s.horizontal == true and s.thumbColour == colour) or "no"
end)
check("ParamOnOffButton.setNormalImage", function()
	local b = bound("ParamOnOffButton")
	b:setNormalImage("n.png")
	b:setPressedImage("p.png")
	b:setOverImage("o.png")
	return (b.normalImage == "n.png" and b.pressedImage == "p.png" and b.overImage == "o.png") or "no"
end)
check("ParamMenu.attributes", function()
	local m = bound("ParamMenu")
	m.hierarchical, m.displayArrow, m.menuHeight, m.popupFontSize = true, false, 18, 12
	return (m.hierarchical == true and m.menuHeight == 18) or "no"
end)
check("ParamNumBox.attributes", function()
	local n = bound("ParamNumBox")
	n.hitTestOnlyOnText, n.align, n.fontSize = false, "left", 11
	return (n.align == "left") or "no"
end)

check("ParameterValue.value", function()
	local p = ParameterValue(element, "Gain")
	return (p.bound == true and p.value == element:getParameter("Gain")) or "no"
end)
check("ParameterValue.unbound", function()
	local p = ParameterValue("u")
	return (p.bound == false and p.value == nil) or "no"
end)
check("ParameterValue.write", function()
	local p = ParameterValue(element, "Gain")
	p.value = 0.5
	return element:getParameter("Gain") == 0.5 or "no"
end)
check("ParameterValue.changed", function()
	local p, n = ParameterValue(element, "Gain"), 0
	p.changed = function() n = n + 1 end
	element:setParameter("Gain", 0.25)
	element:setParameter("Gain", 0.75)
	return n == 2 or n
end)
check("ParameterValue.toString", function()
	local p = ParameterValue(element, "Gain")
	return type(p:toString()) == "string" or "no"
end)
check("ParameterValue.bind", function()
	local p = ParameterValue("u")
	p:bind(element, "Gain")
	return p.bound == true or "no"
end)

-- Functions of the page.
check("setBackground", function() setBackground("b.png") return true end)
check("setBackgroundColour", function() setBackgroundColour("black") return true end)
check("setSize", function() setSize(300, 200) return true end)
check("setHeight", function() setHeight(200) return true end)
check("makePerformanceView", function() makePerformanceView() return true end)
check("moveControl", function() moveControl(Knob("g", 0, 0, 1), 1, 1) return true end)
check("setKeyColour", function() setKeyColour(60, "red") return true end)
check("resetKeyColour", function() resetKeyColour(60) return true end)
