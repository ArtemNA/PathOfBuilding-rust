//! The display half of `PassiveTree` (Classes/PassiveTree.lua) that the calc port (`pob-tree`) leaves out on purpose: the assets, the sprite
//! map, the node overlays (:446-497), the connectors (`BuildConnector`/`BuildArc`, :936-1039), and the `pairs` orders of `tree.groups` and
//! `tree.nodes` that the view walks (MINEFIELD D13: number keys, hash order; the same in every process but for the `"root"` key below).

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use pob_gfx::{ImageHandle, LoadFlag};
use pob_tree::lj_table::LjTable;
use pob_tree::{NodeType, PassiveTree, TreeNode};

use crate::lua_data::{LuaKey, LuaTab, LuaVal, parse_return};

/// `tree.assets[name]` / a `spriteMap[name][type]` entry: `{ handle, width, height, [1..4] = texture coordinates }`.
pub struct Asset {
    pub handle: ImageHandle,
    /// 0 while an ASYNC image loads: `DrawAsset` asks again (PassiveTreeView.lua:1290-1295, MINEFIELD U7).
    pub width: Cell<f64>,
    pub height: Cell<f64>,
    pub tc: Option<[f64; 4]>,
}

pub type AssetRef = Rc<Asset>;

/// `PassiveTree:LoadImage` (:909-933), without the download branch (`main.allowTreeDownload` is never set).
fn load_image(root: &str, version: &str, img_name: &str, flags: &[LoadFlag]) -> AssetRef {
    let direct = format!("{root}TreeData/{img_name}");
    let path = if std::path::Path::new(&direct).is_file() {
        direct
    } else {
        format!("{root}TreeData/{version}/{img_name}")
    };
    let handle = ImageHandle::new();
    // `handle:Load` of a missing file leaves an image without size: `DrawAsset` skips it.
    let _ = handle.load(path, flags);
    let (w, h) = handle.image_size();
    Rc::new(Asset {
        handle,
        width: Cell::new(f64::from(w)),
        height: Cell::new(f64::from(h)),
        tc: None,
    })
}

/// `self.nodeOverlay[type]` (:446-492) -> the asset name of an overlay key.
fn overlay_name(ty: NodeType, key: &str, version_num: f64) -> Option<&'static str> {
    let new = version_num >= 3.10;
    Some(match (ty, key) {
        (NodeType::Normal, "alloc") => "PSSkillFrameActive",
        (NodeType::Normal, "path") => "PSSkillFrameHighlighted",
        (NodeType::Normal, "unalloc") => "PSSkillFrame",
        (NodeType::Normal, "allocAscend") if new => "AscendancyFrameSmallAllocated",
        (NodeType::Normal, "allocAscend") => "PassiveSkillScreenAscendancyFrameSmallAllocated",
        (NodeType::Normal, "pathAscend") if new => "AscendancyFrameSmallCanAllocate",
        (NodeType::Normal, "pathAscend") => "PassiveSkillScreenAscendancyFrameSmallCanAllocate",
        (NodeType::Normal, "unallocAscend") if new => "AscendancyFrameSmallNormal",
        (NodeType::Normal, "unallocAscend") => "PassiveSkillScreenAscendancyFrameSmallNormal",
        (NodeType::Notable, "alloc") => "NotableFrameAllocated",
        (NodeType::Notable, "path") => "NotableFrameCanAllocate",
        (NodeType::Notable, "unalloc") => "NotableFrameUnallocated",
        (NodeType::Notable, "allocAscend") if new => "AscendancyFrameLargeAllocated",
        (NodeType::Notable, "allocAscend") => "PassiveSkillScreenAscendancyFrameLargeAllocated",
        (NodeType::Notable, "pathAscend") if new => "AscendancyFrameLargeCanAllocate",
        (NodeType::Notable, "pathAscend") => "PassiveSkillScreenAscendancyFrameLargeCanAllocate",
        (NodeType::Notable, "unallocAscend") if new => "AscendancyFrameLargeNormal",
        (NodeType::Notable, "unallocAscend") => "PassiveSkillScreenAscendancyFrameLargeNormal",
        (NodeType::Notable, "allocBlighted") => "BlightedNotableFrameAllocated",
        (NodeType::Notable, "pathBlighted") => "BlightedNotableFrameCanAllocate",
        (NodeType::Notable, "unallocBlighted") => "BlightedNotableFrameUnallocated",
        (NodeType::Keystone, "alloc" | "allocBlighted") => "KeystoneFrameAllocated",
        (NodeType::Keystone, "path" | "pathBlighted") => "KeystoneFrameCanAllocate",
        (NodeType::Keystone, "unalloc" | "unallocBlighted") => "KeystoneFrameUnallocated",
        (NodeType::Socket, "alloc") => "JewelFrameAllocated",
        (NodeType::Socket, "path") => "JewelFrameCanAllocate",
        (NodeType::Socket, "unalloc") => "JewelFrameUnallocated",
        (NodeType::Socket, "allocAlt") => "JewelSocketAltActive",
        (NodeType::Socket, "pathAlt") => "JewelSocketAltCanAllocate",
        (NodeType::Socket, "unallocAlt") => "JewelSocketAltNormal",
        (NodeType::Mastery, "alloc") => "AscendancyFrameLargeAllocated",
        (NodeType::Mastery, "path") => "AscendancyFrameLargeCanAllocate",
        (NodeType::Mastery, "unalloc") => "AscendancyFrameLargeNormal",
        _ => return None,
    })
}

