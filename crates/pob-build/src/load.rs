//! `load_build_xml` -- XML -> `BuildModel`, ported from pob-ts `src/build/load.ts` (itself ported from the Lua tab `Load`s:
//! Build.lua:976-1015, TreeTab.lua:500-527, PassiveSpec.lua:103-195, ItemsTab.lua:1216-1327, SkillsTab.lua:440-570,
//! ConfigTab.lua:839-938, CalcsTab.lua:163-180). Pure: no state survives a call (W12).

use std::fmt;

use indexmap::IndexMap;
use pob_data::tree::TreeJson;
use pob_lua::number::{max2, min2};
use pob_lua::{Capture, Repl, gmatch, gsub, tonumber};

use crate::config::{ConfigModLists, ConfigState};
use crate::model::*;
use crate::sanitise::sanitise_text;
use crate::xml::{XmlElement, XmlNode, parse_xml_document};

/// GameVersions.lua:4 `legacyTargetVersion`.
const LEGACY_TARGET_VERSION: &str = "2_6";

/// `tonumber(attrib.x)` (nil-safe).
fn num(el: &XmlElement, name: &str) -> Option<f64> {
    el.attr(name).and_then(tonumber)
}

fn attr(el: &XmlElement, name: &str) -> Option<String> {
    el.attr(name).map(str::to_owned)
}

/// `attrib.x == "true"`.
fn is_true(el: &XmlElement, name: &str) -> bool {
    el.attr(name) == Some("true")
}

/// `attrib.x == "true" or attrib.x == nil` (SkillsTab.lua:498-499, ConfigTab.lua:896).
fn default_true(el: &XmlElement, name: &str) -> bool {
    matches!(el.attr(name), None | Some("true"))
}

/// JS `Math.min(a, b)` (NaN wins).
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

fn empty_element(name: &str) -> XmlElement {
    XmlElement {
        name: name.to_owned(),
        attrib: IndexMap::new(),
        children: Vec::new(),
        empty: true,
    }
}

/// The captures of one `gmatch` step as numbers (`tonumber` of digit runs).
fn capture_nums(s: &[u8], caps: &pob_lua::Captures) -> Vec<f64> {
    caps.iter()
        .map(|c| match c {
            Capture::Str { start, end } => {
                tonumber(std::str::from_utf8(&s[start..end]).expect("digits")).expect("a %d+ capture is a number")
            }
            Capture::Pos(p) => p as f64,
        })
        .collect()
}

const KNOWN_GEM_LEVELS: [&str; 5] = ["normalMaximum", "corruptedMaximum", "awakenedMaximum", "characterLevel", "levelOne"];

/// SkillsTab.lua:528-535: `DropDownControl:SelByValue` keeps the prior selection on an unknown value; a single-shot load
/// has none, so the control's construction default (index 1, "normalMaximum") stands.
fn select_default_gem_level(el: &XmlElement) -> String {
    if is_true(el, "matchGemLevelToCharacterLevel") {
        return "characterLevel".into();
    }
    match el.attr("defaultGemLevel") {
        Some(v) if tonumber(v).is_none() && KNOWN_GEM_LEVELS.contains(&v) => v.to_owned(),
        _ => "normalMaximum".into(),
    }
}

/// ConfigTab.lua:852-858: `enemyIsBoss` = `s:lower():gsub("(%l)(%w*)", upper(a)..b):gsub("Uber Atziri", "Boss")
/// :gsub("Shaper", "Pinnacle"):gsub("Sirus", "Pinnacle")`; `presetBossSkills` drops a leading "Uber ".
fn config_string_value(name: &str, raw: &str) -> String {
    let g = |s: &[u8], p: &[u8], r: &[u8]| gsub(s, p, Repl::Str(r), None).expect("ConfigTab pattern").0;
    let out = match name {
        "enemyIsBoss" => {
            let lower = pob_lua::string::lower(raw);
            let mut title = |subject: &[u8], caps: &pob_lua::Captures| -> Option<Vec<u8>> {
                let mut out = Vec::new();
                for c in caps.iter() {
                    if let Capture::Str { start, end } = c {
                        out.extend_from_slice(&subject[start..end]);
                    }
                }
                if let Some(first) = out.first_mut() {
                    *first = first.to_ascii_uppercase();
                }
                Some(out)
            };
            let s = gsub(lower.as_bytes(), b"(%l)(%w*)", Repl::Fn(&mut title), None)
                .expect("ConfigTab pattern")
                .0;
            let s = g(&s, b"Uber Atziri", b"Boss");
            let s = g(&s, b"Shaper", b"Pinnacle");
            g(&s, b"Sirus", b"Pinnacle")
        }
        "presetBossSkills" => g(raw.as_bytes(), b"^Uber ", b""),
        _ => return raw.to_owned(),
    };
    String::from_utf8(out).expect("ASCII-only rewrites keep UTF-8")
}

