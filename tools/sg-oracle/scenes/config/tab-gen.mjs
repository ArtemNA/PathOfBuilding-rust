// The Config and Notes tab scenes (U7): node tab-gen.mjs writes <scene>.json and index.json next to it. Public goldens only.
import { writeFileSync } from "node:fs";
const dir = new URL(".", import.meta.url);
const click = { hold: ["LEFTBUTTON"], events: [["KeyDown", "LEFTBUTTON"]] };
const up = (more = {}) => ({ release: ["LEFTBUTTON"], events: [["KeyUp", "LEFTBUTTON"]], dump: true, ...more });
const clickAt = (pos, more = {}) => [{ ...pos, ...click }, up(more)];
const away = { cursor: [-100, -100] };
const rest = (more = {}) => ({ ...away, dump: true, ...more });
const wheel = (key, n) => Array.from({ length: n }, () => ["KeyUp", key]);
const type = (s) => [...s].map((c) => ["Char", c]);
const keys = (k, n = 1) => Array.from({ length: n }, () => ["KeyDown", k]).flatMap((e) => [e, ["KeyUp", k]]);
const ctrl = (k, more = {}) => [{ hold: ["CTRL"], events: [["KeyDown", k], ["KeyUp", k]], dump: true, ...more }, { release: ["CTRL"] }];
const open = (b) => [{ open: b }, { waitAssets: true }, { ...away, time: 10000 }];
const toConfig = (p) => clickAt({ cursorBuild: "modeConfig" }, { shot: p + "-open" });
const toNotes = (p) => clickAt({ cursorBuild: "modeNotes" }, { shot: p + "-open" });
// open a dropdown, pick a row, let the recalculation frame run
const sv = (v) => ({ scrollVar: v });
const pick = (v, row, more = {}) => [sv(v), ...clickAt({ cursorVar: v }), ...clickAt({ cursorDrop: [v, row] }, more), rest()];
const flip = (v, more = {}) => [sv(v), ...clickAt({ cursorVar: v }, more), rest()];
const edit = (v, s, more = {}) => [sv(v), ...clickAt({ cursorVar: v }), { events: type(s), dump: true }, rest(more)];

const options = (p, b) => [
	...open(b),
	...toConfig(p),
	rest(),
	// scroll
	{ cursor: [800, 400], events: wheel("WHEELDOWN", 4), dump: true, shot: p + "-scrolled" },
	{ cursor: [800, 400], events: wheel("WHEELUP", 4), dump: true },
	rest(),
	// check: the tooltip with its comparison, the click, the recalculation frame
	sv("buffOnslaught"),
	{ cursorVar: "buffOnslaught", dump: true, shot: p + "-check-tip" },
	...flip("buffOnslaught", { shot: p + "-check-on" }),
	// ifOption: the override appears
	...flip("usePowerCharges", { shot: p + "-ifoption" }),
	sv("overridePowerCharges"),
	{ cursorVar: "overridePowerCharges", dump: true, shot: p + "-override-tip" },
	...edit("overridePowerCharges", "2"),
	...flip("usePowerCharges"),
	// count, integer, countAllowZero (placeholder), numeric buttons by keys
	...edit("multiplierMapModEffect", "25", { shot: p + "-count" }),
	...edit("enemyPhysicalReduction", "-5", { shot: p + "-integer" }),
	...edit("enemyFireResist", "40"),
	{ cursorVar: "enemyFireResist", dump: true, shot: p + "-placeholder-tip" },
	...clickAt({ cursorVar: "enemyFireResist" }),
	{ events: keys("BACK", 2), dump: true },
	rest({ shot: p + "-placeholder" }),
	...clickAt({ cursorVar: "enemyFireResist" }),
	{ events: keys("UP"), dump: true },
	rest(),
	{ events: keys("ESCAPE"), dump: true },
	// list: the open list, the tooltip of a row with its comparison, a pick
	sv("resistancePenalty"),
	...clickAt({ cursorVar: "resistancePenalty" }, { shot: p + "-list-open" }),
	{ cursorDrop: ["resistancePenalty", 1], dump: true, shot: p + "-list-hover" },
	...clickAt({ cursorDrop: ["resistancePenalty", 1] }),
	rest({ shot: p + "-list-picked" }),
	// the tooltip functions
	sv("bandit"),
	{ cursorVar: "bandit", dump: true, shot: p + "-bandit-tip" },
	sv("pantheonMajorGod"),
	...clickAt({ cursorVar: "pantheonMajorGod" }),
	{ cursorDrop: ["pantheonMajorGod", 2], dump: true, shot: p + "-pantheon-tip" },
	{ events: keys("ESCAPE"), dump: true },
	rest(),
	sv("MapPrefix1"),
	...clickAt({ cursorVar: "MapPrefix1" }),
	{ cursorDrop: ["MapPrefix1", 2], dump: true, shot: p + "-map-tip" },
	{ cursorDrop: ["MapPrefix1", 5], dump: true },
	{ events: keys("ESCAPE"), dump: true },
	rest(),
	sv("presetBossSkills"),
	...clickAt({ cursorVar: "presetBossSkills" }),
	{ cursorDrop: ["presetBossSkills", 2], dump: true, shot: p + "-boss-tip" },
	{ events: keys("ESCAPE"), dump: true },
	rest(),
	// placeholders over BuildModList, the disabled enemyDamageType, back
	...pick("enemyIsBoss", 3, { shot: p + "-pinnacle" }),
	...pick("presetBossSkills", 2, { shot: p + "-preset" }),
	...pick("presetBossSkills", 1),
	...pick("enemyIsBoss", 1),
	// undo / redo
	...ctrl("z"),
	...ctrl("z"),
	...ctrl("y", { shot: p + "-redo" }),
	rest(),
];

