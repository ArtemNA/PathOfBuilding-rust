//! `Classes/PassiveTreeView.lua` (upstream 3dbca4463): the passive tree viewer -- input, zoom/pan, hover/path, allocation, the artwork.
//!
//! A branch not ported is marked where it is (`UNIMPLEMENTED`, pushed once into [`PassiveTreeView::unimplemented`]).

use std::collections::HashMap;
use std::rc::Rc;

use pob_engine::engine::Engine;
use pob_gfx::{Frame, ImageHandle, LoadFlag};
use pob_tree::lj_table::LjTable;
use pob_tree::{NodeType, PassiveSpec, SpecNode};

use crate::colors::color_code;
use crate::control::{Control, Rect};
use crate::host::{InputEvent, InputEvents, ipairs};
use crate::modules::main::main;
use crate::sg;
use crate::tooltip::Tooltip;
use crate::tree_art::{AssetRef, Connector, TreeArt, overlay, overlay_size};

thread_local! {
    /// `main.tree[version]` (Main.lua:352): one per process, across the opened builds.
    static ARTS: std::cell::RefCell<HashMap<String, Rc<TreeArt>>> = std::cell::RefCell::new(HashMap::new());
}

const JEWEL_RADIUS_TINT_NEUTRAL: [f64; 4] = [1.0, 1.0, 1.0, 0.7];
const JEWEL_RADIUS_TINT_PRIMARY_ONLY: [f64; 4] = [1.0, 0.0, 0.0, 0.7];
const JEWEL_RADIUS_TINT_COMPARE_ONLY: [f64; 4] = [0.0, 1.0, 0.0, 0.7];

/// `compareJewelsEqual` (PassiveTreeView.lua:161-166).
pub(crate) fn jewels_equal(a: Option<&Rc<pob_items::item::Item>>, b: Option<&Rc<pob_items::item::Item>>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x.build_raw() == y.build_raw(),
        (None, None) => true,
        _ => false,
    }
}

/// `jewel` of `GetSocketAndJewelForNodeID` (what the view reads off it).
fn socket_jewel(e: &Engine, spec: &PassiveSpec, node_id: u32) -> Option<Rc<pob_items::item::Item>> {
    let id = spec.jewels.get(f64::from(node_id)).copied()?;
    e.result().env.item(id).cloned()
}

impl PassiveTreeView {
    /// `GetCompareJewel(nodeId)` (:118-128): on the tab of one build `compareSpec.build` is this build, its sockets show the active spec.
    pub(crate) fn compare_jewel(&self, e: &Engine, spec: &PassiveSpec, node_id: u32) -> Option<Rc<pob_items::item::Item>> {
        let compare = self.compare_spec.as_deref()?;
        match self.compare_owner.as_ref().and_then(std::rc::Weak::upgrade) {
            Some(b) => {
                let eng = b.engine.borrow();
                socket_jewel(eng.as_ref()?, compare, node_id)
            }
            None => socket_jewel(e, spec, node_id),
        }
    }
}

thread_local! {
    /// `data.jewelRadius` (Data.lua:583-589): one table for all the builds of the process, the key of the last `data.setJewelRadiiGlobally`.
    static JEWEL_RADIUS: std::cell::Cell<&'static str> = const { std::cell::Cell::new("3_16") };
}

/// The key of `data.jewelRadius` at this moment.
pub fn jewel_radius_table() -> &'static str {
    JEWEL_RADIUS.with(std::cell::Cell::get)
}
/// `data.setJewelRadiiGlobally(treeVersion)` (Data.lua:583-589): `Build:Init` (Build.lua:606), `main:LoadTree` (a new spec, Main.lua:354-357), `TreeTab:SetActiveSpec` (TreeTab.lua:552), also of an entry of Compare.
pub fn set_jewel_radii_globally(tree_version: &str) {
    JEWEL_RADIUS.with(|c| c.set(radius_key(tree_version)));
}

/// `GetJewelSocketOverlay` (:131-159).
fn jewel_socket_overlay(jewel: &pob_items::item::Item, is_expansion: bool) -> Option<&'static str> {
    let base_name = jewel.base_name.as_deref().unwrap_or("");
    let sub_type = jewel.base.and_then(|b| b.sub_type.as_deref());
    Some(match base_name {
        "Crimson Jewel" if is_expansion => "JewelSocketActiveRedAlt",
        "Crimson Jewel" => "JewelSocketActiveRed",
        "Viridian Jewel" if is_expansion => "JewelSocketActiveGreenAlt",
        "Viridian Jewel" => "JewelSocketActiveGreen",
        "Cobalt Jewel" if is_expansion => "JewelSocketActiveBlueAlt",
        "Cobalt Jewel" => "JewelSocketActiveBlue",
        "Prismatic Jewel" if is_expansion => "JewelSocketActivePrismaticAlt",
        "Prismatic Jewel" => "JewelSocketActivePrismatic",
        _ if sub_type == Some("Abyss") => {
            if is_expansion {
                "JewelSocketActiveAbyssAlt"
            } else {
                "JewelSocketActiveAbyss"
            }
        }
        "Ursine Charm" if sub_type == Some("Charm") => "CharmSocketActiveStr",
        "Corvine Charm" if sub_type == Some("Charm") => "CharmSocketActiveInt",
        "Lupine Charm" if sub_type == Some("Charm") => "CharmSocketActiveDex",
        _ if sub_type == Some("Charm") => return None,
        "Timeless Jewel" if is_expansion => "JewelSocketActiveLegionAlt",
        "Timeless Jewel" => "JewelSocketActiveLegion",
        "Large Cluster Jewel" => "JewelSocketActiveAltPurple",
        "Medium Cluster Jewel" => "JewelSocketActiveAltBlue",
        "Small Cluster Jewel" => "JewelSocketActiveAltRed",
        _ => return None,
    })
}

/// `isAbyssConquered(node)` (:23-26).
fn is_abyss_conquered(node: &SpecNode) -> bool {
    node.conquered_by.as_ref().is_some_and(|c| c.conqueror_type.starts_with("abyss_"))
}

/// The view's derived tables of one spec object (rebuilt when the engine has a new spec): hash orders and connectors of its subgraphs.
struct SpecCache {
    spec: Rc<PassiveSpec>,
    /// `pairs(spec.nodes)`: indices into `spec.nodes`.
    node_order: Vec<usize>,
    /// `tree.connectors` with their node indices (`spec.nodes[connector.nodeId*]`).
    tree_connectors: Vec<(usize, usize, usize)>,
    /// `pairs(spec.subGraphs)`: group position, orbit, connectors with node indices.
    sub_graphs: Vec<SubGraphArt>,
    /// `pairs(tree.sockets)`: indices into `spec.nodes`.
    sockets: Vec<usize>,
}

struct SubGraphArt {
    x: f64,
    y: f64,
    node_orbit: f64,
    connectors: Vec<(Connector, usize, usize)>,
}

pub struct PassiveTreeView {
    ring: ImageHandle,
    highlight_ring: ImageHandle,
    jewel_shaded_outer_ring: ImageHandle,
    jewel_shaded_outer_ring_flipped: ImageHandle,
    jewel_shaded_inner_ring: ImageHandle,
    jewel_shaded_inner_ring_flipped: ImageHandle,
    legion_rings: Vec<(&'static str, ImageHandle, ImageHandle)>,
    pub tooltip: Tooltip,
    pub skill_tooltip: Tooltip,
    pub zoom_level: f64,
    pub zoom: f64,
    pub zoom_x: f64,
    pub zoom_y: f64,
    pub search_str: String,
    search_str_cached: String,
    search_params: Vec<String>,
    search_str_results: HashMap<usize, bool>,
    pub show_stat_differences: bool,
    pub hover_node: Option<usize>,
    pub drag: Option<(f64, f64)>,
    pub dragging: bool,
    pub trace_mode: bool,
    pub trace_path: Option<Vec<usize>>,
    /// `self.showHeatMap`.
    pub show_heat_map: bool,
    /// `self.compareSpec` (TreeTab.lua:450).
    pub compare_spec: Option<Rc<PassiveSpec>>,
    /// CompareTab.lua:3425-3428 the patched `GetCursorPos`: what is subtracted from the cursor.
    pub cursor_offset: (f64, f64),
    /// The build of a viewer that is not of `main.modes.BUILD` (a compare entry).
    pub owner: Option<std::rc::Weak<crate::modules::build::BuildMode>>,
    /// `compareSpec.build` when it is another build than this viewer's.
    pub compare_owner: Option<std::rc::Weak<crate::modules::build::BuildMode>>,
    socket_order_sample: Option<Vec<u32>>,
    cache: Option<SpecCache>,
    pub unimplemented: Vec<String>,
    /// Test harness only: a sample of the `pairs(tree.nodes)`-dependent orders of one oracle process (see [`Self::set_order_sample`]).
    order_sample: Option<(Vec<u32>, Vec<String>)>,
    /// Test harness only: `pairs(spec.subGraphs)` of the same process ([`Self::set_sub_graph_order_sample`]).
    sub_graph_order_sample: Option<Vec<u32>>,
}

fn clamp_image(root: &str, path: &str) -> ImageHandle {
    let h = ImageHandle::new();
    let _ = h.load(crate::src_cache::resolve(root, path), &[LoadFlag::Clamp]);
    h
}

/// `math.min(a, b)` / `math.max(a, b)` as LuaJIT folds them for numbers (`minsd`/`maxsd`: the second operand on NaN or equal).
fn m_min(a: f64, b: f64) -> f64 {
    if a < b { a } else { b }
}

fn m_max(a: f64, b: f64) -> f64 {
    if a > b { a } else { b }
}

impl PassiveTreeView {
    /// `PassiveTreeView()` (:31-85); `root` = the original's `src/`.
    pub fn new(root: &str) -> Self {
        let legion = |t: &'static str, n: &str| {
            (
                t,
                clamp_image(root, &format!("TreeData/PassiveSkillScreen{n}JewelCircle1.png")),
                clamp_image(root, &format!("TreeData/PassiveSkillScreen{n}JewelCircle2.png")),
            )
        };
        Self {
            ring: clamp_image(root, "Assets/ring.png"),
            highlight_ring: clamp_image(root, "Assets/small_ring.png"),
            jewel_shaded_outer_ring: clamp_image(root, "Assets/ShadedOuterRing.png"),
            jewel_shaded_outer_ring_flipped: clamp_image(root, "Assets/ShadedOuterRingFlipped.png"),
            jewel_shaded_inner_ring: clamp_image(root, "Assets/ShadedInnerRing.png"),
            jewel_shaded_inner_ring_flipped: clamp_image(root, "Assets/ShadedInnerRingFlipped.png"),
            legion_rings: vec![
                legion("Brutal Restraint", "Maraketh"),
                legion("Elegant Hubris", "EternalEmpire"),
                legion("Glorious Vanity", "Vaal"),
                legion("Lethal Pride", "Karui"),
                legion("Militant Faith", "Templar"),
                legion("Heroic Tragedy", "Kalguuran"),
            ],
            tooltip: Tooltip::new(),
            skill_tooltip: Tooltip::new(),
            zoom_level: 3.0,
            zoom: 1.2_f64.powf(3.0),
            zoom_x: 0.0,
            zoom_y: 0.0,
            cursor_offset: (0.0, 0.0),
            owner: None,
            compare_owner: None,
            socket_order_sample: None,
            search_str: String::new(),
            search_str_cached: String::new(),
            search_params: Vec::new(),
            search_str_results: HashMap::new(),
            show_stat_differences: true,
            hover_node: None,
            drag: None,
            dragging: false,
            trace_mode: false,
            trace_path: None,
            show_heat_map: false,
            compare_spec: None,
            cache: None,
            unimplemented: Vec::new(),
            order_sample: None,
            sub_graph_order_sample: None,
        }
    }

