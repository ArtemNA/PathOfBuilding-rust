// U3b: generates scenes/tree/tab.json (the public golden only; every coordinate is a scenario: the gate compares what it did on both sides).
//   node tools/sg-oracle/scenes/tree/tab-gen.mjs
import fs from 'fs';
const steps = [];
const add = (s) => steps.push(s);
const off = { cursor: [-100, -100] };
const d = (o = {}) => ({ dump: true, tab: true, ...o });
const click = (x, y, extra = {}) => {
  add({ cursor: [x, y] });
  add({ hold: ['LEFTBUTTON'], events: [['KeyDown', 'LEFTBUTTON']] });
  add({ release: ['LEFTBUTTON'], events: [['KeyUp', 'LEFTBUTTON']], ...d(extra) });
};
const rclick = (node, extra = {}) => {
  add({ cursorNode: node, dump: true });
  add({ events: [['KeyDown', 'RIGHTBUTTON']] });
  add({ events: [['KeyUp', 'RIGHTBUTTON']], ...d(extra) });
};
const lclickNode = (node, extra = {}) => {
  add({ cursorNode: node });
  add({ hold: ['LEFTBUTTON'], events: [['KeyDown', 'LEFTBUTTON']] });
  add({ release: ['LEFTBUTTON'], events: [['KeyUp', 'LEFTBUTTON']], ...d(extra) });
};
const key = (k, extra = {}) => add({ events: [['KeyDown', k], ['KeyUp', k]], ...d(extra) });
const ctrl = (k, extra = {}) => {
  add({ hold: ['CTRL'], events: [['KeyDown', k], ['KeyUp', k]] });
  add({ release: ['CTRL'], ...d(extra) });
};
const type = (text) => add({ events: [...text].map((c) => ['Char', c]), ...d() });
const shot = (name) => add({ ...off, ...d({ shot: name }) });

add({ open: 'Generals Perforate Zerker' });
add({ waitAssets: true });
add({ ...off, time: 10000 });
add({ treeOrder: 'order-tab13' });
shot('tab-old');
// the convert line: Copy + Convert to the latest
click(971, 730);
add({ waitAssets: true });
add({ treeOrder: 'order-tab29' });
add({ ...d({ shot: 'tab-converted-msg' }) });
key('RETURN');
shot('tab-converted');
add({ cursor: [796, 388], events: Array(5).fill(['KeyUp', 'WHEELDOWN']) });
shot('tab-zoomed-out');
add({ perfSpec: 1 });
add({ perfSpec: 2 });
add({ perfSpec: 1 });
add({ perfSpec: 2 });
// the spec list
ctrl('m', { shot: 'tab-manage' });
click(600, 315);
click(670, 292, { shot: 'tab-copy-name' }); // Copy
type('Copy A');
key('RETURN', { shot: 'tab-manage-3' });
click(608, 292); // Rename (the new row is selected)
type('X');
key('RETURN');
click(540, 524); // Import Tree
type('Imp');
click(800, 395);
type('https://www.pathofexile.com/passive-skill-tree/3.29.0/AAAA');
click(595, 437, { shot: 'tab-import-error' });
click(800, 395);
ctrl('a');
type('https://www.pathofexile.com/passive-skill-tree/3.29.0/AAAABgEAAAAA');
click(595, 437);
click(640, 524, { shot: 'tab-export' }); // Export Tree
click(760, 430); // Done of the export popup
click(737, 524); // Done
shot('tab-after-manage');
key('UP');
key('UP');
key('DOWN');
key('UP');
shot('tab-spec-2');
// compare
click(590, 762, { shot: 'tab-compare-13' });
click(700, 762, { shot: 'tab-compare-drop' });
click(700, 725);
add({ cursorNode: 292, ...d({ shot: 'tab-compare-hover' }) });
shot('tab-compare-other');
click(590, 762);
// mastery
lclickNode(292, { shot: 'tab-mastery' });
add({ cursor: [640, 395], ...d({ shot: 'tab-mastery-hover' }) });
add({ hold: ['LEFTBUTTON'], events: [['KeyDown', 'LEFTBUTTON']] });
add({ release: ['LEFTBUTTON'], events: [['KeyUp', 'LEFTBUTTON']], ...d() });
shot('tab-mastery-saved');
rclick(292, { shot: 'tab-mastery-right' });
key('ESCAPE');
ctrl('z');
ctrl('y');
ctrl('z', { shot: 'tab-undone' });
click(680, 762, { shot: 'tab-reset' });
key('ESCAPE');
click(680, 762);
click(510, 425); // Reset Tree
shot('tab-tree-reset');
ctrl('z');
// the class, through the nodes and the drop-downs
lclickNode(982, { shot: 'tab-ascend-node' });
lclickNode(1945, { shot: 'tab-class-confirm' });
key('ESCAPE');
lclickNode(1945);
key('RETURN');
shot('tab-class-node');
ctrl('z');
// version drop-down
click(900, 762, { shot: 'tab-version-drop' });
click(900, 600, { shot: 'tab-version-popup' });
click(765, 445);
click(900, 762);
click(900, 600);
key('RETURN');
add({ waitAssets: true });
add({ dump: true, tab: true });
const write = (name, st) => fs.writeFileSync(new URL(name, import.meta.url), JSON.stringify({ steps: st }).replace(/\},\{/g, '},\n{') + '\n');
write('tab.json', steps);