const search = (p, b) => [
	...open(b),
	...toConfig(p),
	...clickAt({ cursorTab: "search" }),
	{ events: type("life"), dump: true, shot: p + "-life" },
	{ events: keys("BACK", 4), dump: true },
	{ events: type("(fire|cold)"), dump: true, shot: p + "-or" },
	{ events: keys("BACK", 11), dump: true },
	{ events: type("a.b+c["), dump: true },
	{ events: keys("BACK", 6), dump: true },
	rest(),
	{ ...away, hold: ["CTRL"], events: [["KeyDown", "f"], ["KeyUp", "f"]], dump: true },
	{ release: ["CTRL"], events: type("x"), dump: true },
	{ events: keys("BACK"), dump: true },
	rest(),
	// show all
	...clickAt({ cursorTab: "toggleConfigs" }, { shot: p + "-showall" }),
	rest(),
	...clickAt({ cursorTab: "toggleConfigs" }),
	rest(),
	// collapse, a search over a collapsed section
	{ scrollTop: true },
	...clickAt({ cursorSection: "General" }, { shot: p + "-collapsed" }),
	rest(),
	{ scrollTop: true },
	...clickAt({ cursorTab: "search" }),
	{ events: type("bandit"), dump: true, shot: p + "-collapsed-search" },
	{ events: keys("BACK", 6), dump: true },
	rest(),
	...clickAt({ cursorSection: "General" }),
	rest(),
];

const custom = (p, b) => [
	...open(b),
	...toConfig(p),
	{ scrollBlock: 1 },
	...clickAt({ cursorTab: "customModsAddBlock" }, { shot: p + "-added" }),
	rest(),
	{ scrollBlock: 1 },
	...clickAt({ cursorBlock: [1, "text"] }),
	{ events: type("+100 to maximum Life"), dump: true },
	rest({ shot: p + "-typed" }),
	...clickAt({ cursorBlock: [1, "text"] }),
	{ events: [["Char", "\r"], ...type("not a mod")], dump: true },
	rest({ shot: p + "-unsupported" }),
	{ cursorBlock: [1, "check"], dump: true, shot: p + "-check-tip" },
	...clickAt({ cursorBlock: [1, "check"] }),
	rest({ shot: p + "-disabled" }),
	...clickAt({ cursorBlock: [1, "check"] }),
	rest(),
	{ scrollBlock: 2 },
	...clickAt({ cursorBlock: [2, "title"] }),
	{ events: type("x"), dump: true },
	rest(),
	...ctrl("z"),
	rest(),
	...ctrl("y"),
	rest(),
	{ scrollBlock: 2 },
	...clickAt({ cursorBlock: [2, "delete"] }, { shot: p + "-deleted" }),
	rest(),
	{ scrollBlock: 1 },
	...clickAt({ cursorBlock: [1, "delete"] }),
	rest({ shot: p + "-default" }),
];