fn load_build_section(el: &XmlElement, m: &mut BuildModel) {
    m.level = num(el, "level").unwrap_or(1.0);
    m.target_version = attr(el, "targetVersion").unwrap_or_else(|| LEGACY_TARGET_VERSION.into());
    if let Some(v) = attr(el, "viewMode") {
        m.view_mode = Some(v);
    }
    m.character_level_auto_mode = is_true(el, "characterLevelAutoMode");
    m.class_name = attr(el, "className").unwrap_or_default();
    m.ascend_class_name = attr(el, "ascendClassName").unwrap_or_default();
    m.bandit = attr(el, "bandit").unwrap_or_else(|| "None".into());
    m.pantheon_major_god = attr(el, "pantheonMajorGod").unwrap_or_else(|| "None".into());
    m.pantheon_minor_god = attr(el, "pantheonMinorGod").unwrap_or_else(|| "None".into());
    m.main_socket_group = num(el, "mainSkillIndex").or_else(|| num(el, "mainSocketGroup")).unwrap_or(1.0);
    for child in el.child_elements() {
        match child.name.as_str() {
            "Spectre" => {
                if let Some(id) = attr(child, "id") {
                    m.spectres.push(id);
                }
            }
            "TimelessData" => {
                m.timeless_data = Some(TimelessDataModel {
                    jewel_type_id: num(child, "jewelTypeId"),
                    conqueror_type_id: num(child, "conquerorTypeId"),
                    devotion_variant1: num(child, "devotionVariant1").unwrap_or(1.0),
                    devotion_variant2: num(child, "devotionVariant2").unwrap_or(1.0),
                    jewel_socket_id: num(child, "jewelSocketId"),
                    fallback_weight_mode_idx: num(child, "fallbackWeightModeIdx"),
                    socket_filter: is_true(child, "socketFilter"),
                    socket_allocate: is_true(child, "socketAllocate"),
                    socket_filter_distance: num(child, "socketFilterDistance").unwrap_or(0.0),
                    search_list: attr(child, "searchList"),
                    search_list_fallback: attr(child, "searchListFallback"),
                });
            }
            _ => {}
        }
    }
}

fn empty_spec() -> SpecModel {
    SpecModel {
        title: None,
        tree_version: String::new(),
        cluster_hash_format_version: 2.0,
        class_id: 0.0,
        ascend_class_id: 0.0,
        secondary_ascend_class_id: Some(0.0),
        nodes: Vec::new(),
        mastery_effects: NumMap::default(),
        jewels: NumMap::default(),
        url: None,
        overrides: Vec::new(),
    }
}

/// PassiveSpec.lua:103-195.
fn load_spec(el: &XmlElement) -> SpecModel {
    let mut url = None;
    let mut jewels = NumMap::default();
    for child in el.child_elements() {
        match child.name.as_str() {
            "URL" => url = Some(child.text()),
            "Sockets" => {
                for s in child.child_elements().filter(|s| s.name == "Socket") {
                    if let (Some(node_id), Some(item_id)) = (num(s, "nodeId"), num(s, "itemId"))
                        && item_id > 0.0
                    {
                        jewels.set(node_id, item_id);
                    }
                }
            }
            _ => {}
        }
    }
    let nodes_attr = el.attr("nodes");
    // PassiveSpec.lua:106: `tonumber(...) or (xml.attrib.nodes and 1 or 2)`.
    let cluster_hash_format_version = num(el, "clusterHashFormatVersion").unwrap_or(if nodes_attr.is_some() { 1.0 } else { 2.0 });
    let mut spec = SpecModel {
        title: attr(el, "title"),
        tree_version: attr(el, "treeVersion").unwrap_or_default(),
        cluster_hash_format_version,
        jewels,
        url,
        ..empty_spec()
    };
    let Some(nodes_attr) = nodes_attr else {
        // Legacy URL-only spec: decoding `url` is tree work (R3.3); kept raw above.
        return spec;
    };
    // `for hash in xml.attrib.nodes:gmatch("%d+")` (L9: no digits -> {}).
    for caps in gmatch(nodes_attr.as_bytes(), b"%d+") {
        spec.nodes.extend(capture_nums(nodes_attr.as_bytes(), &caps.expect("%d+")));
    }
    if let Some(me) = el.attr("masteryEffects") {
        for caps in gmatch(me.as_bytes(), b"{(%d+),(%d+)}") {
            let v = capture_nums(me.as_bytes(), &caps.expect("{(%d+),(%d+)}"));
            spec.mastery_effects.set(v[0], v[1]);
        }
    }
    for child in el.child_elements().filter(|c| c.name == "Overrides") {
        for o in child.child_elements() {
            if let Some(node_id) = num(o, "nodeId")
                && o.name == "Override"
            {
                spec.overrides.push(SpecOverrideModel {
                    node_id,
                    icon: attr(o, "icon"),
                    active_effect_image: attr(o, "activeEffectImage"),
                    dn: attr(o, "dn"),
                    attrib: o.attrib.clone(),
                });
            }
        }
    }
    spec.class_id = num(el, "classId").unwrap_or(0.0);
    spec.ascend_class_id = num(el, "ascendClassId").unwrap_or(0.0);
    // PassiveSpec.lua:190 `tonumber(xml.attrib.secondaryAscendClassId or 0)` -- PassiveSpec:Save writes "nil" (a tree without alternate ascendancies):
    // that loads as nil, not 0
    spec.secondary_ascend_class_id = match attr(el, "secondaryAscendClassId") {
        None => Some(0.0),
        Some(_) => num(el, "secondaryAscendClassId"),
    };
    spec
}