    /// `PassiveTreeView:Load` (:87-103) over what `SaveExtras` read.
    /// `viewer.highlightRing`.
    pub fn highlight_ring(&self) -> &ImageHandle {
        &self.highlight_ring
    }

    /// The build of this viewer.
    pub fn owner_build(&self) -> std::rc::Rc<crate::modules::build::BuildMode> {
        self.owner
            .as_ref()
            .and_then(std::rc::Weak::upgrade)
            .unwrap_or_else(|| main().build_mode.clone())
    }

    pub fn load(&mut self, st: &pob_engine::save::TreeViewState) {
        if let Some(z) = st.zoom_level {
            self.zoom_level = z;
            self.zoom = 1.2_f64.powf(z);
        }
        if let (Some(x), Some(y)) = (st.zoom_x, st.zoom_y) {
            self.zoom_x = x;
            self.zoom_y = y;
        }
        self.search_str.clone_from(&st.search_str);
        self.show_stat_differences = st.show_stat_differences;
    }

    /// The hash orders the view walks, for the oracle comparison of `tests/tree.rs` (`{ nodes, groups, connectors, subGraphs }`).
    pub fn debug_orders(&self) -> serde_json::Value {
        let c = self.cache.as_ref().expect("drawn once");
        let art = ARTS.with_borrow(|a| a.get(&c.spec.tree.tree_version).cloned()).expect("art");
        serde_json::json!({
            "nodes": c.node_order.iter().map(|i| c.spec.nodes[*i].id).collect::<Vec<_>>(),
            "groups": art.group_order.iter().filter(|g| c.spec.tree.groups.contains_key(g)).collect::<Vec<_>>(),
            "connectors": art.connectors.iter().map(|k| format!("{}-{}:{}", k.node_id1, k.node_id2, k.ty)).collect::<Vec<_>>(),
            "subGraphs": pob_tree::lj_table::pairs_order(&c.spec.sub_graph_views().iter().map(|v| f64::from(v.id)).collect::<Vec<_>>()),
        })
    }

    /// `pairs(tree.nodes)` differs between processes of the original: the `"root"` string key keeps one hash slot (PassiveTree.lua:510), and
    /// its place follows the per-process string hash (R13; measured: 1938 of 2890 `pairs(spec.nodes)` positions are the same in two runs).
    /// So `tests/tree.rs` replays the oracle process' own sample of `pairs(spec.nodes)` and `tree.connectors` (as `"id1-id2:type"`): the
    /// overlap order of that one screenshot, not a model. The app uses the model (no `root`), one of the orders the original can have.
    pub fn set_order_sample(&mut self, nodes: Vec<u32>, connectors: Vec<String>) {
        self.order_sample = Some((nodes, connectors));
        self.cache = None;
    }

    /// `pairs(spec.subGraphs)` follows the sockets' order of that process too (two orders of 4 subgraphs in 8 runs): which of two overlapping subgraphs is on top.
    pub fn set_sub_graph_order_sample(&mut self, ids: Vec<u32>) {
        self.sub_graph_order_sample = Some(ids);
        self.cache = None;
    }

    /// `pairs(tree.sockets)`: filled in the order of `pairs(self.nodes)` (PassiveTree.lua:571), so of the process too: which of two overlapping radius rings is on top.
    pub fn set_socket_order_sample(&mut self, ids: Vec<u32>) {
        self.socket_order_sample = Some(ids);
        self.cache = None;
    }

    pub fn note_unimplemented(&mut self, what: &str) {
        self.note(what);
    }

    fn note(&mut self, what: &str) {
        if !self.unimplemented.iter().any(|u| u == what) {
            self.unimplemented.push(what.to_string());
        }
    }

    /// The tree's art, read once per version (`main.tree[version]`, Main.lua:352).
    pub fn art(&mut self, root: &str, tree: &pob_tree::PassiveTree) -> Rc<TreeArt> {
        ARTS.with_borrow_mut(|arts| {
            arts.entry(tree.tree_version.clone())
                .or_insert_with(|| Rc::new(TreeArt::load(root, tree).unwrap_or_else(|e| panic!("TreeData: {e}"))))
                .clone()
        })
    }

    fn sync_cache(&mut self, spec: &Rc<PassiveSpec>, art: &TreeArt) {
        if self.cache.as_ref().is_some_and(|c| Rc::ptr_eq(&c.spec, spec)) {
            return;
        }
        // PassiveSpec.lua:53-59: a `{}` filled in `pairs(tree.nodes)` order, then `BuildSubgraph` (:2120, :2342) adds its nodes
        let mut t = LjTable::new();
        for id in &art.node_order {
            if spec.node_idx(*id).is_some_and(|i| !spec.nodes[i].is_subgraph_node()) {
                t.set(f64::from(*id));
            }
        }
        for n in spec.nodes.iter().filter(|n| n.is_subgraph_node()) {
            t.set(f64::from(n.id));
        }
        let mut node_order: Vec<usize> = t.pairs_keys().into_iter().filter_map(|k| spec.node_idx(k as u32)).collect();
        let mut tree_connectors: Vec<(usize, usize, usize)> = art
            .connectors
            .iter()
            .enumerate()
            .filter_map(|(c, cn)| Some((c, spec.node_idx(cn.node_id1)?, spec.node_idx(cn.node_id2)?)))
            .collect();
        if let Some((nodes, connectors)) = &self.order_sample {
            let rank: HashMap<usize, usize> = nodes
                .iter()
                .enumerate()
                .filter_map(|(r, id)| Some((spec.node_idx(*id)?, r)))
                .collect();
            node_order.sort_by_key(|i| rank.get(i).copied().unwrap_or(usize::MAX));
            let mut queues: HashMap<&str, std::collections::VecDeque<usize>> = HashMap::new();
            for (r, k) in connectors.iter().enumerate() {
                queues.entry(k.as_str()).or_default().push_back(r);
            }
            let mut ranked: Vec<(usize, (usize, usize, usize))> = tree_connectors
                .iter()
                .map(|t| {
                    let c = &art.connectors[t.0];
                    let k = format!("{}-{}:{}", c.node_id1, c.node_id2, c.ty);
                    (
                        queues
                            .get_mut(k.as_str())
                            .and_then(std::collections::VecDeque::pop_front)
                            .unwrap_or(usize::MAX),
                        *t,
                    )
                })
                .collect();
            ranked.sort_by_key(|(r, _)| *r);
            tree_connectors = ranked.into_iter().map(|(_, t)| t).collect();
        }
        let views = spec.sub_graph_views();
        let order = match &self.sub_graph_order_sample {
            Some(s) => s.iter().map(|k| f64::from(*k)).collect(),
            None => pob_tree::lj_table::pairs_order(&views.iter().map(|v| f64::from(v.id)).collect::<Vec<_>>()),
        };
        let mut sub_graphs = Vec::new();
        for k in order {
            let Some(v) = views.iter().find(|v| f64::from(v.id) == k) else {
                continue;
            };
            let mut connectors = Vec::new();
            let group_of = |i: usize| {
                let n = &spec.nodes[i];
                if n.is_subgraph_node() {
                    (v.x, v.y)
                } else {
                    n.tree_node()
                        .group
                        .and_then(|g| spec.tree.groups.get(&g))
                        .map_or((0.0, 0.0), |g| (g.x, g.y))
                }
            };
            for (a, b) in &v.links {
                let mut out = Vec::new();
                art.build_connector(
                    spec.nodes[*a].tree_node(),
                    spec.nodes[*b].tree_node(),
                    group_of(*a),
                    group_of(*b),
                    &mut out,
                );
                for c in out {
                    let (Some(i1), Some(i2)) = (spec.node_idx(c.node_id1), spec.node_idx(c.node_id2)) else {
                        continue;
                    };
                    connectors.push((c, i1, i2));
                }
            }
            sub_graphs.push(SubGraphArt {
                x: v.x,
                y: v.y,
                node_orbit: v.node_orbit,
                connectors,
            });
        }
        let sockets = match &self.socket_order_sample {
            Some(ids) => ids.iter().filter_map(|id| spec.node_idx(*id)).collect(),
            None => spec.tree.sockets.keys().filter_map(|id| spec.node_idx(*id)).collect(),
        };
        self.cache = Some(SpecCache {
            spec: spec.clone(),
            node_order,
            tree_connectors,
            sub_graphs,
            sockets,
        });
        // `searchStrResults` is keyed by node: a changed node list is searched again
        self.search_str_cached = String::from("\0");
    }

    /// `PassiveTreeViewClass:Focus(x, y, viewPort, build)` (:1323-1332).
    pub fn focus(&mut self, x: f64, y: f64, view_port: &Rect, tree: &pob_tree::PassiveTree) {
        self.zoom_level = 12.0;
        self.zoom = 1.2_f64.powf(self.zoom_level);
        let size = ARTS.with_borrow(|a| a.get(&tree.tree_version).map(|t| t.size)).unwrap_or(1.0);
        let scale = m_min(view_port.width, view_port.height) / size * self.zoom;
        self.zoom_x = -x * scale;
        self.zoom_y = -y * scale;
    }

