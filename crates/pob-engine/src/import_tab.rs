//! What `Classes/ImportTab.lua` does to a build with the JSON of a character (`ImportPassiveTreeAndJewels` :1131-1253, `ImportItemsAndSkills` :1381-1491, `ImportItem` :1497-1821,
//! `ImportSocketedItems` :1823-1898). The steps are cut where the Lua touches a control or an undo list: the tab (`pob_ui::tabs::import`) runs them in order and does its part between them.
//! Like every mutator of the engine they write the model and the live tables and do not recalculate, unless the Lua reads the new state in the same frame. Gate: `tests/import_tab.rs`
//! (path equivalence), `pob-ui/tests/import.rs` (= the original on the same synthetic JSON).

use std::rc::Rc;

use pob_calc::skills_tab::{GemInstance, SkillsTab, SocketGroup, process_socket_group};
use pob_data::gems::gems;
use pob_data::item_bases::item_bases;
use pob_items::item::{INFLUENCE_INFO, Item, ModLine, Socket};
use serde_json::Value as Json;

use crate::engine::{Engine, active_spec_mut};

/// :1340
const GUARD_ITEM_SET: &str = "Animate Guardian";
/// :1255
const REIMPORT_KEY_SEPARATOR: &str = "\x1f";

/// :1493 `rarityMap`.
fn rarity_of(frame_type: f64) -> Option<&'static str> {
    Some(match frame_type as i64 {
        0 => "NORMAL",
        1 => "MAGIC",
        2 => "RARE",
        3 => "UNIQUE",
        9 | 10 => "RELIC",
        _ => return None,
    })
}

/// :1494-1495 `slotMap`.
fn slot_of_inventory(id: &str) -> Option<&'static str> {
    Some(match id {
        "Weapon" => "Weapon 1",
        "Offhand" => "Weapon 2",
        "Weapon2" => "Weapon 1 Swap",
        "Offhand2" => "Weapon 2 Swap",
        "Helm" => "Helmet",
        "BodyArmour" => "Body Armour",
        "Gloves" => "Gloves",
        "Boots" => "Boots",
        "Amulet" => "Amulet",
        "Ring" => "Ring 1",
        "Ring2" => "Ring 2",
        "Ring3" => "Ring 3",
        "Belt" => "Belt",
        "BrequelGrafts" => "Graft 1",
        "BrequelGrafts2" => "Graft 2",
        _ => return None,
    })
}

/// :1595-1608 `catalystMap`.
fn catalyst_of(name: &str) -> Option<f64> {
    Some(match name {
        "Attack" => 1.0,
        "Speed" => 2.0,
        "Suffix" => 3.0,
        "Life and Mana" => 4.0,
        "Caster" => 5.0,
        "Attribute" => 6.0,
        "Physical and Chaos Damage" => 7.0,
        "Resistance" => 8.0,
        "Prefix" => 9.0,
        "Defense" => 10.0,
        "Elemental Damage" => 11.0,
        "Critical" => 12.0,
        _ => return None,
    })
}

/// :1778-1791 `foilVariants`.
const FOIL_VARIANTS: [&str; 12] = [
    "Amethyst",
    "Verdant",
    "Ruby",
    "Cobalt",
    "Sunset",
    "Aureate",
    "Celestial Quartz",
    "Celestial Ruby",
    "Celestial Emerald",
    "Celestial Aureate",
    "Celestial Pearl",
    "Celestial Amethyst",
];

/// `value.subgraph.groups[groupId]` of `spec.jewel_data` (ImportTab.lua:1139-1142, PassiveSpec.lua:1858-1868).
struct ImportedGroup {
    id: String,
    proxy: Option<String>,
    nodes: Vec<String>,
}

/// `value.subgraph.nodes[nodeId]`.
struct ImportedNode {
    id: String,
    group: Option<String>,
    orbit_index: Option<f64>,
    is_keystone: bool,
    is_mastery: bool,
}

/// `spec.jewel_data`, `spec.extended_hashes`. Upstream asks them inside `BuildSubgraph`; `pob-tree` has that branch as "no import" (`add_to_allocated_subgraph_nodes`), so here the graphs are built and the nodes
/// `addToAllocatedSubgraphNodes` would pick go to `allocSubgraphNodes` for the next build, until nothing is new: the same set of nodes. A later rebuild in the session (another jewel in a socket) does not ask them again.
#[derive(Default)]
struct ImportedSubgraphs {
    groups: Vec<ImportedGroup>,
    nodes: Vec<ImportedNode>,
    extended_hashes: Vec<f64>,
}

impl ImportedSubgraphs {
    /// :1139-1142, :1152, PassiveSpec.lua:1856-1868
    fn of(passives: &Json) -> Self {
        let mut im = Self {
            extended_hashes: arr(&passives["hashes_ex"]).iter().filter_map(tonumber).collect(),
            ..Self::default()
        };
        for v in passives["jewel_data"].as_object().into_iter().flat_map(serde_json::Map::values) {
            let sub = &v["subgraph"];
            if !truthy(sub) {
                continue;
            }
            for (id, g) in sub["groups"].as_object().into_iter().flatten() {
                im.groups.retain(|x| x.id != *id);
                im.groups.push(ImportedGroup {
                    id: id.clone(),
                    proxy: g["proxy"].as_str().map(str::to_owned),
                    nodes: arr(&g["nodes"]).iter().filter_map(|n| n.as_str().map(str::to_owned)).collect(),
                });
            }
            for (id, n) in sub["nodes"].as_object().into_iter().flatten() {
                im.nodes.retain(|x| x.id != *id);
                im.nodes.push(ImportedNode {
                    id: id.clone(),
                    group: n["group"].as_str().map(str::to_owned),
                    orbit_index: tonumber(&n["orbitIndex"]),
                    is_keystone: truthy(&n["isKeystone"]),
                    is_mastery: truthy(&n["isMastery"]),
                });
            }
        }
        im
    }

    /// PassiveSpec.lua:2070-2093 `addToAllocatedSubgraphNodes(node)`. R13: `pairs(importedNodes)` over string keys; a group holds one node per orbit index.
    fn picks(&self, proxy: Option<&str>, keystone: bool, oidx: Option<f64>) -> bool {
        let Some(group) = proxy.and_then(|p| self.groups.iter().find(|g| g.proxy.as_deref() == Some(p))) else {
            return false;
        };
        for data in self.nodes.iter().filter(|d| d.group.as_deref() == Some(group.id.as_str())) {
            let matches = (keystone && data.is_keystone) || (oidx.is_some() && oidx == data.orbit_index && !data.is_mastery);
            if matches {
                return group
                    .nodes
                    .iter()
                    .any(|ext| *ext == data.id && pob_lua::tonumber(&data.id).is_some_and(|id| self.extended_hashes.contains(&id)));
            }
        }
        false
    }