// tab-tattoo: a new build of the latest tree, in a process of its own (run.ps1 -ScriptArgs tab-tattoo): `tree.tattoo.nodes` is one table per process and
// PassiveTree.lua:772-773 points its sprites at the tree built last -- a scene that also opened an older tree draws a new tattoo without art.
steps.length = 0;
add({ openNew: true });
add({ waitAssets: true });
add({ ...off, time: 10000 });
add({ treeOrder: 'order-tabt' });
add({ cursor: [796, 388], events: Array(3).fill(['KeyUp', 'WHEELDOWN']) });
rclick(31961, { shot: 'tab-tattoo' });
key('RETURN');
shot('tab-tattooed');
rclick(476, { shot: 'tab-tattoo-attr' });
key('RETURN');
shot('tab-tattooed-2');
ctrl('z');
ctrl('y');
add({ ...off, ...d({ shot: 'tab-tattoo-redo' }) });
click(680, 762);
click(640, 425); // Remove All Tattoos
shot('tab-tattoos-removed');
lclickNode(982, { shot: 'tab-ascend-other' });
lclickNode(1945, { shot: 'tab-class-node2' });
write('tab-tattoo.json', steps);

// tab-power: node power on the golden as loaded: `node.path` over equal shortest paths follows the history of the allocNodes table (pairs order), so a spec
// edited in the session and the same spec loaded differ in pathPower -- upstream's own difference.
steps.length = 0;
add({ open: 'Generals Perforate Zerker' });
add({ waitAssets: true });
add({ ...off, time: 10000 });
add({ treeOrder: 'order-tabp' });
add({ cursor: [914, 786] });
add({ hold: ['LEFTBUTTON'], events: [['KeyDown', 'LEFTBUTTON']] });
add({ release: ['LEFTBUTTON'], events: [['KeyUp', 'LEFTBUTTON']], ...d({ shot: 'tab-heat-start' }) });
add({ waitPower: true });
add({ ...off, time: 40000, ...d({ shot: 'tab-heat' }) });
add({ perfPower: 'Life' });
add({ ...off, time: 70000, ...d({ shot: 'tab-heat-life' }) });
add({ perfPower: 'Life' });
add({ ...off, time: 90000, ...d() });
click(764, 786);
add({ ...off, ...d({ shot: 'tab-report' }) });
click(600, 652);
add({ ...off, ...d({ shot: 'tab-report-jump' }) });
write('tab-power.json', steps);