    /// `PassiveTreeViewClass:Zoom` (:1307-1320).
    pub fn do_zoom(&mut self, f: &Frame<'_>, level: f64, view_port: &Rect) {
        self.zoom_level = m_max(0.0, m_min(12.0, self.zoom_level + level));
        let old_zoom = self.zoom;
        self.zoom = 1.2_f64.powf(self.zoom_level);
        let factor = self.zoom / old_zoom;
        let (cx, cy) = f.get_cursor_pos();
        let (cx, cy) = (cx - self.cursor_offset.0, cy - self.cursor_offset.1);
        let rel_x = cx - view_port.x - view_port.width / 2.0;
        let rel_y = cy - view_port.y - view_port.height / 2.0;
        self.zoom_x = rel_x + (self.zoom_x - rel_x) * factor;
        self.zoom_y = rel_y + (self.zoom_y - rel_y) * factor;
    }

    /// `PassiveTreeView:Draw` (:197-1259). `root`: the original's `src/`. Sets `build_flag` / `mod_flag` as the Lua does on the build.
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        f: &mut Frame<'_>,
        root: &str,
        engine: &std::cell::RefCell<Option<Engine>>,
        build_flag: &std::cell::Cell<bool>,
        mod_flag: &std::cell::Cell<bool>,
        view_port: &Rect,
        input_events: &mut InputEvents,
    ) {
        let mut spec = engine.borrow().as_ref().expect("engine").spec_rc();
        let compare_imported: Vec<u32> = self.compare_spec.as_ref().map_or_else(Vec::new, |c| c.imported_tattoos.clone());
        let art = self.art(root, &spec.tree);
        self.sync_cache(&spec, &art);
        let cur_class_id = spec.cur_class_id;
        let (cursor_x, cursor_y) = f.get_cursor_pos();
        let (cursor_x, cursor_y) = (cursor_x - self.cursor_offset.0, cursor_y - self.cursor_offset.1);
        let m_over = cursor_x >= view_port.x
            && cursor_x < view_port.x + view_port.width
            && cursor_y >= view_port.y
            && cursor_y < view_port.y + view_port.height;

        // :204-247 input events
        let mut tree_click: Option<&str> = None;
        let events: Vec<InputEvent> = ipairs(input_events).map(|(_, e)| e.clone()).collect();
        for event in &events {
            match event {
                InputEvent::KeyDown { key, .. } => match key.as_str() {
                    "LEFTBUTTON" => {
                        if m_over {
                            self.drag = Some((cursor_x, cursor_y));
                        }
                    }
                    "p" => self.show_heat_map = !self.show_heat_map,
                    "d" if sg::is_key_down(f, "CTRL") => self.show_stat_differences = !self.show_stat_differences,
                    "c" if sg::is_key_down(f, "CTRL") && self.hover_node.is_some_and(|h| spec.nodes[h].ty() != NodeType::Socket) => {
                        let n = &spec.nodes[self.hover_node.expect("checked")];
                        let mut result = format!("# {}\n", n.dn());
                        for line in n.sd() {
                            result.push_str(line);
                            result.push('\n');
                        }
                        f.copy(&result);
                    }
                    "PAGEUP" => {
                        let l = if sg::is_key_down(f, "SHIFT") { 3.0 } else { 1.0 };
                        self.do_zoom(f, l, view_port);
                    }
                    "PAGEDOWN" => {
                        let l = if sg::is_key_down(f, "SHIFT") { -3.0 } else { -1.0 };
                        self.do_zoom(f, l, view_port);
                    }
                    // :228-229 `itemLib.wiki.open(self.hoverNode.name or self.hoverNode.dn)`
                    "F1" => {
                        if let Some(h) = self.hover_node {
                            let name = engine.borrow().as_ref().expect("engine").spec_rc().nodes[h].dn().to_string();
                            crate::tabs::items::wiki_open(f, &name);
                        }
                    }
                    _ => {}
                },
                InputEvent::KeyUp { key } => {
                    if key == "LEFTBUTTON" {
                        if self.drag.is_some() && !self.dragging {
                            tree_click = Some("LEFT");
                        }
                    } else if m_over {
                        match key.as_str() {
                            "RIGHTBUTTON" => tree_click = Some("RIGHT"),
                            "WHEELUP" => {
                                let l = if sg::is_key_down(f, "SHIFT") { 3.0 } else { 1.0 };
                                self.do_zoom(f, l, view_port);
                            }
                            "WHEELDOWN" => {
                                let l = if sg::is_key_down(f, "SHIFT") { -3.0 } else { -1.0 };
                                self.do_zoom(f, l, view_port);
                            }
                            _ => {}
                        }
                    }
                }
                InputEvent::Char { .. } => {}
            }
        }

        // :249-267 dragging
        if !sg::is_key_down(f, "LEFTBUTTON") {
            self.dragging = false;
            self.drag = None;
        }
        if let Some((dx, dy)) = self.drag {
            if !self.dragging && ((cursor_x - dx).abs() > 5.0 || (cursor_y - dy).abs() > 5.0) {
                self.dragging = true;
            }
            if self.dragging {
                self.zoom_x = self.zoom_x + cursor_x - dx;
                self.zoom_y = self.zoom_y + cursor_y - dy;
                self.drag = Some((cursor_x, cursor_y));
            }
        }

        // :269-273 Ctrl-click to zoom
        if let Some(c) = tree_click
            && sg::is_key_down(f, "CTRL")
        {
            self.do_zoom(f, if c == "RIGHT" { -2.0 } else { 2.0 }, view_port);
            tree_click = None;
        }

        // :275-278 clamp the zoom offset
        let clamp_factor = self.zoom * 2.0 / 3.0;
        self.zoom_x = m_min(m_max(self.zoom_x, -view_port.width * clamp_factor), view_port.width * clamp_factor);
        self.zoom_y = m_min(
            m_max(self.zoom_y, -view_port.height * clamp_factor),
            view_port.height * clamp_factor,
        );

        // :280-291
        let scale = m_min(view_port.width, view_port.height) / art.size * self.zoom;
        let offset_x = self.zoom_x + view_port.x + view_port.width / 2.0;
        let offset_y = self.zoom_y + view_port.y + view_port.height / 2.0;
        let to_screen = |x: f64, y: f64| (x * scale + offset_x, y * scale + offset_y);

        // :293-300 path tracing mode
        if sg::is_key_down(f, "SHIFT") {
            self.trace_mode = true;
            if self.trace_path.is_none() {
                self.trace_path = Some(Vec::new());
            }
        } else {
            self.trace_mode = false;
            self.trace_path = None;
        }

        // :302-332 the node under the cursor
        let node_order = std::mem::take(&mut self.cache.as_mut().expect("synced").node_order);
        let mut hover_node: Option<usize> = None;
        if m_over {
            let cur_x = (cursor_x - offset_x) / scale;
            let cur_y = (cursor_y - offset_y) / scale;
            for &i in &node_order {
                let n = &spec.nodes[i];
                if let Some(size) = overlay_size(n.ty()) {
                    let t = n.tree_node();
                    let (Some(x), Some(y)) = (t.x, t.y) else { continue };
                    let (vx, vy) = (cur_x - x, cur_y - y);
                    if vx * vx + vy * vy <= size * size {
                        hover_node = Some(i);
                        break;
                    }
                }
            }
        }
        // :318-331 compare-only nodes
        let compare = self.compare_spec.clone();
        // :118-128 `GetCompareJewel`: `compareSpec.build.itemsTab`
        let compare_owner = self.compare_owner.as_ref().and_then(std::rc::Weak::upgrade);
        let compare_same_build = compare_owner.is_none();
        let compare_jewel = |id: u32| -> Option<Rc<pob_items::item::Item>> {
            let b = compare_owner.as_ref()?;
            let eng = b.engine.borrow();
            let e = eng.as_ref()?;
            socket_jewel(e, compare.as_deref()?, id)
        };
        let mut hover_compare: Option<usize> = None;
        if m_over
            && hover_node.is_none()
            && let Some(cs) = &compare
        {
            let cur_x = (cursor_x - offset_x) / scale;
            let cur_y = (cursor_y - offset_y) / scale;
            for (ci, c) in cs.nodes.iter().enumerate() {
                if spec.node_idx(c.id).is_none() && c.alloc && !c.ty().is_start() {
                    let t = c.tree_node();
                    if let (Some(size), Some(x), Some(y)) = (overlay_size(c.ty()), t.x, t.y) {
                        let (vx, vy) = (cur_x - x, cur_y - y);
                        if vx * vx + vy * vy <= size * size {
                            hover_compare = Some(ci);
                            break;
                        }
                    }
                }
            }
        }
        self.hover_node = hover_node;

        // :335-386 path / dependents of the hovered node
        let n_nodes = spec.nodes.len();
        let mut hover_path: Option<Vec<bool>> = None;
        let mut hover_dep: Option<Vec<bool>> = None;
        if self.trace_mode {
            let tp = self.trace_path.as_mut().expect("trace mode");
            if let Some(h) = hover_node {
                let node = &spec.nodes[h];
                match &node.path {
                    None => hover_node = None,
                    Some(path) => {
                        if tp.is_empty() {
                            for p in path {
                                tp.insert(0, *p);
                            }
                        } else {
                            let last = *tp.last().expect("not empty");
                            if h != last {
                                if node.linked.contains(&last) {
                                    if let Some(index) = tp.iter().position(|p| *p == h) {
                                        tp.remove(index);
                                        tp.push(h);
                                    } else if spec.nodes[last].ty() == NodeType::Mastery {
                                        hover_node = None;
                                    } else {
                                        tp.push(h);
                                    }
                                } else {
                                    hover_node = None;
                                }
                            }
                        }
                    }
                }
            }
            let mut hp = vec![false; n_nodes];
            for p in tp.iter() {
                hp[*p] = true;
            }
            hover_path = Some(hp);
        } else if let Some(h) = hover_node
            && let Some(path) = &spec.nodes[h].path
        {
            let mut hp = vec![false; n_nodes];
            if spec.nodes[h].intuitive_leap_likes_affecting.is_empty() {
                for p in path {
                    hp[*p] = true;
                }
            }
            hover_path = Some(hp);
            let mut hd = vec![false; n_nodes];
            for d in &spec.nodes[h].depends {
                hd[*d] = true;
            }
            hover_dep = Some(hd);
        }

        // :391-544 clicks
        if tree_click == Some("LEFT") {
            if let Some(h) = hover_node {
                let id = spec.nodes[h].id;
                let tab = self.owner_build().tree_tab.borrow().clone();
                if let Some(t) = &tab {
                    t.ensure_undo();
                }
                let add_undo = || {
                    if let Some(t) = &tab {
                        t.add_undo_state();
                    }
                };
                if spec.nodes[h].alloc {
                    let res = engine.borrow_mut().as_mut().expect("engine").dealloc_node(id);
                    if let Err(e) = res {
                        self.note(&e);
                    } else {
                        add_undo();
                        mod_flag.set(true);
                        build_flag.set(true);
                    }
                } else {
                    let mut leave = false;
                    if let Some(asc) = spec.nodes[h].ascendancy_name().map(str::to_owned) {
                        let bloodline = spec
                            .tree
                            .secondary_ascend_name_map
                            .as_ref()
                            .is_some_and(|m| m.contains_key(asc.as_str()));
                        let cur_secondary_id = secondary_ascend_id(&spec);
                        let cur_secondary = cur_secondary_id.as_deref();
                        if bloodline {
                            // :408-422
                            if cur_secondary != Some(asc.as_str()) {
                                let k = engine.borrow().as_ref().expect("engine").find_bloodline(&asc);
                                if let Some(k) = k {
                                    let res = engine.borrow_mut().as_mut().expect("engine").select_secondary_ascend_class(k);
                                    res.unwrap_or_else(|e| panic!("{e}"));
                                    add_undo();
                                    if self.owner.is_none() {
                                        main().build_mode.set_title_pub(f);
                                    }
                                    build_flag.set(true);
                                }
                            }
                        } else if (spec.cur_ascend_class_id.unwrap_or(0) == 0
                            || Some(asc.as_str()) != spec.cur_ascend_class_base_name.as_deref())
                            && cur_secondary != Some(asc.as_str())
                        {
                            // :432-505
                            let cur_class = spec.cur_class_id.expect("class");
                            let same = spec.tree.classes[cur_class]
                                .classes
                                .iter()
                                .position(|a| a.id.as_deref() == Some(asc.as_str()));
                            if let Some(a) = same {
                                let res = engine.borrow_mut().as_mut().expect("engine").select_ascend_class(a as f64);
                                res.unwrap_or_else(|e| panic!("{e}"));
                                add_undo();
                                if self.owner.is_none() {
                                    main().build_mode.set_title_pub(f);
                                }
                                build_flag.set(true);
                            } else {
                                let found = engine.borrow().as_ref().expect("engine").find_ascendancy(&asc);
                                if let Some((c, a, class_name)) = found {
                                    let used = spec.count_alloc_nodes().used;
                                    let connected = engine.borrow().as_ref().expect("engine").is_class_connected(c);
                                    if used == 0 || connected {
                                        crate::tabs::tree::switch_class(f, c, Some((a, id)));
                                    } else {
                                        crate::tabs::tree::confirm_class_change(f, c, &class_name, Some((a, id)));
                                        leave = true;
                                    }
                                }
                            }
                        }
                    }
                    if !leave {
                        spec = engine.borrow().as_ref().expect("engine").spec_rc();
                        // :510-518 normal allocation
                        if let Some(h2) = spec.node_idx(id)
                            && spec.nodes[h2].path.is_some()
                            && !spec.nodes[h2].alloc
                        {
                            let alt: Option<Vec<u32>> = self
                                .trace_path
                                .as_ref()
                                .filter(|tp| tp.last() == Some(&h))
                                .map(|tp| tp.iter().map(|i| spec.nodes[*i].id).collect());
                            if spec.nodes[h2].ty() == NodeType::Mastery && art.nodes.get(&id).is_some_and(|n| n.has_mastery_effects) {
                                crate::tabs::tree::open_mastery_popup(f, id, alt);
                            } else {
                                let res = engine.borrow_mut().as_mut().expect("engine").alloc_node(id, alt.as_deref());
                                if let Err(e) = res {
                                    self.note(&e);
                                } else {
                                    add_undo();
                                    mod_flag.set(true);
                                    build_flag.set(true);
                                }
                            }
                        }
                    }
                }
                spec = engine.borrow().as_ref().expect("engine").spec_rc();
                self.sync_cache(&spec, &art);
                hover_node = spec.node_idx(id);
                self.hover_node = hover_node;
                // the node list may have changed (a cluster socket's graph, another class' tree): the frame's path tables go with it
                if spec.nodes.len() != n_nodes {
                    hover_path = None;
                    hover_dep = None;
                    self.trace_path = None;
                    self.trace_mode = false;
                }
            }
        } else if tree_click == Some("RIGHT")
            && let Some(h) = hover_node
        {
            // :522-543
            let n = &spec.nodes[h];
            let id = n.id;
            let has_effects = art.nodes.get(&id).is_some_and(|x| x.has_mastery_effects);
            let can_tattoo = engine.borrow().as_ref().expect("engine").node_can_tattoo(id);
            if n.alloc && n.ty() == NodeType::Socket {
                // :522-529 jump to the item page and focus the item slot control
                let b = self.owner_build();
                let tab = b.items_tab.borrow().clone();
                if let Some(slot) = tab.as_ref().and_then(|t| t.sockets.borrow().get(&id).cloned())
                    && slot.is_enabled(f)
                {
                    slot.drop.dropped.set(true);
                    tab.as_ref().expect("itemsTab").host.select_control(f, Some(slot));
                    *b.view_mode.borrow_mut() = "ITEMS".to_string();
                }
            } else if can_tattoo {
                crate::tabs::tree::modify_node_popup(f, id);
                build_flag.set(true);
            } else if n.alloc && n.ty() == NodeType::Mastery && has_effects {
                crate::tabs::tree::open_mastery_popup(f, id, None);
                build_flag.set(true);
            } else if !n.alloc && n.ty() == NodeType::Mastery && has_effects {
                crate::tabs::tree::modify_node_popup(f, id);
                build_flag.set(true);
            }
        }

        // :546-555 the background
        let bg = art
            .assets
            .get("Background2")
            .or_else(|| art.assets.get("Background1"))
            .expect("Background")
            .clone();
        if bg.width.get() == 0.0 {
            let (w, h) = bg.handle.image_size();
            bg.width.set(f64::from(w));
            bg.height.set(f64::from(h));
        }
        if bg.width.get() > 0.0 {
            let bg_size = bg.width.get() * scale * 1.33 * 2.5;
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            sg::draw_image(
                f,
                Some(&bg.handle),
                view_port.x,
                view_port.y,
                view_port.width,
                view_port.height,
                Some([
                    (self.zoom_x + view_port.width / 2.0) / -bg_size,
                    (self.zoom_y + view_port.height / 2.0) / -bg_size,
                    (view_port.width / 2.0 - self.zoom_x) / bg_size,
                    (view_port.height / 2.0 - self.zoom_y) / bg_size,
                ]),
            );
        }

        // :557-576 class background art (`curClassId` 0 draws none)
        let class_bg = match cur_class_id {
            Some(1) => Some((-2750.0, 1600.0, "BackgroundStr")),
            Some(2) => Some((2550.0, 1600.0, "BackgroundDex")),
            Some(3) => Some((-250.0, -2200.0, "BackgroundInt")),
            Some(4) => Some((-150.0, 2350.0, "BackgroundStrDex")),
            Some(5) => Some((-2100.0, -1500.0, "BackgroundStrInt")),
            Some(6) => Some((2350.0, -1950.0, "BackgroundDexInt")),
            _ => None,
        };
        if let Some((x, y, name)) = class_bg {
            let (sx, sy) = to_screen(x, y);
            draw_asset(f, art.assets.get(name), sx, sy, scale, false);
        }

        // :578-647 renderGroup; :649-657 the group backgrounds
        let cur_asc = spec.cur_ascend_class_base_name.clone();
        // `spec.curSecondaryAscendClass.id`
        let cur_sec = secondary_ascend_id(&spec);
        let zoom = self.zoom;
        let render_group = |f: &mut Frame<'_>, x: f64, y: f64, asc: Option<&str>, asc_start: bool, oo: [bool; 4], is_expansion: bool| {
            let (scr_x, scr_y) = to_screen(x, y);
            if let Some(asc) = asc {
                if asc_start {
                    let is_selected = Some(asc) == cur_asc.as_deref() || cur_sec.as_deref() == Some(asc);
                    if !is_selected {
                        sg::set_draw_color_a(f, 1.0, 1.0, 1.0, 0.5);
                    }
                    draw_asset(f, art.assets.get(&format!("Classes{asc}")), scr_x, scr_y, scale, false);
                    if art.has_class_ascendancies
                        && let Some(a) = art.ascendancies.iter().find(|a| a.id == asc)
                        && let (Some(rect), Some(colour), Some(text)) = (a.flavour_rect, &a.flavour_text_colour, &a.flavour_text)
                    {
                        {
                            let off_x = rect.0 - if a.is_alternate { 744.0 } else { 650.0 };
                            let off_y = rect.1 - if a.is_alternate { 706.0 } else { 650.0 };
                            let (tx, ty) = to_screen(x + off_x, y + off_y);
                            if zoom >= 2.5 {
                                let color = if is_selected {
                                    format!("^x{colour}")
                                } else {
                                    let c = |r: std::ops::Range<usize>| {
                                        (u32::from_str_radix(&colour[r], 16).unwrap_or(0) as f64 * 0.5).floor() as u32
                                    };
                                    format!("^x{:02X}{:02X}{:02X}", c(0..2), c(2..4), c(4..6))
                                };
                                sg::draw_string(f, tx, ty, Some("LEFT"), 52.0 * scale, "FONTIN ITALIC", &format!("{color}{text}"));
                            }
                        }
                    }
                    sg::set_draw_color(f, 1.0, 1.0, 1.0);
                }
            } else if oo[3] {
                let n = if is_expansion {
                    "GroupBackgroundLargeHalfAlt"
                } else {
                    "PSGroupBackground3"
                };
                draw_asset(f, art.assets.get(n), scr_x, scr_y, scale, true);
            } else if oo[2] {
                let n = if is_expansion {
                    "GroupBackgroundMediumAlt"
                } else {
                    "PSGroupBackground2"
                };
                draw_asset(f, art.assets.get(n), scr_x, scr_y, scale, false);
            } else if oo[1] {
                let n = if is_expansion {
                    "GroupBackgroundSmallAlt"
                } else {
                    "PSGroupBackground1"
                };
                draw_asset(f, art.assets.get(n), scr_x, scr_y, scale, false);
            }
        };
        for g in &art.group_order {
            let Some(group) = spec.tree.groups.get(g) else { continue };
            if !group.is_proxy {
                let oo = art.group_oo.get(g).copied().unwrap_or([false; 4]);
                render_group(
                    f,
                    group.x,
                    group.y,
                    group.ascendancy_name.as_deref(),
                    group.is_ascendancy_start,
                    oo,
                    false,
                );
            }
        }
        let cache = self.cache.as_ref().expect("synced");
        for sg_art in &cache.sub_graphs {
            let mut oo = [false; 4];
            if (0.0..4.0).contains(&sg_art.node_orbit) {
                oo[sg_art.node_orbit as usize] = true;
            }
            render_group(f, sg_art.x, sg_art.y, None, false, oo, true);
        }
        // :658-667 compare-only subgraphs
        if let Some(cs) = &compare {
            for v in cs.sub_graph_views() {
                if spec.sub_graph_views().iter().any(|o| o.id == v.id) {
                    continue;
                }
                let mut oo = [false; 4];
                if (0.0..4.0).contains(&v.node_orbit) {
                    oo[v.node_orbit as usize] = true;
                }
                sg::set_draw_color_a(f, 0.0, 1.0, 0.0, 0.6);
                render_group(f, v.x, v.y, None, false, oo, true);
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
            }
        }

        // :669-753 connectors
        let split_conn = {
            let eng = engine.borrow();
            split_personality_path(eng.as_ref().expect("engine"), &spec)
        };
        let cur_asc_ref = spec.cur_ascend_class_base_name.as_deref();
        let render_connector = |f: &mut Frame<'_>, connector: &Connector, i1: usize, i2: usize| {
            let (n1, n2) = (&spec.nodes[i1], &spec.nodes[i2]);
            let mut state = 0usize; // Normal
            if n1.alloc && n2.alloc {
                state = 2;
            } else if let Some(hp) = &hover_path {
                let on = |i: usize, n: &SpecNode| n.alloc || Some(i) == hover_node || hp[i];
                if on(i1, n1) && on(i2, n2) {
                    state = 1;
                }
            }
            // :696-709 `baseState` over the compare spec's nodes
            let mut compare_color: Option<(f64, f64, f64)> = None;
            if let Some(cs) = &compare {
                let base_active = match (cs.node(n1.id), cs.node(n2.id)) {
                    (Some(c1), Some(c2)) => c1.alloc && c2.alloc,
                    _ => state == 2,
                };
                if base_active && state != 2 {
                    state = 2;
                    compare_color = Some((0.0, 1.0, 0.0));
                }
                if !base_active && state == 2 {
                    compare_color = Some((1.0, 0.0, 0.0));
                }
            }
            let vert = &connector.vert[state];
            let mut c = [0.0; 8];
            for k in 0..4 {
                let (x, y) = to_screen(vert[k * 2], vert[k * 2 + 1]);
                c[k * 2] = x;
                c[k * 2 + 1] = y;
            }
            if hover_dep.as_ref().is_some_and(|hd| hd[i1] && hd[i2]) {
                sg::set_draw_color(f, 1.0, 0.0, 0.0);
            } else if connector.ascendancy_name.is_some() && connector.ascendancy_name.as_deref() != cur_asc_ref {
                sg::set_draw_color(f, 0.75, 0.75, 0.75);
            } else if let Some((r, g, b)) = compare_color {
                sg::set_draw_color(f, r, g, b);
            } else if split_conn[i1] && split_conn[i2] {
                // `setConnectorColor(colorCodes.SPLITPERSONALITY)` -- an escape string in slot 1: `SetDrawColor(unpack{str, 1, 1})`
                sg::set_draw_color_str(f, &color_code("SPLITPERSONALITY").unwrap_or_default());
            } else {
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
            }
            let state_name = ["Normal", "Intermediate", "Active"][state];
            let mut asset = None;
            if is_abyss_conquered(n1) && is_abyss_conquered(n2) {
                asset = art.assets.get(&format!("Abyss{}{state_name}", connector.ty));
            }
            let asset = asset.or_else(|| art.assets.get(&format!("{}{state_name}", connector.ty)));
            let Some(asset) = asset else { return };
            let (left, top, width, height) = match asset.tc {
                Some(t) => (t[0], t[1], t[2] - t[0], t[3] - t[1]),
                None => (0.0, 0.0, 1.0, 1.0),
            };
            let t = &connector.tc;
            f.draw_image_quad(
                Some(&asset.handle),
                c,
                Some([
                    left + t[0] * width,
                    top + t[1] * height,
                    left + t[2] * width,
                    top + t[3] * height,
                    left + t[4] * width,
                    top + t[5] * height,
                    left + t[6] * width,
                    top + t[7] * height,
                ]),
            )
            .unwrap_or_else(|e| panic!("{e}"));
        };
        sg::set_draw_layer(f, None, 20);
        for (c, i1, i2) in &cache.tree_connectors {
            render_connector(f, &art.connectors[*c], *i1, *i2);
        }
        for sg_art in &cache.sub_graphs {
            for (c, i1, i2) in &sg_art.connectors {
                render_connector(f, c, *i1, *i2);
            }
        }
        // :755-781 connectors of compare-only subgraphs
        if let Some(cs) = &compare {
            let own: Vec<u32> = spec.sub_graph_views().iter().map(|v| v.id).collect();
            for v in cs.sub_graph_views() {
                if own.contains(&v.id) {
                    continue;
                }
                let group_of = |i: usize| {
                    let n = &cs.nodes[i];
                    if n.is_subgraph_node() {
                        (v.x, v.y)
                    } else {
                        n.tree_node()
                            .group
                            .and_then(|g| cs.tree.groups.get(&g))
                            .map_or((0.0, 0.0), |g| (g.x, g.y))
                    }
                };
                for (a, b) in &v.links {
                    let mut out = Vec::new();
                    art.build_connector(
                        cs.nodes[*a].tree_node(),
                        cs.nodes[*b].tree_node(),
                        group_of(*a),
                        group_of(*b),
                        &mut out,
                    );
                    for c in out {
                        let (Some(n1), Some(n2)) = (cs.node(c.node_id1), cs.node(c.node_id2)) else {
                            continue;
                        };
                        if !(n1.alloc && n2.alloc) {
                            continue;
                        }
                        let vert = &c.vert[2];
                        let mut q = [0.0; 8];
                        for k in 0..4 {
                            let (x, y) = to_screen(vert[k * 2], vert[k * 2 + 1]);
                            q[k * 2] = x;
                            q[k * 2 + 1] = y;
                        }
                        sg::set_draw_color(f, 0.0, 1.0, 0.0);
                        if let Some(asset) = art
                            .assets
                            .get(&format!("{}Active", c.ty))
                            .or_else(|| art.assets.get(&format!("{}Normal", c.ty)))
                        {
                            // `unpack(connector.c)`: the tex coords of the connector as they are
                            f.draw_image_quad(Some(&asset.handle), q, Some(c.tc))
                                .unwrap_or_else(|e| panic!("{e}"));
                        }
                    }
                }
            }
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        }

        // :783-787
        let power = if self.show_heat_map {
            self.owner_build().tree_tab.borrow().as_ref().and_then(|t| {
                t.build_power(f);
                t.power.borrow().clone()
            })
        } else {
            None
        };
        let heat_stat_single = self
            .owner_build()
            .tree_tab
            .borrow()
            .as_ref()
            .is_some_and(|t| t.power_stat.borrow().as_ref().is_some_and(|s| s.stat.is_some()));
        let theme = main().s.borrow().node_power_theme.clone();

        let eng = engine.borrow();
        let e = eng.as_ref().expect("engine");
        let granted = &e.result().env.granted_passives;

        // :789-813 the search
        if self.search_str_cached != self.search_str {
            self.search_str_cached.clone_from(&self.search_str);
            self.search_params = prep_search(&self.search_str);
            self.search_str_results.clear();
            for &i in &node_order {
                let m = !self.search_params.is_empty() && node_matches_search(&spec.nodes[i], &art, &self.search_params);
                self.search_str_results.insert(i, m);
            }
        }

        let split_path = &split_conn;
        let edge_search_highlight = main().s.borrow().edge_search_highlight;
        let ctrl = sg::is_key_down(f, "CTRL");
        let shift = sg::is_key_down(f, "SHIFT");
        let no_popups = main().popups.borrow().is_empty();
        let mut draw_tooltip_for: Option<(usize, f64, f64)> = None;
        let jewel_radius = &pob_data::rules::rules().jewel_radii;
        let radii = jewel_radius.get(jewel_radius_table()).expect("jewelRadii");

        // :815-1110 the nodes
        for &i in &node_order {
            let node = &spec.nodes[i];
            let id = node.id;
            let ty = node.ty();
            let tn = node.tree_node();
            let compare_node = compare.as_ref().and_then(|c| c.node(id));
            let is_alloc = node.alloc || granted.contains_key(&id) || compare_node.is_some_and(|c| c.alloc);
            let node_default = split_path[i].then(|| color_code("SPLITPERSONALITY")).flatten();
            let art_node = art.nodes.get(&id);
            let mut base: Option<AssetRef> = None;
            let mut overlay_asset: Option<String> = None;
            let mut effect: Option<AssetRef> = None;

            sg::set_draw_layer(f, None, 25);
            let trace_last = self.trace_mode && self.trace_path.as_ref().and_then(|t| t.last()) == Some(&i);
            match ty {
                NodeType::ClassStart => {
                    overlay_asset = Some(if is_alloc {
                        class_start_art(tn.class_start_index.unwrap_or(0.0)).to_string()
                    } else {
                        "PSStartNodeBackgroundInactive".to_string()
                    });
                }
                NodeType::AscendClassStart => {
                    let mut o = String::new();
                    if let Some(asc) = node.ascendancy_name()
                        && let Some(p) = art.bloodline_prefix.get(asc)
                    {
                        o.push_str(p);
                    }
                    o.push_str(if art.version_num >= 3.10 {
                        "AscendancyMiddle"
                    } else {
                        "PassiveSkillScreenAscendancyMiddle"
                    });
                    overlay_asset = Some(o);
                }
                _ => {
                    let state = if self.show_heat_map || is_alloc || Some(i) == hover_node || trace_last {
                        "alloc"
                    } else if hover_path.as_ref().is_some_and(|hp| hp[i]) {
                        "path"
                    } else {
                        "unalloc"
                    };
                    match ty {
                        NodeType::Socket => {
                            let alt = if tn.expansion_jewel.is_some() { "Alt" } else { "" };
                            let key = format!("{state}{alt}");
                            let charm = node.name() == Some("Charm Socket");
                            base = overlay(ty, &key, art.version_num)
                                .and_then(|n| {
                                    if charm {
                                        art.assets.get(&format!("Azmeri{n}"))
                                    } else {
                                        art.assets.get(n)
                                    }
                                })
                                .cloned();
                            if is_alloc && let Some(jewel) = socket_jewel(e, &spec, id) {
                                overlay_asset = jewel_socket_overlay(&jewel, tn.expansion_jewel.is_some()).map(str::to_owned);
                            }
                        }
                        NodeType::Mastery => {
                            let over = spec.hash_overrides.get(&id);
                            let sprites = over.and_then(|o| art.sprite_map.get(&o.icon));
                            let effect_sprites = over.and_then(|o| art.sprite_map.get(&o.active_effect_image));
                            let has_effects = art_node.is_some_and(|n| n.has_mastery_effects);
                            let lookup = |icon: Option<&Rc<str>>, ty: &str| {
                                icon.and_then(|ic| art.sprite_map.get(&**ic)).and_then(|m| m.get(ty)).cloned()
                            };
                            if has_effects {
                                let an = art_node.expect("has effects");
                                if is_alloc {
                                    base = sprites
                                        .and_then(|s| s.get("masteryActiveSelected"))
                                        .cloned()
                                        .or_else(|| lookup(an.active_icon.as_ref(), "masteryActiveSelected"));
                                    effect = effect_sprites
                                        .and_then(|s| s.get("masteryActiveEffect"))
                                        .cloned()
                                        .or_else(|| lookup(an.active_effect_image.as_ref(), "masteryActiveEffect"));
                                } else if Some(i) == hover_node {
                                    base = sprites
                                        .and_then(|s| s.get("masteryConnected"))
                                        .cloned()
                                        .or_else(|| lookup(an.inactive_icon.as_ref(), "masteryConnected"));
                                } else {
                                    base = sprites
                                        .and_then(|s| s.get("masteryInactive"))
                                        .cloned()
                                        .or_else(|| lookup(an.inactive_icon.as_ref(), "masteryInactive"));
                                }
                            } else {
                                base = sprites.and_then(|s| s.get("mastery")).cloned().or_else(|| {
                                    node_sprites(&art, node, &spec.imported_tattoos)
                                        .and_then(|s| s.get("mastery"))
                                        .cloned()
                                });
                            }
                            sg::set_draw_layer(f, None, 15);
                        }
                        _ => {
                            // Normal, notable, keystone
                            if node.is_tattoo()
                                && let Some((_, image)) = node.tattoo_art()
                            {
                                effect = art.sprite_map.get(image).and_then(|m| m.get("tattooActiveEffect")).cloned();
                            }
                            let lower = ty.as_str().to_lowercase();
                            let suffix = if is_alloc { "Active" } else { "Inactive" };
                            base = node_sprites(&art, node, &spec.imported_tattoos)
                                .and_then(|s| s.get(&format!("{lower}{suffix}")))
                                .cloned();
                            let asc = node.ascendancy_name();
                            let key = format!(
                                "{state}{}{}",
                                if asc.is_some() { "Ascend" } else { "" },
                                if tn.is_blighted { "Blighted" } else { "" }
                            );
                            let mut name = overlay(ty, &key, art.version_num).map(str::to_owned);
                            if is_abyss_conquered(node) && matches!(ty, NodeType::Notable | NodeType::Keystone) {
                                let frame_type = if asc.is_some() { "Ascendancy" } else { ty.as_str() };
                                let frame_state = match state {
                                    "alloc" => "Allocated",
                                    "path" => "CanAllocate",
                                    _ => "Unallocated",
                                };
                                let abyss = format!("Abyss{frame_type}Frame{frame_state}");
                                if art.assets.contains_key(&abyss) {
                                    name = Some(abyss);
                                }
                            }
                            overlay_asset = match (asc.and_then(|a| art.bloodline_prefix.get(a)), name) {
                                (Some(p), Some(n)) => Some(format!("{p}{n}")),
                                (_, n) => n,
                            };
                        }
                    }
                }
            }

            let (Some(x), Some(y)) = (tn.x, tn.y) else { continue };
            let (scr_x, scr_y) = to_screen(x, y);

            // :917-959 heat map / `GetCompareNodeColor` (:175-194) / `nodeDefaultColor`
            let cmp_colour: Option<(f64, f64, f64)> = compare_node.and_then(|c| {
                if c.alloc && !node.alloc {
                    Some((0.0, 1.0, 0.0))
                } else if !c.alloc && node.alloc {
                    Some((1.0, 0.0, 0.0))
                } else if ty == NodeType::Mastery
                    && c.alloc
                    && node.alloc
                    && (compare.as_ref().is_some_and(|cs| cs.tree_version != spec.tree_version)
                        || compare.as_ref().and_then(|cs| cs.mastery_selections.get(&id)) != spec.mastery_selections.get(&id))
                {
                    Some((0.0, 0.0, 1.0))
                } else if ty == NodeType::Socket
                    && c.alloc
                    && node.alloc
                    && !compare_same_build
                    && !jewels_equal(socket_jewel(e, &spec, id).as_ref(), compare_jewel(id).as_ref())
                {
                    // :185-191
                    Some((0.0, 0.0, 1.0))
                } else {
                    // :185-191: on the tab of one build `GetCompareJewel` reads the active sockets: the same jewel
                    None
                }
            });
            if self.show_heat_map && !is_alloc && !ty.is_start() {
                let p = power.as_ref();
                let np = p.and_then(|r| r.nodes.get(i));
                let mx = p.map(|r| r.power_max).unwrap_or_default();
                if heat_stat_single {
                    let stat = m_max(np.and_then(|n| n.single_stat).unwrap_or(0.0), 0.0);
                    let col = (stat / mx.single_stat * 1.5).powf(0.5);
                    match theme.as_str() {
                        "RED/BLUE" => sg::set_draw_color(f, col, 0.0, 0.0),
                        "RED/GREEN" => sg::set_draw_color(f, 0.0, col, 0.0),
                        "GREEN/BLUE" => sg::set_draw_color(f, 0.0, 0.0, col),
                        _ => {}
                    }
                } else {
                    let offence = m_max(np.and_then(|n| n.offence).unwrap_or(0.0), 0.0);
                    let defence = m_max(np.and_then(|n| n.defence).unwrap_or(0.0), 0.0);
                    let dps = (offence / mx.offence * 1.5).powf(0.5);
                    let def = (defence / mx.defence * 1.5).powf(0.5);
                    let mix = (m_max(dps - 0.5, 0.0) + m_max(def - 0.5, 0.0)) / 2.0;
                    match theme.as_str() {
                        "RED/BLUE" => sg::set_draw_color(f, dps, mix, def),
                        "RED/GREEN" => sg::set_draw_color(f, dps, def, mix),
                        "GREEN/BLUE" => sg::set_draw_color(f, mix, dps, def),
                        _ => {}
                    }
                }
            } else if crate::launch::launch().dev_mode_alt.get() && !self.show_heat_map {
                // :948-956 debug display (`elseif` of `if self.showHeatMap` :916: with the heat map an allocated node keeps its own colour)
                if node.extra() {
                    sg::set_draw_color(f, 1.0, 0.0, 0.0);
                } else if node.unknown() {
                    sg::set_draw_color(f, 0.0, 1.0, 1.0);
                } else {
                    sg::set_draw_color(f, 0.0, 0.0, 0.0);
                }
            } else if let Some((r, g, b)) = cmp_colour {
                sg::set_draw_color(f, r, g, b);
            } else {
                match &node_default {
                    Some(c) => sg::set_draw_color_str(f, c),
                    None => sg::set_draw_color(f, 1.0, 1.0, 1.0),
                }
            }

            if let Some(eff) = &effect {
                sg::set_draw_layer(f, None, 15);
                draw_asset(f, Some(eff), scr_x, scr_y, scale, false);
                sg::set_draw_layer(f, None, 25);
            }

            if let Some(b) = &base {
                if ty == NodeType::Socket && hover_dep.as_ref().is_some_and(|hd| hd[i]) {
                    sg::set_draw_color(f, 1.0, 0.0, 0.0);
                    draw_asset(f, Some(b), scr_x, scr_y, scale, false);
                    sg::set_draw_color(f, 1.0, 1.0, 1.0);
                } else {
                    draw_asset(f, Some(b), scr_x, scr_y, scale, false);
                }
            }

            if let Some(o) = &overlay_asset {
                if !matches!(ty, NodeType::ClassStart | NodeType::AscendClassStart)
                    && let Some(h) = hover_node
                    && h != i
                {
                    if hover_dep.as_ref().is_some_and(|hd| hd[i]) {
                        sg::set_draw_color(f, 1.0, 0.0, 0.0);
                    } else if spec.nodes[h].ty() == NodeType::Socket
                        && let Some(nir) = spec.nodes[h].nodes_in_radius()
                    {
                        let jewel = socket_jewel(e, &spec, spec.nodes[h].id);
                        let is_toh = jewel.as_ref().is_some_and(|j| j.jewel_radius_label.as_deref() == Some("Variable"));
                        for (index, data) in radii.iter().enumerate() {
                            if nir.get(index).is_some_and(|l| l.contains(&id)) && ((data.inner != 0.0) == is_toh) {
                                sg::set_draw_color_str(f, &data.col);
                                break;
                            }
                        }
                    }
                }
                draw_asset(f, art.assets.get(o), scr_x, scr_y, scale, false);
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
            }
            if self.search_str_results.get(&i).copied().unwrap_or(false) {
                sg::set_draw_layer(f, None, 30);
                sg::set_draw_color(f, 1.0, 0.0, 0.0);
                let mut size = 175.0 * scale / self.zoom.powf(0.4);
                if edge_search_highlight {
                    let peekaboo = 1.15;
                    let wide = (view_port.x - size / peekaboo, view_port.x + view_port.width - size * peekaboo);
                    let high = (view_port.y - size / peekaboo, view_port.y + view_port.height - size * peekaboo);
                    let mut new_x = m_min(m_max(scr_x - size, wide.0), wide.1);
                    let mut new_y = m_min(m_max(scr_y - size, high.0), high.1);
                    if new_x != scr_x - size || new_y != scr_y - size {
                        size *= 0.6667;
                        new_x += size / 2.0;
                        new_y += size / 2.0;
                    }
                    sg::draw_image(f, Some(&self.highlight_ring), new_x, new_y, size * 2.0, size * 2.0, None);
                } else {
                    sg::draw_image(
                        f,
                        Some(&self.highlight_ring),
                        scr_x - size,
                        scr_y - size,
                        size * 2.0,
                        size * 2.0,
                        None,
                    );
                }
            }
            if Some(i) == hover_node
                && (ty != NodeType::Socket || !shift)
                && (ty != NodeType::Mastery || art_node.is_some_and(|n| n.has_mastery_effects))
                && !ctrl
                && no_popups
            {
                draw_tooltip_for = Some((i, scr_x, scr_y));
            }
        }
        // the tooltip is drawn in the node loop in Lua; its commands go to sub-layers 99/100, above everything of this layer
        if let Some((i, scr_x, scr_y)) = draw_tooltip_for {
            crate::tree_tooltip::draw_node_tooltip(self, f, e, &art, &spec, i, scr_x, scr_y, scale, view_port);
        }
        sg::set_draw_color(f, 1.0, 1.0, 1.0);

        // :1112-1165 compare-only nodes
        if let Some(cs) = &compare {
            sg::set_draw_layer(f, None, 25);
            for (ci, c) in cs.nodes.iter().enumerate() {
                let t = c.tree_node();
                if spec.node_idx(c.id).is_some() || !c.alloc || c.ty().is_start() {
                    continue;
                }
                let (Some(x), Some(y)) = (t.x, t.y) else { continue };
                let (scr_x, scr_y) = to_screen(x, y);
                sg::set_draw_color(f, 0.0, 1.0, 0.0);
                let base: Option<AssetRef>;
                let mut over: Option<String> = None;
                match c.ty() {
                    NodeType::Socket => {
                        let key = if t.expansion_jewel.is_some() { "allocAlt" } else { "alloc" };
                        let n = overlay(c.ty(), key, art.version_num).unwrap_or("JewelSocketActiveBlue");
                        base = art.assets.get(n).cloned();
                        if let Some(j) = compare_jewel(c.id).or_else(|| socket_jewel(e, &spec, c.id).filter(|_| compare_same_build)) {
                            over = jewel_socket_overlay(&j, t.expansion_jewel.is_some()).map(str::to_owned);
                        }
                    }
                    NodeType::Mastery => {
                        base = art
                            .nodes
                            .get(&c.id)
                            .and_then(|an| an.active_icon.as_ref())
                            .and_then(|ic| art.sprite_map.get(&**ic))
                            .and_then(|m| m.get("masteryActiveSelected"))
                            .cloned()
                            .or_else(|| node_sprites(&art, c, &compare_imported).and_then(|s| s.get("mastery")).cloned());
                    }
                    ty => {
                        base = node_sprites(&art, c, &compare_imported)
                            .and_then(|s| s.get(&format!("{}Active", ty.as_str().to_lowercase())))
                            .cloned();
                        let key = format!(
                            "alloc{}{}",
                            if c.ascendancy_name().is_some() { "Ascend" } else { "" },
                            if t.is_blighted { "Blighted" } else { "" }
                        );
                        over = overlay(ty, &key, art.version_num).map(str::to_owned);
                    }
                }
                if let Some(b) = &base {
                    draw_asset(f, Some(b), scr_x, scr_y, scale, false);
                }
                if let Some(o) = &over {
                    draw_asset(f, art.assets.get(o), scr_x, scr_y, scale, false);
                }
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
                if hover_compare == Some(ci)
                    && (c.ty() != NodeType::Mastery || art.nodes.get(&c.id).is_some_and(|n| n.has_mastery_effects))
                    && !ctrl
                    && no_popups
                {
                    sg::set_draw_layer(f, None, 100);
                    let size = (overlay_size(c.ty()).unwrap_or(0.0) * scale).floor();
                    if self
                        .tooltip
                        .check_for_update((c.id, false, None::<Vec<usize>>, e.output_revision()))
                    {
                        crate::tree_tooltip::add_compare_node_tooltip(self, f, e, &art, c);
                    }
                    self.tooltip.center = true;
                    self.tooltip.draw(
                        f,
                        (scr_x - size).floor(),
                        (scr_y - size).floor(),
                        Some((size * 2.0, size * 2.0)),
                        view_port,
                    );
                    sg::set_draw_layer(f, None, 25);
                }
            }
        }

        // :1167-1258 ring overlays for the jewel sockets
        sg::set_draw_layer(f, None, 25);
        let t = f.get_time() as f64 * 0.00003;
        let show_animations = main().s.borrow().show_animations;
        let cache = self.cache.as_ref().expect("synced");
        for &i in &cache.sockets {
            let node = &spec.nodes[i];
            if node.name() == Some("Charm Socket") {
                continue;
            }
            if node.tree_node().expansion_jewel.as_ref().is_some_and(|e| e.size != Some(2.0)) {
                continue;
            }
            let (Some(x), Some(y)) = (node.tree_node().x, node.tree_node().y) else {
                continue;
            };
            let (scr_x, scr_y) = to_screen(x, y);
            let jewel = socket_jewel(e, &spec, node.id);
            if Some(i) == hover_node {
                // :1224-1225 `jewel or cJewel`
                let effective = jewel.clone().or_else(|| compare_jewel(node.id));
                let is_toh = effective
                    .as_ref()
                    .is_some_and(|j| j.jewel_radius_label.as_deref() == Some("Variable"));
                for rad in radii {
                    let outer = rad.outer * scale;
                    let inner = rad.inner * scale;
                    if is_toh {
                        if inner != 0.0 {
                            sg::set_draw_color_str(f, &rad.col);
                            sg::draw_image(f, Some(&self.ring), scr_x - outer, scr_y - outer, outer * 2.0, outer * 2.0, None);
                            sg::draw_image(f, Some(&self.ring), scr_x - inner, scr_y - inner, inner * 2.0, inner * 2.0, None);
                        }
                    } else if inner == 0.0 {
                        sg::set_draw_color_str(f, &rad.col);
                        sg::draw_image(f, Some(&self.ring), scr_x - outer, scr_y - outer, outer * 2.0, outer * 2.0, None);
                    }
                }
            }
            // :1245-1256
            let c_jewel = compare_jewel(node.id).or_else(|| compare_same_build.then(|| jewel.clone()).flatten());
            let mut rings: Vec<(Rc<pob_items::item::Item>, [f64; 4])> = Vec::new();
            if node.alloc || compare.as_ref().and_then(|c| c.node(node.id)).is_some_and(|c| c.alloc) {
                let same = jewels_equal(jewel.as_ref(), c_jewel.as_ref());
                if let Some(j) = jewel.as_ref().filter(|j| j.jewel_radius_index.is_some()) {
                    let tint = if compare.is_none() || same {
                        JEWEL_RADIUS_TINT_NEUTRAL
                    } else {
                        JEWEL_RADIUS_TINT_PRIMARY_ONLY
                    };
                    rings.push((j.clone(), tint));
                }
                if !same && let Some(j) = c_jewel.as_ref().filter(|j| j.jewel_radius_index.is_some()) {
                    rings.push((j.clone(), JEWEL_RADIUS_TINT_COMPARE_ONLY));
                }
            }
            for (j, c) in &rings {
                let ri = j.jewel_radius_index.expect("a ring");
                // :1168-1214 drawJewelRadius (`isAbyssConquered(jewel.jewelData)`)
                let abyss = j.jewel_data.as_ref().is_some_and(|d| {
                    d.get("conqueredBy")
                        .get("conqueror")
                        .get("type")
                        .as_str()
                        .is_some_and(|s| s.starts_with("abyss_"))
                });
                if abyss {
                    continue;
                }
                let rad = &radii[ri as usize - 1];
                let outer = rad.outer * scale;
                let inner = rad.inner * scale * 1.06;
                sg::set_draw_color_a(f, c[0], c[1], c[2], c[3]);
                let title = j.title.as_deref().unwrap_or("");
                let d = outer * 2.0;
                if title.contains("Impossible Escape") {
                    // :1177-1189 the ring shows on the keystones (`pairs`: they do not overlap)
                    let inner = 150.0 * scale;
                    let keys = j.jewel_data.as_ref().map(|d| d.get("impossibleEscapeKeystones"));
                    if let Some(pob_mods::Value::Table(keys)) = &keys {
                        for name in keys.hash.keys() {
                            let Some(ks) = spec.tree.keystone_map.get(&**name).map(|k| &spec.tree.nodes[*k]) else {
                                continue;
                            };
                            let (Some(kx), Some(ky)) = (ks.x, ks.y) else { continue };
                            let (kx, ky) = to_screen(kx, ky);
                            draw_image_rotated(f, &self.jewel_shaded_outer_ring, kx, ky, d, d, -0.8, t, show_animations);
                            draw_image_rotated(f, &self.jewel_shaded_outer_ring_flipped, kx, ky, d, d, 1.0, t, show_animations);
                            draw_image_rotated(
                                f,
                                &self.jewel_shaded_inner_ring,
                                kx,
                                ky,
                                inner * 2.0,
                                inner * 2.0,
                                -1.2,
                                t,
                                show_animations,
                            );
                            draw_image_rotated(
                                f,
                                &self.jewel_shaded_inner_ring_flipped,
                                kx,
                                ky,
                                inner * 2.0,
                                inner * 2.0,
                                1.0,
                                t,
                                show_animations,
                            );
                        }
                    }
                } else if let Some((_, a, b)) = self.legion_rings.iter().find(|(n, _, _)| title.starts_with(n)) {
                    draw_image_rotated(f, a, scr_x, scr_y, d, d, -0.7, t, show_animations);
                    draw_image_rotated(f, b, scr_x, scr_y, d, d, 0.7, t, show_animations);
                } else {
                    let di = inner * 2.0;
                    draw_image_rotated(f, &self.jewel_shaded_outer_ring, scr_x, scr_y, d, d, -0.7, t, show_animations);
                    draw_image_rotated(
                        f,
                        &self.jewel_shaded_outer_ring_flipped,
                        scr_x,
                        scr_y,
                        d,
                        d,
                        0.7,
                        t,
                        show_animations,
                    );
                    draw_image_rotated(f, &self.jewel_shaded_inner_ring, scr_x, scr_y, di, di, -0.7, t, show_animations);
                    draw_image_rotated(
                        f,
                        &self.jewel_shaded_inner_ring_flipped,
                        scr_x,
                        scr_y,
                        di,
                        di,
                        0.7,
                        t,
                        show_animations,
                    );
                }
            }
        }
        drop(eng);
        self.cache.as_mut().expect("synced").node_order = node_order;
    }
}