    /// `spec:BuildClusterJewelGraphs()` with the import's data.
    fn build_graphs(&self, spec: &mut pob_tree::PassiveSpec, lookup: pob_tree::JewelLookup<'_>) -> Result<(), String> {
        spec.build_cluster_jewel_graphs(lookup).map_err(|e| e.0)?;
        loop {
            let mut new = Vec::new();
            // `sub_graph_views` is what `pob-tree` shows of a graph: its links, the last one `(entranceNode, parentSocket)` (:2340-2345); a keystone's graph is one node with no links of its own
            for sg in spec.sub_graph_views() {
                let (parent, members): (Option<usize>, Vec<usize>) = match sg.links.last() {
                    Some(&(_, parent)) => {
                        let mut m: Vec<usize> = sg.links.iter().flat_map(|l| [l.0, l.1]).filter(|i| *i != parent).collect();
                        m.sort_unstable();
                        m.dedup();
                        (Some(parent), m)
                    }
                    None => match spec.node_idx(sg.id) {
                        Some(i) => (spec.nodes[i].linked.first().copied(), vec![i]),
                        None => (None, Vec::new()),
                    },
                };
                let Some(parent) = parent else { continue };
                let proxy = spec.nodes[parent]
                    .tree_node()
                    .expansion_jewel
                    .as_ref()
                    .and_then(|e| e.proxy.as_deref());
                for idx in members {
                    let n = &spec.nodes[idx];
                    if !n.alloc && self.picks(proxy, n.ty() == pob_tree::NodeType::Keystone, n.tree_node().oidx) {
                        new.push(f64::from(n.id));
                    }
                }
            }
            if new.is_empty() {
                return Ok(());
            }
            spec.alloc_subgraph_nodes.extend(new);
            spec.build_cluster_jewel_graphs(lookup).map_err(|e| e.0)?;
        }
    }
}

/// Lua truth of a decoded JSON value.
fn truthy(v: &Json) -> bool {
    !v.is_null() && *v != Json::Bool(false)
}

/// `json_null` and a missing field are both nil for dkjson's caller here.
fn arr(v: &Json) -> &[Json] {
    v.as_array().map_or(&[], Vec::as_slice)
}

/// `tonumber(x)` of a value that may be a JSON number or a string.
fn tonumber(v: &Json) -> Option<f64> {
    match v {
        Json::Number(n) => n.as_f64(),
        Json::String(s) => pob_lua::tonumber(s),
        _ => None,
    }
}

fn lua_sub_gsub(s: &str, pat: &str, repl: &str) -> String {
    let out = pob_lua::gsub(s.as_bytes(), pat.as_bytes(), pob_lua::Repl::Str(repl.as_bytes()), None)
        .expect("pattern")
        .0;
    String::from_utf8_lossy(&out).into_owned()
}

/// `property.values[1][1]`.
fn first_value(p: &Json) -> &Json {
    &p["values"][0][0]
}

/// `tonumber(property.values[1][1]:match("%d+"))`.
fn first_digits(p: &Json) -> Option<f64> {
    let s = first_value(p).as_str()?;
    let start = s.find(|c: char| c.is_ascii_digit())?;
    let end = s[start..].find(|c: char| !c.is_ascii_digit()).map_or(s.len(), |e| start + e);
    pob_lua::tonumber(&s[start..end])
}

/// `line:gmatch("[^\n]+")`.
fn lines_of(s: &str) -> impl Iterator<Item = &str> {
    s.split('\n').filter(|l| !l.is_empty())
}

fn mod_line(line: &str, flag: &[(&str, bool)]) -> ModLine {
    let mut m = ModLine {
        line: line.to_owned(),
        ..ModLine::default()
    };
    for (k, v) in flag {
        if *v {
            m.set_flag_pub(k);
        }
    }
    m
}

/// A plain list of strings (`enchantMods`, `scourgeMods`, `fracturedMods`, ...).
fn push_lines(out: &mut Vec<ModLine>, list: &Json, flag: &str) {
    for l in arr(list) {
        for line in lines_of(l.as_str().unwrap_or("")) {
            out.push(mod_line(line, &[(flag, true)]));
        }
    }
}

/// :1254-1269 `getSocketGroupReimportKey`.
fn reimport_key(g: &SocketGroup) -> String {
    let names: Vec<String> = g
        .gem_list
        .iter()
        .map(|x| x.name_spec.as_deref().unwrap_or("").to_lowercase())
        .collect();
    [
        g.slot.as_deref().unwrap_or(""),
        g.source.as_deref().unwrap_or(""),
        &g.gem_list.len().to_string(),
        &names.join(REIMPORT_KEY_SEPARATOR),
    ]
    .join(REIMPORT_KEY_SEPARATOR)
}

/// :1271-1303 `snapshotSocketGroupReimportState`: the fields a re-import keeps.
#[derive(Clone)]
struct GroupState {
    group: SocketGroup,
    is_main: bool,
}

/// :1305-1338
fn apply_group_state(g: &mut SocketGroup, st: &GroupState) {
    let s = &st.group;
    g.enabled = s.enabled;
    g.include_in_full_dps = s.include_in_full_dps;
    g.group_count = s.group_count;
    g.label.clone_from(&s.label);
    g.main_active_skill = s.main_active_skill;
    g.main_active_skill_calcs = s.main_active_skill_calcs;
    for (gem, old) in g.gem_list.iter_mut().zip(&s.gem_list) {
        gem.enabled = old.enabled;
        gem.count = old.count;
        gem.skill_part = old.skill_part;
        gem.skill_part_calcs = old.skill_part_calcs;
        gem.skill_stage_count = old.skill_stage_count;
        gem.skill_stage_count_calcs = old.skill_stage_count_calcs;
        gem.skill_mine_count = old.skill_mine_count;
        gem.skill_mine_count_calcs = old.skill_mine_count_calcs;
        gem.skill_minion.clone_from(&old.skill_minion);
        gem.skill_minion_calcs.clone_from(&old.skill_minion_calcs);
        gem.skill_minion_item_set = old.skill_minion_item_set;
        gem.skill_minion_item_set_calcs = old.skill_minion_item_set_calcs;
        gem.skill_minion_skill = old.skill_minion_skill;
        gem.skill_minion_skill_calcs = old.skill_minion_skill_calcs;
        gem.enable_global1 = old.enable_global1;
        gem.enable_global2 = old.enable_global2;
    }
}