/// `overlay.size` (`artWidth * 1.33`, :493-497); `nil` for the types without an overlay (the class starts).
pub fn overlay_size(ty: NodeType) -> Option<f64> {
    let art_width = match ty {
        NodeType::Normal => 40.0,
        NodeType::Notable | NodeType::Socket => 58.0,
        NodeType::Keystone => 84.0,
        NodeType::Mastery => 65.0,
        _ => return None,
    };
    Some(art_width * 1.33)
}

pub fn overlay(ty: NodeType, key: &str, version_num: f64) -> Option<&'static str> {
    overlay_name(ty, key, version_num)
}

/// `{ handle, tc }` of the Normal/Intermediate/Active states.
#[derive(Clone)]
pub struct Connector {
    pub ascendancy_name: Option<String>,
    pub node_id1: u32,
    pub node_id2: u32,
    /// `connector.type`: `"LineConnector"` / `"Orbit<o>"`.
    pub ty: String,
    /// `connector.vert[state]`: Normal, Intermediate, Active.
    pub vert: [[f64; 8]; 3],
    /// `connector.c[9..16]`.
    pub tc: [f64; 8],
}

/// What PassiveTree.lua reads for art off a node of `tree.lua`.
#[derive(Default)]
pub struct ArtNode {
    pub icon: Option<Rc<str>>,
    pub active_icon: Option<Rc<str>>,
    pub inactive_icon: Option<Rc<str>>,
    pub active_effect_image: Option<Rc<str>>,
    /// `node.masteryEffects` exists.
    pub has_mastery_effects: bool,
    pub recipe: Vec<String>,
    pub flavour_text: Vec<String>,
    pub out: Vec<u32>,
}

pub struct AscendArt {
    pub id: String,
    pub is_alternate: bool,
    pub flavour_text: Option<String>,
    pub flavour_text_colour: Option<String>,
    pub flavour_rect: Option<(f64, f64)>,
}