// tab-power-edit (U3c): node power after edits of the session -- the converted golden, a mastery, a tattoo and allocations: `node.path` over equal shortest
// paths follows the history of the live `allocNodes` table.
steps.length = 0;
add({ open: 'Generals Perforate Zerker' });
add({ waitAssets: true });
add({ ...off, time: 10000 });
add({ treePaths: 'paths-e0' });
click(971, 730);
add({ waitAssets: true });
add({ treeOrder: 'order-tabe' });
add({ treePaths: 'paths-e1' });
key('RETURN');
add({ cursor: [796, 388], events: Array(5).fill(['KeyUp', 'WHEELDOWN']) });
lclickNode(292);
add({ cursor: [640, 395] });
add({ hold: ['LEFTBUTTON'], events: [['KeyDown', 'LEFTBUTTON']] });
add({ release: ['LEFTBUTTON'], events: [['KeyUp', 'LEFTBUTTON']], ...d() });
add({ treePaths: 'paths-e2' });
lclickNode(31961);
add({ treePaths: 'paths-e3' });
rclick(476);
key('RETURN');
add({ treePaths: 'paths-e4' });
lclickNode(476);
shot('tab-edit-before');
add({ treePaths: 'paths-edit' });
click(914, 786);
add({ waitPower: true });
add({ ...off, time: 40000, ...d({ shot: 'tab-edit-heat' }) });
add({ perfPower: 'Life' });
add({ ...off, time: 70000, ...d({ shot: 'tab-edit-heat-life' }) });
// the same spec after leaving it and coming back: upstream keeps the spec object
key('UP');
add({ waitPower: true });
add({ ...off, ...d() });
key('DOWN');
add({ waitPower: true });
add({ ...off, ...d() });
add({ treePaths: 'paths-e5' });
add({ perfPower: 'Life' });
add({ ...off, time: 90000, ...d() });
// undo: `ImportFromNodeList` on the same object
const undoWait = () => {
  add({ hold: ['CTRL'], events: [['KeyDown', 'z'], ['KeyUp', 'z']] });
  add({ release: ['CTRL'] });
  add({ waitPower: true });
};
undoWait();
add({ ...off, ...d() });
add({ treePaths: 'paths-e6' });
undoWait();
add({ ...off, ...d() });
add({ treePaths: 'paths-e7' });
add({ perfPower: 'Life' });
add({ ...off, time: 110000, ...d() });
write('tab-power-edit.json', steps);

// tab-timeless (U3c): FindTimelessJewel on the converted golden. No network: the runner's LaunchSubScript returns nil. Popup 1036x588 at (122,106).
steps.length = 0;
add({ open: 'Generals Perforate Zerker' });
add({ waitAssets: true });
add({ ...off, time: 10000 });
click(971, 730);
add({ waitAssets: true });
add({ treeOrder: 'order-tabl' });
key('RETURN');
add({ ...off, ...d() });
const hover = (x, y, name) => add({ cursor: [x, y], ...d(name ? { shot: name } : {}) });
const JEWEL = 139, SOCKET = 185, NODE = 323;
const row = (top, n) => top + 25 + (n - 1) * 13;
click(690, 786, { shot: 'tl-open' });
click(742, JEWEL, { shot: 'tl-jewel-drop' });
click(742, row(JEWEL - 8, 2)); // Lethal Pride
add({ ...off, ...d({ shot: 'tl-karui' }) });
click(742, SOCKET);
hover(742, row(SOCKET - 8, 3), 'tl-socket-hover');
click(742, row(SOCKET - 8, 3));
click(742, NODE);
hover(742, row(NODE - 8, 3), 'tl-node-hover');
click(742, row(NODE - 8, 3));
click(742, NODE);
click(742, row(NODE - 8, 6));
add({ ...off, ...d({ shot: 'tl-nodes' }) });
hover(720, 254, 'tl-slider-hover');
click(720, 254);
add({ ...off, ...d() });
click(620, 300);
add({ ...off, ...d({ shot: 'tl-sliders' }) });
click(550, 672, { perfClick: 'searchKarui' });
add({ ...off, ...d({ shot: 'tl-results' }) });
click(800, 470);
add({ hold: ['SHIFT'] });
click(800, 510);
add({ release: ['SHIFT'] });
add({ ...off, ...d({ shot: 'tl-highlight' }) });
add({ cursor: [800, 470] });
add({ hold: ['LEFTBUTTON'], events: [['KeyDown', 'LEFTBUTTON', true]] });
add({ release: ['LEFTBUTTON'], events: [['KeyUp', 'LEFTBUTTON']], ...d() });
add({ ...off, ...d({ shot: 'tl-added' }) });
click(650, 208, { shot: 'tl-filter' });
add({ ...off, ...d() });
click(640, 672, { shot: 'tl-reset' });
add({ ...off, ...d() });
click(742, JEWEL);
click(742, row(JEWEL - 8, 4)); // Militant Faith
add({ ...off, ...d({ shot: 'tl-templar' }) });
click(742, NODE);
click(742, row(NODE - 8, 2)); // Total Devotion
click(550, 672, { perfClick: 'searchTemplar' });
add({ ...off, ...d({ shot: 'tl-results-templar' }) });
click(730, 672); // Cancel
add({ ...off, ...d() });
click(690, 786, { shot: 'tl-reopen' });
click(742, SOCKET);
click(742, row(SOCKET - 8, 1)); // All Sockets
click(650, 208);
add({ ...off, ...d() });
click(550, 672, { perfClick: 'searchAll' });
add({ ...off, ...d({ shot: 'tl-results-all' }) });
click(742, JEWEL);
click(742, row(JEWEL - 8, 1)); // Glorious Vanity
click(742, SOCKET);
click(742, row(SOCKET - 8, 5));
click(742, NODE);
click(742, row(NODE - 8, 4));
add({ ...off, ...d({ shot: 'tl-vaal' }) });
click(550, 672, { perfClick: 'searchVaal' });
add({ ...off, ...d({ shot: 'tl-results-vaal' }) });
click(550, 672, { perfClick: 'searchVaal2' });
add({ ...off, ...d() });
write('tab-timeless.json', steps);

