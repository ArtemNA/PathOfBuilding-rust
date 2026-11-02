#@ SimpleGraphic
-- U1.2 cold-start probe: same window setup as scene-runner.lua, exits on the first OnFrame after drawing one string.
-- run.ps1 prints the process wall time (launch -> exit) = cold start to the first frame + exit.
ConExecute("set vid_mode 8")
ConExecute("set vid_resizable 3")
ConExecute("set vid_last 1280,800,40,40,0")
local main = {}
SetMainObject(main)
function main:OnInit()
	RenderInit("DPI_AWARE")
end
function main:OnFrame()
	SetDrawColor(1, 1, 1)
	DrawString(10, 10, "LEFT", 16, "VAR", "first frame")
	Exit()
end