/// `spec.splitPersonalityPath` (PassiveSpec.lua:1036-1048 over `GetShortestPathToClassStart` :992-1033): spec node index -> on a path.
fn split_personality_path(e: &Engine, spec: &PassiveSpec) -> Vec<bool> {
    let mut on = vec![false; spec.nodes.len()];
    let start = spec
        .cur_class_id
        .and_then(|c| spec.tree.classes.get(c))
        .and_then(|c| c.start_node_id);
    for (node_id, item_id) in &spec.jewels.0 {
        let Some(item) = e.result().env.item(*item_id) else { continue };
        if !item.jewel_data.as_ref().is_some_and(|d| {
            !matches!(
                d.get("jewelIncEffectFromClassStart"),
                pob_mods::Value::Nil | pob_mods::Value::Bool(false)
            )
        }) {
            continue;
        }
        let Some(root) = spec.node_idx(*node_id as u32).filter(|r| spec.nodes[*r].alloc) else {
            continue;
        };
        let mut parent: HashMap<usize, usize> = HashMap::new();
        let mut queue = vec![root];
        let mut o = 0;
        'walk: while o < queue.len() {
            let node = queue[o];
            o += 1;
            for &other in &spec.nodes[node].linked {
                if Some(spec.nodes[other].id) == start {
                    on[root] = true;
                    on[other] = true;
                    let mut cur = Some(node);
                    while let Some(c) = cur {
                        on[c] = true;
                        cur = parent.get(&c).copied();
                    }
                    break 'walk;
                }
                if spec.nodes[other].alloc
                    && spec.nodes[node].ty() != NodeType::Mastery
                    && !matches!(spec.nodes[other].ty(), NodeType::ClassStart | NodeType::AscendClassStart)
                    && !parent.contains_key(&other)
                    && other != root
                {
                    parent.insert(other, node);
                    queue.push(other);
                }
            }
        }
    }
    on
}

