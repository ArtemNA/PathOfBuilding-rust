#@ SimpleGraphic
-- R-UP measurement: `pairs` order of a string-keyed table in the sandbox's LuaJIT (lua51.dll), for the options popup's
-- `controls` (Main.lua OpenOptionsPopup of the upstream, keys in assignment order) -- ControlHost draws/hit-tests its
-- controls with `pairs`. Two fresh tables per run (same order within a process?) plus jit.version.
--   pwsh tools/sg-oracle/run.ps1 -Script pairs-probe.lua
-- Writes sg/out/pairs-probe.json (not a gate fixture: the order differs between processes with the v2.5.5 lua51.dll).
local json = require("dkjson")
local base = GetScriptPath()
local UPSTREAM = dofile(base .. "/upstream.lua")
local f = assert(io.open(UPSTREAM.root .. "src/Modules/Main.lua", "rb"))
local src = f:read("*a")
f:close()
local body = src:match("function main:OpenOptionsPopup%(savedState%)(.-)\nfunction main:")
local keys, seen = { }, { }
for k in body:gmatch("controls%.([%w_]+)%s*=") do
	if not seen[k] then seen[k] = true keys[#keys + 1] = k end
end
local function order()
	local t = { }
	for _, k in ipairs(keys) do t[k] = true end
	local out = { }
	for k in pairs(t) do out[#out + 1] = k end
	return out
end
MakeDir(base .. "/out")
local o = assert(io.open(base .. "/out/pairs-probe.json", "wb"))
o:write(json.encode({ jit = jit and jit.version, inserted = keys, pairs1 = order(), pairs2 = order() }, { indent = true }))
o:close()
local main = { }
SetMainObject(main)
function main:OnInit() RenderInit("DPI_AWARE") end
function main:OnFrame() Exit() end