/// TreeTab.lua:500-527 (`activeSpec` clamped to the spec count, :526/:551).
fn load_tree(el: &XmlElement) -> TreeTabModel {
    let mut specs: Vec<SpecModel> = el.child_elements().filter(|c| c.name == "Spec").map(load_spec).collect();
    if specs.is_empty() {
        specs.push(empty_spec());
    }
    let active_spec = js_min(num(el, "activeSpec").unwrap_or(1.0), specs.len() as f64);
    TreeTabModel { specs, active_spec }
}

fn load_item(el: &XmlElement) -> ItemModel {
    ItemModel {
        id: num(el, "id").unwrap_or(0.0),
        variant: num(el, "variant"),
        variant_alt: num(el, "variantAlt"),
        variant_alt2: num(el, "variantAlt2"),
        variant_alt3: num(el, "variantAlt3"),
        variant_alt4: num(el, "variantAlt4"),
        variant_alt5: num(el, "variantAlt5"),
        raw: el.text(),
        mod_ranges: el
            .child_elements()
            .filter(|c| c.name == "ModRange")
            .map(|c| ModRangeModel {
                id: num(c, "id").unwrap_or(0.0),
                range: num(c, "range").unwrap_or(1.0),
            })
            .collect(),
    }
}

fn load_item_set(el: &XmlElement) -> ItemSetModel {
    let mut slots = IndexMap::new();
    let mut socket_id_urls = NumMap::default();
    for child in el.child_elements() {
        match child.name.as_str() {
            "Slot" => {
                slots.insert(
                    attr(child, "name").unwrap_or_default(),
                    ItemSetSlotModel {
                        item_id: num(child, "itemId").unwrap_or(0.0),
                        active: is_true(child, "active"),
                        pb_url: Some(attr(child, "itemPbURL").unwrap_or_default()),
                    },
                );
            }
            "SocketIdURL" => {
                if let Some(node_id) = num(child, "nodeId") {
                    socket_id_urls.set(node_id, attr(child, "itemPbURL").unwrap_or_default());
                }
            }
            _ => {}
        }
    }
    ItemSetModel {
        id: num(el, "id").unwrap_or(0.0),
        title: attr(el, "title"),
        use_second_weapon_set: is_true(el, "useSecondWeaponSet"),
        slots,
        socket_id_urls,
    }
}

/// ItemsTab.lua:1216-1327. Legacy top-level `<Slot>`s fold into set 1 only when the file has no `<ItemSet>` (otherwise
/// `SetActiveItemSet` overwrites them before anything reads them, :1317-1322). TradeSearchWeights: out of scope.
fn load_items_into(el: &XmlElement, m: &mut BuildModel) {
    let mut legacy_slots = IndexMap::new();
    for child in el.child_elements() {
        match child.name.as_str() {
            "Item" => {
                let item = load_item(child);
                m.item_order.push(item.id);
                m.items.push(item);
            }
            "Slot" => {
                legacy_slots.insert(
                    attr(child, "name").unwrap_or_default(),
                    ItemSetSlotModel {
                        item_id: num(child, "itemId").unwrap_or(0.0),
                        active: is_true(child, "active"),
                        pb_url: None,
                    },
                );
            }
            "ItemSet" => m.item_sets.push(load_item_set(child)),
            _ => {}
        }
    }
    if m.item_sets.is_empty() {
        m.item_sets.push(ItemSetModel {
            id: 1.0,
            title: None,
            use_second_weapon_set: is_true(el, "useSecondWeaponSet"),
            slots: legacy_slots,
            socket_id_urls: NumMap::default(),
        });
    }
    let requested = num(el, "activeItemSet").unwrap_or(1.0);
    m.active_item_set = if m.item_sets.iter().any(|s| s.id == requested) {
        requested
    } else {
        m.item_sets.first().map_or(1.0, |s| s.id)
    };
}

/// SkillsTab.lua:440-521 LoadSkill, per `<Gem>`.
fn load_gem(el: &XmlElement) -> GemModel {
    GemModel {
        name_spec: sanitise_text(el.attr("nameSpec").unwrap_or("")),
        gem_id: attr(el, "gemId"),
        variant_id: attr(el, "variantId"),
        skill_id: attr(el, "skillId"),
        level: num(el, "level"),
        quality: num(el, "quality"),
        quality_id: attr(el, "qualityId"),
        enabled: default_true(el, "enabled"),
        enable_global1: default_true(el, "enableGlobal1"),
        enable_global2: is_true(el, "enableGlobal2"),
        count: num(el, "count").unwrap_or(1.0),
        skill_part: num(el, "skillPart"),
        skill_part_calcs: num(el, "skillPartCalcs"),
        skill_stage_count: num(el, "skillStageCount"),
        skill_stage_count_calcs: num(el, "skillStageCountCalcs"),
        skill_mine_count: num(el, "skillMineCount"),
        skill_mine_count_calcs: num(el, "skillMineCountCalcs"),
        skill_minion: attr(el, "skillMinion"),
        skill_minion_calcs: attr(el, "skillMinionCalcs"),
        skill_minion_item_set: num(el, "skillMinionItemSet"),
        skill_minion_item_set_calcs: num(el, "skillMinionItemSetCalcs"),
        skill_minion_skill: num(el, "skillMinionSkill"),
        skill_minion_skill_calcs: num(el, "skillMinionSkillCalcs"),
    }
}

