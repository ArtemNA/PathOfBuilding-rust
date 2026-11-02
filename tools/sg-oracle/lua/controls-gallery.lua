-- U2a control galleries for controls-runner.lua: each builder creates ORIGINAL control classes in a ControlHost and
-- returns { host = ControlHost, dump = function() -> state table }. The Rust twin of every builder lives in
-- crates/pob-ui/tests/gallery/mod.rs and must stay line-for-line equivalent (same rects, labels, callbacks).
local G = {}

-- Label, Button (plain / disabled / locked / + / - / x / tooltip), CheckBox (left/right label, disabled, tooltip),
-- RectangleOutline, Section.
function G.basic()
	local host = new("ControlHost"):ControlHost()
	local c = host.controls
	local st = { clicks = 0, checked = 0, lastCheck = "none" }
	c.section = new("SectionControl"):SectionControl(nil, { 20, 30, 400, 250 }, "^7Section label")
	c.title = new("LabelControl"):LabelControl(nil, { 40, 50, 0, 16 }, "^7Label 16 ^1red ^xFFD700gold")
	c.small = new("LabelControl"):LabelControl({ "TOPLEFT", c.title, "BOTTOMLEFT" }, { 0, 4, 0, 14 }, function() return "^8Clicks: " .. st.clicks end)
	c.button = new("ButtonControl"):ButtonControl({ "TOPLEFT", c.small, "BOTTOMLEFT" }, { 0, 10, 100, 20 }, "Click me", function()
		st.clicks = st.clicks + 1
	end)
	c.button.tooltipText = "Button tooltip\nSecond line ^2green"
	c.disabled = new("ButtonControl"):ButtonControl({ "LEFT", c.button, "RIGHT" }, { 8, 0, 90, 20 }, "Disabled", function() end)
	c.disabled.enabled = false
	c.locked = new("ButtonControl"):ButtonControl({ "LEFT", c.disabled, "RIGHT" }, { 8, 0, 70, 20 }, "Locked", function() end)
	c.locked.locked = function() return st.clicks == 0 end
	c.plus = new("ButtonControl"):ButtonControl({ "TOPLEFT", c.button, "BOTTOMLEFT" }, { 0, 10, 20, 20 }, "+", function() end)
	c.minus = new("ButtonControl"):ButtonControl({ "LEFT", c.plus, "RIGHT" }, { 4, 0, 20, 20 }, "-", function() end)
	c.cross = new("ButtonControl"):ButtonControl({ "LEFT", c.minus, "RIGHT" }, { 4, 0, 20, 20 }, "x", function() end)
	c.check = new("CheckBoxControl"):CheckBoxControl({ "TOPLEFT", c.plus, "BOTTOMLEFT" }, { 120, 12, 20 }, "^7Check me:", function(state)
		st.checked = st.checked + 1
		st.lastCheck = tostring(state)
	end, "CheckBox tooltip")
	c.checkOn = new("CheckBoxControl"):CheckBoxControl({ "LEFT", c.check, "RIGHT" }, { 140, 0, 18 }, "On:", nil, nil, true)
	c.checkRight = new("CheckBoxControl"):CheckBoxControl({ "TOPLEFT", c.check, "BOTTOMLEFT" }, { 0, 8, 16 }, "right label", nil, nil, true)
	c.checkRight.labelRight = true
	c.checkOff = new("CheckBoxControl"):CheckBoxControl({ "LEFT", c.checkRight, "RIGHT" }, { 160, 0, 20 }, "Disabled:", nil, nil, true)
	c.checkOff.enabled = false
	c.outline = new("RectangleOutlineControl"):RectangleOutlineControl(nil, { 450, 40, 120, 60 }, { 0.2, 0.8, 1 }, 2)
	c.outline2 = new("RectangleOutlineControl"):RectangleOutlineControl(nil, { 450.5, 120.25, 80, 30 })
	return {
		host = host,
		dump = function()
			return { clicks = st.clicks, checked = st.checked, lastCheck = st.lastCheck, check = c.check.state == true,
				sel = host.selControl == c.button and "button" or host.selControl == c.check and "check" or host.selControl and "other" or "nil" }
		end,
	}
end