/// The display half of one `PassiveTree`.
pub struct TreeArt {
    pub assets: HashMap<String, AssetRef>,
    pub sprite_map: HashMap<String, HashMap<String, AssetRef>>,
    pub connectors: Vec<Connector>,
    pub nodes: HashMap<u32, ArtNode>,
    /// `pairs(tree.groups)` key order.
    pub group_order: Vec<i64>,
    /// `group.oo`: orbit -> true.
    pub group_oo: HashMap<i64, [bool; 4]>,
    /// `pairs(tree.nodes)`, ids (without the removed `root`).
    pub node_order: Vec<u32>,
    pub bloodline_prefix: HashMap<String, String>,
    pub ascendancies: Vec<AscendArt>,
    pub has_class_ascendancies: bool,
    pub size: f64,
    pub version_num: f64,
    /// Every image handle `Load`ed ASYNC (the test harness waits for them, as the oracle does).
    pub async_handles: Vec<ImageHandle>,
}

fn node_key_id(k: &LuaKey) -> Option<u32> {
    match k {
        LuaKey::Num(n) if *n >= 0.0 => Some(*n as u32),
        LuaKey::Str(s) => s.parse().ok(),
        LuaKey::Num(_) => None,
    }
}

fn read_ids(v: Option<&Rc<LuaTab>>) -> Vec<u32> {
    let Some(t) = v else { return Vec::new() };
    // `pairs(node.out or {})` of a list: index order
    t.seq()
        .into_iter()
        .filter_map(|v| match v {
            LuaVal::Num(n) => Some(*n as u32),
            LuaVal::Str(s) => s.parse().ok(),
            _ => None,
        })
        .collect()
}

fn read_strs(v: Option<&Rc<LuaTab>>) -> Vec<String> {
    let Some(t) = v else { return Vec::new() };
    t.seq().into_iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()
}

/// `coords.x`: a number or a table of numbers (`coords.x[#coords.x]`, :315-318).
fn coord(t: &LuaTab, k: &str) -> f64 {
    match t.get(k) {
        Some(LuaVal::Num(n)) => *n,
        Some(LuaVal::Table(l)) => l.seq().last().and_then(|v| v.as_num()).unwrap_or(0.0),
        _ => 0.0,
    }
}

fn sprite(sheet: &ImageHandle, sheet_w: f64, sheet_h: f64, x: f64, y: f64, w: f64, h: f64) -> AssetRef {
    Rc::new(Asset {
        handle: sheet.clone(),
        width: Cell::new(w),
        height: Cell::new(h),
        tc: Some([x / sheet_w, y / sheet_h, (x + w) / sheet_w, (y + h) / sheet_h]),
    })
}

fn read_lua(path: &str) -> Result<Rc<LuaTab>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    match parse_return(&bytes).map_err(|e| format!("{path}: {e}"))? {
        LuaVal::Table(t) => Ok(t),
        _ => Err(format!("{path}: not a table")),
    }
}

/// `pairs` over the number keys of a constructor table: `keys` in source order, `nhash` keyed fields in the constructor.
fn constructor_pairs(keys: &[f64], nhash: usize) -> Vec<f64> {
    let mut t = LjTable::with_constructor_size(nhash);
    for &k in keys {
        t.set(k);
    }
    t.pairs_keys()
}