fn load_skill_group(el: &XmlElement) -> SocketGroupModel {
    let mut gems: Vec<GemModel> = el.child_elements().map(load_gem).collect();
    let imbued_support = match (el.attr("imbuedSupport"), el.attr("slot")) {
        (Some(s), Some(_)) => Some(s.to_owned()),
        _ => None,
    };
    if el.attr("skillPart").is_some()
        && let Some(g) = gems.first_mut()
    {
        g.skill_part = num(el, "skillPart");
    }
    SocketGroupModel {
        enabled: is_true(el, "active") || is_true(el, "enabled"),
        include_in_full_dps: is_true(el, "includeInFullDPS"),
        include_in_full_dps_attr: el.attr("includeInFullDPS").is_some(),
        group_count: num(el, "groupCount"),
        label: attr(el, "label"),
        slot: attr(el, "slot"),
        source: attr(el, "source"),
        main_active_skill: num(el, "mainActiveSkill").unwrap_or(1.0),
        main_active_skill_calcs: num(el, "mainActiveSkillCalcs").unwrap_or(1.0),
        imbued_support,
        gems,
    }
}

/// SkillsTab.lua:523-570.
fn load_skills(el: &XmlElement) -> SkillsModel {
    let mut sets: Vec<SkillSetModel> = Vec::new();
    let mut legacy_set: Option<usize> = None;
    for child in el.child_elements() {
        match child.name.as_str() {
            "Skill" => {
                let i = *legacy_set.get_or_insert_with(|| {
                    sets.push(SkillSetModel {
                        id: 1.0,
                        title: None,
                        groups: Vec::new(),
                    });
                    sets.len() - 1
                });
                sets[i].groups.push(load_skill_group(child));
            }
            "SkillSet" => sets.push(SkillSetModel {
                id: num(child, "id").unwrap_or(0.0),
                title: attr(child, "title"),
                groups: child.child_elements().filter(|g| g.name == "Skill").map(load_skill_group).collect(),
            }),
            _ => {}
        }
    }
    if sets.is_empty() {
        sets.push(SkillSetModel {
            id: 1.0,
            title: None,
            groups: Vec::new(),
        });
    }
    let requested = num(el, "activeSkillSet").unwrap_or(1.0);
    let active_skill_set = if sets.iter().any(|s| s.id == requested) {
        requested
    } else {
        sets[0].id
    };
    SkillsModel {
        default_gem_level: select_default_gem_level(el),
        // SkillsTab.lua:536 `m_max(m_min(tonumber(defaultGemQuality) or 0, 23), 0)`
        default_gem_quality: max2(min2(num(el, "defaultGemQuality").unwrap_or(0.0), 23.0), 0.0),
        sort_gems_by_dps: el.attr("sortGemsByDPS").map(|v| v == "true"),
        show_legacy_gems: el.attr("showLegacyGems").map(|v| v == "true"),
        show_support_gem_types: attr(el, "showSupportGemTypes").unwrap_or_else(|| "ALL".into()),
        sort_gems_by_dps_field: attr(el, "sortGemsByDPSField").unwrap_or_else(|| "CombinedDPS".into()),
        sets,
        active_skill_set,
    }
}

/// ConfigTab.lua:893-897 / :911-915: `{ title = attrib.title or "Default", enabled = (enabled == "true" or enabled == nil),
/// text = node[1] or "" }` -- the FIRST child only (a later text run, e.g. after a CDATA, is not read).
fn load_custom_mods_block(el: &XmlElement) -> CustomModsBlockModel {
    CustomModsBlockModel {
        title: attr(el, "title").unwrap_or_else(|| "Default".into()),
        enabled: default_true(el, "enabled"),
        text: match el.children.first() {
            Some(XmlNode::Text(t)) => BlockText::Str(t.clone()),
            Some(XmlNode::Element(e)) => BlockText::Element(e.clone()),
            None => BlockText::Str(String::new()),
        },
    }
}

/// `launch:ShowErrMsg("^1Error parsing '%s': ...", fileName)` of ConfigTab:Load.
fn config_parse_error(what: &str) -> BuildLoadError {
    BuildLoadError(format!("^1Error parsing '{DB_FILE_NAME}': {what}"))
}