-- ScrollBar (vertical / horizontal / disabled / autoHide) and Slider (plain with tooltipFunc / divCount).
function G.scroll()
	local host = new("ControlHost"):ControlHost()
	local c = host.controls
	local st = { changes = 0 }
	c.v = new("ScrollBarControl"):ScrollBarControl(nil, { 40, 40, 16, 200 }, 20)
	c.v:SetContentDimension(1000, 200)
	c.h = new("ScrollBarControl"):ScrollBarControl(nil, { 80, 40, 300, 16 }, 30, "HORIZONTAL")
	c.h:SetContentDimension(450, 300)
	c.off = new("ScrollBarControl"):ScrollBarControl(nil, { 80, 70, 200, 12 }, nil, "HORIZONTAL")
	c.off:SetContentDimension(100, 200)
	c.auto = new("ScrollBarControl"):ScrollBarControl(nil, { 400, 40, 16, 100 }, 10, nil, true)
	c.slider = new("SliderControl"):SliderControl(nil, { 80, 100, 200, 18 }, function(val) st.changes = st.changes + 1 end)
	c.slider.tooltipFunc = function(tooltip, val)
		tooltip:Clear()
		tooltip:AddLine(14, string.format("^7Value: %.3f", val))
	end
	c.div = new("SliderControl"):SliderControl(nil, { 80, 130, 200, 20 })
	c.div.divCount = 4
	c.div:SetVal(0.5)
	local f6 = function(v) return string.format("%.6f", v) end
	return {
		host = host,
		dump = function()
			return { v = c.v.offset, h = c.h.offset, slider = f6(c.slider.val), div = f6(c.div.val), changes = st.changes,
				dragging = c.v.dragging == true, sliderDragging = c.slider.dragging == true,
				sel = host.selControl == c.v and "v" or host.selControl == c.h and "h" or host.selControl == c.slider and "slider" or host.selControl and "other" or "nil" }
		end,
	}
end

-- EditControl: single line (limit, changeFunc, enterFunc), prompt + placeholder, numeric (+/- buttons), multi-line with
-- scroll bars, protected, clearable, disabled.
function G.edit()
	local host = new("ControlHost"):ControlHost()
	local c = host.controls
	local st = { changes = 0, last = "", enter = "" }
	c.plain = new("EditControl"):EditControl(nil, { 40, 40, 200, 20 }, "Hello world", nil, nil, 50, function(buf)
		st.changes = st.changes + 1
		st.last = buf
	end)
	c.plain.enterFunc = function(buf) st.enter = buf end
	c.prompt = new("EditControl"):EditControl(nil, { 40, 70, 260, 20 }, "", "Search")
	c.prompt:SetPlaceholder("type here")
	c.num = new("EditControl"):EditControl(nil, { 40, 100, 100, 20 }, "5", nil, "%D", 4)
	c.multi = new("EditControl"):EditControl(nil, { 40, 130, 300, 60 }, "line one\nline two is longer than the box is wide\n\tthird\nfour", nil, "^%w%p%s", nil, nil, 16)
	c.protect = new("EditControl"):EditControl(nil, { 40, 240, 150, 20 }, "secret")
	c.protect:SetProtected()
	c.clear = new("EditControl"):EditControl(nil, { 200, 240, 150, 20 }, "clear me", nil, nil, nil, nil, nil, nil, true)
	c.disabled = new("EditControl"):EditControl(nil, { 360, 240, 120, 20 }, "disabled", "P")
	c.disabled.enabled = false
	local function e(ctl) return { buf = ctl.buf, caret = ctl.caret, sel = ctl.sel or "nil", focus = ctl.hasFocus == true } end
	return {
		host = host,
		dump = function()
			local sel = "nil"
			for k, v in pairs(c) do if host.selControl == v then sel = k end end
			return { plain = e(c.plain), prompt = e(c.prompt), num = e(c.num), multi = e(c.multi), clear = e(c.clear),
				multiScroll = { c.multi.controls.scrollBarH.offset, c.multi.controls.scrollBarV.offset },
				changes = st.changes, last = st.last, enter = st.enter, clip = Paste() or "nil", sel = sel }
		end,
	}
end

-- DropDownControl (+SearchHost): 30 strings with scroll bar, table entries with detail/searchFilter, drop-up near the
-- bottom, auto dropped width, disabled.
function G.drop()
	local host = new("ControlHost"):ControlHost()
	local c = host.controls
	local st = { sels = 0, lastSel = "none" }
	local list = {}
	for i = 1, 30 do list[i] = "Item " .. i end
	list[5] = "^xFF8080Coloured five"
	c.dd = new("DropDownControl"):DropDownControl(nil, { 40, 40, 200, 20 }, list, function(index, value)
		st.sels = st.sels + 1
		st.lastSel = index .. ":" .. tostring(value)
	end, "Dropdown tooltip")
	c.tbl = new("DropDownControl"):DropDownControl(nil, { 300, 40, 220, 20 }, {
		{ label = "Alpha", detail = "1st" }, { label = "Beta", detail = "2nd" }, { label = "Gamma beta", detail = "3rd", searchFilter = "gamma delta" },
	}, function(index, value)
		st.sels = st.sels + 1
		st.lastSel = index .. ":" .. value.label
	end)
	c.up = new("DropDownControl"):DropDownControl(nil, { 40, 760, 150, 20 }, { "one", "two", "three", "four", "five" })
	c.wide = new("DropDownControl"):DropDownControl(nil, { 600, 40, 100, 20 }, { "short", "a much longer entry than the box" })
	c.wide:CheckDroppedWidth(true)
	c.off = new("DropDownControl"):DropDownControl(nil, { 40, 100, 150, 20 }, { "disabled" })
	c.off.enabled = false
	local function d(ctl)
		return { sel = ctl.selIndex, dropped = ctl.dropped == true, term = ctl.searchTerm, count = ctl:GetDropCount(),
			scroll = ctl.controls.scrollBar.offset, hover = ctl.hoverSel or "nil", up = ctl.dropUp == true, dw = ctl.droppedWidth }
	end
	return {
		host = host,
		dump = function()
			local sel = "nil"
			for k, v in pairs(c) do if host.selControl == v then sel = k end end
			return { dd = d(c.dd), tbl = d(c.tbl), up = d(c.up), wide = d(c.wide), sels = st.sels, lastSel = st.lastSel, sel = sel }
		end,
	}