// tab-abyss (U3d): the Abyss jewels (types 7-11, DataAbyssJewelLookUpTableHelper.lua) on the converted golden.
steps.length = 0;
add({ open: 'Generals Perforate Zerker' });
add({ waitAssets: true });
add({ ...off, time: 10000 });
click(971, 730);
add({ waitAssets: true });
add({ treeOrder: 'order-taba' });
key('RETURN');
add({ ...off, ...d() });
click(690, 786, { shot: 'ab-open' });
const pickJewel = (n) => {
  click(742, JEWEL);
  click(742, row(JEWEL - 8, n));
};
const pickNode = (n) => {
  click(742, NODE);
  click(742, row(NODE - 8, n));
};
const pickSocket = (n) => {
  click(742, SOCKET);
  click(742, row(SOCKET - 8, n));
};
pickJewel(7); // Festering Vengeance
add({ ...off, ...d({ shot: 'ab-tecrod' }) });
pickSocket(5);
click(384, 550); // a node with two rolls, by hand: the first line of the desired list
type('abyss_murderous_notable_18, 1, 2, 0');
pickNode(2);
pickNode(3);
add({ ...off, ...d({ shot: 'ab-tecrod-nodes' }) });
click(550, 672, { perfClick: 'searchAbyssFirst' });
add({ ...off, ...d({ shot: 'ab-results' }) });
click(550, 672, { perfClick: 'searchAbyss' });
add({ ...off, ...d() });
click(800, 470);
add({ cursor: [800, 470] });
add({ hold: ['LEFTBUTTON'], events: [['KeyDown', 'LEFTBUTTON', true]] });
add({ release: ['LEFTBUTTON'], events: [['KeyUp', 'LEFTBUTTON']], ...d() });
add({ ...off, ...d({ shot: 'ab-added' }) });
click(650, 208); // Filter Nodes
click(550, 672);
add({ ...off, ...d({ shot: 'ab-filter' }) });
click(650, 208);
pickJewel(8); // Extinguishing Grasp
click(384, 550);
type('abyss_searching_notable_27, 2, 1, 1');
pickNode(2);
pickNode(10);
click(550, 672);
add({ ...off, ...d({ shot: 'ab-ulaman' }) });
pickJewel(9); // Baleful Dominion
pickNode(2);
pickNode(10);
click(550, 672);
add({ ...off, ...d() });
pickJewel(10); // Destructive Aspiration
pickNode(2);
pickNode(10);
click(550, 672);
add({ ...off, ...d() });
pickJewel(11); // Reclaimed Malevolence
add({ ...off, ...d({ shot: 'ab-zorath' }) });
pickSocket(2); // an allocated socket: the path to the class start
pickNode(2);
pickNode(10);
click(550, 672, { perfClick: 'searchZorath' });
add({ ...off, ...d({ shot: 'ab-zorath-results' }) });
click(230, 155); // the required ascendancy outcome
click(230, row(147, 3));
add({ ...off, ...d({ shot: 'ab-zorath-outcome' }) });
click(550, 672);
add({ ...off, ...d() });
click(650, 208); // the protected nodes appear
add({ ...off, ...d({ shot: 'ab-zorath-protect' }) });
click(146, 220); // the first of them
click(550, 672);
add({ ...off, ...d() });
click(650, 208);
pickJewel(7);
pickSocket(1); // All Sockets
pickNode(2);
pickNode(3);
click(550, 672, { perfClick: 'searchAbyssAll' });
add({ ...off, ...d({ shot: 'ab-all' }) });
write('tab-abyss.json', steps);

