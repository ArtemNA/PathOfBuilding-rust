#@ SimpleGraphic
-- U1.3 probe (gated by tests/text.rs): DrawStringWidth of strings at heights 28..32 (VAR) -> sg/out/text-probe.json, then Exit.
ConExecute("set vid_mode 8") ConExecute("set vid_resizable 3") ConExecute("set vid_last 1280,800,40,40,0")
local json = require("dkjson")
local strs = { "h a", "hh a", "h3 a", "30 a", "h0 a", "h30a", "h30  a", "hhh", "hhhh a", "h30 a", "0 a", "30: Hag", "ha", "h a b c d", "hhhhhhhh|", "mmmm|", "a h a h a|" }
local main = {}
SetMainObject(main)
function main:OnInit() RenderInit("DPI_AWARE") end
function main:OnFrame()
	local out = {}
	for _, h in ipairs({ 28, 29, 30, 31, 32, 70 }) do
		for _, s in ipairs(strs) do out[#out + 1] = { h, s, DrawStringWidth(h, "VAR", s) } end
	end
	local f = io.open(GetScriptPath() .. "/out/text-probe.json", "wb") f:write(json.encode(out), "\n") f:close()
	Exit()
end