end

-- ListControl (columns + labels, tooltip, clipping, scroll, mutable reorder, delete hook), a string list with row
-- separators, an empty list with defaultText, and a TextListControl with columns/underline/sectionHeights.
function G.list()
	local host = new("ControlHost"):ControlHost()
	local c = host.controls
	local st = { selects = 0, clicks = 0, order = 0, deleted = 0, textClicks = 0 }
	local items = {}
	for i = 1, 25 do items[i] = { name = "Entry number " .. i, n = i } end
	items[3].name = "A rather long entry name that will be clipped by the column width"
	c.list = new("ListControl"):ListControl(nil, { 40, 40, 260, 200 }, 18, "VERTICAL", true, items)
	c.list.GetRowValue = function(self, column, index, value) return column == 1 and value.name or ("#" .. value.n) end
	c.list.colList = { { width = 180, label = "Name", sortable = true }, { label = "N" } }
	c.list.colLabels = true
	c.list.label = "^7List label"
	c.list.AddValueTooltip = function(self, tooltip, index, value)
		tooltip:Clear()
		tooltip:AddLine(14, "^7Tooltip for " .. value.name)
	end
	c.list.OnSelect = function(self, index, value) st.selects = st.selects + 1 end
	c.list.OnSelClick = function(self, index, value, dbl) st.clicks = st.clicks + 1 end
	c.list.OnOrderChange = function(self, a, b) st.order = st.order + 1 end
	c.list.OnSelDelete = function(self, index, value)
		table.remove(self.list, index)
		self.selIndex = nil
		self.selValue = nil
		st.deleted = st.deleted + 1
	end
	c.sep = new("ListControl"):ListControl(nil, { 340, 40, 200, 120 }, 20, nil, false, { "one", "two", "three" })
	c.sep.GetRowValue = function(self, column, index, value) return value end
	c.sep.showRowSeparators = true
	c.empty = new("ListControl"):ListControl(nil, { 340, 180, 200, 60 }, 16, true, false, {})
	c.empty.GetRowValue = function(self, column, index, value) return value end
	c.empty.defaultText = "^8No entries"
	local lines = {}
	for i = 1, 12 do
		lines[i] = { height = 16, "^7Line " .. i, "^x80FF80" .. i * 10, underline = (i == 2) and { true, true } or nil }
	end
	lines[4].font = "FIXED"
	lines[5].align = "CENTER_X"
	lines[5].x = 100
	c.text = new("TextListControl"):TextListControl(nil, { 580, 40, 220, 150 }, { { x = 0, align = "LEFT" }, { x = 196, align = "RIGHT_X" } }, lines, { 0, 64 })
	c.text.onClick = function(line) st.textClicks = st.textClicks + 1 end
	return {
		host = host,
		dump = function()
			local sel = "nil"
			for k, v in pairs(c) do if host.selControl == v then sel = k end end
			local order = {}
			for i, v in ipairs(c.list.list) do order[i] = v.n end
			return { sel = sel, selIndex = c.list.selIndex or "nil", hasSelValue = c.list.selValue ~= nil, order = table.concat(order, ","),
				hover = c.list.hoverIndex or "nil", scroll = c.list.controls.scrollBarV.offset, drag = c.list.selDragActive == true,
				sepSel = c.sep.selIndex or "nil", textScroll = c.text.controls.scrollBar.offset,
				textHover = c.text.hoveredLine and c.text.hoveredLine.y or "nil",
				selects = st.selects, clicks = st.clicks, orderChanges = st.order, deleted = st.deleted, textClicks = st.textClicks }
		end,
	}
end