/// ConfigTab.lua:843-881 `setInputAndPlaceholder`: every write recorded in order, a failed `tonumber` as a nil write.
/// Its `return true` after `ShowErrMsg` is ignored by the caller and loading goes on -- but the first message is the
/// prompt the user sees (Launch.lua:382), so the load reports it.
fn set_input_and_placeholder(node: &XmlElement, set: &mut ConfigSetModel) -> Result<(), BuildLoadError> {
    match node.name.as_str() {
        "Input" => {
            let Some(name) = node.attr("name") else {
                return Err(config_parse_error("'Input' element missing name attribute"));
            };
            let v = if let Some(n) = node.attr("number") {
                tonumber(n).map(InputValue::Num)
            } else if let Some(s) = node.attr("string") {
                Some(InputValue::Str(config_string_value(name, s)))
            } else if node.attr("boolean").is_some() {
                Some(InputValue::Bool(is_true(node, "boolean")))
            } else {
                return Err(config_parse_error("'Input' element missing number, string or boolean attribute"));
            };
            set.input.insert(name.to_owned(), v);
        }
        "Placeholder" => {
            let Some(name) = node.attr("name") else {
                return Err(config_parse_error("'Placeholder' element missing name attribute"));
            };
            if let Some(n) = node.attr("number") {
                set.placeholder.insert(name.to_owned(), tonumber(n));
            } else if let Some(s) = node.attr("string") {
                // upstream quirk, ConfigTab.lua:876: a string Placeholder is written into `input` -- verbatim.
                set.input.insert(name.to_owned(), Some(InputValue::Str(s.to_owned())));
            } else {
                return Err(config_parse_error("'Placeholder' element missing number"));
            }
        }
        _ => {}
    }
    Ok(())
}

/// ConfigTab.lua:1280-1300 `NewConfigSet(id, title)` as far as the model goes (the defaults are `ConfigState::from_set`'s):
/// a fresh set, replacing one with the same id in place.
fn new_config_set(sets: &mut Vec<ConfigSetModel>, id: f64, title: Option<String>) -> usize {
    let set = ConfigSetModel {
        id,
        title,
        input: IndexMap::new(),
        placeholder: IndexMap::new(),
        custom_mods_list: vec![CustomModsBlockModel::default_block()],
    };
    match sets.iter().position(|s| s.id == id) {
        Some(i) => {
            sets[i] = set;
            i
        }
        None => {
            sets.push(set);
            sets.len() - 1
        }
    }
}

/// The set `ConfigTab:ConfigTab` creates (:146-148 `configSetOrderList = { 1 }`, `NewConfigSet(1)` -- no title) and that a
/// build without `<Config>` keeps.
fn ctor_config() -> ConfigModel {
    let mut sets = Vec::new();
    new_config_set(&mut sets, 1.0, None);
    ConfigModel {
        sets,
        active_config_set: 1.0,
        set_order_list: vec![(1.0, 1.0)],
    }
}

/// ConfigTab.lua:923-936, one set of `ipairs(configSetOrderList)`: the legacy `input.customMods` migration.
fn migrate_legacy_custom_mods(set: &mut ConfigSetModel) {
    // `configSet.input and configSet.input.customMods or ""` (L4: a stored `false` reads as "").
    let legacy = match set.input.get("customMods") {
        Some(Some(InputValue::Bool(false)) | None) | None => None,
        Some(Some(v)) => Some(v.clone()),
    };
    let legacy_nonempty = match &legacy {
        Some(InputValue::Str(s)) => !s.is_empty(),
        Some(_) => true,
        None => false,
    };
    let list = &set.custom_mods_list;
    let blank = list.is_empty() || (list.len() == 1 && list[0].text == BlockText::Str(String::new()));
    if legacy_nonempty && blank {
        let text = match legacy.expect("non-empty") {
            InputValue::Str(s) => BlockText::Str(s),
            InputValue::Num(n) => BlockText::Num(n),
            InputValue::Bool(b) => BlockText::Bool(b),
        };
        set.custom_mods_list = vec![CustomModsBlockModel {
            title: "Default".into(),
            enabled: true,
            text,
        }];
    } else if list.is_empty() {
        set.custom_mods_list = vec![CustomModsBlockModel::default_block()];
    }
    // `configSet.input.customMods = nil`: ConfigOptions has no `customMods` var, so no default comes back.
    set.input.shift_remove("customMods");
}