impl TreeArt {
    /// The art of `tree`, read from `{root}TreeData/{version}/` (`root` = the original's `src/`, UI-ARCH §10 п. 7).
    pub fn load(root: &str, tree: &PassiveTree) -> Result<TreeArt, String> {
        let version = tree.tree_version.as_str();
        let version_num = tree.version_num;
        let src = read_lua(&format!("{root}TreeData/{version}/tree.lua"))?;

        // :234-258 the assets
        let own_assets = src.tab("assets").cloned();
        let assets_tab = match own_assets {
            Some(a) => a,
            None => {
                let t = read_lua(&format!("{root}TreeData/3_19/Assets.lua"))?;
                t.tab("assets").ok_or("3_19/Assets.lua: no assets")?.clone()
            }
        };
        let mut asset_names: Vec<String> = assets_tab
            .fields
            .iter()
            .filter_map(|(k, _)| if let LuaKey::Str(s) = k { Some(s.to_string()) } else { None })
            .collect();
        if src.tab("alternate_ascendancies").is_some() && src.tab("assets").is_none() {
            asset_names.extend(
                [
                    "ClassesPrimalist",
                    "ClassesWarlock",
                    "ClassesWarden",
                    "AzmeriAscendancyMiddle",
                    "AzmeriAscendancyFrameLargeNormal",
                    "AzmeriAscendancyFrameLargeCanAllocate",
                    "AzmeriAscendancyFrameLargeAllocated",
                    "AzmeriAscendancyFrameSmallNormal",
                    "AzmeriAscendancyFrameSmallCanAllocate",
                    "AzmeriAscendancyFrameSmallAllocated",
                    "AzmeriJewelFrameUnallocated",
                    "AzmeriJewelFrameCanAllocate",
                    "AzmeriJewelFrameAllocated",
                    "CharmSocketActiveStr",
                    "CharmSocketActiveInt",
                    "CharmSocketActiveDex",
                ]
                .map(String::from),
            );
        }
        let mut assets: HashMap<String, AssetRef> = HashMap::new();
        let mut async_handles = Vec::new();
        // :262-264 `not name:match("[OL][ri][bn][ie][tC]") and "ASYNC"`: the connector art loads at once (BuildConnector reads its size)
        for name in asset_names {
            let sync = pob_lua::find(name.as_bytes(), b"[OL][ri][bn][ie][tC]", None, false)
                .map_err(|e| e.0)?
                .is_some();
            let a = if sync {
                load_image(root, version, &format!("{name}.png"), &[])
            } else {
                let a = load_image(root, version, &format!("{name}.png"), &[LoadFlag::Async]);
                async_handles.push(a.handle.clone());
                a
            };
            assets.insert(name, a);
        }

        // :266-329 sprite sheets
        let mut sprite_map: HashMap<String, HashMap<String, AssetRef>> = HashMap::new();
        let mut sheets: HashMap<String, (ImageHandle, f64, f64)> = HashMap::new();
        let sprites_file;
        let skill_sprites = match src.tab("skillSprites").or_else(|| src.tab("sprites")) {
            Some(s) => s.clone(),
            None => {
                sprites_file = read_lua(&format!("{root}TreeData/{version}/sprites.lua"))?;
                sprites_file
                    .tab("sprites")
                    .or_else(|| sprites_file.tab("skillSprites"))
                    .ok_or("sprites.lua: no sprites")?
                    .clone()
            }
        };
        let has_zoom_levels = src.tab("imageZoomLevels").is_some();
        for (ty, data) in &skill_sprites.fields {
            let (LuaKey::Str(ty), LuaVal::Table(data)) = (ty, data) else {
                continue;
            };
            let max_zoom: Rc<LuaTab> = if !has_zoom_levels {
                data.clone()
            } else if version_num >= 3.19 {
                let v = data.get_num(0.3835).or_else(|| data.get_num(1.0));
                match v.and_then(LuaVal::as_tab) {
                    Some(t) => t.clone(),
                    None => continue,
                }
            } else {
                match data.seq().last().and_then(|v| v.as_tab()) {
                    Some(t) => t.clone(),
                    None => continue,
                }
            };
            let Some(filename) = max_zoom.str("filename") else { continue };
            let (handle, sw, sh) = sheets
                .entry(filename.to_string())
                .or_insert_with(|| {
                    // `filename:gsub("%?%x+$","")`, `:gsub(".*/","")` from 3.16
                    let no_query = filename.split('?').next().unwrap_or(filename);
                    let name = if version_num >= 3.16 {
                        no_query.rsplit('/').next().unwrap_or(no_query)
                    } else {
                        no_query
                    };
                    let a = load_image(root, version, name, &[LoadFlag::Clamp]);
                    (a.handle.clone(), a.width.get(), a.height.get())
                })
                .clone();
            let Some(coords) = max_zoom.tab("coords") else { continue };
            for (name, c) in &coords.fields {
                let (LuaKey::Str(name), LuaVal::Table(c)) = (name, c) else {
                    continue;
                };
                let s = sprite(&handle, sw, sh, coord(c, "x"), coord(c, "y"), coord(c, "w"), coord(c, "h"));
                sprite_map.entry(name.to_string()).or_default().insert(ty.to_string(), s);
            }
        }

        // :331-370 bloodline sprites
        const BLOODLINES: [(&str, &str); 11] = [
            ("Trialmaster", "trialmasterBloodline"),
            ("Oshabi", "oshabiBloodline"),
            ("Olroth", "olrothBloodline"),
            ("Lycia", "lyciaBloodline"),
            ("KingInTheMists", "kingInTheMistsBloodline"),
            ("Farrul", "farrulBloodline"),
            ("Delirious", "deliriousBloodline"),
            ("Catarina", "catarinaBloodline"),
            ("Breachlord", "breachlordBloodline"),
            ("Aul", "aulBloodline"),
            ("Azmeri", "azmeriBloodline"),
        ];
        const BLOODLINE_ASSETS: [&str; 10] = [
            "AscendancyButton",
            "AscendancyButtonHighlight",
            "AscendancyButtonPressed",
            "AscendancyFrameLargeNormal",
            "AscendancyFrameLargeCanAllocate",
            "AscendancyFrameLargeAllocated",
            "AscendancyFrameSmallNormal",
            "AscendancyFrameSmallCanAllocate",
            "AscendancyFrameSmallAllocated",
            "AscendancyMiddle",
        ];
        let mut bloodline_prefix = HashMap::new();
        for (asc, ty) in BLOODLINES {
            let mut has = false;
            for a in BLOODLINE_ASSETS {
                if let Some(s) = sprite_map.get(a).and_then(|m| m.get(ty)) {
                    assets.insert(format!("{asc}{a}"), s.clone());
                    has = true;
                }
            }
            if has {
                bloodline_prefix.insert(asc.to_string(), asc.to_string());
            }
        }
        // :372-387 `next(spriteSet)`: each `Classes*` sprite set has one sprite type
        if src.tab("alternate_ascendancies").is_some() {
            for (name, set) in &sprite_map {
                if name.starts_with("Classes")
                    && !matches!(name.as_str(), "ClassesPrimalist" | "ClassesWarlock" | "ClassesWarden")
                    && !assets.contains_key(name)
                    && let Some(s) = set.values().next()
                {
                    assets.insert(name.clone(), s.clone());
                }
            }
        }

        // :389-435 legion sheets
        let legion = read_lua(&format!("{root}TreeData/legion/tree-legion.lua"))?;
        let mut legion_sheet = |file: &str| -> (ImageHandle, f64, f64) {
            sheets
                .entry(file.to_string())
                .or_insert_with(|| {
                    let h = ImageHandle::new();
                    let _ = h.load(format!("{root}TreeData/legion/{file}"), &[]);
                    let (w, ht) = h.image_size();
                    (h, f64::from(w), f64::from(ht))
                })
                .clone()
        };
        if let Some(ta) = legion.tab("treeAssets") {
            for data in ta.seq() {
                let Some(data) = data.as_tab() else { continue };
                let (h, sw, sh) = legion_sheet(data.str("filename").unwrap_or(""));
                if let Some(coords) = data.tab("coords") {
                    for (name, c) in &coords.fields {
                        let (LuaKey::Str(name), LuaVal::Table(c)) = (name, c) else {
                            continue;
                        };
                        assets.insert(
                            name.to_string(),
                            sprite(&h, sw, sh, coord(c, "x"), coord(c, "y"), coord(c, "w"), coord(c, "h")),
                        );
                    }
                }
            }
        }
        for (ty, data) in &legion.fields {
            let (LuaKey::Str(ty), LuaVal::Table(data)) = (ty, data) else {
                continue;
            };
            if &**ty == "treeAssets" {
                continue;
            }
            let Some(max_zoom) = data.seq().last().and_then(|v| v.as_tab()).cloned() else {
                continue;
            };
            let (h, sw, sh) = legion_sheet(max_zoom.str("filename").unwrap_or(""));
            if let Some(coords) = max_zoom.tab("coords") {
                for (name, c) in &coords.fields {
                    let (LuaKey::Str(name), LuaVal::Table(c)) = (name, c) else {
                        continue;
                    };
                    let s = sprite(&h, sw, sh, coord(c, "x"), coord(c, "y"), coord(c, "w"), coord(c, "h"));
                    sprite_map.entry(name.to_string()).or_default().insert(ty.to_string(), s);
                }
            }
        }

        // the orders
        let groups = src.tab("groups").ok_or("tree.lua: no groups")?;
        let mut group_oo = HashMap::new();
        let mut group_keys = Vec::new();
        for (k, v) in &groups.fields {
            let (LuaKey::Num(k), LuaVal::Table(g)) = (k, v) else { continue };
            group_keys.push(*k);
            let mut oo = [false; 4];
            if let Some(orbits) = g.tab("orbits") {
                // :503-506 `group.oo[orbit] = true`
                for o in orbits.seq() {
                    if let Some(o) = o.as_num()
                        && (0.0..4.0).contains(&o)
                    {
                        oo[o as usize] = true;
                    }
                }
            } else if let Some(t) = g.tab("oo") {
                for (i, slot) in oo.iter_mut().enumerate() {
                    *slot = t.get_num(i as f64).is_some() || t.get(&i.to_string()).is_some();
                }
            }
            group_oo.insert(*k as i64, oo);
        }
        let group_order: Vec<i64> = constructor_pairs(&group_keys, groups.fields.len())
            .into_iter()
            .map(|k| k as i64)
            .collect();

        let nodes_src = src.tab("nodes").ok_or("tree.lua: no nodes")?;
        let mut nodes = HashMap::new();
        let mut node_keys = Vec::new();
        for (k, v) in &nodes_src.fields {
            let LuaVal::Table(n) = v else { continue };
            // :510 `self.nodes.root = nil` -- the string key keeps one hash slot whose place differs per process (R13): not modelled
            if matches!(k, LuaKey::Str(s) if &**s == "root") {
                continue;
            }
            let Some(id) = n.num("skill").or_else(|| n.num("id")).map(|i| i as u32).or_else(|| node_key_id(k)) else {
                continue;
            };
            if let LuaKey::Num(key) = k {
                node_keys.push((*key, id));
            }
            nodes.insert(
                id,
                ArtNode {
                    icon: n.str("icon").map(Rc::from),
                    active_icon: n.str("activeIcon").map(Rc::from),
                    inactive_icon: n.str("inactiveIcon").map(Rc::from),
                    active_effect_image: n.str("activeEffectImage").map(Rc::from),
                    has_mastery_effects: n.get("masteryEffects").is_some(),
                    recipe: read_strs(n.tab("recipe")),
                    flavour_text: read_strs(n.tab("flavourText")),
                    out: read_ids(n.tab("out")),
                },
            );
        }
        let keys: Vec<f64> = node_keys.iter().map(|(k, _)| *k).collect();
        let id_of: HashMap<u64, u32> = node_keys.iter().map(|(k, id)| (k.to_bits(), *id)).collect();
        let node_order: Vec<u32> = constructor_pairs(&keys, nodes_src.fields.len())
            .into_iter()
            .filter_map(|k| id_of.get(&k.to_bits()).copied())
            .collect();

        // the ascendancies (flavour text, PassiveTreeView.lua:588-634)
        let mut ascendancies = Vec::new();
        let read_asc = |a: &LuaTab, is_alternate: bool| AscendArt {
            id: a.str("id").unwrap_or("").to_string(),
            is_alternate,
            flavour_text: a.str("flavourText").map(str::to_owned),
            flavour_text_colour: a.str("flavourTextColour").map(str::to_owned),
            flavour_rect: a
                .tab("flavourTextRect")
                .map(|r| (r.num("x").unwrap_or(0.0), r.num("y").unwrap_or(0.0))),
        };
        let mut has_class_ascendancies = false;
        if let Some(cl) = src.tab("classes") {
            let list = cl.seq();
            has_class_ascendancies = list
                .first()
                .and_then(|c| c.as_tab())
                .is_some_and(|c| c.get("ascendancies").is_some());
            for c in list {
                if let Some(asc) = c.as_tab().and_then(|c| c.tab("ascendancies")) {
                    for a in asc.seq() {
                        if let Some(a) = a.as_tab() {
                            ascendancies.push(read_asc(a, false));
                        }
                    }
                }
            }
        }
        if let Some(alt) = src.tab("alternate_ascendancies") {
            for a in alt.seq() {
                if let Some(a) = a.as_tab() {
                    ascendancies.push(read_asc(a, true));
                }
            }
        }

        // :116
        let num = |k: &str| src.num(k).unwrap_or(0.0);
        let size = (num("max_x") - num("min_x")).min(num("max_y") - num("min_y")) * 1.1;

        let mut art = TreeArt {
            assets,
            sprite_map,
            connectors: Vec::new(),
            nodes,
            group_order,
            group_oo,
            node_order,
            bloodline_prefix,
            ascendancies,
            has_class_ascendancies,
            size,
            version_num,
            async_handles,
        };
        art.build_tree_connectors(tree);
        Ok(art)
    }

