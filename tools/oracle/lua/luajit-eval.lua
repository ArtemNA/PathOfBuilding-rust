-- Origin: pob-ts tools/oracle/lua/luajit-eval.lua at pob-ts commit 70911c1b629cfe6887795cb40356c6d298c1922e, copied once into
-- pob-rust (R-INDEPENDENT-UPDATE A1) and owned here since (tools/oracle/luajit-eval.ts mounts this directory as /bench).
-- P3.6: run an arbitrary Lua chunk inside desktop LuaJIT (the official PoB docker image), after
-- the same HeadlessWrapper boot the wasm oracle does -- the docker twin of boot.lua's
-- engine_evalJSON (tools/oracle/luajit-eval.ts). No build is loaded: the chunk sees the booted
-- globals (modLib, round, floor, data, ...) and whatever it builds itself.
--
--   luajit /bench/luajit-eval.lua <code.lua>
--
-- Prints one line of JSON (`{ result = <return value> }`, dkjson) as the LAST stdout line;
-- HeadlessWrapper's own boot messages precede it. A runtime error in the chunk is re-raised,
-- so the process exits non-zero and the TS side sees the message on stderr.
local codePath = ...
assert(codePath, "usage: luajit luajit-eval.lua <path-to-code.lua>")

dofile("HeadlessWrapper.lua")
if __mainObject__ and __mainObject__.promptMsg then
	error("FATAL boot failed: " .. tostring(__mainObject__.promptMsg))
end

local dkjson = require("dkjson")

local f = assert(io.open(codePath, "r"), "could not open " .. tostring(codePath))
local code = f:read("*a")
f:close()

local fn = assert(loadstring(code, "eval"))
local ok, res = pcall(fn)
if not ok then
	error(res)
end
print(dkjson.encode({ result = res }))