/// ConfigTab.lua:839-938 `ConfigTab:Load` + the `SetActiveConfigSet` id resolution (:1337-1354). Children are walked as
/// `ipairs(xml)` -- text nodes included: one creates set 1 like any non-`<ConfigSet>` child and counts as an index.
fn load_config(el: &XmlElement) -> Result<ConfigModel, BuildLoadError> {
    let mut sets: Vec<ConfigSetModel> = Vec::new();
    let mut order: Vec<(f64, f64)> = vec![(1.0, 1.0)];
    if el.empty {
        new_config_set(&mut sets, 1.0, Some("Default".into())); // :885-887
    }
    for (k, node) in el.children.iter().enumerate() {
        let index = (k + 1) as f64;
        match node {
            XmlNode::Element(n) if n.name == "ConfigSet" => {
                let Some(id) = num(n, "id") else {
                    // :905 `NewConfigSet(nil, ...)` picks a free id, :906 stores nil, :907 `configSets[nil].customModsList`.
                    return Err(BuildLoadError::runtime("Classes/ConfigTab.lua:907: attempt to index a nil value"));
                };
                let i = new_config_set(&mut sets, id, Some(attr(n, "title").unwrap_or_else(|| "Default".into())));
                match order.iter_mut().find(|(ix, _)| *ix == index) {
                    Some(slot) => slot.1 = id,
                    None => {
                        order.push((index, id));
                        order.sort_by(|a, b| a.0.total_cmp(&b.0));
                    }
                }
                sets[i].custom_mods_list.clear();
                for child in &n.children {
                    match child {
                        XmlNode::Element(c) if c.name == "CustomModifierBlock" => {
                            sets[i].custom_mods_list.push(load_custom_mods_block(c));
                        }
                        XmlNode::Element(c) => set_input_and_placeholder(c, &mut sets[i])?,
                        XmlNode::Text(_) => {} // `child.elem` of a string is nil
                    }
                }
            }
            _ => {
                let i = match sets.iter().position(|s| s.id == 1.0) {
                    Some(i) => i,
                    None => new_config_set(&mut sets, 1.0, Some("Default".into())),
                };
                match node {
                    XmlNode::Element(n) if n.name == "CustomModifierBlock" => {
                        sets[i].custom_mods_list.push(load_custom_mods_block(n));
                    }
                    XmlNode::Element(n) => set_input_and_placeholder(n, &mut sets[i])?,
                    XmlNode::Text(_) => {}
                }
            }
        }
    }
    // :924 `for _, configSetId in ipairs(self.configSetOrderList)` -- stops at the first hole.
    let mut ix = 1.0;
    while let Some(&(_, id)) = order.iter().find(|(x, _)| *x == ix) {
        let i = sets
            .iter()
            .position(|s| s.id == id)
            .expect("every listed id was created by NewConfigSet");
        migrate_legacy_custom_mods(&mut sets[i]);
        ix += 1.0;
    }
    // :937 `SetActiveConfigSet(tonumber(xml.attrib.activeConfigSet) or 1)`.
    let requested = num(el, "activeConfigSet").unwrap_or(1.0);
    let active_config_set = if sets.iter().any(|s| s.id == requested) {
        requested
    } else {
        order.iter().find(|(x, _)| *x == 1.0).map_or(1.0, |&(_, id)| id)
    };
    if !sets.iter().any(|s| s.id == active_config_set) {
        return Err(BuildLoadError::runtime("Classes/ConfigTab.lua:1352: attempt to index a nil value"));
    }
    Ok(ConfigModel {
        sets,
        active_config_set,
        set_order_list: order,
    })
}

/// CalcsTab.lua:163-180 (`<Section>` skipped).
fn load_calcs(el: &XmlElement) -> IndexMap<String, InputValue> {
    let mut out = IndexMap::new();
    for child in el.child_elements().filter(|c| c.name == "Input") {
        let Some(name) = child.attr("name") else { continue };
        if let Some(n) = child.attr("number") {
            if let Some(n) = tonumber(n) {
                out.insert(name.to_owned(), InputValue::Num(n));
            }
        } else if let Some(s) = child.attr("string") {
            out.insert(name.to_owned(), InputValue::Str(s.to_owned()));
        } else if child.attr("boolean").is_some() {
            out.insert(name.to_owned(), InputValue::Bool(is_true(child, "boolean")));
        }
    }
    out
}

/// A build PoB refuses or fails to load: the text `launch:ShowErrMsg` shows (`string.format(fmt, ...)` of Launch.lua:386,
/// without the frame -- see [`BuildLoadError::prompt`]). The first message wins (Launch.lua:382 `if not self.promptMsg`),
/// so a load stops at the first one. Runtime Lua errors reach it through `launch:OnFrame`'s `PCall` (Launch.lua:118) as
/// `In 'OnFrame': <LuaJIT message>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildLoadError(pub String);

impl BuildLoadError {
    /// A Lua error raised while the build loads (Launch.lua:118).
    pub fn runtime(message: &str) -> BuildLoadError {
        BuildLoadError(format!("In 'OnFrame': {message}"))
    }

    /// `launch.promptMsg` as Launch.lua:381-388 builds it for PoB `versionNumber` (no `versionBranch`).
    pub fn prompt(&self, version: &str) -> String {
        format!(
            "^1Error:\n\n^0{}\n^8v{version}\n^0Press Enter/Escape to dismiss, F4 to return to build selection, or F5 to restart the application.\nPress CTRL + C to copy error text.",
            self.0
        )
    }
}

impl fmt::Display for BuildLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BuildLoadError {}

/// `build.dbFileName` that ConfigTab:Load's messages print: `false` -- the build comes as text (HeadlessWrapper.lua:60-62
/// `main:SetMode("BUILD", false, name, xmlText)`), which is what `load_build_xml` models.
const DB_FILE_NAME: &str = "false";