/// The key of `data.jewelRadii` of `data.jewelRadius` after Build.lua:606 (the latest tree: "3_16" table, Data.lua:583-601).
fn radius_key(tree_version: &str) -> &'static str {
    // `data.jewelRadius` as `SetActiveSpec` leaves it: `data.setJewelRadiiGlobally(curSpec.treeVersion)` (TreeTab.lua:552, Data.lua:583-589)
    let mut parts = tree_version.split('_').map(|p| p.parse::<u32>().unwrap_or(u32::MAX));
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) if major <= 3 && minor <= 15 => "3_15",
        _ => "3_16",
    }
}

/// `spec.curSecondaryAscendClass.id`.
fn secondary_ascend_id(spec: &PassiveSpec) -> Option<String> {
    let id = spec.cur_secondary_ascend_class_id?;
    spec.tree
        .alternate_ascendancies
        .as_ref()?
        .iter()
        .find(|(k, _)| *k as f64 == id)
        .and_then(|(_, a)| a.id.clone())
}

/// `classArt[node.classStartIndex]` (PassiveTree.lua:437-445).
fn class_start_art(index: f64) -> &'static str {
    match index as i64 {
        1 => "centermarauder",
        2 => "centerranger",
        3 => "centerwitch",
        4 => "centerduelist",
        5 => "centertemplar",
        6 => "centershadow",
        _ => "centerscion",
    }
}