/// `gem.grantedEffect` of a live gem: the name, when it is not a support. The field itself, as the Lua reads it: `ProcessSocketGroup` sets it only on a gem of a granted skill, a gem with `gemData` has none.
fn active_effect_name(gem: &GemInstance, skills: &pob_data::skills::Skills) -> Option<String> {
    let ge = skills.get(gem.granted_effect.as_ref()?)?;
    (!ge.support).then(|| ge.name.clone())
}

/// What `ImportItemsAndSkills` needs from the first part to the second (`skillOrder`, `preservedSocketGroupStateByKey`).
#[derive(Default)]
pub struct SkillsCarry {
    main_skill_empty: bool,
    skill_order: Option<Vec<String>>,
    preserved: Vec<(String, Vec<GroupState>)>,
}

/// What the tab needs of `ImportPassiveTreeAndJewels` after the tree went in (:1199-1251).
#[derive(Clone, Debug, PartialEq)]
pub struct TreeImportOut {
    /// `charData.level or 100`.
    pub level: f64,
    /// `charData.level` for `SetText`.
    pub level_text: Option<String>,
    /// `resistancePenaltyIndex` (:1218-1226).
    pub resistance_penalty_index: usize,
    /// `bandit`, `majorGod`, `minorGod`.
    pub bandit: String,
    pub major_god: String,
    pub minor_god: String,
    /// `charData.name, charData.class, charData.league` of `SetWindowTitleSubtext`.
    pub title: (String, String, String),
    /// `self.treeVersion` changed (`treeTab.showConvert`).
    pub version_changed: bool,
}

impl Engine {
    /// `self.build.latestTree`.
    fn latest_tree(&self) -> Result<Rc<pob_tree::tree::PassiveTree>, String> {
        let v = pob_data::tree::tree_versions().map_err(|e| e.0)?.latest_tree_version.clone();
        pob_tree::tree::load_passive_tree_mode(&v, pob_calc::ParseMode::Full).map_err(|e| e.0)
    }

    fn slot_exists(&self, name: &str) -> Result<Option<u32>, String> {
        // `self.build.itemsTab.slots[slotName]`: the gear slots and the sockets of the latest tree (ItemsTab.lua:72-87)
        if let Some(rest) = name.strip_prefix("Jewel ")
            && let Some(id) = pob_lua::tonumber(rest).and_then(pob_tree::tree::as_id)
        {
            let latest = self.latest_tree()?;
            if latest.sockets.contains_key(&id) {
                return Ok(Some(id));
            }
            return Err(format!("Classes/ImportTab.lua:1818: attempt to index a nil value (slot '{name}')"));
        }
        if self.result().env.ordered_slots.iter().any(|s| s.slot_name == name) {
            return Ok(None);
        }
        Err(format!("Classes/ImportTab.lua:1818: attempt to index a nil value (slot '{name}')"))
    }