    /// :626-653: `pairs(self.nodes)`, `pairs(node.out)`.
    fn build_tree_connectors(&mut self, tree: &PassiveTree) {
        let mut connectors = Vec::new();
        for id in &self.node_order {
            let Some(node) = tree.node(*id) else { continue };
            let Some(art) = self.nodes.get(id) else { continue };
            for other_id in &art.out {
                let Some(other) = tree.node(*other_id) else { continue };
                let group_proxy = |n: &TreeNode| n.group.and_then(|g| tree.groups.get(&g)).is_some_and(|g| g.is_proxy);
                if node.ty != NodeType::ClassStart
                    && other.ty != NodeType::ClassStart
                    && node.ty != NodeType::Mastery
                    && other.ty != NodeType::Mastery
                    && node.ascendancy_name == other.ascendancy_name
                    && !node.is_proxy
                    && !other.is_proxy
                    // `not node.group.isProxy and not node.group.isProxy` (twice `node`): dropped for a group-less node, which has no art
                    && !group_proxy(node)
                    && node.group.is_some()
                {
                    let group_xy = |n: &TreeNode| n.group.and_then(|g| tree.groups.get(&g)).map_or((0.0, 0.0), |g| (g.x, g.y));
                    self.build_connector(node, other, group_xy(node), group_xy(other), &mut connectors);
                }
            }
        }
        self.connectors = connectors;
    }