/// `node.sprites`: `spriteMap[node.icon]` of the node or of what replaced it; a mastery with effects and a node with an unknown icon read
/// `MasteryBlank` (PassiveTree.lua:875-885).
fn node_sprites<'a>(art: &'a TreeArt, node: &SpecNode, imported_tattoos: &[u32]) -> Option<&'a HashMap<String, AssetRef>> {
    if let Some((icon, _)) = node.tattoo_art() {
        let m = art.sprite_map.get(icon);
        // `old.sprites = nil` after ImportFromNodeList: the tree node's, through the metatable (R21)
        if m.is_some() || !imported_tattoos.contains(&node.id) {
            return m;
        }
    }
    if let Some(l) = node.legion_node() {
        return art.sprite_map.get(l.icon.as_str());
    }
    let tn = node.tree_node();
    let icon: &str = match art.nodes.get(&node.id).and_then(|n| n.icon.as_deref()) {
        Some(i) if !node.is_subgraph_node() => i,
        _ => tn.icon.as_str(),
    };
    art.sprite_map
        .get(icon)
        .or_else(|| art.sprite_map.get("Art/2DArt/SkillIcons/passives/MasteryBlank.png"))
}

/// `PassiveTreeView:DrawAsset` (:1286-1304).
pub fn draw_asset(f: &mut Frame<'_>, data: Option<&AssetRef>, x: f64, y: f64, scale: f64, is_half: bool) {
    let Some(data) = data else { return };
    if data.width.get() == 0.0 {
        let (w, h) = data.handle.image_size();
        data.width.set(f64::from(w));
        data.height.set(f64::from(h));
        if w == 0 {
            return;
        }
    }
    let width = data.width.get() * scale * 1.33;
    let height = data.height.get() * scale * 1.33;
    if is_half {
        sg::draw_image(f, Some(&data.handle), x - width, y - height * 2.0, width * 2.0, height * 2.0, None);
        sg::draw_image(
            f,
            Some(&data.handle),
            x - width,
            y,
            width * 2.0,
            height * 2.0,
            Some([0.0, 1.0, 1.0, 0.0]),
        );
    } else {
        sg::draw_image(f, Some(&data.handle), x - width, y - height, width * 2.0, height * 2.0, data.tc);
    }
}

