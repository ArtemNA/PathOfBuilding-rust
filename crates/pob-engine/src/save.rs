//! `Build:SaveDB` (Modules/Build.lua:2277-2302) -- the build back to XML from the state a load + `BuildOutput` leave (R8.8,
//! `KNOWLEDGE/R8-ARCH.md` §1 п. 2, decision R8.2 (b)): `Build:Save` (:1017-1098) and the `Save` of every saver (:629-639) --
//! TreeTab/PassiveSpec, SkillsTab, ItemsTab, ConfigTab, CalcsTab, NotesTab, ImportTab, PartyTab, PassiveTreeView -- composed with
//! `xml.lua` `ComposeXML` (`pob_build::compose_xml`).
//!
//! Order: upstream 3dbca4463 (#10337/#10338/#10339/#10340) made SaveDB deterministic -- sections `pairsSortByKey` (Build.lua:2288),
//! attributes sorted by key (xml.lua:120-140, `pob_build::compose_xml`), ConfigTab `Input`/`Placeholder` (:970/:983) and CalcsTab
//! `Input` (:204) `pairsSortByKey`, ItemsTab `Slot` in `orderedSlots` order (:1439), a spec's `nodes`, `masteryEffects`, `Sockets`,
//! `Overrides` and URL sorted by node id (PassiveSpec.lua:198-260, :555). Before that these were LuaJIT `pairs` orders (R13/R11).
//!
//! The state that `pob_build::BuildModel` does not keep (Notes, Party, Import, TreeView, the Calcs collapse states, the items tab's
//! `showStatDifferences`) is read from the XML by [`SaveExtras::from_xml`] -- the `Load` of those tabs.

use std::collections::HashMap;

use indexmap::IndexMap;
use pob_calc::pob_build::{self, BlockText, BuildModel, ConfigSetModel, ConfigState, InputValue, SpecModel, XmlElement, XmlNode};
use pob_calc::{CalculateResult, FullDpsSkill};
use pob_tree::PassiveSpec;

use crate::display::{DisplayEnv, MinionBorrow, match_flags};
use crate::lua::LuaV;
use crate::stats::{DISPLAY_STATS, EXTRA_SAVE_STATS, MINION_DISPLAY_STATS};

// --- element helpers -------------------------------------------------------------------------------------------------------------

fn elem(name: &str) -> XmlElement {
    XmlElement {
        name: name.to_owned(),
        attrib: IndexMap::new(),
        children: Vec::new(),
        empty: false,
    }
}

/// `attrib = { k = v, ... }`: a nil (`None`) value leaves the key out.
fn with(mut e: XmlElement, attrs: &[(&str, Option<String>)]) -> XmlElement {
    for (k, v) in attrs {
        if let Some(v) = v {
            e.attrib.insert((*k).to_owned(), v.clone());
        }
    }
    e
}

fn push(parent: &mut XmlElement, child: XmlElement) {
    parent.children.push(XmlNode::Element(child));
}

fn text(parent: &mut XmlElement, t: &str) {
    parent.children.push(XmlNode::Text(t.to_owned()));
}

/// `tostring(n)`.
fn ts(n: f64) -> String {
    pob_lua::tostring_num(n)
}

/// `tostring(b)`.
fn tb(b: bool) -> String {
    if b { "true" } else { "false" }.to_owned()
}

/// `tostring(x)` of an optional number (`"nil"` for nil).
fn tsn(n: Option<f64>) -> String {
    n.map_or_else(|| "nil".to_owned(), ts)
}

fn tbn(b: Option<bool>) -> String {
    b.map_or_else(|| "nil".to_owned(), tb)
}

/// `tostring(v)` of an output value; a table would print its address (not reproducible).
fn tostring_lua(v: &LuaV<'_>, what: &str) -> Result<String, String> {
    match v {
        LuaV::Nil => Ok("nil".into()),
        LuaV::Bool(b) => Ok(tb(*b)),
        LuaV::Num(n) => Ok(ts(*n)),
        LuaV::Str(s) => Ok(s.to_string()),
        LuaV::Table(_) => Err(format!(
            "Modules/Build.lua:1058 tostring of the table output.{what} (an address; not reproducible)"
        )),
    }
}

/// `tonumber(s)` of an XML attribute.
fn attr_num(e: &XmlElement, k: &str) -> Option<f64> {
    e.attr(k).and_then(pob_lua::tonumber)
}

// --- the tab state BuildModel does not keep ------------------------------------------------------------------------------------

/// PartyTab (PartyTab.lua:516-573): the edit buffers and the three controls.
#[derive(Clone, Debug, PartialEq)]
pub struct PartyState {
    /// `(name, text)` of each `ImportedBuffs` buffer, PartyTab:Save order (:575-611).
    pub buffers: IndexMap<String, String>,
    pub destination: String,
    pub append: bool,
    pub show_advance_tools: bool,
}

/// PartyTab.lua:39 `partyDestinations`.
const PARTY_DESTINATIONS: [&str; 8] = [
    "All",
    "Party Member Stats",
    "Aura",
    "Curse",
    "Warcry Skills",
    "Link Skills",
    "EnemyConditions",
    "EnemyMods",
];

/// PartyTab:Save (:575-611) buffer order: `(ImportedBuffs name, buffer)`.
const PARTY_BUFFERS: [&str; 7] = [
    "PartyMemberStats",
    "Aura",
    "Curse",
    "Warcry Skills",
    "Link Skills",
    "EnemyConditions",
    "EnemyMods",
];

/// ImportTab (ImportTab.lua:742-757) + `build.importLink` (Build.lua:2244-2252).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportState {
    pub last_realm: Option<String>,
    pub last_league: Option<String>,
    pub last_account_hash: Option<String>,
    pub last_character_hash: Option<String>,
    pub import_link: Option<String>,
    pub export_party: bool,
    pub build_import_link: Option<String>,
}