    fn art_size(&self, name: &str) -> (f64, f64) {
        self.assets.get(name).map_or((0.0, 0.0), |a| (a.width.get(), a.height.get()))
    }

    /// `PassiveTree:BuildConnector` (:936-997) over two nodes and the positions of their groups.
    pub fn build_connector(&self, n1: &TreeNode, n2: &TreeNode, g1: (f64, f64), g2: (f64, f64), out: &mut Vec<Connector>) {
        use std::f64::consts::PI;
        let (id1, id2) = (n1.id, n2.id);
        let asc = n1.ascendancy_name.clone();
        if n1.g == n2.g && n1.o == n2.o {
            let (mut a, mut b) = (n1, n2);
            if a.angle.unwrap_or(0.0) > b.angle.unwrap_or(0.0) {
                std::mem::swap(&mut a, &mut b);
            }
            let mut arc_angle = b.angle.unwrap_or(0.0) - a.angle.unwrap_or(0.0);
            if arc_angle >= PI {
                std::mem::swap(&mut a, &mut b);
                arc_angle = PI * 2.0 - arc_angle;
            }
            let ga = if std::ptr::eq(a, n1) { g1 } else { g2 };
            if arc_angle <= PI {
                let mut second = None;
                if arc_angle > PI / 2.0 {
                    arc_angle /= 2.0;
                    second = Some(self.build_arc(arc_angle, a, ga, id1, id2, asc.clone(), true));
                }
                out.push(self.build_arc(arc_angle, a, ga, id1, id2, asc, false));
                if let Some(s) = second {
                    out.push(s);
                }
                return;
            }
        }
        // :979-996 a straight line
        let (art_w, art_h) = self.art_size("LineConnectorNormal");
        let (x1, y1, x2, y2) = (n1.x.unwrap_or(0.0), n1.y.unwrap_or(0.0), n2.x.unwrap_or(0.0), n2.y.unwrap_or(0.0));
        let (vx, vy) = (x2 - x1, y2 - y1);
        let dist = (vx * vx + vy * vy).sqrt();
        let scale = art_h * 1.33 / dist;
        let (nx, ny) = (vx * scale, vy * scale);
        let end_s = dist / (art_w * 1.33);
        let v = [x1 - ny, y1 + nx, x1 + ny, y1 - nx, x2 + ny, y2 - nx, x2 - ny, y2 + nx];
        out.push(Connector {
            ascendancy_name: asc,
            node_id1: id1,
            node_id2: id2,
            ty: "LineConnector".into(),
            vert: [v, v, v],
            tc: [0.0, 1.0, 0.0, 0.0, end_s, 0.0, end_s, 1.0],
        });
    }