/// `PassiveTreeView:DrawImageRotated` (:1260-1283); `t` = `GetTime() * 0.00003`.
#[allow(clippy::too_many_arguments)]
fn draw_image_rotated(
    f: &mut Frame<'_>,
    handle: &ImageHandle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    angle: f64,
    t: f64,
    show_animations: bool,
) {
    if !show_animations {
        sg::draw_image(f, Some(handle), x - width / 2.0, y - height / 2.0, width, height, None);
        return;
    }
    let rot = angle * t;
    let (hw, hh) = (width / 2.0, height / 2.0);
    let (cos_a, sin_a) = (rot.cos(), rot.sin());
    let xy = [
        x - hw * cos_a + hh * sin_a,
        y - hw * sin_a - hh * cos_a,
        x + hw * cos_a + hh * sin_a,
        y + hw * sin_a - hh * cos_a,
        x + hw * cos_a - hh * sin_a,
        y + hw * sin_a + hh * cos_a,
        x - hw * cos_a - hh * sin_a,
        y - hw * sin_a + hh * cos_a,
    ];
    f.draw_image_quad(Some(handle), xy, None).unwrap_or_else(|e| panic!("{e}"));
}

/// `prepSearch` (:793-807).
fn prep_search(search: &str) -> Vec<String> {
    let lower = search.to_ascii_lowercase();
    let mut words = Vec::new();
    let mut rest = String::new();
    // `search:gmatch('"([^"]*)"')` then the quoted parts are removed
    let mut parts = lower.split('"');
    if let Some(first) = parts.next() {
        rest.push_str(first);
    }
    let parts: Vec<&str> = parts.collect();
    let mut i = 0;
    while i < parts.len() {
        if i + 1 < parts.len() {
            words.push(parts[i].to_string());
            rest.push_str(parts[i + 1]);
            i += 2;
        } else {
            // an unmatched quote stays in the text
            rest.push('"');
            rest.push_str(parts[i]);
            i += 1;
        }
    }
    for w in rest.split(|c: char| c.is_ascii_whitespace()) {
        if !w.is_empty() {
            words.push(w.to_string());
        }
    }
    words
}