/// PassiveTreeView (PassiveTreeView.lua:73-104): `zoomLevel`/`zoomX`/`zoomY` are numbers or nil (`tonumber` of the attribute).
#[derive(Clone, Debug, PartialEq)]
pub struct TreeViewState {
    pub zoom_level: Option<f64>,
    pub zoom_x: Option<f64>,
    pub zoom_y: Option<f64>,
    pub search_str: String,
    pub show_stat_differences: bool,
}

/// What `Load` of the tabs outside `BuildModel` leaves: the input of their `Save`.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveExtras {
    /// NotesTab: `controls.edit.buf` (the last text node, NotesTab.lua:75-82).
    pub notes: String,
    pub party: PartyState,
    pub import: ImportState,
    pub tree_view: TreeViewState,
    /// CalcsTab:Load (:184-195): `subsection.collapsed` by `(section id, subsection id)` -- only the first section of an id.
    pub calcs_collapsed: HashMap<(String, String), bool>,
    /// ItemsTab `showStatDifferences` (ctor true, :1323-1325).
    pub items_show_stat_differences: bool,
}

impl Default for SaveExtras {
    fn default() -> SaveExtras {
        SaveExtras {
            notes: String::new(),
            party: PartyState {
                buffers: IndexMap::new(),
                destination: "All".into(),
                append: false,
                show_advance_tools: false,
            },
            import: ImportState::default(),
            tree_view: TreeViewState {
                zoom_level: Some(3.0),
                zoom_x: Some(0.0),
                zoom_y: Some(0.0),
                search_str: String::new(),
                show_stat_differences: true,
            },
            calcs_collapsed: HashMap::new(),
            items_show_stat_differences: true,
        }
    }
}