    /// `PassiveTree:BuildArc` (:999-1039).
    #[allow(clippy::too_many_arguments)]
    fn build_arc(
        &self,
        arc_angle: f64,
        n1: &TreeNode,
        g: (f64, f64),
        id1: u32,
        id2: u32,
        asc: Option<String>,
        mirrored: bool,
    ) -> Connector {
        use std::f64::consts::{FRAC_PI_4, PI, SQRT_2};
        let ty = format!("Orbit{}", pob_lua::tostring_num(n1.o.unwrap_or(0.0)));
        let clip_angle = FRAC_PI_4 - arc_angle / 2.0;
        let p = 1.0 - clip_angle.tan().max(0.0);
        let mut angle = n1.angle.unwrap_or(0.0) - clip_angle;
        if mirrored {
            angle += arc_angle;
        }
        let mut vert = [[0.0; 8]; 3];
        for (i, state) in ["Normal", "Intermediate", "Active"].iter().enumerate() {
            let (art_w, _) = self.art_size(&format!("{ty}{state}"));
            let size = art_w * 2.0 * 1.33;
            // `m_sqrt(2)` as written; `PI / 4` = `m_pi / 4`
            let (ox, oy) = (size * SQRT_2 * (angle + PI / 4.0).sin(), size * SQRT_2 * -(angle + PI / 4.0).cos());
            let (cx, cy) = (g.0 + ox, g.1 + oy);
            let mut v = [
                g.0,
                g.1,
                cx + (size * angle.sin() - ox) * p,
                cy + (size * -angle.cos() - oy) * p,
                cx,
                cy,
                cx + (size * angle.cos() - ox) * p,
                cy + (size * angle.sin() - oy) * p,
            ];
            if mirrored {
                v.swap(2, 6);
                v.swap(3, 7);
            }
            vert[i] = v;
        }
        Connector {
            ascendancy_name: asc,
            node_id1: id1,
            node_id2: id2,
            ty,
            vert,
            tc: [1.0, 1.0, 0.0, p, 0.0, 0.0, p, 0.0],
        }
    }
}