const browser = (p, b) => [
	...open(b),
	...toConfig(p),
	{ scrollBlock: 1 },
	...clickAt({ cursorBlock: [1, "add"] }, { shot: p + "-popup", rowsOut: p + "-rows" }),
	{ cursorRow: ["listControl", 3], dump: true, shot: p + "-hover" },
	...clickAt({ cursorRow: ["listControl", 3] }),
	...clickAt({ cursorPopup: "search" }),
	{ events: type("maximum life"), dump: true, shot: p + "-search", rowsOut: p + "-rows-search" },
	{ events: keys("BACK", 12), dump: true },
	{ events: type("zzzqqq"), dump: true, shot: p + "-none" },
	{ events: keys("BACK", 6), dump: true },
	{ events: type("fire"), dump: true },
	...clickAt({ cursorRow: ["listControl", 2] }),
	...clickAt({ cursorPopup: "save" }),
	rest({ shot: p + "-added" }),
	...clickAt({ cursorBlock: [1, "add"] }),
	...clickAt({ cursorPopup: "close" }),
	rest(),
];

const sets = (p, b, file) => [
	...open(b),
	...toConfig(p),
	...flip("buffOnslaught"),
	{ scrollTop: true },
	...clickAt({ cursorTab: "setManage" }, { shot: p + "-manage" }),
	...clickAt({ cursorList: ["1", "new"] }, { shot: p + "-new" }),
	{ events: type("Second"), dump: true },
	...clickAt({ cursorPopup: "save" }, { shot: p + "-saved" }),
	...clickAt({ cursorRow: ["1", 1] }),
	...clickAt({ cursorList: ["1", "copy"] }),
	{ events: type(" copy"), dump: true },
	...clickAt({ cursorPopup: "save" }),
	...clickAt({ cursorList: ["1", "rename"] }),
	{ events: keys("BACK", 2), dump: true },
	...clickAt({ cursorPopup: "cancel" }),
	// double click: the set becomes active
	...clickAt({ cursorRow: ["1", 2] }),
	{ ...click, events: [["KeyDown", "LEFTBUTTON", true]] },
	up({ shot: p + "-switched" }),
	...clickAt({ cursorPopup: "2" }),
	rest({ shot: p + "-second" }),
	...flip("buffArcaneSurge"),
	...ctrl("z"),
	...ctrl("y"),
	rest(),
	// the dropdown
	{ scrollTop: true },
	...clickAt({ cursorTab: "setSelect" }, { shot: p + "-select-open" }),
	...clickAt({ cursorRel: [0, 20] }),
	rest({ shot: p + "-first" }),
	// delete the active one
	{ scrollTop: true },
	...clickAt({ cursorTab: "setManage" }),
	...clickAt({ cursorRow: ["1", 1] }),
	...clickAt({ cursorList: ["1", "delete"] }, { shot: p + "-confirm" }),
	...clickAt({ cursorPopup: "confirm" }),
	...clickAt({ cursorPopup: "2" }),
	rest({ shot: p + "-after-delete" }),
	// the loadout list follows; the saved file
	{ hold: ["CTRL"], events: [["KeyDown", "s"], ["KeyUp", "s"]], dump: true, file: file, fileOut: p + "-saved.xml" },
	{ release: ["CTRL"], dump: true },
];