    /// `ImportTab:ImportItem(itemData, slotName, ignoreWeaponSwap, itemSetId)` (:1497-1821). `display`: `displayGroup` of the skills tab, what `gemChangeFunc` writes to.
    #[allow(clippy::too_many_lines)]
    pub fn import_item(
        &mut self,
        item_data: &Json,
        slot_name: Option<&str>,
        ignore_swap: bool,
        item_set: Option<f64>,
        display: Option<usize>,
    ) -> Result<(), String> {
        let inventory = item_data["inventoryId"].as_str();
        let slot_name: Option<String> = match slot_name {
            Some(s) => Some(s.to_owned()),
            None => {
                let x = tonumber(&item_data["x"]);
                if inventory == Some("PassiveJewels") {
                    let latest = self.latest_tree()?;
                    let slots = latest.json.jewel_slots.as_deref().unwrap_or(&[]);
                    let x = x.ok_or("Classes/ImportTab.lua:1500: attempt to perform arithmetic on field 'x' (a nil value)")?;
                    let id = slots
                        .get(x as usize)
                        .ok_or("Classes/ImportTab.lua:1500: attempt to concatenate a nil value")?;
                    Some(format!("Jewel {}", pob_lua::tostring_num(*id)))
                } else if inventory == Some("Flask") {
                    let x = x.ok_or("Classes/ImportTab.lua:1502: attempt to perform arithmetic on field 'x' (a nil value)")?;
                    Some(format!("Flask {}", pob_lua::tostring_num(x + 1.0)))
                } else if !(ignore_swap && matches!(inventory, Some("Weapon2" | "Offhand2"))) {
                    inventory.and_then(slot_of_inventory).map(str::to_owned)
                } else {
                    None
                }
            }
        };
        // :1507-1510 any item that won't go into a known slot
        let Some(slot_name) = slot_name else { return Ok(()) };

        let mut explicit_mods: Vec<Json> = arr(&item_data["explicitMods"]).to_vec();
        let mut no_implicits = false;
        let mut item = Item::default();
        // :1515
        let mut rarity = tonumber(&item_data["frameType"]).and_then(rarity_of);
        let name_of = |k: &str| item_data[k].as_str().unwrap_or("");
        let sanitise = pob_build::sanitise_text;
        if !name_of("name").is_empty() {
            let title = sanitise(name_of("name"));
            let mut base_name = sanitise(name_of("typeLine")).replace("Synthesised ", "");
            base_name = lua_sub_gsub(&base_name, "^Vestigial ", "");
            item.name = format!("{title}, {base_name}");
            item.title = Some(title);
            if base_name == "Two-Toned Boots" {
                base_name = "Two-Toned Boots (Armour/Energy Shield)".to_owned();
            }
            item.base = item_bases().base(&base_name);
            item.ty = item.base.map(|b| b.ty.clone());
            item.base_name = Some(base_name);
        } else {
            item.name = sanitise(name_of("typeLine"));
            if item.name.contains("Energy Blade") {
                let rules = pob_data::rules::rules();
                let one_handed = arr(&item_data["properties"]).iter().any(|p| {
                    p["name"]
                        .as_str()
                        .and_then(|n| rules.weapon_type_info.0.get(n))
                        .is_some_and(|w| w.one_hand == Some(true))
                });
                item.name = if one_handed {
                    "Energy Blade One Handed"
                } else {
                    "Energy Blade Two Handed"
                }
                .to_owned();
                rarity = Some("NORMAL");
                no_implicits = true;
                explicit_mods.clear();
            }
            // :1545-1554 `pairs(itemBases)`: the first base inside the name (R13). A name holds its base once, and prefix + base + suffix gives back the name whichever one wins
            // (`BuildRaw`); the sorted walk is that of `Item:ParseRaw`.
            for base_name in item_bases().names_sorted() {
                if let Some(s) = item.name.find(base_name) {
                    let e = s + base_name.len();
                    item.name_prefix = item.name[..s].to_owned();
                    item.name_suffix = item.name[e..].to_owned();
                    item.base_name = Some(base_name.to_owned());
                    break;
                }
            }
            if item.base_name.is_none()
                && let Some(s) = item.name.find("Two-Toned Boots")
            {
                item.name_prefix = item.name[..s].to_owned();
                item.name_suffix = item.name[s + "Two-Toned Boots".len()..].to_owned();
                item.base_name = Some("Two-Toned Boots (Armour/Energy Shield)".to_owned());
            }
            item.base = item.base_name.as_deref().and_then(|n| item_bases().base(n));
            item.ty = item.base.map(|b| b.ty.clone());
        }
        // :1567-1569
        let (Some(base), Some(rarity)) = (item.base, rarity) else {
            return Ok(());
        };
        item.rarity = rarity.to_owned();

        // :1572-1589
        item.unique_id = item_data["id"].as_str().map(str::to_owned);
        if truthy(&item_data["influences"]) {
            for (i, (_, display)) in INFLUENCE_INFO.iter().enumerate() {
                item.influence[i] = truthy(&item_data["influences"][display.to_lowercase()]);
            }
        }
        let influence_index = |key: &str| INFLUENCE_INFO.iter().position(|(k, _)| *k == key).expect("influence");
        if truthy(&item_data["searing"]) {
            item.influence[influence_index("cleansing")] = true;
        }
        if truthy(&item_data["tangled"]) {
            item.influence[influence_index("tangle")] = true;
        }
        let ilvl = tonumber(&item_data["ilvl"]).ok_or("Classes/ImportTab.lua:1584: attempt to compare number with nil")?;
        if ilvl > 0.0 {
            item.item_level = Some(ilvl);
        }
        if base.weapon.is_some() || base.armour.is_some() || base.flask.is_some() || base.tincture.is_some() {
            item.quality = Some(0.0);
        }
        // :1590-1639
        for p in arr(&item_data["properties"]) {
            let name = p["name"].as_str().unwrap_or("");
            if name == "Quality" {
                item.quality = first_digits(p);
            } else if name.contains("Quality (") {
                let kind = name
                    .strip_prefix("Quality (")
                    .or_else(|| name.split_once("Quality (").map(|x| x.1))
                    .and_then(|r| r.strip_suffix(" Modifiers)").or_else(|| r.split_once(" Modifiers)").map(|x| x.0)));
                item.catalyst = kind.and_then(catalyst_of);
                item.catalyst_quality = first_digits(p);
            } else if name == "Radius" {
                item.jewel_radius_label = first_value(p).as_str().map(str::to_owned);
            } else if name == "Limited to" {
                item.limit = tonumber(first_value(p));
            } else if name == "Evasion Rating" {
                if item.base_name.as_deref() == Some("Two-Toned Boots (Armour/Energy Shield)") {
                    item.base_name = Some("Two-Toned Boots (Armour/Evasion)".to_owned());
                    item.base = item_bases().base("Two-Toned Boots (Armour/Evasion)");
                }
            } else if name == "Energy Shield" {
                if item.base_name.as_deref() == Some("Two-Toned Boots (Armour/Evasion)") {
                    item.base_name = Some("Two-Toned Boots (Evasion/Energy Shield)".to_owned());
                    item.base = item_bases().base("Two-Toned Boots (Evasion/Energy Shield)");
                }
            } else if name.contains("Intangibility") {
                item.intangibility = first_digits(p);
            } else if name == "Memory Strands" {
                item.memory_strands = tonumber(first_value(p));
            }
            if matches!(name, "Energy Shield" | "Ward" | "Armour" | "Evasion Rating") {
                let key = name.replace(" Rating", "").replace(' ', "");
                let ad = item.armour_data.get_or_insert_with(Default::default);
                for value in arr(&p["values"]) {
                    let v = tonumber(&value[0]).ok_or("Classes/ImportTab.lua:1635: attempt to perform arithmetic on a nil value")?;
                    let cur = ad.num(&key).unwrap_or(0.0);
                    ad.set(&key, pob_mods::Value::Num(cur + v));
                }
            }
        }
        // :1640-1645
        item.split = truthy(&item_data["split"]);
        item.mirrored = truthy(&item_data["duplicated"]) || truthy(&item_data["mirrored"]);
        item.corrupted = truthy(&item_data["corrupted"]);
        item.fractured = truthy(&item_data["fractured"]);
        item.synthesised = truthy(&item_data["synthesised"]);
        item.vestigial = truthy(&item_data["vestigial"]);
        // :1646-1657
        let sockets = arr(&item_data["sockets"]);
        if !sockets.is_empty() {
            let mut abyssal = None;
            for s in sockets {
                let color = s["sColour"].as_str().unwrap_or("");
                if color == "A" && abyssal.is_none() {
                    // `item.abyssalSocketCount or 0 + 1`
                    abyssal = Some(1);
                }
                item.sockets.push(Socket {
                    color: color.to_owned(),
                    group: tonumber(&s["group"]).unwrap_or(0.0),
                });
            }
            if let Some(n) = abyssal
                && item.name.contains("Energy Blade")
            {
                explicit_mods.push(Json::String(format!("Has {n} Abyssal Sockets")));
            }
        }
        // :1658-1660
        let socketed = arr(&item_data["socketedItems"]);
        if truthy(&item_data["socketedItems"]) {
            self.import_socketed_items(&item, socketed, &slot_name, display)?;
        }
        // :1661-1671
        if truthy(&item_data["requirements"]) && socketed.is_empty() {
            item.requirements = pob_items::item::Requirements::default();
            for req in arr(&item_data["requirements"]) {
                let v = &req["values"][0][0];
                match req["name"].as_str() {
                    Some("Level") => item.requirements.level = tonumber(v),
                    Some("Class:") => item.class_restriction = v.as_str().map(str::to_owned),
                    _ => {}
                }
            }
        }
        // :1672-1776
        let mut enchant = Vec::new();
        let mut scourge = Vec::new();
        let mut implicit = Vec::new();
        let mut explicit = Vec::new();
        let mut crucible = Vec::new();
        push_lines(&mut enchant, &item_data["enchantMods"], "crafted");
        push_lines(&mut scourge, &item_data["scourgeMods"], "scourge");
        if !no_implicits {
            for m in arr(&item_data["implicitMods"]) {
                let text = m["description"].as_str().or(m.as_str()).unwrap_or("");
                let flags = if m.is_string() { &Json::Null } else { &m["flags"] };
                for line in lines_of(text) {
                    implicit.push(mod_line(line, &[("vestigial", truthy(&flags["vestigial"]))]));
                }
            }
        }
        push_lines(&mut explicit, &item_data["fracturedMods"], "fractured");
        for m in &explicit_mods {
            let text = m["description"].as_str().or(m.as_str()).unwrap_or("");
            let flags = if m.is_string() { &Json::Null } else { &m["flags"] };
            for line in lines_of(text) {
                explicit.push(mod_line(
                    line,
                    &[
                        ("fractured", truthy(&flags["fractured"])),
                        ("crafted", truthy(&flags["crafted"])),
                        ("mutated", truthy(&flags["mutated"])),
                        ("vestigial", truthy(&flags["vestigial"])),
                    ],
                ));
            }
        }
        push_lines(&mut crucible, &item_data["crucibleMods"], "crucible");
        push_lines(&mut explicit, &item_data["craftedMods"], "crafted");
        push_lines(&mut explicit, &item_data["mutatedMods"], "mutated");
        for l in arr(&item_data["flavourText"]) {
            for line in lines_of(l.as_str().unwrap_or("")) {
                let line = match (line.find('{'), line.find('}')) {
                    (Some(a), Some(b)) if b > a => &line[a + 1..b],
                    _ => line,
                };
                // the line is an explicit mod when it parses
                if pob_parser::parse_mod(line, false).is_ok_and(|p| p.mod_list.is_some()) {
                    explicit.push(mod_line(line, &[]));
                }
            }
        }
        item.enchant_mod_lines = enchant;
        item.scourge_mod_lines = scourge;
        item.implicit_mod_lines = implicit;
        item.explicit_mod_lines = explicit;
        item.crucible_mod_lines = crucible;
        // :1777-1793
        if truthy(&item_data["foilVariation"]) || truthy(&item_data["isRelic"]) {
            let v = tonumber(&item_data["foilVariation"]).and_then(|v| (v >= 1.0).then(|| FOIL_VARIANTS.get(v as usize - 1)).flatten());
            item.foil_type = Some(v.copied().unwrap_or("Rainbow").to_owned());
        }

        // :1796 `BuildAndParseRaw`: what the fields say, as text and back
        item.parse_mode = pob_calc::ParseMode::Full;
        item.build_and_parse_raw();
        if item.base.is_none() {
            return Ok(());
        }
        // :1799-1814 `pairs(self.items)`, `item.uniqueID == itemData.id` (nil == nil too)
        let wanted = item_data["id"].as_str();
        let rep = self
            .items_pairs()
            .into_iter()
            .find(|(_, it)| it.unique_id.as_deref() == wanted)
            .map(|(id, _)| id as f64);
        item.id = rep;
        let id = self.add_item(&item, None)?;
        // :1815-1819
        if let Some(set) = item_set
            && set != self.active_item_set_id()
        {
            self.set_item_set_slot(set, &slot_name, id);
        } else {
            match self.slot_exists(&slot_name)? {
                Some(node) => self.set_socket_item(node, id)?,
                None => self.set_slot_sel_item(&slot_name, id),
            }
        }
        Ok(())
    }

