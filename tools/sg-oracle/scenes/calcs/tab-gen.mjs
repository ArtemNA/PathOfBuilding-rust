// The Calcs tab scenes (U4): node tab-gen.mjs writes <scene>.json and index.json next to it. Public goldens only.
import { readFileSync, writeFileSync } from "node:fs";
const dir = new URL(".", import.meta.url);
const click = { hold: ["LEFTBUTTON"], events: [["KeyDown", "LEFTBUTTON"]] };
const up = (more = {}) => ({ release: ["LEFTBUTTON"], events: [["KeyUp", "LEFTBUTTON"]], dump: true, ...more });
const clickAt = (pos, more = {}) => [{ ...pos, ...click }, up(more)];
const away = { cursor: [-100, -100] };
const wheel = (key, n) => Array.from({ length: n }, () => ["KeyUp", key]);
const open = (b) => [{ open: b }, { waitAssets: true }, { ...away, time: 10000 }];
const toCalcs = (p) => clickAt({ cursor: [260, 96] }, { shot: p + "-open" });
const pick = (control, i, more = {}) => [...clickAt({ cursorControl: ["SkillSelect", control] }), ...clickAt({ cursorRel: [0, 14 + 12 * (i - 1)] }, more)];
const allScene = (p, build, extra = []) => [
	...open(build),
	{ treeOrder: "order-" + p },
	...toCalcs(p),
	...extra,
	{ ...away, dump: true },
	// scroll, the page's own bar
	{ cursor: [800, 400], events: wheel("WHEELDOWN", 4), dump: true, shot: p + "-scrolled" },
	{ cursor: [800, 400], events: wheel("WHEELUP", 4), dump: true },
	// the hover popup of a cell, its pin, the popup's wheel, a click outside
	{ cursorCell: ["Attributes", "Strength", 1], dump: true, shot: p + "-hover" },
	...clickAt({ cursorCell: ["Attributes", "Strength", 1] }, { shot: p + "-pinned" }),
	{ ...away, dump: true },
	...clickAt({ cursor: [600, 780] }),
	{ ...away, dump: true },
	// a popup with a mod table
	{ cursorCell: ["Life", "Inc. from Tree", 1], dump: true, shot: p + "-life" },
	...clickAt({ cursorCell: ["Attributes", "Strength", 1] }),
	{ cursorBd: ["tab", "Tree", "source"], dump: true, shot: p + "-source-tip" },
	{ cursorBd: ["tab", "Tree", "sourceName"], dump: true, shot: p + "-source-node" },
	...clickAt({ cursor: [600, 780] }),
	{ ...away, dump: true },
	// collapse / expand
	...clickAt({ cursorControl: ["Attributes", "toggle1"] }, { shot: p + "-collapsed" }),
	{ ...away, dump: true },
	...clickAt({ cursorControl: ["Attributes", "toggle1"] }),
	// the calculation mode
	...clickAt({ cursorControl: ["SkillSelect", "mode"] }, { shot: p + "-mode-open" }),
	...clickAt({ cursorRel: [0, 18] }, { shot: p + "-unbuffed" }),
	{ ...away, dump: true },
	...clickAt({ cursorControl: ["SkillSelect", "mode"] }),
	...clickAt({ cursorRel: [0, 50] }),
	// another socket group
	...clickAt({ cursorControl: ["SkillSelect", "mainSocketGroup"] }),
	...clickAt({ cursorRel: [0, 36] }, { shot: p + "-group2" }),
	{ ...away, dump: true },
	// the side bar: hover, pin, unpin
	{ cursorStat: "Life:", dump: true, shot: p + "-side-hover" },
	...clickAt({ cursorStat: "Life:" }, { shot: p + "-side-pinned" }),
	{ ...away, dump: true },
	...clickAt({ cursor: [600, 780] }),
	{ ...away, dump: true },
	// the pop-out pane
	...clickAt({ cursorControl: ["Attributes", "popOut"] }, { shot: p + "-overlay" }),
	{ ...away, dump: true },
];
const scenes = {
	attack: allScene("attack", "Generals Perforate Zerker"),
	savior: allScene("savior", "Dual Savior"),
	coc: allScene("coc", "Dual Wield Cospris CoC", [{ buffOrder: "buffs-coc" }]),
	dot: allScene("dot", "Mirage Archer Toxic Rain"),
	spell: allScene("spell", "OccVortex"),
	radius: [
		...open("OccVortex"),
		{ treeOrder: "order-radius" },
		...toCalcs("radius"),
		{ cursorCell: ["SkillTypeStats", "Radius", 1], dump: true, shot: "radius-hover" },
		...clickAt({ cursorCell: ["SkillTypeStats", "Radius", 1] }, { shot: "radius-pinned" }),
		...clickAt({ cursor: [600, 780] }),
		{ ...away, dump: true },
	],
	minion: [
		...open("Spectre Test"),
		...toCalcs("minion"),
		...pick("mainSocketGroup", 1, { shot: "minion-group" }),
		{ ...away, dump: true },
		...pick("mainSkillMinionSkill", 2),
		{ ...away, dump: true },
		...pick("mainSkillMinion", 2, { shot: "minion-second" }),
		{ ...away, dump: true },
		...clickAt({ cursorControl: ["SkillSelect", "showMinion"] }, { shot: "minion-stats" }),
		{ ...away, dump: true },
		{ cursorCell: ["Life", "Inc. from Tree", 1], dump: true, shot: "minion-hover" },
		{ ...away, dump: true },
		...clickAt({ cursorControl: ["SkillSelect", "showMinion"] }),
		{ ...away, dump: true },
	],
	misc: [
		...open("Spectre Test"),
		{ treeOrder: "order-misc" },
		...toCalcs("misc"),
		// mines
		...pick("mainSocketGroup", 2),
		{ ...away, dump: true },
		...clickAt({ cursorControl: ["SkillSelect", "mainSkillMineCount"] }),
		{ events: [["Char", "3"]], dump: true, shot: "misc-mines" },
		...clickAt({ cursor: [600, 780] }),
		{ ...away, dump: true },
		// stages
		...pick("mainSocketGroup", 3),
		{ ...away, dump: true },
		...clickAt({ cursorControl: ["SkillSelect", "mainSkillStageCount"] }),
		{ events: [["Char", "\b"]], dump: true },
		{ events: [["Char", "2"]], dump: true, shot: "misc-stages" },
		...clickAt({ cursor: [600, 780] }),
		{ ...away, dump: true },
		// undo / redo
		...pick("mode", 2),
		{ ...away, dump: true },
		{ hold: ["CTRL"], events: [["KeyDown", "z"], ["KeyUp", "z"]], dump: true },
		{ events: [["KeyDown", "y"], ["KeyUp", "y"]], dump: true, shot: "misc-redo" },
		{ release: ["CTRL"], dump: true },
		// search
		{ hold: ["CTRL"], events: [["KeyDown", "f"], ["KeyUp", "f"]], dump: true },
		{ release: ["CTRL"], events: [["Char", "l"], ["Char", "i"], ["Char", "f"], ["Char", "e"]], dump: true, shot: "misc-search" },
		...clickAt({ cursor: [600, 780] }),
		{ ...away, dump: true },
		// the pane: drag, a cell, a toggle, close
		...clickAt({ cursorControl: ["Attributes", "popOut"] }),
		{ cursorOverlay: ["Attributes", 10, 10], hold: ["LEFTBUTTON"], events: [["KeyDown", "LEFTBUTTON"]] },
		{ cursorRel: [60, 40], dump: true },
		{ release: ["LEFTBUTTON"], events: [["KeyUp", "LEFTBUTTON"]], dump: true, shot: "misc-dragged" },
		{ cursorOverlay: ["Attributes", 140, 51], dump: true, shot: "misc-overlay-hover" },
		...clickAt({ cursorOverlay: ["Attributes", 140, 51] }, { shot: "misc-overlay-pinned" }),
		{ ...away, dump: true },
		...clickAt({ cursor: [600, 780] }),
		{ ...away, dump: true },
		...clickAt({ cursorOverlay: ["Attributes", 217, 33] }, { shot: "misc-overlay-collapsed" }),
		...clickAt({ cursorOverlay: ["Attributes", 217, 33] }),
		...clickAt({ cursorOverlay: ["Attributes", 217, 8] }, { shot: "misc-overlay-closed" }),
		{ ...away, dump: true },
	],
	edit: [
		...open("Generals Perforate Zerker"),
		...toCalcs("edit"),
		{ perfAlloc: 6712 },
		{ ...away, dump: true },
		{ perfAlloc: 9194 },
		{ ...away, dump: true },
		{ cursorCell: ["Life", "Inc. from Tree", 1], dump: true, shot: "edit-life" },
		{ ...away, dump: true },
	],
};
// personal builds of builds/local-builds.json (key "calcs"): local-<n>.json, ignored by git
let local = [];
try { local = JSON.parse(readFileSync(new URL("../../../../builds/local-builds.json", dir), "utf8")).calcs ?? []; } catch {}
local.forEach((b, i) => writeFileSync(new URL(`local-${i + 1}.json`, dir), JSON.stringify({ steps: allScene(`local-${i + 1}`, b) }) + "\n"));
// not in the index: the numbers of U4 (test calcs_perf)
const perf = [
	...open("Generals Perforate Zerker"),
	{ ...clickAt({ cursor: [260, 96] })[0] },
	{ release: ["LEFTBUTTON"], events: [["KeyUp", "LEFTBUTTON"]], perfClick: "tab open" },
	{ ...away },
	{ perf: 60 },
	{ cursorCell: ["Attributes", "Strength", 1], perfClick: "hover" },
	{ perf: 60 },
	{ cursor: [-100, -100] },
	{ perfRecalc: 5 },
];
writeFileSync(new URL("perf.json", dir), JSON.stringify({ steps: perf }) + "\n");
for (const [k, steps] of Object.entries(scenes)) writeFileSync(new URL(k + ".json", dir), JSON.stringify({ steps }, null, 0) + "\n");
writeFileSync(new URL("index.json", dir), JSON.stringify(Object.keys(scenes)) + "\n");