/// Build.lua:2222-2233 `LoadDB(xmlText, "Unnamed build")`: `ParseXML`, then `dbXML[1]` must be `<PathOfBuilding>`.
fn parse_root(xml_text: &str) -> Result<XmlElement, BuildLoadError> {
    let mut top = parse_xml_document(xml_text).map_err(|e| {
        if e.thrown {
            BuildLoadError::runtime(&e.message)
        } else {
            BuildLoadError(format!("^1{}", e.message)) // :2229 `ShowErrMsg("^1"..errMsg)`
        }
    })?;
    if top.is_empty() {
        // :2231 `dbXML[1].elem` with `dbXML[1] == nil` (empty or whitespace-only text). Message text not oracle-checked.
        return Err(BuildLoadError::runtime(
            "Modules/Build.lua:2231: attempt to index field '?' (a nil value)",
        ));
    }
    match top.swap_remove(0) {
        XmlNode::Element(e) if e.name == "PathOfBuilding" => Ok(e),
        // A text node (e.g. a UTF-8 BOM before the root) has no `.elem`.
        _ => Err(BuildLoadError(
            "^1Error parsing 'Unnamed build': 'PathOfBuilding' root element missing".into(),
        )),
    }
}

/// `xmlText` -> the model, calling `on_config` right after each `<Config>` section's `ConfigTab:Load` (where Lua's
/// `SetActiveConfigSet` runs `BuildModList`).
fn load_model(xml_text: &str, on_config: &mut dyn FnMut(&BuildModel) -> Result<(), BuildLoadError>) -> Result<BuildModel, BuildLoadError> {
    let root = parse_root(xml_text)?;
    let mut m = BuildModel {
        level: 1.0,
        target_version: LEGACY_TARGET_VERSION.into(),
        view_mode: None,
        character_level_auto_mode: false,
        class_name: String::new(),
        ascend_class_name: String::new(),
        bandit: "None".into(),
        pantheon_major_god: "None".into(),
        pantheon_minor_god: "None".into(),
        main_socket_group: 1.0,
        spectres: Vec::new(),
        timeless_data: None,
        tree: TreeTabModel {
            specs: Vec::new(),
            active_spec: 1.0,
        },
        items: Vec::new(),
        item_order: Vec::new(),
        item_sets: Vec::new(),
        active_item_set: 1.0,
        skills: load_skills(&empty_element("Skills")),
        config: ctor_config(),
        calcs_input: IndexMap::new(),
        unknown_sections: Vec::new(),
        has_config_section: false,
        party_imported_buffs: Vec::new(),
    };
    // Build.lua:2236-2241: only the FIRST <Build>, before every other section (it has no saver, :628-638).
    if let Some(b) = root.child_elements().find(|e| e.name == "Build") {
        load_build_section(b, &mut m);
    }
    for el in root.child_elements() {
        match el.name.as_str() {
            "Build" => {}
            "Calcs" => m.calcs_input = load_calcs(el),
            "Skills" => m.skills = load_skills(el),
            "Tree" => m.tree = load_tree(el),
            // Legacy bare `<Spec>`, TreeTab.lua:502-509.
            "Spec" => {
                m.tree = TreeTabModel {
                    specs: vec![load_spec(el)],
                    active_spec: 1.0,
                }
            }
            "Items" => load_items_into(el, &mut m),
            "Config" => {
                m.config = load_config(el)?;
                m.has_config_section = true;
                on_config(&m)?;
            }
            "Party" => {
                // PartyTab:Load (PartyTab.lua:516-558): `ParseBuffs(..., node[1] or "")` -- an element there is a table, and
                // `buf:gmatch` on it fails (the line of the `gmatch` of that buffer type).
                let buffers = crate::model::party_imported_buffs(el);
                for b in &buffers {
                    let line = match b.name.as_str() {
                        "PartyMemberStats" => 768,
                        "Aura" | "Curse" | "Warcry Skills" | "Link Skills" => 806,
                        "EnemyConditions" => 724,
                        "EnemyMods" => 732,
                        _ => continue,
                    };
                    if b.text == crate::model::PartyText::Element {
                        return Err(BuildLoadError::runtime(&format!(
                            "Classes/PartyTab.lua:{line}: attempt to call method 'gmatch' (a nil value)"
                        )));
                    }
                }
                m.party_imported_buffs.extend(buffers);
                m.unknown_sections.push("Party".to_owned());
            }
            other => m.unknown_sections.push(other.to_owned()),
        }
    }
    if m.tree.specs.is_empty() {
        m.tree = load_tree(&empty_element("Tree"));
    }
    if m.item_sets.is_empty() {
        load_items_into(&empty_element("Items"), &mut m);
    }
    Ok(m)
}

/// Parse a build XML and load every section the calc reads (Build.lua:2222-2260 + the tab `Load`s). Errors are the ones
/// Lua reports while doing exactly that: the XML (`nil, err` of xml.lua), the root element, ConfigTab:Load's messages and
/// its `<ConfigSet>` without a numeric id. `BuildModList` and the calc are not run here -- see [`load_build`].
pub fn load_build_xml(xml_text: &str) -> Result<BuildModel, BuildLoadError> {
    load_model(xml_text, &mut |_| Ok(()))
}

