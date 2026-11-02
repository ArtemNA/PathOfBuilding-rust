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
click(595, 437);
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
add({ ...off, ...d() });
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
click(914, 786);
add({ waitPower: true });
add({ ...off, time: 40000, ...d({ shot: 'tab-heat' }) });
add({ perfPower: 'Life' });
add({ ...off, time: 70000, ...d({ shot: 'tab-heat-life' }) });
add({ perfPower: 'Life' });
add({ ...off, time: 90000, ...d() });
click(764, 786);
add({ ...off, ...d({ shot: 'tab-report', mask: [318, 650, 998, 798] }) });
click(600, 652);
add({ ...off, ...d({ shot: 'tab-report-jump', mask: [318, 650, 998, 798] }) });
write('tab-power.json', steps);