// tab-generate (U3d): the fallback weights, TreeTab.lua:1951-2117: fixed (types 2-6), variable with several stats (1), Abyss additions, the second Generate of the session.
steps.length = 0;
add({ open: 'Generals Perforate Zerker' });
add({ waitAssets: true });
add({ ...off, time: 10000 });
click(971, 730);
add({ waitAssets: true });
add({ treeOrder: 'order-tabg' });
key('RETURN');
add({ ...off, ...d() });
click(690, 786);
const generate = (extra = {}) => click(880, 347, extra);
const FALLBACK = [300, 434];
pickSocket(5);
pickJewel(2); // Lethal Pride
generate({ perfClick: 'generateKarui' });
add({ ...off, ...d({ shot: 'gen-karui' }) });
click(...FALLBACK);
add({ ...off, ...d({ shot: 'gen-karui-list' }) });
click(550, 672);
add({ ...off, ...d({ shot: 'gen-karui-results' }) });
click(200, 434); // Desired Nodes
pickJewel(4); // Militant Faith: Total Devotion
generate();
add({ ...off, ...d() });
pickJewel(1); // Glorious Vanity
generate({ perfClick: 'generateVaal' });
click(...FALLBACK);
add({ ...off, ...d({ shot: 'gen-vaal' }) });
click(550, 672);
add({ ...off, ...d() });
generate();
add({ ...off, ...d() });
click(200, 434);
pickJewel(7); // Festering Vengeance
generate({ perfClick: 'generateAbyss' });
click(...FALLBACK);
add({ ...off, ...d({ shot: 'gen-abyss' }) });
generate(); // `modListGenerated` of the additions: no divisor the second time
add({ ...off, ...d({ shot: 'gen-abyss-2' }) });
click(742, 347); // another stat
add({ ...off, ...d({ shot: 'gen-modes' }) });
click(742, row(339, 3));
generate();
add({ ...off, ...d() });
write('tab-generate.json', steps);