    /// `ImportTab:ImportSocketedItems(item, socketedItems, slotName)` (:1823-1898).
    fn import_socketed_items(&mut self, item: &Item, socketed: &[Json], slot_name: &str, display: Option<usize>) -> Result<(), String> {
        let data = gems();
        let mut groups: Vec<(f64, SocketGroup)> = Vec::new();
        let mut abyssal_socket_id = 1;
        for s in socketed {
            if truthy(&s["abyssJewel"]) {
                self.import_item(
                    s,
                    Some(&format!("{slot_name} Abyssal Socket {abyssal_socket_id}")),
                    false,
                    None,
                    display,
                )?;
                abyssal_socket_id += 1;
                continue;
            }
            let lookup = |name: &str| data.gem_for_base_name.0.get(&name.to_lowercase()).cloned();
            let mut gem_id = lookup(&pob_build::sanitise_text(s["typeLine"].as_str().unwrap_or("")));
            if truthy(&s["hybrid"]) {
                gem_id = lookup(&pob_build::sanitise_text(s["hybrid"]["baseTypeName"].as_str().unwrap_or("")));
                if truthy(&s["hybrid"]["isVaalGem"])
                    && let Some(id) = &gem_id
                {
                    gem_id = data
                        .gem_granted_effect_id_for_vaal_gem_id
                        .0
                        .get(&data.gems[id].granted_effect_id)
                        .cloned();
                }
            }
            let Some(gem_id) = gem_id else { continue };
            let gd = data
                .gems
                .get(&gem_id)
                .ok_or_else(|| format!("Classes/ImportTab.lua:1844: no data.gems['{gem_id}']"))?;
            let mut level = 20.0;
            let mut quality = 0.0;
            for p in arr(&s["properties"]) {
                match p["name"].as_str() {
                    Some("Level") => level = first_digits(p).unwrap_or(level),
                    Some("Quality") => quality = first_digits(p).unwrap_or(quality),
                    _ => {}
                }
            }
            let gem = GemInstance {
                name_spec: Some(gd.name.clone()),
                gem_id: Some(gem_id.clone()),
                level: Some(level),
                quality: Some(quality),
                enabled: true,
                enable_global1: Some(true),
                ..GemInstance::default()
            };
            let socket = tonumber(&s["socket"])
                .ok_or("Classes/ImportTab.lua:1853: attempt to perform arithmetic on field 'socket' (a nil value)")?;
            let group_id = item
                .sockets
                .get(socket as usize)
                .ok_or("Classes/ImportTab.lua:1853: attempt to index a nil value (item.sockets)")?
                .group;
            let at = match groups.iter().position(|(g, _)| *g == group_id) {
                Some(i) => i,
                None => {
                    groups.push((
                        group_id,
                        SocketGroup {
                            label: Some(String::new()),
                            enabled: true,
                            slot: Some(slot_name.to_owned()),
                            ..SocketGroup::new_empty()
                        },
                    ));
                    groups.len() - 1
                }
            };
            groups[at].1.gem_list.push(gem);
            if let Some(b) = s["builtInSupport"].as_str() {
                let imbued = lua_sub_gsub(b, "Supported by Level 1 ", "");
                groups[at].1.imbued_support = Some(imbued.clone());
                // :1861 `data.gems[data.gemForBaseName[...]]` nil: `gemData.id` is an error in the callback's `if`? `gemData and ...` is false: the support is cleared
                let support = data.gem_for_base_name.0.get(&format!("{} support", imbued.to_lowercase())).cloned();
                let shown = display.filter(|d| self.socket_groups().get(d - 1).and_then(|g| g.slot.as_deref()) == Some(slot_name));
                self.set_imbued_support(slot_name, shown, support.as_deref())?;
            }
        }
        // :1868 `pairs(itemSocketGroupList)`: the numeric keys, the order of the table
        let mut order = pob_tree::lj_table::LjTable::new();
        for (g, _) in &groups {
            order.set(*g);
        }
        for key in order.pairs_keys() {
            let (_, mut group) = groups.iter().find(|(g, _)| *g == key).cloned().expect("a key of the table");
            self.edit_skills(|t, s| {
                let list = t.socket_group_list_mut();
                let rep = list.iter().position(|sg| {
                    sg.gem_list.len() == group.gem_list.len()
                        && (sg.slot.is_none() || sg.slot.as_deref() == Some(slot_name))
                        && sg.gem_list.iter().zip(&group.gem_list).all(|(a, b)| {
                            a.name_spec.as_deref().unwrap_or("").to_lowercase() == b.name_spec.as_deref().unwrap_or("").to_lowercase()
                        })
                });
                match rep {
                    Some(i) => {
                        for (gem, new) in list[i].gem_list.iter_mut().zip(&group.gem_list) {
                            gem.level = new.level;
                            gem.quality = new.quality;
                        }
                    }
                    None => list.push(group.clone()),
                }
                // :1896 `ProcessSocketGroup(itemSocketGroup)`: the group of the list when it went in, a copy nobody holds when a group matched
                process_socket_group(&mut group, s)?;
                if rep.is_none() {
                    *list.last_mut().expect("just pushed") = group.clone();
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    /// :1382-1414: what comes before the items.
    pub fn import_items_begin(&mut self, clear_items: bool, clear_skills: bool) -> Result<SkillsCarry, String> {
        if clear_items {
            let ids: Vec<f64> = self
                .result()
                .env
                .ordered_slots
                .iter()
                .filter(|s| s.node_id.is_none() && s.sel_item_id != 0.0)
                .map(|s| s.sel_item_id)
                .collect();
            for id in ids {
                if self.item_by_id(id).is_some() {
                    self.delete_item(id)?;
                }
            }
        }
        let mut carry = SkillsCarry {
            main_skill_empty: self.socket_groups().is_empty(),
            ..SkillsCarry::default()
        };
        if clear_skills {
            let skills = self.skill_data();
            let main = self.main_socket_group();
            let mut order = Vec::new();
            for g in self.socket_groups() {
                for gem in &g.gem_list {
                    if let Some(name) = active_effect_name(gem, &skills) {
                        order.push(name);
                    }
                }
            }
            for (i, g) in self.socket_groups().iter().enumerate() {
                let key = reimport_key(g);
                let st = GroupState {
                    group: g.clone(),
                    is_main: (i + 1) as f64 == main,
                };
                match carry.preserved.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, l)) => l.push(st),
                    None => carry.preserved.push((key, vec![st])),
                }
            }
            carry.skill_order = Some(order);
            self.edit_skills(|t, _| {
                t.socket_group_list_mut().clear();
                rebuild_imbued_of(t);
                Ok(())
            })?;
        }
        Ok(carry)
    }

    /// `ImportTab:GetOrCreateGuardianItemSet()` (:1342-1354).
    fn guardian_item_set(&mut self) -> f64 {
        if let Some(s) = self.item_sets().iter().find(|s| s.title.as_deref() == Some(GUARD_ITEM_SET)) {
            return s.id;
        }
        self.new_item_set(Some(GUARD_ITEM_SET), None)
    }

    /// `ImportTab:AssignGuardianItemSet(itemSetId)` (:1357-1372).
    fn assign_guardian_item_set(&mut self, set_id: f64) -> Result<(), String> {
        let titles: Vec<(f64, Option<String>)> = self.item_sets().iter().map(|s| (s.id, s.title.clone())).collect();
        let skills = self.skill_data();
        let is_guard = |cur: f64| titles.iter().any(|(id, t)| *id == cur && t.as_deref() == Some(GUARD_ITEM_SET));
        let known = |cur: f64| titles.iter().any(|(id, _)| *id == cur);
        self.edit_skills(|t, _| {
            for g in t.socket_group_list_mut() {
                for gem in &mut g.gem_list {
                    if active_effect_name(gem, &skills).as_deref() == Some("Animate Guardian") {
                        match gem.skill_minion_item_set {
                            Some(c) if known(c) && !is_guard(c) => {}
                            Some(c) if !known(c) => {}
                            _ => gem.skill_minion_item_set = Some(set_id),
                        }
                        match gem.skill_minion_item_set_calcs {
                            Some(c) if known(c) && !is_guard(c) => {}
                            Some(c) if !known(c) => {}
                            _ => gem.skill_minion_item_set_calcs = Some(set_id),
                        }
                    }
                }
            }
            Ok(())
        })
    }

    /// :1415-1480: the items, the guardian's, the order of the groups, what a re-import keeps, the main group.
    pub fn import_items_body(
        &mut self,
        char_data: &Json,
        ignore_swap: bool,
        display: Option<usize>,
        carry: SkillsCarry,
    ) -> Result<(), String> {
        for item in arr(&char_data["equipment"]) {
            self.import_item(item, None, ignore_swap, None, display)?;
        }
        if char_data["guardian"].get(0).is_some_and(truthy) {
            let set = self.guardian_item_set();
            for item in arr(&char_data["guardian"]) {
                self.import_item(item, None, ignore_swap, Some(set), display)?;
            }
            self.assign_guardian_item_set(set)?;
        }
        let skills = self.skill_data();
        if let Some(order) = &carry.skill_order {
            let rank = |g: &SocketGroup| -> Option<usize> {
                g.gem_list
                    .iter()
                    .filter_map(|gem| active_effect_name(gem, &skills))
                    .filter_map(|n| order.iter().position(|o| *o == n))
                    .min()
            };
            self.edit_skills(|t, _| {
                // :1430-1460 a total order: `table.sort` and a stable sort give the same list
                t.socket_group_list_mut().sort_by(|a, b| match (rank(a), rank(b)) {
                    (Some(x), Some(y)) => x.cmp(&y),
                    (None, None) => std::cmp::Ordering::Equal,
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                });
                Ok(())
            })?;
        }
        let mut preserved = carry.preserved;
        if carry.skill_order.is_some() {
            let keys: Vec<String> = self.socket_groups().iter().map(reimport_key).collect();
            let mut restored_main = None;
            let mut states: Vec<(usize, GroupState)> = Vec::new();
            for (i, k) in keys.iter().enumerate() {
                if let Some((_, l)) = preserved.iter_mut().find(|(p, _)| p == k)
                    && !l.is_empty()
                {
                    let st = l.remove(0);
                    if st.is_main {
                        restored_main = Some(i + 1);
                    }
                    states.push((i, st));
                }
            }
            self.edit_skills(|t, _| {
                let list = t.socket_group_list_mut();
                for (i, st) in &states {
                    apply_group_state(&mut list[*i], st);
                }
                Ok(())
            })?;
            if let Some(i) = restored_main {
                self.set_main_pending(i as f64);
            }
        }
        if carry.main_skill_empty {
            // :1901-1911 `GuessMainSocketGroup`
            let (mut size, mut index) = (0, 1);
            for (i, g) in self.socket_groups().iter().enumerate() {
                if g.gem_list.len() > size {
                    size = g.gem_list.len();
                    index = i + 1;
                }
            }
            self.set_main_pending(index as f64);
        }
        Ok(())
    }

    /// `saveDetails` (:239-246), the part in the build.
    pub fn import_save_details(&mut self, realm_id: Option<&str>, league: Option<&str>, char_name: Option<&str>) {
        let i = &mut self.extras_mut().import;
        if let Some(r) = realm_id {
            i.last_realm = Some(r.to_owned());
        }
        i.last_league = league.map(str::to_owned);
        if let Some(n) = char_name {
            i.last_character_hash = Some(pob_net::sha1_hex(n.as_bytes()));
        }
    }

    /// `self.build.characterLevel = level` (:1213, :1485), no clamp and no `changeFunc`.
    pub fn import_set_level(&mut self, level: f64) {
        self.model_mut().level = level;
    }

    /// `build.importLink` of `Init(..., importLink)` (Build.lua:67).
    pub fn import_set_build_link(&mut self, link: &str) {
        self.extras_mut().import.build_import_link = Some(link.to_owned());
    }

    /// `configTab.input[var]`, a string.
    pub fn config_input_string(&self, var: &str) -> Option<String> {
        match self.get_config(var) {
            Ok(Some(pob_calc::pob_build::InputValue::Str(s))) => Some(s),
            _ => None,
        }
    }

    /// `self.lastLeague`.
    pub fn import_last_league(&self) -> Option<String> {
        self.extras().import.last_league.clone()
    }

    pub fn import_set_last_league(&mut self, league: Option<&str>) {
        self.extras_mut().import.last_league = league.map(str::to_owned);
    }

    pub fn import_set_last_realm(&mut self, realm: &str) {
        self.extras_mut().import.last_realm = Some(realm.to_owned());
    }

    /// `self.lastAccountHash = common.sha1(accountName)` (:970).
    pub fn import_set_account(&mut self, account: &str) {
        self.extras_mut().import.last_account_hash = Some(pob_net::sha1_hex(account.as_bytes()));
    }

    /// `self.lastCharacterHash = common.sha1(name)` (:834).
    pub fn import_set_character(&mut self, name: &str) {
        self.extras_mut().import.last_character_hash = Some(pob_net::sha1_hex(name.as_bytes()));
    }

    /// `self.lastCharacterHash`.
    pub fn import_last_character_hash(&self) -> Option<String> {
        self.extras().import.last_character_hash.clone()
    }

    /// The first part of `ImportPassiveTreeAndJewels` (:1132-1163): `spec.jewel_data`, `spec.extended_hashes`, the deleted jewels, the imported ones.
    pub fn import_jewels(&mut self, char_data: &Json, delete_jewels: bool, display: Option<usize>) -> Result<(), String> {
        let passives = &char_data["passives"];
        // :1152 `copyTable(charPassives.hashes_ex)`
        if passives["hashes_ex"].as_array().is_none() {
            return Err("Modules/Common.lua: copyTable: bad argument #1 to 'pairs' (table expected, got nil)".into());
        }
        if delete_jewels {
            // :1154-1160 every jewel of the sockets of this spec; `ignoreAllocatingSubgraph`: the allocated cluster nodes are not carried over
            let ids: Vec<f64> = self
                .result()
                .env
                .ordered_slots
                .iter()
                .filter(|s| s.node_id.is_some() && s.sel_item_id != 0.0)
                .map(|s| s.sel_item_id)
                .collect();
            for id in ids {
                if self.item_by_id(id).is_some() {
                    self.delete_item(id)?;
                }
            }
        }
        for item in arr(&char_data["jewels"]) {
            self.import_item(item, None, false, None, display)?;
        }
        Ok(())
    }

    /// `ExtraPoints` of the last `BuildOutput`: what `EstimatePlayerProgress` reads (:1217) before the next one.
    pub fn import_extra_points(&self) -> f64 {
        self.result().perform.player.output.opt_num("ExtraPoints").unwrap_or(0.0)
    }

    /// The second part (:1167-1251): the tree. `extra`: [`Engine::import_extra_points`] taken before.
    #[allow(clippy::too_many_lines)]
    pub fn import_passive_tree(&mut self, char_data: &Json, extra: f64) -> Result<TreeImportOut, String> {
        let passives = &char_data["passives"];
        let latest = self.latest_tree()?;
        let class_name = char_data["class"].as_str().unwrap_or("");
        // :1184-1196
        let alt = &passives["alternate_ascendancy"];
        let secondary: Option<f64> = if truthy(alt) {
            match alt {
                Json::String(s) => latest.secondary_ascend_name_map.as_ref().and_then(|m| m.get(s)).map(|k| *k as f64),
                other => tonumber(other),
            }
        } else {
            Some(0.0)
        };
        // :1199-1200
        let league = char_data["league"]
            .as_str()
            .ok_or("Classes/ImportTab.lua:1199: attempt to index field 'league' (a nil value)")?;
        let ruthless = if league.contains("Ruthless") { "_ruthless" } else { "" };
        let in_tree = latest
            .classes
            .iter()
            .any(|c| c.name == class_name || c.classes.iter().any(|a| a.name == class_name));
        let phrecia = if in_tree { "" } else { "_alternate" };
        let version = format!("{}{ruthless}{phrecia}", latest.tree_version);
        // :1136-1138 masteries
        let mut masteries = pob_build::NumMap::default();
        for (k, v) in passives["mastery_effects"].as_object().into_iter().flatten() {
            if let (Some(k), Some(v)) = (pob_lua::tonumber(k), tonumber(v)) {
                masteries.set(k, v);
            }
        }
        // :1143-1150 `skill_overrides`: the tattoos by name
        let mut overrides: Vec<pob_tree::tattoo::OverrideInput> = Vec::new();
        for (k, o) in passives["skill_overrides"].as_object().into_iter().flatten() {
            overrides.push(pob_tree::tattoo::OverrideInput {
                node_id: pob_lua::tonumber(k),
                dn: o["name"].as_str().map(str::to_owned),
                icon: o["icon"].as_str().map(str::to_owned),
                active_effect_image: o["activeEffectImage"].as_str().map(str::to_owned),
            });
        }
        let hashes: Vec<f64> = arr(&passives["hashes"]).iter().filter_map(tonumber).collect();
        // :1201-1209 `ImportFromNodeList(className, nil, nil, alternateAscendancyId, hashes, skillOverrides, masteries, version)`
        let old = self.result().env.spec.clone();
        let version_changed = version != old.tree_version;
        let mut spec = if version_changed {
            // :278-281 `self:Init(treeVersion)`: a new table of everything, `jewels` too; what a character import left stays (the fields are not `Init`'s)
            let tree = pob_tree::tree::load_passive_tree_mode(&version, pob_calc::ParseMode::Full).map_err(|e| e.0)?;
            pob_tree::PassiveSpec::new(tree).map_err(|e| e.0)?
        } else {
            (*old).clone()
        };
        let tree = spec.tree.clone();
        // :283-289 the class of the name
        let (class_id, ascend_id) = match tree.ascend_name_map.get(class_name) {
            Some(e) => (e.class_id as f64, e.ascend_class_id as f64),
            None => match tree.class_name_map.get(class_name) {
                Some(c) => (*c as f64, 0.0),
                None => return Err(format!("Classes/PassiveSpec.lua:291: no class '{class_name}' in tree {version}")),
            },
        };
        spec.hash_overrides.clear();
        spec.load_overrides(&overrides).map_err(|e| e.0)?;
        spec.import_from_node_list(class_id, ascend_id, secondary, &hashes, &masteries)
            .map_err(|e| e.0)?;
        // the live object carries the jewels of the sockets over `Init`: `SetActiveSpec` writes them back (TreeTab.lua:561-574, `prevSpec == curSpec`)
        let jewels: Vec<(f64, f64)> = self
            .result()
            .env
            .ordered_slots
            .iter()
            .filter_map(|s| s.node_id.map(|n| (f64::from(n), s.sel_item_id)))
            .collect();
        for (n, id) in jewels {
            spec.jewels.set(n, id);
        }
        self.write_spec_to_model(spec);
        {
            let sm = active_spec_mut(self.model_mut());
            sm.tree_version.clone_from(&version);
            // :1198
            sm.cluster_hash_format_version = 2.0;
            sm.overrides.clear();
            for o in &overrides {
                let dn = o.dn.clone();
                sm.overrides.push(pob_build::SpecOverrideModel {
                    node_id: o.node_id.unwrap_or(0.0),
                    icon: o.icon.clone(),
                    active_effect_image: o.active_effect_image.clone(),
                    dn,
                    attrib: indexmap::IndexMap::new(),
                });
            }
        }
        // :1210-1211 `SetActiveSpec`, `BuildClusterJewelGraphs`
        let active = self.active_spec();
        self.set_active_spec(active)?;
        let mut spec = (*self.result().env.spec).clone();
        let items = self.result().env.items.clone();
        let lookup = move |id: f64| crate::tree_tab::cluster_lookup(&items, id);
        ImportedSubgraphs::of(passives).build_graphs(&mut spec, &lookup)?;
        self.write_spec_to_model(spec);
        // :1213-1214
        let level = tonumber(&char_data["level"]).unwrap_or(100.0);
        // :1217-1226
        let act = crate::shell::point_display_with(self.result(), extra).act;
        let resistance_penalty_index = match act {
            None => 3,
            Some(a) if a < 5 => 1,
            Some(a) if a > 5 && a < 11 => 2,
            Some(a) if a > 10 => 3,
            Some(_) => 3,
        };
        let name = |v: &Json| v.as_str().map(str::to_owned);
        let bandit = match passives["bandit_choice"].as_str() {
            None | Some("Eramir") => "None".to_owned(),
            Some(b) => b.to_owned(),
        };
        Ok(TreeImportOut {
            level,
            level_text: match &char_data["level"] {
                Json::Null => None,
                v => tonumber(v).map(pob_lua::tostring_num),
            },
            resistance_penalty_index,
            bandit,
            major_god: name(&passives["pantheon_major"]).unwrap_or_else(|| "None".to_owned()),
            minor_god: name(&passives["pantheon_minor"]).unwrap_or_else(|| "None".to_owned()),
            title: (
                char_data["name"].as_str().unwrap_or("nil").to_owned(),
                class_name.to_owned(),
                league.to_owned(),
            ),
            version_changed,
        })
    }

    /// Whether the active spec has allocated nodes (`self.build.spec:CountAllocNodes() > 0`, ImportTab.lua:270, :432).
    pub fn import_has_alloc_nodes(&self) -> bool {
        self.result().env.spec.count_alloc_nodes().used > 0
    }
}

fn rebuild_imbued_of(tab: &mut SkillsTab) {
    crate::skills_tab::rebuild_imbued(tab);
}