-- U2b: PopupDialog (as main:OpenPopup shows it: layer 10, dimmed screen; input through popup:ProcessInput) with numbered
-- labels, an edit (defaultControl), enter/escape buttons and a disabled button.
function G.popup()
	main.screenW, main.screenH = 1280, 800
	local st = { saved = 0, cancelled = 0, changes = 0 }
	local controls = { }
	controls[1] = new("LabelControl"):LabelControl(nil, { 0, 20, 0, 16 }, "^7First line of the message")
	controls[2] = new("LabelControl"):LabelControl(nil, { 0, 36, 0, 16 }, "^8second ^1line")
	controls.edit = new("EditControl"):EditControl(nil, { 0, 60, 350, 20 }, "start", nil, "\\/:%*%?\"<>|%c", 100, function(buf)
		st.changes = st.changes + 1
		controls.save.enabled = buf:match("%S") ~= nil
	end)
	controls.save = new("ButtonControl"):ButtonControl(nil, { -45, 90, 80, 20 }, "Save", function() st.saved = st.saved + 1 end)
	controls.cancel = new("ButtonControl"):ButtonControl(nil, { 45, 90, 80, 20 }, "Cancel", function() st.cancelled = st.cancelled + 1 end)
	controls.off = new("ButtonControl"):ButtonControl({ "TOPLEFT", nil, "TOPLEFT" }, { 10, 90, 50, 20 }, "Off", function() end)
	controls.off.enabled = false
	local popup = new("PopupDialog"):PopupDialog(370, 120, "Popup ^1Title", controls, "save", "edit", "cancel")
	local host = {
		ProcessControlsInput = function(_, inputEvents, viewPort) popup:ProcessInput(inputEvents, viewPort) end,
		DrawControls = function(_, viewPort)
			SetDrawLayer(10)
			SetDrawColor(0, 0, 0, 0.5)
			DrawImage(nil, 0, 0, main.screenW, main.screenH)
			popup:Draw(viewPort)
			SetDrawLayer(0)
		end,
	}
	return {
		host = host,
		dump = function()
			local sel = "nil"
			for k, v in pairs(controls) do if popup.selControl == v then sel = tostring(k) end end
			local x, y = popup:GetPos()
			return { saved = st.saved, cancelled = st.cancelled, changes = st.changes, buf = controls.edit.buf, sel = sel, x = x, y = y,
				saveEnabled = controls.save.enabled == true }
		end,
	}
end

-- U2b: DraggerControl (labels + / - / x / // / text, callbacks) and ResizableEditControl (drag and right-click reset).
function G.dragger()
	local host = new("ControlHost"):ControlHost()
	local c = host.controls
	local st = { downX = -1, downY = -1, upDX = 0, upDY = 0, right = 0 }
	c.drag = new("DraggerControl"):DraggerControl(nil, { 40, 40, 20, 20 }, "+", function(pos)
		st.downX, st.downY = pos.X, pos.Y
	end, function(delta)
		st.upDX, st.upDY = delta.X, delta.Y
	end, function(pos)
		st.right = st.right + 1
	end)
	c.drag.tooltipText = "Dragger tooltip"
	c.minus = new("DraggerControl"):DraggerControl({ "LEFT", c.drag, "RIGHT" }, { 4, 0, 20, 20 }, "-")
	c.cross = new("DraggerControl"):DraggerControl({ "LEFT", c.minus, "RIGHT" }, { 4, 0, 20, 20 }, "x")
	c.slash = new("DraggerControl"):DraggerControl({ "LEFT", c.cross, "RIGHT" }, { 4, 0, 20, 20 }, "//")
	c.text = new("DraggerControl"):DraggerControl({ "LEFT", c.slash, "RIGHT" }, { 4, 0, 60, 20 }, "Drag")
	c.locked = new("DraggerControl"):DraggerControl({ "LEFT", c.text, "RIGHT" }, { 4, 0, 60, 20 }, "Lock")
	c.locked.locked = true
	c.off = new("DraggerControl"):DraggerControl({ "LEFT", c.locked, "RIGHT" }, { 4, 0, 60, 20 }, "Off")
	c.off.enabled = false
	c.redit = new("ResizableEditControl"):ResizableEditControl(nil, { 100, 100, 200, 40, 100, 30, 400, 200 }, "line one\nline two", nil, nil, nil, nil, 16)
	return {
		host = host,
		dump = function()
			local sel = "nil"
			for k, v in pairs(c) do if host.selControl == v then sel = k end end
			return { downX = st.downX, downY = st.downY, upDX = st.upDX, upDY = st.upDY, right = st.right,
				dragging = c.drag.dragging == true, clicked = c.drag.clicked == true, sel = sel,
				w = c.redit.width, h = c.redit.height, rdrag = c.redit.controls.draggerHeight.dragging == true }
		end,
	}
end
return G