// tab-cover (U3c): what U3b wrote without the oracle: ConvertAll's popup, the tooltip of specSelect, compare-only nodes, the class drop-downs, Connect Path, a cluster socket by click, a poeplanner link.
steps.length = 0;
add({ open: 'Generals Perforate Zerker' });
add({ waitAssets: true });
add({ ...off, time: 10000 });
add({ treeOrder: 'order-tabc' });
click(1120, 730, { shot: 'cv-convertall' });
click(698, 445);
add({ ...off, ...d() });
click(971, 730);
add({ waitAssets: true });
add({ treeOrder: 'order-tabc1' });
key('RETURN');
add({ cursor: [796, 388], events: Array(5).fill(['KeyUp', 'WHEELDOWN']) });
add({ cursor: [410, 762], ...d({ shot: 'cv-spec-tip' }) });
click(410, 762);
add({ cursor: [410, 725], ...d({ shot: 'cv-spec-tip-row' }) });
key('ESCAPE');
add({ ...off, ...d() });
click(590, 762);
add({ ...off, ...d({ shot: 'cv-compare' }) });
add({ cursorFind: 'compareOnly', ...d({ shot: 'cv-compare-only' }) });
add({ cursorFind: 'compareAlloc', ...d({ shot: 'cv-compare-alloc' }) });
add({ cursorNode: 292, ...d({ shot: 'cv-compare-node' }) });
click(590, 762);
add({ ...off, ...d() });
// the socket's own tooltip is itemsTab:AddItemTooltip (U6): no dump or shot on these steps; the next hover replaces the tooltip
add({ cursorFind: 'clusterSocket' });
add({ hold: ['LEFTBUTTON'], events: [['KeyDown', 'LEFTBUTTON']] });
add({ release: ['LEFTBUTTON'], events: [['KeyUp', 'LEFTBUTTON']] });
add({ cursorNode: 292, ...d() });
add({ ...off, ...d({ shot: 'cv-cluster-off' }) });
add({ treeOrder: 'order-tabc2' });
add({ treePaths: 'paths-cluster' });
ctrl('z');
add({ ...off, ...d({ shot: 'cv-cluster-undo' }) });
click(687, 14, { shot: 'cv-class-drop' });
click(687, 66, { shot: 'cv-class-confirm' });
key('ESCAPE');
add({ ...off, ...d() });
click(687, 14);
click(687, 66);
click(640, 431, { shot: 'cv-connect-path' });
add({ ...off, ...d({ shot: 'cv-connected' }) });
ctrl('z');
ctrl('m');
click(540, 524); // Import Tree
type('Planner');
click(800, 395);
const planner = (version) => {
  const u16 = (v) => [v & 255, v >> 8];
  const list = (a) => [...u16(a.length), ...a.flatMap(u16)];
  // the converted golden's allocation (connected: upstream's decode does not rebuild the paths, a load does)
  const nodes = [476, 2225, 2491, 3644, 5152, 5233, 5865, 6289, 7082, 8592, 9271, 10532, 10594, 12795, 13714, 14056, 14930, 14993, 15868, 16380, 18302, 18901, 20551, 22627, 22703, 23090, 23984, 24383, 24914, 24970, 25367, 25933, 26712, 28475, 29294, 29353, 30380, 30691, 30733, 31080, 31257, 32251, 32739, 33287, 34009, 34400, 35053, 35288, 36047, 37501, 37647, 38664, 38999, 39023, 43374, 44169, 46519, 46578, 47175, 48287, 48438, 48807, 49080, 49178, 49412, 49415, 50024, 50904, 53118, 53987, 55166, 55190, 55485, 55676, 56355, 56460, 57839, 57992, 58449, 58831, 59494, 59920, 59928, 61050, 61198, 61666, 62319, 62429, 63048, 63282, 63673, 63723, 64024];
  const bytes = [0, 0, 0, 0, 0, 0, ...u16(version), 1, 2, 0, ...list(nodes), ...list([]), ...list([]), ...u16(1), ...u16(292), ...u16(31822)];
  return 'https://poeplanner.com/' + Buffer.from(bytes).toString('base64').replace(/\+/g, '-').replace(/\//g, '_');
};
type(planner(7));
click(595, 437, { shot: 'cv-planner-badver' });
click(800, 395);
ctrl('a');
type('https://poeplanner.com/AAAA');
click(595, 437);
click(800, 395);
ctrl('a');
type(planner(51));
click(595, 437);
add({ ...off, ...d({ shot: 'cv-planner' }) });
click(737, 524); // Done
add({ ...off, ...d({ shot: 'cv-planner-done' }) });
add({ treePaths: 'paths-planner' });
write('tab-cover.json', steps);

// u3c-crash: only for this port (no oracle record): the unported branches are notes, tests/tree.rs unported_branches_do_not_crash.
steps.length = 0;
add({ open: 'Generals Perforate Zerker' });
add({ waitAssets: true });
add({ ...off, time: 10000 });
ctrl('m');
click(640, 524); // Export Tree
click(550, 430); // Shrink with PoEURL
key('RETURN');
click(760, 430); // Done
click(540, 524); // Import Tree
type('x');
click(800, 395);
type('http://poeurl.com/a');
click(595, 437);
key('RETURN');
click(685, 437); // Cancel of the import popup
click(737, 524); // Done of the list
click(690, 786); // Find Timeless Jewel
click(742, 139);
click(742, 234); // Festering Vengeance
click(550, 672); // Search
key('RETURN');
click(880, 347); // Generate
key('RETURN');
click(730, 672); // Cancel
write('u3c-crash.json', steps);