impl SaveExtras {
    /// The `Load` of NotesTab, PartyTab, ImportTab, PassiveTreeView, the CalcsTab `Section`s and ItemsTab's own attributes, over the
    /// sections of `dbXML[1]` in document order (Build.lua:2236-2263, :655-677).
    pub fn from_xml(xml_text: &str) -> Result<SaveExtras, String> {
        let doc = pob_build::parse_xml_document(xml_text).map_err(|e| format!("XML: {e}"))?;
        let root = match doc.first() {
            Some(XmlNode::Element(e)) if e.name == "PathOfBuilding" => e,
            _ => return Err("'PathOfBuilding' root element missing".into()),
        };
        let mut x = SaveExtras::default();
        let sections = crate::calcs::sections();
        for node in root.child_elements() {
            match node.name.as_str() {
                // NotesTab.lua:75-82
                "Notes" => {
                    for c in &node.children {
                        if let XmlNode::Text(t) = c {
                            x.notes.clone_from(t);
                        }
                    }
                }
                "Party" => x.party_load(node)?,
                // ImportTab.lua:742-757
                "Import" => {
                    let a = |k: &str| node.attr(k).map(str::to_owned);
                    x.import.last_realm = a("lastRealm");
                    x.import.last_league = a("lastLeague");
                    x.import.last_account_hash = a("lastAccountHash");
                    x.import.import_link = a("importLink");
                    x.import.export_party = node.attr("exportParty") == Some("true");
                    x.import.last_character_hash = a("lastCharacterHash");
                }
                // PassiveTreeView.lua:87-104
                "TreeView" => {
                    let tv = &mut x.tree_view;
                    if node.attr("zoomLevel").is_some() {
                        tv.zoom_level = attr_num(node, "zoomLevel");
                    }
                    if node.attr("zoomX").is_some() && node.attr("zoomY").is_some() {
                        tv.zoom_x = attr_num(node, "zoomX");
                        tv.zoom_y = attr_num(node, "zoomY");
                    }
                    if let Some(s) = node.attr("searchStr") {
                        tv.search_str = s.to_owned();
                    }
                    if let Some(s) = node.attr("showStatDifferences") {
                        tv.show_stat_differences = s == "true";
                    }
                }
                // CalcsTab.lua:163-200 (the `Input`s are in the model)
                "Calcs" => {
                    for c in node.child_elements().filter(|c| c.name == "Section") {
                        let Some(id) = c.attr("id") else {
                            return Err("Classes/CalcsTab.lua:181 'Section' element missing id attribute".into());
                        };
                        if let (Some(sec), Some(sub)) = (sections.iter().find(|s| s.id == id), c.attr("subsection"))
                            && sec.sub_sections.iter().any(|s| sub_id(&s.label) == sub)
                        {
                            x.calcs_collapsed
                                .insert((id.to_owned(), sub.to_owned()), c.attr("collapsed") == Some("true"));
                        }
                    }
                }
                // ItemsTab.lua:1220/:1296-1313/:1323-1325
                "Items" => {
                    if let Some(s) = node.attr("showStatDifferences") {
                        x.items_show_stat_differences = s == "true";
                    }
                    for tsw in node.child_elements().filter(|c| c.name == "TradeSearchWeights") {
                        if tsw.child_elements().next().is_some() {
                            return Err(
                                "unimplemented: Classes/ItemsTab.lua:1297-1312 TradeSearchWeights stats (data.powerStatList)".into(),
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        // Build.lua:2244-2252: the first `<Import>`'s importLink
        x.import.build_import_link = root
            .child_elements()
            .find(|n| n.name == "Import")
            .and_then(|n| n.attr("importLink"))
            .map(str::to_owned);
        Ok(x)
    }

    /// PartyTab.lua:516-573 (the buffers' `ParseBuffs` feed the calc -- not modelled by the calc either).
    fn party_load(&mut self, node: &XmlElement) -> Result<(), String> {
        for c in node.child_elements() {
            if c.name == "ImportedBuffs" {
                let Some(name) = c.attr("name") else { continue };
                // `node[1] or ""`
                let t = match c.children.first() {
                    Some(XmlNode::Text(t)) => t.clone(),
                    Some(XmlNode::Element(_)) => {
                        return Err(format!(
                            "Classes/PartyTab.lua:523 ImportedBuffs '{name}': node[1] is an element (SetText of a table)"
                        ));
                    }
                    None => String::new(),
                };
                let key = match name {
                    "PartyMemberStats" | "Aura" | "Curse" | "Warcry Skills" | "Link Skills" | "EnemyConditions" | "EnemyMods" => name,
                    _ => continue,
                };
                self.party.buffers.insert(key.to_owned(), t);
            }
        }
        // :566 `SelByValue(destination or "All")`: an unknown value keeps the selection (index 1 after the ctor)
        if let Some(d) = PARTY_DESTINATIONS.iter().find(|d| **d == node.attr("destination").unwrap_or("All")) {
            (*d).clone_into(&mut self.party.destination);
        }
        self.party.append = node.attr("append") == Some("true");
        self.party.show_advance_tools = node.attr("ShowAdvanceTools") == Some("true");
        Ok(())
    }
}

/// CalcSectionControl.lua:33 `subSec.id = subSec.label:gsub("%W", "")`.
pub fn sub_id(label: &str) -> String {
    label.chars().filter(char::is_ascii_alphanumeric).collect()
}

// --- the saved state of the main run ----------------------------------------------------------------------------------------------

/// What [`save_db`] reads: the model the session runs on, the tab state outside it, the MAIN result (`calcsTab.mainEnv` /
/// `mainOutput`) and `mainOutput.SkillDPS` in the order the sidebar's in-place `table.sort`s left it (Build.lua:1934).
pub struct SaveInput<'a> {
    pub model: &'a BuildModel,
    pub extras: &'a SaveExtras,
    pub res: &'a CalculateResult,
    pub skill_dps: Option<&'a [FullDpsSkill]>,
    /// `calcsTab.calcsEnv` (`buildOutput(CALCS)`, run after MAIN on the same tab tables upstream): `calcsInput.skill_number`
    /// (CalcSetup.lua:1586), `group.mainActiveSkillCalcs` (:1883) and the gems' `*Calcs` fields (CalcActiveSkill.lua:251/:697/...) come
    /// from it. `None` = no CALCS run (those fields as the MAIN run left them).
    pub calcs_env: Option<&'a pob_calc::Env>,
}

/// `Build:SaveDB` -- the `<PathOfBuilding>` tree (compose with [`pob_build::compose_xml`]).
pub fn save_db(inp: &SaveInput<'_>) -> Result<XmlElement, String> {
    let mut root = elem("PathOfBuilding");
    push(&mut root, save_build(inp)?);
    push(&mut root, save_calcs(inp));
    push(&mut root, save_config(inp)?);
    push(&mut root, save_import(inp.extras));
    push(&mut root, save_items(inp)?);
    let mut notes = elem("Notes");
    // NotesTab.lua:84-88 `SetShowColorCodes(false)` then the buffer
    let n = pob_lua::gsub(
        inp.extras.notes.as_bytes(),
        b"%^_x(%x%x%x%x%x%x)",
        pob_lua::Repl::Str(b"^x%1"),
        None,
    )
    .map_err(|e| e.0)?
    .0;
    let n = pob_lua::gsub(&n, b"%^_(%d)", pob_lua::Repl::Str(b"^%1"), None).map_err(|e| e.0)?.0;
    text(&mut notes, &String::from_utf8(n).map_err(|e| e.to_string())?);
    push(&mut root, notes);
    push(&mut root, save_party(inp)?);
    push(&mut root, save_skills(inp)?);
    push(&mut root, save_tree(inp)?);
    push(&mut root, save_tree_view(inp.extras));
    Ok(root)
}

/// `SaveDB` composed: the XML text.
pub fn save_db_xml(inp: &SaveInput<'_>) -> Result<String, String> {
    Ok(pob_build::compose_xml(&save_db(inp)?))
}

// --- Build:Save (Build.lua:1017-1098) ---------------------------------------------------------------------------------------------

fn save_build(inp: &SaveInput<'_>) -> Result<XmlElement, String> {
    let (m, env) = (inp.model, &inp.res.env);
    let cfg = |k: &str| match env.config.input.get(k) {
        Some(InputValue::Str(s)) => Ok(Some(s.clone())),
        None => Ok(None),
        Some(v) => Err(format!(
            "Modules/Build.lua:1024 configTab.input.{k} is {v:?} (a non-string attribute)"
        )),
    };
    let mut b = with(
        elem("Build"),
        &[
            ("targetVersion", Some(m.target_version.clone())),
            ("viewMode", Some(m.view_mode.clone().unwrap_or_else(|| "TREE".into()))),
            // `characterLevel`: the model's level, which the engine moves as `EstimatePlayerProgress` does (auto mode)
            ("level", Some(ts(m.level))),
            ("className", Some(env.spec.cur_class_name.clone())),
            ("ascendClassName", Some(env.spec.cur_ascend_class_name.clone())),
            ("bandit", cfg("bandit")?),
            ("pantheonMajorGod", cfg("pantheonMajorGod")?),
            ("pantheonMinorGod", cfg("pantheonMinorGod")?),
            ("mainSocketGroup", Some(ts(env.main_socket_group))),
            ("characterLevelAutoMode", Some(tb(m.character_level_auto_mode))),
        ],
    );
    // :1030-1032 (Build.lua:989-992 kept only `data.minions` ids)
    for id in m.spectres.iter().filter(|id| pob_calc::skill_modlist::is_minion(id)) {
        push(&mut b, with(elem("Spectre"), &[("id", Some(id.clone()))]));
    }
    let minion = MinionBorrow::new(inp.res);
    let denv = DisplayEnv::new(inp.res, &minion)?;
    // :1033-1064
    let mut added: Vec<String> = Vec::new();
    for sd in DISPLAY_STATS {
        if !match_flags(sd.flag, sd.not_flag, denv.player.skill_flags) {
            continue;
        }
        let Some(stat) = sd.stat else { continue };
        let name = format!("{stat}{}", sd.child_stat.unwrap_or(""));
        if added.contains(&name) {
            continue;
        }
        if stat == "SkillDPS" {
            let list = inp
                .skill_dps
                .ok_or("Modules/Build.lua:1040: bad argument #1 to 'ipairs' (table expected, got nil)")?;
            for s in list {
                let lhs = if s.count >= 2.0 {
                    format!("{}x {}", ts(s.count), s.name)
                } else {
                    s.name.clone()
                };
                push(
                    &mut b,
                    with(
                        elem("FullDPSSkill"),
                        &[
                            ("stat", Some(lhs)),
                            ("value", Some(ts(s.dps * s.count))),
                            ("skillPart", Some(s.skill_part.clone().unwrap_or_default())),
                            ("source", Some(s.source.clone().or_else(|| s.trigger.clone()).unwrap_or_default())),
                        ],
                    ),
                );
            }
            added.push(name);
        } else {
            let mut v = denv.player.view.get(stat);
            if v.truthy()
                && let Some(c) = sd.child_stat
            {
                v = v.index(c)?;
            }
            // `statVal and (condFunc and condFunc(..) or true)` -- always true for a non-nil value
            if v.truthy() {
                push(
                    &mut b,
                    with(
                        elem("PlayerStat"),
                        &[("stat", Some(name.clone())), ("value", Some(tostring_lua(&v, stat)?))],
                    ),
                );
                added.push(name);
            }
        }
    }
    // :1065-1070
    for stat in EXTRA_SAVE_STATS {
        let v = denv.player.view.get(stat);
        if v.truthy() {
            push(
                &mut b,
                with(
                    elem("PlayerStat"),
                    &[("stat", Some((*stat).to_owned())), ("value", Some(tostring_lua(&v, stat)?))],
                ),
            );
        }
    }
    // :1071-1080
    if let Some(mn) = &denv.minion {
        for sd in MINION_DISPLAY_STATS {
            let Some(stat) = sd.stat else { continue };
            let v = mn.view.get(stat);
            if v.truthy() {
                push(
                    &mut b,
                    with(
                        elem("MinionStat"),
                        &[("stat", Some(stat.to_owned())), ("value", Some(tostring_lua(&v, stat)?))],
                    ),
                );
            }
        }
    }
    // :1081-1097 (Build.lua:81 defaults without a `<TimelessData>`; :993-1013 with one)
    let td = &m.timeless_data;
    let t = with(
        elem("TimelessData"),
        &[
            ("jewelTypeId", td.as_ref().and_then(|t| t.jewel_type_id).map(ts)),
            ("conquerorTypeId", td.as_ref().and_then(|t| t.conqueror_type_id).map(ts)),
            ("devotionVariant1", Some(ts(td.as_ref().map_or(1.0, |t| t.devotion_variant1)))),
            ("devotionVariant2", Some(ts(td.as_ref().map_or(1.0, |t| t.devotion_variant2)))),
            ("jewelSocketId", td.as_ref().and_then(|t| t.jewel_socket_id).map(ts)),
            (
                "fallbackWeightModeIdx",
                td.as_ref().and_then(|t| t.fallback_weight_mode_idx).map(ts),
            ),
            ("socketFilter", td.as_ref().filter(|t| t.socket_filter).map(|_| "true".into())),
            ("socketAllocate", td.as_ref().filter(|t| t.socket_allocate).map(|_| "true".into())),
            ("socketFilterDistance", td.as_ref().map(|t| ts(t.socket_filter_distance))),
            (
                "searchList",
                match td {
                    None => Some(String::new()),
                    Some(t) => t.search_list.clone(),
                },
            ),
            (
                "searchListFallback",
                match td {
                    None => Some(String::new()),
                    Some(t) => t.search_list_fallback.clone(),
                },
            ),
        ],
    );
    push(&mut b, t);
    Ok(b)
}

// --- CalcsTab:Save (CalcsTab.lua:203-224) ------------------------------------------------------------------------------------------

/// `calcsTab.input`: the ctor's `skill_number = 1`, `misc_buffMode = "EFFECTIVE"` (:34-36), then the XML's (:166-177).
pub fn calcs_input(m: &BuildModel) -> IndexMap<String, InputValue> {
    let mut input = IndexMap::new();
    input.insert("skill_number".to_owned(), InputValue::Num(1.0));
    input.insert("misc_buffMode".to_owned(), InputValue::Str("EFFECTIVE".into()));
    for (k, v) in &m.calcs_input {
        input.insert(k.clone(), v.clone());
    }
    input
}

fn input_attr(e: XmlElement, v: &InputValue, string_kind: bool) -> XmlElement {
    match v {
        InputValue::Num(n) => with(e, &[("number", Some(ts(*n)))]),
        InputValue::Bool(b) if string_kind => with(e, &[("string", Some(tb(*b)))]),
        InputValue::Bool(b) => with(e, &[("boolean", Some(tb(*b)))]),
        InputValue::Str(s) => with(e, &[("string", Some(s.clone()))]),
    }
}

fn save_calcs(inp: &SaveInput<'_>) -> XmlElement {
    let mut c = elem("Calcs");
    let mut input = calcs_input(inp.model);
    if let Some(env) = inp.calcs_env {
        for (k, v) in &env.calcs_input {
            input.insert(k.clone(), v.clone());
        }
    }
    // CalcsTab.lua:204 `pairsSortByKey(self.input)` (3dbca4463 #10338)
    input.sort_by(|a, _, b, _| a.as_bytes().cmp(b.as_bytes()));
    for (k, v) in input {
        push(&mut c, input_attr(with(elem("Input"), &[("name", Some(k))]), &v, false));
    }
    for sec in crate::calcs::sections() {
        for sub in &sec.sub_sections {
            let id = sub_id(&sub.label);
            let collapsed = inp
                .extras
                .calcs_collapsed
                .get(&(sec.id.clone(), id.clone()))
                .copied()
                .unwrap_or(sub.default_collapsed);
            push(
                &mut c,
                with(
                    elem("Section"),
                    &[
                        ("id", Some(sec.id.clone())),
                        ("subsection", Some(id)),
                        ("collapsed", Some(tb(collapsed))),
                    ],
                ),
            );
        }
    }
    c
}

// --- ConfigTab:Save (ConfigTab.lua:961-1005) -----------------------------------------------------------------------------------------

/// ConfigTab.lua:682-692 `self.defaultState[var]`.
pub fn default_state() -> &'static IndexMap<String, InputValue> {
    static D: std::sync::OnceLock<IndexMap<String, InputValue>> = std::sync::OnceLock::new();
    D.get_or_init(|| {
        let conv = |s: &pob_data::wire::Scalar| match s {
            pob_data::wire::Scalar::Num(n) => InputValue::Num(*n),
            pob_data::wire::Scalar::Str(t) => InputValue::Str(t.clone()),
            pob_data::wire::Scalar::Bool(b) => InputValue::Bool(*b),
        };
        let mut d = IndexMap::new();
        for v in &pob_data::config_options::config_options().vars {
            let Some(name) = &v.var else { continue };
            let ds = v.default_state.as_ref().map(conv);
            let val = match v.ty.as_deref() {
                Some("check") => Some(ds.unwrap_or(InputValue::Bool(false))),
                Some("count" | "integer" | "countAllowZero" | "float") => Some(ds.unwrap_or(InputValue::Num(0.0))),
                Some("list") => v
                    .list
                    .as_ref()
                    .and_then(|l| l.get(v.default_index.map_or(0, |i| i as usize - 1)))
                    .map(|e| conv(&e.val)),
                Some("text") => Some(ds.unwrap_or(InputValue::Str(String::new()))),
                _ => ds,
            };
            match val {
                Some(x) => {
                    d.insert(name.clone(), x);
                }
                None => {
                    d.shift_remove(name);
                }
            }
        }
        d
    })
}

/// `ConfigTab:GetDefaultState(var, type(v))` (:941-959) -- the ACTIVE set's placeholder first.
fn get_default_state(active_placeholder: &IndexMap<String, InputValue>, var: &str, v: &InputValue) -> InputValue {
    if let Some(p) = active_placeholder.get(var) {
        return p.clone();
    }
    if let Some(d) = default_state().get(var) {
        return d.clone();
    }
    match v {
        InputValue::Num(_) => InputValue::Num(0.0),
        InputValue::Bool(_) => InputValue::Bool(false),
        InputValue::Str(_) => InputValue::Str(String::new()),
    }
}

fn save_config(inp: &SaveInput<'_>) -> Result<XmlElement, String> {
    let m = inp.model;
    let active = &inp.res.env.config;
    let mut c = with(elem("Config"), &[("activeConfigSet", Some(ts(m.config.active_config_set)))]);
    // `ipairs(self.configSetOrderList)`: sparse `[index] = id`, stops at the first hole
    let mut order = m.config.set_order_list.clone();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut expect = 1.0;
    for (index, id) in order {
        if index != expect {
            if index < expect {
                continue;
            }
            break;
        }
        expect += 1.0;
        let set = m
            .config
            .sets
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("Classes/ConfigTab.lua:966 configSets[{}] is nil", ts(id)))?;
        let state = if id == m.config.active_config_set {
            None
        } else {
            Some(ConfigState::from_set(set))
        };
        let st = state.as_ref().unwrap_or(active);
        push(&mut c, save_config_set(set, st, &active.placeholder)?);
    }
    Ok(c)
}

fn save_config_set(
    set: &ConfigSetModel,
    st: &ConfigState,
    active_placeholder: &IndexMap<String, InputValue>,
) -> Result<XmlElement, String> {
    let mut child = with(elem("ConfigSet"), &[("id", Some(ts(set.id))), ("title", set.title.clone())]);
    // ConfigTab.lua:970/:983 `pairsSortByKey` (3dbca4463 #10338)
    let mut inputs: Vec<_> = st.input.iter().collect();
    inputs.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut placeholders: Vec<_> = st.placeholder.iter().collect();
    placeholders.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    for (k, v) in inputs {
        if *v != get_default_state(active_placeholder, k, v) {
            push(&mut child, input_attr(with(elem("Input"), &[("name", Some(k.clone()))]), v, false));
        }
    }
    for (k, v) in placeholders {
        // :983-990 -- a boolean placeholder goes to `string`
        push(
            &mut child,
            input_attr(with(elem("Placeholder"), &[("name", Some(k.clone()))]), v, true),
        );
    }
    for block in &st.custom_mods_list {
        let mut b = with(
            elem("CustomModifierBlock"),
            &[("title", Some(block.title.clone())), ("enabled", Some(tb(block.enabled)))],
        );
        match &block.text {
            BlockText::Str(s) => text(&mut b, s),
            other => {
                return Err(format!(
                    "Classes/ConfigTab.lua:998 CustomModifierBlock text {other:?} is not a string"
                ));
            }
        }
        push(&mut child, b);
    }
    Ok(child)
}

// --- ImportTab:Save (ImportTab.lua:759-773) -------------------------------------------------------------------------------------------

fn save_import(x: &SaveExtras) -> XmlElement {
    let i = &x.import;
    let link = i
        .build_import_link
        .clone()
        .or_else(|| i.import_link.clone())
        .filter(|l| l.len() < 100);
    with(
        elem("Import"),
        &[
            ("lastRealm", i.last_realm.clone()),
            ("lastLeague", i.last_league.clone()),
            ("lastAccountHash", i.last_account_hash.clone()),
            ("lastCharacterHash", i.last_character_hash.clone()),
            ("exportParty", Some(tb(i.export_party))),
            ("importLink", link),
        ],
    )
}

// --- ItemsTab:Save (ItemsTab.lua:1329-1416) -------------------------------------------------------------------------------------------

fn save_items(inp: &SaveInput<'_>) -> Result<XmlElement, String> {
    let (m, env) = (inp.model, &inp.res.env);
    let active_id = pob_build::active_item_set(m).id;
    let active_set = pob_build::active_item_set(m);
    let mut x = with(
        elem("Items"),
        &[
            ("activeItemSet", Some(ts(active_id))),
            ("useSecondWeaponSet", Some(tb(active_set.use_second_weapon_set))),
            ("showStatDifferences", Some(tb(inp.extras.items_show_stat_differences))),
        ],
    );
    // :1335-1384 -- `item:BuildAndParseRaw()` mutates the item upstream; here a copy (the run's items stay as calculated)
    for (&id, item) in &env.items {
        let mut it = (**item).clone();
        let mut child = with(
            elem("Item"),
            &[
                ("id", Some(ts(id as f64))),
                ("variant", it.variant.map(ts)),
                ("variantAlt", it.variant_alt[0].map(ts)),
                ("variantAlt2", it.variant_alt[1].map(ts)),
                ("variantAlt3", it.variant_alt[2].map(ts)),
                ("variantAlt4", it.variant_alt[3].map(ts)),
                ("variantAlt5", it.variant_alt[4].map(ts)),
            ],
        );
        it.build_and_parse_raw();
        text(&mut child, &it.raw);
        for (n, r) in mod_ranges(&it) {
            push(
                &mut child,
                with(elem("ModRange"), &[("id", Some(n.to_string())), ("range", Some(ts(r)))]),
            );
        }
        push(&mut x, child);
    }
    // :1385-1397 -- the active set holds what `SetActiveItemSet` + `PopulateSlots` left in the slots (ItemSlotControl:SetSelItemId writes
    // `activeItemSet[slotName].selItemId`); other sets: `NewItemSet` defaults + the XML.
    for set in &m.item_sets {
        let mut child = with(
            elem("ItemSet"),
            &[
                ("id", Some(ts(set.id))),
                ("title", set.title.clone()),
                ("useSecondWeaponSet", Some(tb(set.use_second_weapon_set))),
            ],
        );
        for slot in &env.ordered_slots {
            match slot.node_id {
                None => {
                    let entry = set.slots.get(&slot.slot_name);
                    let (sel, active) = if set.id == active_id {
                        (slot.sel_item_id, slot.active)
                    } else {
                        (entry.map_or(0.0, |e| e.item_id), entry.map(|e| e.active))
                    };
                    push(
                        &mut child,
                        with(
                            elem("Slot"),
                            &[
                                ("name", Some(slot.slot_name.clone())),
                                ("itemId", Some(ts(sel))),
                                ("itemPbURL", Some(entry.and_then(|e| e.pb_url.clone()).unwrap_or_default())),
                                ("active", active.filter(|a| *a).map(|_| "true".into())),
                            ],
                        ),
                    );
                }
                Some(node_id) if env.spec.alloc_nodes.contains_key(&node_id) => {
                    push(
                        &mut child,
                        with(
                            elem("SocketIdURL"),
                            &[
                                ("name", Some(slot.slot_name.clone())),
                                ("nodeId", Some(node_id.to_string())),
                                (
                                    "itemPbURL",
                                    Some(set.socket_id_urls.get(f64::from(node_id)).cloned().unwrap_or_default()),
                                ),
                            ],
                        ),
                    );
                }
                Some(_) => {}
            }
        }
        push(&mut x, child);
    }
    // :1398-1415 -- `statSortSelectionList` is `{ }` after Load (SaveExtras refuses stats)
    push(&mut x, elem("TradeSearchWeights"));
    Ok(x)
}

/// ItemsTab.lua:1350-1383: `(id, range)` of every mod line with a range, the ids counted over enchant, scourge, implicit, explicit and
/// crucible lines after the buff lines.
pub fn mod_ranges(it: &pob_items::Item) -> Vec<(usize, f64)> {
    let mut out = Vec::new();
    let mut n = it.buff_mod_lines.len() + 1;
    for list in [
        &it.enchant_mod_lines,
        &it.scourge_mod_lines,
        &it.implicit_mod_lines,
        &it.explicit_mod_lines,
        &it.crucible_mod_lines,
    ] {
        for line in list {
            if let Some(r) = line.range {
                out.push((n, r));
            }
            n += 1;
        }
    }
    out
}

// --- PartyTab:Save (PartyTab.lua:575-664) ---------------------------------------------------------------------------------------------

fn save_party(inp: &SaveInput<'_>) -> Result<XmlElement, String> {
    let p = &inp.extras.party;
    let mut x = elem("Party");
    for name in PARTY_BUFFERS {
        if let Some(t) = p.buffers.get(name).filter(|t| !t.is_empty()) {
            let mut c = with(elem("ImportedBuffs"), &[("name", Some(name.to_owned()))]);
            text(&mut c, t);
            push(&mut x, c);
        }
    }
    // :612-654 `exportBuffs` returns "" unless `enableExportBuffs` (ImportTab `exportParty`)
    if inp.extras.import.export_party {
        return Err("unimplemented: Classes/PartyTab.lua:612-654 ExportedBuffs (buffExports of CalcPerform.lua:2207)".into());
    }
    Ok(with(
        x,
        &[
            ("destination", Some(p.destination.clone())),
            ("append", Some(tb(p.append))),
            ("ShowAdvanceTools", Some(tb(p.show_advance_tools))),
        ],
    ))
}

// --- SkillsTab:Save (SkillsTab.lua:573-627) -------------------------------------------------------------------------------------------

/// `build.skillsTab` after `BuildOutput`: the MAIN run's tab with what the CALCS run wrote into the active set's groups (the `*Calcs`
/// fields only -- every other write of a CALCS run is the same as MAIN's: CalcSetup.lua:1883, CalcActiveSkill.lua:251/:268-269/:629-630/
/// :656-657/:697/:738/:748).
fn merged_tab(inp: &SaveInput<'_>) -> Result<pob_calc::SkillsTab, String> {
    let mut tab = inp
        .res
        .env
        .skills
        .as_ref()
        .ok_or("SkillsTab:Save needs the skill phase")?
        .tab
        .clone();
    let Some(calcs) = inp.calcs_env.and_then(|e| e.skills.as_ref()) else {
        return Ok(tab);
    };
    let src = calcs.tab.socket_group_list();
    let dst = tab.socket_group_list_mut();
    if src.len() != dst.len() {
        return Err(format!("SaveDB: the CALCS run has {} socket groups, MAIN {}", src.len(), dst.len()));
    }
    for (d, s) in dst.iter_mut().zip(src) {
        d.main_active_skill_calcs = s.main_active_skill_calcs;
        for (dg, sg) in d.gem_list.iter_mut().zip(&s.gem_list) {
            dg.skill_part_calcs = sg.skill_part_calcs;
            dg.skill_stage_count_calcs = sg.skill_stage_count_calcs;
            dg.skill_mine_count_calcs = sg.skill_mine_count_calcs;
            dg.skill_minion_calcs.clone_from(&sg.skill_minion_calcs);
            dg.skill_minion_item_set_calcs = sg.skill_minion_item_set_calcs;
            dg.skill_minion_skill_calcs = sg.skill_minion_skill_calcs;
        }
    }
    Ok(tab)
}

fn save_skills(inp: &SaveInput<'_>) -> Result<XmlElement, String> {
    let m = &inp.model.skills;
    let tab = &merged_tab(inp)?;
    let mut x = with(
        elem("Skills"),
        &[
            ("activeSkillSet", Some(ts(tab.active_skill_set_id))),
            ("defaultGemLevel", Some(tab.default_gem_level.clone())),
            ("defaultGemQuality", Some(ts(tab.default_gem_quality))),
            ("sortGemsByDPS", Some(tb(m.sort_gems_by_dps.unwrap_or(true)))),
            ("showSupportGemTypes", Some(m.show_support_gem_types.clone())),
            ("sortGemsByDPSField", Some(m.sort_gems_by_dps_field.clone())),
            ("showLegacyGems", Some(tb(m.show_legacy_gems.unwrap_or(false)))),
        ],
    );
    let gems = pob_data::gems::gems();
    for set in &tab.skill_sets {
        let mut child = with(elem("SkillSet"), &[("id", Some(ts(set.id))), ("title", set.title.clone())]);
        for g in &set.socket_group_list {
            let mut node = with(
                elem("Skill"),
                &[
                    ("enabled", Some(tb(g.enabled))),
                    ("includeInFullDPS", Some(tbn(g.include_in_full_dps))),
                    ("groupCount", g.group_count.map(ts)),
                    ("label", g.label.clone()),
                    ("slot", g.slot.clone()),
                    ("source", g.source.clone()),
                    ("mainActiveSkill", Some(tsn(g.main_active_skill))),
                    ("mainActiveSkillCalcs", Some(tsn(g.main_active_skill_calcs))),
                    ("imbuedSupport", g.imbued_support.clone()),
                ],
            );
            for gi in &g.gem_list {
                let gd = gi.gem_data.as_ref().and_then(|id| gems.gems.0.get(id));
                push(
                    &mut node,
                    with(
                        elem("Gem"),
                        &[
                            ("nameSpec", gi.name_spec.clone()),
                            ("skillId", gi.skill_id.clone()),
                            ("gemId", gd.map(|d| d.game_id.clone())),
                            ("variantId", gd.map(|d| d.variant_id.clone())),
                            ("level", Some(tsn(gi.level))),
                            ("quality", Some(tsn(gi.quality))),
                            ("enabled", Some(tb(gi.enabled))),
                            ("enableGlobal1", Some(tbn(gi.enable_global1))),
                            ("enableGlobal2", Some(tbn(gi.enable_global2))),
                            ("count", Some(tsn(gi.count))),
                            ("skillPart", gi.skill_part.map(ts)),
                            ("skillPartCalcs", gi.skill_part_calcs.map(ts)),
                            ("skillStageCount", gi.skill_stage_count.map(ts)),
                            ("skillStageCountCalcs", gi.skill_stage_count_calcs.map(ts)),
                            ("skillMineCount", gi.skill_mine_count.map(ts)),
                            ("skillMineCountCalcs", gi.skill_mine_count_calcs.map(ts)),
                            ("skillMinion", gi.skill_minion.clone()),
                            ("skillMinionCalcs", gi.skill_minion_calcs.clone()),
                            ("skillMinionItemSet", gi.skill_minion_item_set.map(ts)),
                            ("skillMinionItemSetCalcs", gi.skill_minion_item_set_calcs.map(ts)),
                            ("skillMinionSkill", gi.skill_minion_skill.map(ts)),
                            ("skillMinionSkillCalcs", gi.skill_minion_skill_calcs.map(ts)),
                        ],
                    ),
                );
            }
            push(&mut child, node);
        }
        push(&mut x, child);
    }
    Ok(x)
}

// --- TreeTab:Save / PassiveSpec:Save (TreeTab.lua:536-547, PassiveSpec.lua:197-249) ----------------------------------------------------

fn save_tree(inp: &SaveInput<'_>) -> Result<XmlElement, String> {
    save_tree_of(inp.model, &inp.res.env)
}

/// `TreeTab:Save(xml)` -- the `<Tree>` element of SaveDB from the model and the MAIN env (public for the tattoo gate, `tests/tattoo_save.rs`).
pub fn save_tree_of(m: &BuildModel, env: &pob_calc::Env) -> Result<XmlElement, String> {
    let specs = &m.tree.specs;
    // TreeTab.lua:526/:551 `activeSpec = m_min(specId, #specList)`
    let active = pob_lua::number::min2(m.tree.active_spec, specs.len().max(1) as f64);
    let mut x = with(elem("Tree"), &[("activeSpec", Some(ts(active)))]);
    let active_model = pob_build::active_spec(m);
    for sm in specs {
        let loaded;
        let spec: &PassiveSpec = if std::ptr::eq(sm, active_model) {
            &env.spec
        } else {
            // every `<Spec>` is a full PassiveSpec upstream (TreeTab.lua:510-519) with its PostLoad (:529-534); only the active one
            // gets the socket slots' Populate (TreeTab.lua:548-576)
            let jewels = pob_calc::init_env::spec_jewels_of(&sm.jewels, &env.items).map_err(|e| e.0)?;
            let mut s = PassiveSpec::load_inactive_mode_jewels(sm, pob_calc::ParseMode::Full, jewels).map_err(|e| e.0)?;
            pob_calc::init_env::post_load_spec(&mut s, &env.items).map_err(|e| e.0)?;
            loaded = s;
            &loaded
        };
        push(&mut x, save_spec(sm, spec)?);
    }
    Ok(x)
}

fn save_spec(sm: &SpecModel, spec: &PassiveSpec) -> Result<XmlElement, String> {
    // :198-202 `table.sort(allocNodeIdList)` (3dbca4463 #10338; was `pairs(self.allocNodes)`)
    let mut ids = spec.alloc_table_pairs();
    ids.sort_unstable();
    let nodes: Vec<String> = ids.iter().map(u32::to_string).collect();
    // :203-211 masteries sorted by node id
    let mut mastery_ids: Vec<_> = spec.mastery_selections.iter().collect();
    mastery_ids.sort_by_key(|(m, _)| **m);
    let masteries: Vec<String> = mastery_ids
        .into_iter()
        .map(|(mastery, effect)| format!("{mastery},{}", ts(*effect)))
        .map(|s| format!("{{{s}}}"))
        .collect();
    let mut x = with(
        elem("Spec"),
        &[
            ("title", sm.title.clone()),
            ("treeVersion", Some(spec.tree_version.clone())),
            ("clusterHashFormatVersion", Some(ts(spec.cluster_hash_format_version))),
            ("classId", Some(spec.cur_class_id.map_or_else(|| "nil".into(), |c| c.to_string()))),
            (
                "ascendClassId",
                Some(spec.cur_ascend_class_id.map_or_else(|| "nil".into(), |c| c.to_string())),
            ),
            ("secondaryAscendClassId", Some(tsn(spec.cur_secondary_ascend_class_id))),
            ("nodes", Some(nodes.join(","))),
            ("masteryEffects", Some(masteries.join(","))),
        ],
    );
    let mut url = elem("URL");
    text(
        &mut url,
        &encode_url(spec, &ids, "https://www.pathofexile.com/passive-skill-tree/")?,
    );
    push(&mut x, url);
    let mut sockets = elem("Sockets");
    // :232-237 sockets sorted by node id
    let mut jewels: Vec<_> = spec.jewels.0.iter().collect();
    jewels.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (node_id, item_id) in jewels {
        if *item_id > 0.0 {
            push(
                &mut sockets,
                with(elem("Socket"), &[("nodeId", Some(ts(*node_id))), ("itemId", Some(ts(*item_id)))]),
            );
        }
    }
    push(&mut x, sockets);
    // PassiveSpec.lua:247-265 -- the tattoos sorted by node id, each `{ nodeId, icon, activeEffectImage, dn }` + its `sd` lines as text
    let mut overrides = elem("Overrides");
    let mut override_ids: Vec<u32> = spec.hash_overrides.keys().copied().collect();
    override_ids.sort_unstable();
    for node_id in override_ids {
        let t = &spec.hash_overrides[&node_id];
        let mut o = with(
            elem("Override"),
            &[
                ("nodeId", Some(node_id.to_string())),
                ("icon", Some(t.icon.clone())),
                ("activeEffectImage", Some(t.active_effect_image.clone())),
                ("dn", Some(t.dn.clone())),
            ],
        );
        for line in &t.sd {
            text(&mut o, line);
        }
        push(&mut overrides, o);
    }
    push(&mut x, overrides);
    Ok(x)
}

/// `PassiveSpec:EncodeURL(prefix)` (PassiveSpec.lua:545-595) over `ids` = the sorted `allocNodes` ids (:555-561, 3dbca4463 #10338).
pub fn encode_url(spec: &PassiveSpec, ids: &[u32], prefix: &str) -> Result<String, String> {
    let class = spec.cur_class_id.ok_or("PassiveSpec.lua:528 curClassId is nil")? as u64;
    let asc = spec.cur_ascend_class_id.unwrap_or(0) as u64;
    let sec = spec.cur_secondary_ascend_class_id.unwrap_or(0.0) as u64;
    let mut a: Vec<u64> = vec![0, 0, 0, 6, class, (sec << 2) | asc];
    let (mut node_count, mut cluster_count, mut mastery_count) = (0u64, 0u64, 0u64);
    let (mut cluster_ids, mut mastery_ids) = (Vec::new(), Vec::new());
    for &id in ids {
        let node = spec
            .node(id)
            .ok_or_else(|| format!("PassiveSpec.lua:537 allocNodes[{id}] has no node"))?;
        let ty = node.ty();
        let id = u64::from(id);
        if !ty.is_start() && id < 65536 && node_count < 255 {
            a.push(id / 256);
            a.push(id % 256);
            node_count += 1;
            if let Some(&effect) = spec.mastery_selections.get(&node.id) {
                let e = effect as u64;
                mastery_ids.extend([e / 256, e % 256, id / 256, id % 256]);
                mastery_count += 1;
            }
        } else if id >= 65536 {
            let c = id - 65536;
            cluster_ids.extend([c / 256, c % 256]);
            cluster_count += 1;
        }
    }
    a.insert(6, node_count);
    a.push(cluster_count);
    a.extend(cluster_ids);
    a.push(mastery_count);
    a.extend(mastery_ids);
    // `string.char(unpack(a))` -- a value past 255 is the Lua error
    let bytes: Vec<u8> = a
        .iter()
        .map(|&v| u8::try_from(v).map_err(|_| format!("PassiveSpec.lua:569: bad argument to 'char' (value {v} out of range)")))
        .collect::<Result<_, _>>()?;
    let code = pob_build::code::base64_encode(&bytes).replace('+', "-").replace('/', "_");
    Ok(format!("{prefix}{code}"))
}

// --- PassiveTreeView:Save (PassiveTreeView.lua:105-114) -------------------------------------------------------------------------------

fn save_tree_view(x: &SaveExtras) -> XmlElement {
    let t = &x.tree_view;
    with(
        elem("TreeView"),
        &[
            ("zoomLevel", Some(tsn(t.zoom_level))),
            ("zoomX", Some(tsn(t.zoom_x))),
            ("zoomY", Some(tsn(t.zoom_y))),
            ("searchStr", Some(t.search_str.clone())),
            ("showStatDifferences", Some(tb(t.show_stat_differences))),
        ],
    )
}