const notes = (p, b, file) => [
	...open(b),
	...toNotes(p),
	rest(),
	{ events: type("first line"), dump: true, time: 11000, shot: p + "-typed" },
	{ events: [["Char", "\r"]], dump: true },
	...clickAt({ cursorNotes: "fire" }, { shot: p + "-fire" }),
	...clickAt({ cursorNotes: "edit" }),
	{ events: type(" red"), dump: true },
	...ctrl("a"),
	...clickAt({ cursorNotes: "magic" }, { shot: p + "-selection" }),
	...clickAt({ cursorNotes: "toggleColorCodes" }, { shot: p + "-codes" }),
	...clickAt({ cursorNotes: "unique" }),
	...clickAt({ cursorNotes: "default" }),
	...clickAt({ cursorNotes: "toggleColorCodes" }),
	...clickAt({ cursorNotes: "edit" }),
	...ctrl("z"),
	...ctrl("y"),
	...ctrl("-", { shot: p + "-zoom" }),
	...ctrl("0"),
	rest({ shot: p + "-rest" }),
	{ hold: ["CTRL"], events: [["KeyDown", "s"], ["KeyUp", "s"]], dump: true, file: file, fileOut: p + "-saved.xml" },
	{ release: ["CTRL"], dump: true },
	// back to the tree and again: the state stays
	...clickAt({ cursorBuild: "modeTree" }),
	...toNotes(p + "-again"),
];

const loadout = (p, b) => [
	...open(b),
	...toConfig(p),
	rest(),
	// a loadout of another tree and config set; the sets list follows
	...clickAt({ cursorBuild: "buildLoadouts" }, { shot: p + "-list" }),
	...clickAt({ cursorRel: [0, 36] }, { shot: p + "-alt" }),
	rest(),
	{ ...away, dump: true, shot: p + "-alt-rest" },
	...clickAt({ cursorBuild: "buildLoadouts" }),
	...clickAt({ cursorRel: [0, 20] }),
	rest({ shot: p + "-back" }),
	...clickAt({ cursorTab: "setSelect" }),
	...clickAt({ cursorRel: [0, 36] }, { shot: p + "-set-alt" }),
	rest(),
];

const perf = (b) => [
	...open(b),
	{ ...clickAt({ cursorBuild: "modeConfig" })[0] },
	{ release: ["LEFTBUTTON"], events: [["KeyUp", "LEFTBUTTON"]], perfClick: "tab open" },
	{ ...away },
	{ perf: 60 },
	{ cursorVar: "buffOnslaught", hold: ["LEFTBUTTON"], events: [["KeyDown", "LEFTBUTTON"]] },
	{ release: ["LEFTBUTTON"], events: [["KeyUp", "LEFTBUTTON"]] },
	{ ...away, perfClick: "option + recalc" },
	{ ...away },
	{ perf: 60 },
	{ cursorBuild: "modeNotes", hold: ["LEFTBUTTON"], events: [["KeyDown", "LEFTBUTTON"]] },
	{ release: ["LEFTBUTTON"], events: [["KeyUp", "LEFTBUTTON"]], perfClick: "notes open" },
	{ ...away },
	{ perf: 60 },
];

const scenes = {
	options: options("options", "OccVortex"),
	"options-savior": options("savior", "Dual Savior"),
	"options-minion": options("minion", "Spectre Test"),
	search: search("search", "OccVortex"),
	custom: custom("custom", "OccVortex"),
	browser: browser("browser", "OccVortex"),
	sets: sets("sets", "Generals Perforate Zerker", "Generals Perforate Zerker.xml"),
	notes: notes("notes", "Mirage Archer Toxic Rain", "Mirage Archer Toxic Rain.xml"),
	loadout: loadout("loadout", "Loadout Test"),
};
for (const [name, steps] of Object.entries(scenes)) {
	writeFileSync(new URL(name + ".json", dir), JSON.stringify({ steps }) + "\n");
}
writeFileSync(new URL("perf.json", dir), JSON.stringify({ steps: perf("Generals Perforate Zerker") }) + "\n");
writeFileSync(new URL("index.json", dir), JSON.stringify(Object.keys(scenes)) + "\n");