/// What loading a build leaves in the ConfigTab: the model, the active set's state after `BuildModList`, and its lists.
#[derive(Debug)]
pub struct LoadedBuild {
    pub model: BuildModel,
    /// `configSets[activeConfigSetId]` after the last `BuildModList` + `ConfigTab.enemyLevel`.
    pub config: ConfigState,
    pub config_lists: ConfigModLists,
    /// Upstream load steps this port skips (explicit, never silent).
    pub unimplemented: Vec<&'static str>,
}

/// `loadBuildFromXML` as far as pob-build models it: [`load_build_xml`]; each `<Config>`'s `SetActiveConfigSet` ->
/// `BuildModList` (ConfigTab.lua:937, :1358 -- its Lua errors, e.g. an unknown `presetBossSkills`, fail the load); without
/// `<Config>` the ctor's set with Build.lua:649-651's `bandit`/pantheons (D17); then CalcSetup.lua:663-673's pantheon
/// lookup, which fails in `PantheonTools.lua:7` for a god `data.pantheons` does not have.
pub fn load_build(xml_text: &str) -> Result<LoadedBuild, BuildLoadError> {
    let mut last: Option<(ConfigState, ConfigModLists)> = None;
    let model = load_model(xml_text, &mut |m| {
        let mut state = ConfigState::from_set(active_config(m));
        let lists = state.build_mod_list(m.level).map_err(|e| BuildLoadError::runtime(&e.0))?;
        last = Some((state, lists));
        Ok(())
    })?;
    let (config, config_lists) = match last {
        Some(x) => x,
        None => {
            let mut state = ConfigState::from_build(&model);
            let lists = state.build_mod_list(model.level).map_err(|e| BuildLoadError::runtime(&e.0))?;
            (state, lists)
        }
    };
    for key in ["pantheonMajorGod", "pantheonMinorGod"] {
        let god = config.input.get(key);
        if matches!(god, Some(InputValue::Str(g)) if g == "None") {
            continue;
        }
        if !matches!(god, Some(InputValue::Str(g)) if pob_data::pantheons::pantheons().get(g.as_str()).is_some()) {
            return Err(BuildLoadError::runtime(
                "Modules/PantheonTools.lua:7: attempt to index local 'god' (a nil value)",
            ));
        }
    }
    // Build.lua:898-913 `EstimatePlayerProgress` runs after the calc (`calcsTab:BuildOutput()` -> `RefreshStatList()`), so
    // it lives in `pob_calc::calculate`; `LoadedBuild` keeps the `<Build level>` value.
    let unimplemented = Vec::new();
    Ok(LoadedBuild {
        model,
        config,
        config_lists,
        unimplemented,
    })
}

/// The active `<Spec>` (index clamped as TreeTab.lua:526/:551).
pub fn active_spec(m: &BuildModel) -> &SpecModel {
    let idx = js_min(m.tree.active_spec, m.tree.specs.len() as f64);
    if idx >= 1.0
        && idx.fract() == 0.0
        && let Some(s) = m.tree.specs.get(idx as usize - 1)
    {
        return s;
    }
    &m.tree.specs[0]
}

pub fn active_config(m: &BuildModel) -> &ConfigSetModel {
    m.config
        .sets
        .iter()
        .find(|s| s.id == m.config.active_config_set)
        .unwrap_or(&m.config.sets[0])
}

pub fn active_skill_groups(m: &BuildModel) -> &[SocketGroupModel] {
    m.skills
        .sets
        .iter()
        .find(|s| s.id == m.skills.active_skill_set)
        .or(m.skills.sets.first())
        .map_or(&[], |s| &s.groups)
}

pub fn active_item_set(m: &BuildModel) -> &ItemSetModel {
    m.item_sets.iter().find(|s| s.id == m.active_item_set).unwrap_or(&m.item_sets[0])
}

/// `getStats()._meta` from the model alone (pob-ts `metaOf`): `alloc_nodes`/`socket_groups` are raw XML counts until the
/// tree/skills are resolved (R3.6). `mainSocketGroup` = CalcSetup.lua:1735-1737 `m_min(m_max(#groups, 1), main)`.
pub fn meta_of(m: &BuildModel, tree: &TreeJson) -> Meta {
    let spec = active_spec(m);
    let cls = if spec.class_id.fract() == 0.0 {
        tree.class(spec.class_id as i64)
    } else {
        None
    };
    let groups = active_skill_groups(m);
    let ascend_name = if spec.ascend_class_id == 0.0 {
        "None".to_owned()
    } else {
        cls.and_then(|c| c.ascendancies.as_ref())
            .and_then(|a| {
                let i = spec.ascend_class_id - 1.0;
                if i >= 0.0 && i.fract() == 0.0 { a.get(i as usize) } else { None }
            })
            .map_or_else(String::new, |a| a.name.clone())
    };
    Meta {
        alloc_nodes: spec.nodes.len() as f64,
        items: m.items.len() as f64,
        socket_groups: groups.len() as f64,
        main_socket_group: min2(max2(groups.len() as f64, 1.0), m.main_socket_group),
        tree_version: spec.tree_version.clone(),
        class_name: cls.map_or_else(String::new, |c| c.name.clone()),
        ascend_name,
    }
}