/// `haystack:matchOrPattern(need)` (Common.lua): a plain find first, then a Lua pattern when the text is one; a bad pattern is the
/// `PCall` error of `DoesNodeMatchSearchParams` (no match).
fn search_step(haystack: &str, need: &mut Vec<String>) -> Result<(), ()> {
    let mut i = need.len();
    while i > 0 {
        i -= 1;
        match pob_lua::find(haystack.as_bytes(), need[i].as_bytes(), None, false) {
            Ok(Some(_)) => {
                need.remove(i);
            }
            Ok(None) => {}
            Err(_) => return Err(()),
        }
    }
    Ok(())
}

/// `PassiveTreeView:DoesNodeMatchSearchParams` (:1334-1407).
fn node_matches_search(node: &SpecNode, art: &TreeArt, params: &[String]) -> bool {
    let ty = node.ty();
    let has_effects = art.nodes.get(&node.id).is_some_and(|n| n.has_mastery_effects);
    if ty == NodeType::ClassStart || (ty == NodeType::Mastery && !has_effects) {
        return false;
    }
    let mut need = params.to_vec();
    if need[0] == "oil:" {
        if let Some(n) = art.nodes.get(&node.id) {
            for r in &n.recipe {
                if search_step(&r.replace("Oil", "").to_ascii_lowercase(), &mut need).is_err() {
                    return false;
                }
                if need.len() == 1 && need[0] == "oil:" {
                    return true;
                }
            }
        }
        return false;
    }
    if search_step(&node.dn().to_ascii_lowercase(), &mut need).is_err() {
        return false;
    }
    if need.is_empty() {
        return true;
    }
    let mods = node.mods();
    for (index, line) in node.sd().iter().enumerate() {
        if search_step(&line.to_ascii_lowercase(), &mut need).is_err() {
            return false;
        }
        if need.is_empty() {
            return true;
        }
        if let Some(Some(m)) = mods.get(index)
            && let Some(list) = &m.list
        {
            for md in list {
                if search_step(md.name.as_ref(), &mut need).is_err() {
                    return false;
                }
                if need.is_empty() {
                    return true;
                }
            }
        }
    }
    if search_step(&ty.as_str().to_ascii_lowercase(), &mut need).is_err() {
        return false;
    }
    need.is_empty()
}
