//! `load_build_xml` -- XML -> `BuildModel`, ported from pob-ts `src/build/load.ts` (itself ported from the Lua tab `Load`s:
//! Build.lua:976-1015, TreeTab.lua:500-527, PassiveSpec.lua:103-195, ItemsTab.lua:1216-1327, SkillsTab.lua:440-570,
//! ConfigTab.lua:839-938, CalcsTab.lua:163-180). Pure: no state survives a call (W12).

use indexmap::IndexMap;
use pob_data::tree::TreeJson;
use pob_lua::number::{max2, min2};
use pob_lua::{Capture, Repl, gmatch, gsub, tonumber};

use crate::model::*;
use crate::sanitise::sanitise_text;
use crate::xml::{XmlElement, XmlError, parse_xml};

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

fn load_build(el: &XmlElement, m: &mut BuildModel) {
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
        secondary_ascend_class_id: 0.0,
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
    spec.secondary_ascend_class_id = num(el, "secondaryAscendClassId").unwrap_or(0.0);
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

fn load_custom_mods_block(el: &XmlElement) -> CustomModsBlockModel {
    CustomModsBlockModel {
        title: attr(el, "title").unwrap_or_else(|| "Default".into()),
        enabled: default_true(el, "enabled"),
        text: el.text(),
    }
}

/// ConfigTab.lua:843-881 `setInputAndPlaceholder`.
fn apply_input(node: &XmlElement, set: &mut ConfigSetModel) {
    let Some(name) = node.attr("name") else { return };
    match node.name.as_str() {
        "Input" => {
            if let Some(n) = node.attr("number") {
                if let Some(n) = tonumber(n) {
                    set.input.insert(name.to_owned(), InputValue::Num(n));
                }
            } else if let Some(s) = node.attr("string") {
                set.input.insert(name.to_owned(), InputValue::Str(config_string_value(name, s)));
            } else if node.attr("boolean").is_some() {
                set.input.insert(name.to_owned(), InputValue::Bool(is_true(node, "boolean")));
            }
        }
        "Placeholder" => {
            if let Some(n) = node.attr("number") {
                if let Some(n) = tonumber(n) {
                    set.placeholder.insert(name.to_owned(), n);
                }
            } else if let Some(s) = node.attr("string") {
                // upstream quirk, ConfigTab.lua:876: a string Placeholder is written into `input` -- verbatim.
                set.input.insert(name.to_owned(), InputValue::Str(s.to_owned()));
            }
        }
        _ => {}
    }
}

/// ConfigTab.lua:839-938. `input`/`placeholder` carry the XML's own values only (defaults/apply: R3.5).
fn load_config(el: &XmlElement) -> ConfigModel {
    let mut sets: Vec<ConfigSetModel> = Vec::new();
    fn ensure(sets: &mut Vec<ConfigSetModel>, id: f64, title: &str) -> usize {
        if let Some(i) = sets.iter().position(|s| s.id == id) {
            return i;
        }
        sets.push(ConfigSetModel {
            id,
            title: title.to_owned(),
            input: IndexMap::new(),
            placeholder: IndexMap::new(),
            custom_mods_list: Vec::new(),
        });
        sets.len() - 1
    }
    let top: Vec<&XmlElement> = el.child_elements().collect();
    if top.is_empty() {
        ensure(&mut sets, 1.0, "Default"); // ConfigTab.lua:885 `xml.empty`
    }
    for node in top {
        if node.name != "ConfigSet" {
            let i = ensure(&mut sets, 1.0, "Default");
            if node.name == "CustomModifierBlock" {
                sets[i].custom_mods_list.push(load_custom_mods_block(node));
            } else {
                apply_input(node, &mut sets[i]);
            }
        } else {
            let title = attr(node, "title").unwrap_or_else(|| "Default".into());
            let i = ensure(&mut sets, num(node, "id").unwrap_or(0.0), &title);
            sets[i].title = title;
            sets[i].custom_mods_list.clear();
            for child in node.child_elements() {
                if child.name == "CustomModifierBlock" {
                    sets[i].custom_mods_list.push(load_custom_mods_block(child));
                } else {
                    apply_input(child, &mut sets[i]);
                }
            }
        }
    }
    for set in &mut sets {
        // Legacy customMods migration, ConfigTab.lua:923-935.
        let legacy_text = match set.input.get("customMods") {
            Some(InputValue::Str(s)) => s.clone(),
            _ => String::new(),
        };
        let blank = set.custom_mods_list.is_empty() || (set.custom_mods_list.len() == 1 && set.custom_mods_list[0].text.is_empty());
        if !legacy_text.is_empty() && blank {
            set.custom_mods_list = vec![CustomModsBlockModel {
                title: "Default".into(),
                enabled: true,
                text: legacy_text,
            }];
        } else if set.custom_mods_list.is_empty() {
            set.custom_mods_list = vec![CustomModsBlockModel {
                title: "Default".into(),
                enabled: true,
                text: String::new(),
            }];
        }
        set.input.shift_remove("customMods");
    }
    let requested = num(el, "activeConfigSet").unwrap_or(1.0);
    let active_config_set = if sets.iter().any(|s| s.id == requested) {
        requested
    } else {
        sets[0].id
    };
    ConfigModel { sets, active_config_set }
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

/// Parse a build XML and load every section the calc reads.
pub fn load_build_xml(xml_text: &str) -> Result<BuildModel, XmlError> {
    let root = parse_xml(xml_text)?;
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
        config: load_config(&empty_element("Config")),
        calcs_input: IndexMap::new(),
        unknown_sections: Vec::new(),
        has_config_section: false,
    };
    for el in root.child_elements() {
        match el.name.as_str() {
            "Build" => load_build(el, &mut m),
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
                m.config = load_config(el);
                m.has_config_section = true;
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
