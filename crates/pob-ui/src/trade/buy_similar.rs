//! `Classes/CompareBuySimilar.lua`: the "Buy Similar" popup of the Compare and Items tabs, and the trade search URL it builds. It opens a URL in the browser: the only request is the league list.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_gfx::Gfx;
use pob_items::Item;
use pob_items::item::ModLine;
use serde_json::{Value as Json, json};

use super::dkjson;
use super::helpers::{
    b64_gzip_encode, encode_league, find_trade_hash, find_trade_id_option, get_trade_category, get_trade_category_label,
    new_plain_numeric_edit, trade_stats,
};
use super::requests::TradeQueryRequests;
use crate::colors::color_code;
use crate::control::{ControlRef, anchor, anchor_nil, rect};
use crate::controls::basic::{ButtonControl, CheckBoxControl, LabelControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::EditControl;
use crate::item_tooltip::format_mod_line;
use crate::lua::{gsub, lmatch};
use crate::modules::build::BuildMode;
use crate::modules::main::main;

/// `REALM_API_IDS` (:27-31).
fn realm_api_id(display: &str) -> &'static str {
    match display {
        "PS4" => "sony",
        "Xbox" => "xbox",
        _ => "pc",
    }
}

/// `LISTED_STATUS_OPTIONS` (:34-39).
const LISTED: [(&str, &str); 4] = [
    ("Instant Buyout", "securable"),
    ("Instant Buyout & In Person", "available"),
    ("In Person (Online)", "online"),
    ("Any", "any"),
];

thread_local! {
    /// `M.lastRealmIdx`, `M.lastListedIndex`, `M.lastLeagueByRealm`: the module lives as long as the program.
    static LAST_REALM: Cell<Option<usize>> = const { Cell::new(None) };
    static LAST_LISTED: Cell<Option<usize>> = const { Cell::new(None) };
    static LAST_LEAGUE: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    static EXISTING_STATS: RefCell<Option<Rc<HashSet<String>>>> = const { RefCell::new(None) };
}

/// A new program for a test: the module's upvalues.
pub fn reset_module_state() {
    LAST_REALM.with(|c| c.set(None));
    LAST_LISTED.with(|c| c.set(None));
    LAST_LEAGUE.with(|c| c.borrow_mut().clear());
}

/// `getStats` (:13-22).
fn existing_stats() -> Rc<HashSet<String>> {
    if let Some(s) = EXISTING_STATS.with(|s| s.borrow().clone()) {
        return s;
    }
    let mut out = HashSet::new();
    for c in trade_stats().iter() {
        for e in &c.entries {
            out.insert(e.id.clone());
        }
    }
    let s = Rc::new(out);
    EXISTING_STATS.with(|c| *c.borrow_mut() = Some(s.clone()));
    s
}

/// `entry` of `addModEntries`.
#[derive(Clone, Debug)]
pub struct ModEntry {
    pub formatted_lines: Vec<String>,
    pub ty: String,
    pub is_option: bool,
    pub invert: Option<bool>,
    pub trade_ids: Vec<String>,
    /// A number, or the value of an option (a number or a string).
    pub value: Option<Json>,
}

fn as_num(v: &Option<Json>) -> Option<f64> {
    v.as_ref().and_then(Json::as_f64)
}

/// `M.addModEntries(item, modTypeSources)` (:208-280).
pub fn add_mod_entries(item: &Item, sources: &[(&[ModLine], &str)]) -> Vec<ModEntry> {
    let mut entries: Vec<ModEntry> = Vec::new();
    for (list, ty) in sources {
        for line in *list {
            if !item.check_mod_line_variant(line) {
                continue;
            }
            let mut line = line.clone();
            line.extra = None;
            let Some(formatted) = format_mod_line(&line, false) else { continue };
            let resolved = match line.range {
                Some(r) => pob_items::apply_range(&line.line, r, line.value_scalar, None, pob_parser::ParseMode::Full),
                None => line.line.clone(),
            };
            let option = find_trade_id_option(&resolved, ty);
            let mut entry = ModEntry {
                formatted_lines: vec![formatted],
                ty: (*ty).to_string(),
                is_option: option.is_some(),
                invert: Some(false),
                trade_ids: Vec::new(),
                value: None,
            };
            if let Some((id, value)) = option {
                entry.trade_ids.push(id);
                entry.value = value;
            } else {
                let h = find_trade_hash(&resolved);
                let existing = existing_stats();
                entry.trade_ids = h
                    .ids
                    .iter()
                    .map(|x| format!("{ty}.stat_{x}"))
                    .filter(|id| existing.contains(id))
                    .collect();
                entry.value = h.value.map(dkjson::n);
                entry.invert = h.should_negate;
            }
            // `insertOrAddToExisting` (:214-229)
            let mut merged = false;
            if !entry.is_option && !entry.trade_ids.is_empty() {
                for e in &mut entries {
                    if e.trade_ids == entry.trade_ids && e.ty == entry.ty {
                        if let Some(v) = as_num(&entry.value) {
                            let v = if entry.invert != e.invert { -v } else { v };
                            e.value = Some(dkjson::n(as_num(&e.value).unwrap_or(0.0) + v));
                        }
                        e.formatted_lines.push(entry.formatted_lines[0].clone());
                        merged = true;
                        break;
                    }
                }
            }
            if !merged {
                entries.push(entry);
            }
        }
    }
    entries
}

struct Ctl {
    realm: Rc<DropDownControl>,
    league: Rc<DropDownControl>,
    listed: Rc<DropDownControl>,
    base_type: Option<Rc<CheckBoxControl>>,
    ilvl: Option<(Rc<EditControl>, Rc<EditControl>)>,
    defences: Vec<(Rc<CheckBoxControl>, Rc<EditControl>, Rc<EditControl>, &'static str)>,
    mods: Vec<ModRow>,
}

struct ModRow {
    check: Rc<CheckBoxControl>,
    edits: Option<(Rc<EditControl>, Rc<EditControl>)>,
}

fn buf_num(e: &EditControl) -> Option<f64> {
    pob_lua::tonumber(&e.buf.borrow())
}

fn min_max(min: Option<f64>, max: Option<f64>) -> Json {
    let mut o = serde_json::Map::new();
    if let Some(v) = min {
        o.insert("min".into(), dkjson::n(v));
    }
    if let Some(v) = max {
        o.insert("max".into(), dkjson::n(v));
    }
    if o.is_empty() { json!([]) } else { Json::Object(o) }
}

/// `buildURL(item, slotName, controls, modEntries, defenceEntries, isUnique)` (:47-203).
fn build_url(item: &Item, slot_name: &str, c: &Ctl, mods: &[ModEntry], is_unique: bool) -> String {
    let realm = realm_api_id(c.realm.get_sel_value().and_then(|v| v.label).as_deref().unwrap_or("PC"));
    let league = c
        .league
        .get_sel_value()
        .and_then(|v| v.label)
        .filter(|l| !l.is_empty() && l != "Loading...")
        .unwrap_or_else(|| "Standard".to_string());
    let listed = LISTED.get(c.listed.sel_index.get().wrapping_sub(1)).map_or("available", |x| x.1);
    let mut query = serde_json::Map::new();
    query.insert("status".into(), json!({"option": listed}));
    let mut and_filters: Vec<Json> = Vec::new();
    let mut groups: Vec<Json> = Vec::new();
    let mut filters = serde_json::Map::new();
    if is_unique {
        let name = item.title.clone().unwrap_or_else(|| item.name.clone());
        let name = gsub(&name, "^Foulborn%s+", "");
        let name = gsub(&name, "%s+$", "");
        let name = lmatch(&name, "^(.-)%s+%d+$").unwrap_or(name);
        query.insert("name".into(), json!(name));
        if let Some(b) = &item.base_name {
            query.insert("type".into(), json!(b));
        }
        if item.foulborn {
            filters.insert("misc_filters".into(), json!({"filters": {"foulborn_item": {"option": "true"}}}));
        }
    } else {
        if let (Some(cat), _) = get_trade_category(slot_name, Some(item)) {
            filters.insert("type_filters".into(), json!({"filters": {"category": {"option": cat}}}));
        }
        if c.base_type.as_ref().is_some_and(|b| b.state.get())
            && let Some(b) = &item.base_name
        {
            query.insert("type".into(), json!(b));
        }
        if let Some((a, b)) = &c.ilvl {
            let (min, max) = (buf_num(a), buf_num(b));
            if min.is_some() || max.is_some() {
                filters.insert("misc_filters".into(), json!({"filters": {"ilvl": min_max(min, max)}}));
            }
        }
        let mut armour = serde_json::Map::new();
        for (check, a, b, key) in &c.defences {
            if check.state.get() {
                let (min, max) = (buf_num(a), buf_num(b));
                if min.is_some() || max.is_some() {
                    armour.insert((*key).to_string(), min_max(min, max));
                }
            }
        }
        if !armour.is_empty() {
            filters.insert("armour_filters".into(), json!({"filters": armour}));
        }
    }
    for (entry, row) in mods.iter().zip(&c.mods) {
        if !row.check.state.get() {
            continue;
        }
        let filter = |id: &str| -> Json {
            let mut f = json!({"id": id});
            if entry.is_option {
                f["value"] = if lmatch(id, "timeless").is_some() {
                    json!({"min": entry.value, "max": entry.value})
                } else {
                    json!({"option": entry.value})
                };
            } else if entry.value.is_some() {
                let (e_min, e_max) = row
                    .edits
                    .as_ref()
                    .unwrap_or_else(|| panic!("Classes/CompareBuySimilar.lua:150: attempt to index field '?' (a nil value)"));
                let (mut min, mut max) = (buf_num(e_min), buf_num(e_max));
                if entry.invert == Some(true) {
                    std::mem::swap(&mut min, &mut max);
                    min = min.map(|v| -v);
                    max = max.map(|v| -v);
                }
                if min.is_some() || max.is_some() {
                    f["value"] = min_max(min, max);
                }
            }
            f
        };
        if entry.trade_ids.len() == 1 {
            and_filters.push(filter(&entry.trade_ids[0]));
        } else if entry.trade_ids.len() > 1 {
            let list: Vec<Json> = entry.trade_ids.iter().map(|i| filter(i)).collect();
            groups.push(json!({"type": "count", "value": {"min": 1}, "filters": list}));
        }
    }
    let mut stats = vec![json!({"type": "and", "filters": and_filters})];
    stats.extend(groups);
    query.insert("stats".into(), Json::Array(stats));
    if !filters.is_empty() {
        query.insert("filters".into(), Json::Object(filters));
    }
    let text = dkjson::encode(&Json::Object(query));
    let mut url = "https://www.pathofexile.com/trade/search".to_string();
    if realm != "pc" {
        url.push('/');
        url.push_str(realm);
    }
    url.push('/');
    url.push_str(&encode_league(&league));
    url.push('/');
    url.push_str(&b64_gzip_encode(&text));
    url
}

/// `M.openPopup(item, slotName, primaryBuild)` (:282-533).
pub fn open_popup(g: &mut Gfx, item: Rc<Item>, slot_name: String, primary: &BuildMode) {
    let is_unique = item.rarity == "UNIQUE" || item.rarity == "RELIC";
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    let uri = Rc::new(RefCell::new(String::new()));
    let row_height = 24.0;
    let popup_width = 700.0;
    let left = 20.0;
    let min_field_x = popup_width - 130.0;
    let max_field_x = popup_width - 50.0;
    let field_w = 74.0;
    let field_h = 20.0;
    let checkbox = 20.0;
    let sources: [(&[ModLine], &str); 4] = [
        (&item.enchant_mod_lines, "enchant"),
        (&item.implicit_mod_lines, "implicit"),
        (&item.explicit_mod_lines, "explicit"),
        (&item.scourge_mod_lines, "scourge"),
    ];
    let mut mods = add_mod_entries(&item, &sources);
    // :313-330
    let mut defences: Vec<(&'static str, f64, &'static str)> = Vec::new();
    if !is_unique && item.armour_data.is_some() && item.base.is_some_and(|b| b.armour.is_some()) {
        for (key, label, trade) in [
            ("Armour", "Armour", "ar"),
            ("Evasion", "Evasion", "ev"),
            ("EnergyShield", "Energy Shield", "es"),
            ("Ward", "Ward", "ward"),
        ] {
            let v = item.armour_data.as_ref().and_then(|d| d.num(key));
            if let Some(v) = v.filter(|v| *v > 0.0) {
                defences.push((label, v, trade));
            }
        }
    }
    let mut ctrl_y = 25.0;
    // :336-340 `primaryBuild.itemsTab.tradeQuery.tradeQueryRequests`, or a new one
    let requests: Rc<TradeQueryRequests> = primary
        .items_tab
        .borrow()
        .as_ref()
        .and_then(|t| t.trade_query.borrow().as_ref().map(|q| q.requests.clone()))
        .unwrap_or_default();
    let ctl: Rc<RefCell<Option<Ctl>>> = Rc::new(RefCell::new(None));
    let mods_rc: Rc<RefCell<Vec<ModEntry>>> = Rc::new(RefCell::new(Vec::new()));
    let rebuild: Rc<dyn Fn()> = {
        let (ctl, mods_rc, uri, item, slot_name) = (ctl.clone(), mods_rc.clone(), uri.clone(), item.clone(), slot_name.clone());
        Rc::new(move || {
            if let Some(c) = &*ctl.borrow() {
                *uri.borrow_mut() = build_url(&item, &slot_name, c, &mods_rc.borrow(), is_unique);
            }
        })
    };
    let league_cell: Rc<RefCell<Option<Rc<DropDownControl>>>> = Rc::new(RefCell::new(None));
    // `fetchLeaguesForRealm` (:347-375)
    let fetch: Rc<dyn Fn(&mut Gfx, &'static str)> = {
        let (lc, requests, rebuild) = (league_cell.clone(), requests, rebuild.clone());
        Rc::new(move |g: &mut Gfx, realm: &'static str| {
            let last = LAST_LEAGUE.with(|l| l.borrow().iter().find(|(r, _)| r == realm).map(|(_, v)| v.clone()));
            let league = lc.borrow().clone().expect("leagueDrop");
            league.set_list(g, Some(vec![DropItem::str("Loading...")]));
            league.sel_index.set(1);
            let (lc2, rb) = (lc.clone(), rebuild.clone());
            requests.fetch_leagues(
                realm,
                Rc::new(move |g: &mut Gfx, leagues: Vec<String>, err: Option<String>| {
                    let league = lc2.borrow().clone().expect("leagueDrop");
                    if err.is_some() {
                        league.set_list(g, Some(vec![DropItem::str("Standard")]));
                        rb();
                        return;
                    }
                    let mut list: Vec<String> = Vec::new();
                    for l in leagues {
                        if !matches!(l.as_str(), "Standard" | "Ruthless" | "Hardcore" | "Hardcore Ruthless") {
                            if l.contains("Hardcore") || l.contains("Ruthless") {
                                list.push(l);
                            } else {
                                list.insert(0, l);
                            }
                        }
                    }
                    for l in ["Standard", "Hardcore", "Ruthless", "Hardcore Ruthless"] {
                        list.push(l.to_string());
                    }
                    let at = last.as_ref().and_then(|l| list.iter().position(|x| x == l)).map_or(1, |i| i + 1);
                    league.set_list(g, Some(list.iter().map(|s| DropItem::str(s)).collect()));
                    league.set_sel(g, at as i64, true);
                    rb();
                }),
            );
        })
    };
    // :378-387
    let realm_label = LabelControl::new(
        anchor_nil("TOPLEFT", "TOPLEFT"),
        rect(left, ctrl_y, 0.0, 16.0),
        "^7Realm:".to_string(),
    );
    let rl: ControlRef = realm_label.clone();
    controls.insert("realmLabel".into(), realm_label);
    let (f2, rb) = (fetch.clone(), rebuild.clone());
    let realm = DropDownControl::new(
        g,
        anchor("LEFT", &rl, "RIGHT"),
        rect(4.0, 0.0, 80.0, 20.0),
        vec![DropItem::str("PC"), DropItem::str("PS4"), DropItem::str("Xbox")],
        Some(Rc::new(move |g: &mut Gfx, index: usize, v: &DropItem| {
            f2(g, realm_api_id(v.label.as_deref().unwrap_or("PC")));
            rb();
            LAST_REALM.with(|c| c.set(Some(index)));
        })),
        None,
        false,
    );
    if let Some(i) = LAST_REALM.with(Cell::get) {
        realm.set_sel(g, i as i64, true);
    }
    let rr: ControlRef = realm.clone();
    controls.insert("realmDrop".into(), realm.clone());
    let league_label = LabelControl::new(anchor("LEFT", &rr, "RIGHT"), rect(12.0, 0.0, 0.0, 16.0), "^7League:".to_string());
    let ll: ControlRef = league_label.clone();
    controls.insert("leagueLabel".into(), league_label);
    let (rb, r2) = (rebuild.clone(), realm.clone());
    let league = DropDownControl::new(
        g,
        anchor("LEFT", &ll, "RIGHT"),
        rect(4.0, 0.0, 160.0, 20.0),
        vec![DropItem::str("Loading...")],
        Some(Rc::new(move |_g: &mut Gfx, _i: usize, v: &DropItem| {
            let realm = realm_api_id(r2.get_sel_value().and_then(|x| x.label).as_deref().unwrap_or("PC")).to_string();
            let value = v.label.clone().unwrap_or_default();
            LAST_LEAGUE.with(|l| {
                let mut l = l.borrow_mut();
                match l.iter_mut().find(|(r, _)| *r == realm) {
                    Some(e) => e.1 = value,
                    None => l.push((realm, value)),
                }
            });
            rb();
        })),
        None,
        false,
    );
    let w = Rc::downgrade(&league);
    league.base.enabled.set_fn(move |_| {
        let d = w.upgrade().expect("leagueDrop");
        let l = d.list.borrow();
        !l.is_empty() && l[0].label.as_deref() != Some("Loading...")
    });
    *league_cell.borrow_mut() = Some(league.clone());
    controls.insert("leagueDrop".into(), league.clone());
    // :401-408
    let rb = rebuild.clone();
    let listed = DropDownControl::new(
        g,
        anchor_nil("TOPRIGHT", "TOPRIGHT"),
        rect(-left, ctrl_y, 242.0, 20.0),
        LISTED.iter().map(|(l, _)| DropItem::str(l)).collect(),
        Some(Rc::new(move |_g: &mut Gfx, i: usize, _v: &DropItem| {
            LAST_LISTED.with(|c| c.set(Some(i)));
            rb();
        })),
        None,
        false,
    );
    if let Some(i) = LAST_LISTED.with(Cell::get) {
        listed.set_sel(g, i as i64, true);
    }
    let lr: ControlRef = listed.clone();
    controls.insert("listedDrop".into(), listed.clone());
    controls.insert(
        "listedLabel".into(),
        LabelControl::new(anchor("RIGHT", &lr, "LEFT"), rect(-4.0, 0.0, 0.0, 16.0), "^7Listed:".to_string()),
    );
    // :411
    fetch(
        g,
        realm_api_id(realm.get_sel_value().and_then(|v| v.label).as_deref().unwrap_or("PC")),
    );
    ctrl_y += row_height + 4.0;
    let on_change = |rb: &Rc<dyn Fn()>| -> Rc<dyn Fn(&mut Gfx, bool)> {
        let rb = rb.clone();
        Rc::new(move |_g: &mut Gfx, _s: bool| rb())
    };
    let on_edit = |rb: &Rc<dyn Fn()>| -> crate::controls::edit::ChangeFunc {
        let rb = rb.clone();
        Rc::new(move |_g: &mut Gfx, _s: &str, _p: bool| rb())
    };
    let check_x = -popup_width / 2.0 + left + checkbox / 2.0;
    let mut base_type = None;
    let mut ilvl = None;
    let mut defence_rows = Vec::new();
    if is_unique {
        // :417
        let code = color_code(&item.rarity).unwrap_or_else(|| "FFFFFF".to_string());
        let label = format!("^x{}{}", gsub(&code, "%^x", ""), item.name);
        controls.insert("nameLabel".into(), LabelControl::new(None, rect(0.0, ctrl_y, 0.0, 16.0), label));
        ctrl_y += row_height;
    } else {
        controls.insert(
            "categoryLabel".into(),
            LabelControl::new(
                anchor_nil("TOPLEFT", "TOPLEFT"),
                rect(left, ctrl_y, 0.0, 16.0),
                format!("^7Category: {}", get_trade_category_label(&item)),
            ),
        );
        ctrl_y += row_height;
        let b = CheckBoxControl::new(
            g,
            None,
            [check_x, ctrl_y, checkbox],
            Some(""),
            Some(on_change(&rebuild)),
            None,
            false,
        );
        let br: ControlRef = b.clone();
        controls.insert(
            "baseTypeLabel".into(),
            LabelControl::new(
                anchor("LEFT", &br, "RIGHT"),
                rect(4.0, 0.0, 0.0, 16.0),
                format!("^7Use specific base: {}", item.base_name.as_deref().unwrap_or("Unknown")),
            ),
        );
        controls.insert("baseTypeCheck".into(), b.clone());
        base_type = Some(b);
        ctrl_y += row_height;
        ctrl_y += 4.0;
        controls.insert(
            "ilvlLabel".into(),
            LabelControl::new(
                anchor_nil("TOPLEFT", "TOPLEFT"),
                rect(left, ctrl_y, 0.0, 16.0),
                "^7Item Level:".to_string(),
            ),
        );
        let a = new_plain_numeric_edit(
            g,
            None,
            rect(min_field_x - popup_width / 2.0, ctrl_y, field_w, field_h),
            "",
            "Min",
            4,
            true,
            Some(on_edit(&rebuild)),
        );
        let c = new_plain_numeric_edit(
            g,
            None,
            rect(max_field_x - popup_width / 2.0, ctrl_y, field_w, field_h),
            "",
            "Max",
            4,
            true,
            Some(on_edit(&rebuild)),
        );
        controls.insert("ilvlMin".into(), a.clone());
        controls.insert("ilvlMax".into(), c.clone());
        ilvl = Some((a, c));
        ctrl_y += row_height;
        for (i, (label, value, trade)) in defences.iter().enumerate() {
            let p = format!("def{}", i + 1);
            let check = CheckBoxControl::new(
                g,
                None,
                [check_x, ctrl_y, checkbox],
                Some(""),
                Some(on_change(&rebuild)),
                None,
                false,
            );
            let cr: ControlRef = check.clone();
            controls.insert(
                format!("{p}Label"),
                LabelControl::new(anchor("LEFT", &cr, "RIGHT"), rect(4.0, 0.0, 0.0, 16.0), format!("^7{label}")),
            );
            controls.insert(format!("{p}Check"), check.clone());
            let a = new_plain_numeric_edit(
                g,
                None,
                rect(min_field_x - popup_width / 2.0, ctrl_y, field_w, field_h),
                &pob_lua::format::tostring_num(value.floor()),
                "Min",
                6,
                true,
                Some(on_edit(&rebuild)),
            );
            let c = new_plain_numeric_edit(
                g,
                None,
                rect(max_field_x - popup_width / 2.0, ctrl_y, field_w, field_h),
                "",
                "Max",
                6,
                true,
                Some(on_edit(&rebuild)),
            );
            controls.insert(format!("{p}Min"), a.clone());
            controls.insert(format!("{p}Max"), c.clone());
            defence_rows.push((check, a, c, *trade));
            ctrl_y += row_height;
        }
        if !defences.is_empty() {
            ctrl_y += 8.0;
        }
    }
    // :454-510
    let mut prev_type: Option<String> = None;
    let mut mod_rows = Vec::new();
    for (idx, entry) in mods.iter_mut().enumerate() {
        if prev_type.as_ref().is_some_and(|p| *p != entry.ty) {
            ctrl_y += row_height;
        }
        prev_type = Some(entry.ty.clone());
        let p = format!("mod{}", idx + 1);
        let can_search = !entry.trade_ids.is_empty();
        let rows = entry.formatted_lines.len() as f64;
        let font_size = 16.0;
        let control_y = ctrl_y + (rows - 1.0) * 8.0;
        let check = CheckBoxControl::new(
            g,
            None,
            [check_x, control_y, checkbox],
            Some(""),
            Some(on_change(&rebuild)),
            None,
            false,
        );
        check.base.enabled.set_fn(move |_| can_search);
        let cr: ControlRef = check.clone();
        for t in &mut entry.formatted_lines {
            let mut text = gsub(t, " Passive Skills in Radius also grant", ":");
            // `displayText:match("(%^x%x%x%x%x%x%x)") or displayText:gsub("(%^%x)", "") or ""`: the first match, else the text without codes
            let colour = lmatch(&text, "(%^x%x%x%x%x%x%x)").unwrap_or_else(|| gsub(&text, "(%^%x)", ""));
            if !can_search {
                text = format!("^8{}", gsub(&gsub(&text, "%^x%x%x%x%x%x%x", ""), "%^%x", ""));
            }
            if text.len() > colour.len() + 62 {
                text = format!("{}...", crate::lua::sub(&text, 1, (colour.len() + 54) as i64));
            }
            *t = text;
        }
        let label_offset = (rows - 1.0) * -8.0;
        controls.insert(
            format!("{p}Label"),
            LabelControl::new(
                anchor("LEFT", &cr, "RIGHT"),
                rect(4.0, label_offset, 0.0, font_size),
                entry.formatted_lines.join("\n"),
            ),
        );
        controls.insert(format!("{p}Check"), check.clone());
        let mut edits = None;
        if !entry.is_option && entry.value.is_some() {
            let v = as_num(&entry.value).unwrap_or(0.0);
            let init = if v != 0.0 {
                pob_lua::format::tostring_num(v)
            } else {
                String::new()
            };
            let a = new_plain_numeric_edit(
                g,
                None,
                rect(min_field_x - popup_width / 2.0, control_y, field_w, field_h),
                &init,
                "Min",
                8,
                false,
                Some(on_edit(&rebuild)),
            );
            let c = new_plain_numeric_edit(
                g,
                None,
                rect(max_field_x - popup_width / 2.0, control_y, field_w, field_h),
                "",
                "Max",
                8,
                false,
                Some(on_edit(&rebuild)),
            );
            if !can_search {
                a.base.enabled.set(false);
                c.base.enabled.set(false);
            }
            controls.insert(format!("{p}Min"), a.clone());
            controls.insert(format!("{p}Max"), c.clone());
            edits = Some((a, c));
        }
        mod_rows.push(ModRow { check, edits });
        ctrl_y += row_height.max(font_size * rows + 8.0);
    }
    *mods_rc.borrow_mut() = mods;
    *ctl.borrow_mut() = Some(Ctl {
        realm,
        league,
        listed,
        base_type,
        ilvl,
        defences: defence_rows,
        mods: mod_rows,
    });
    // :513-525
    ctrl_y += 8.0;
    let u = uri.clone();
    let search = ButtonControl::new(None, rect(0.0, ctrl_y, 110.0, 20.0), "Open URL".to_string(), move |g| {
        let url = u.borrow().clone();
        g.copy(&url);
        g.open_url(&url);
    });
    search
        .tip
        .tooltip_text
        .set(Some("The URL is also copied to the clipboard.".to_string()));
    let u = uri.clone();
    search.base.enabled.set_fn(move |_| !u.borrow().is_empty());
    controls.insert("search".into(), search);
    controls.insert(
        "close".into(),
        ButtonControl::new(
            None,
            rect(popup_width / 2.0 - 50.0, ctrl_y, 60.0, 20.0),
            "Close".to_string(),
            |_g| {
                main().close_popup();
            },
        ),
    );
    let popup_height = (ctrl_y + field_h + 16.0).min(600.0);
    main().open_popup(
        g,
        popup_width,
        popup_height,
        "Buy Similar",
        controls,
        Some("search"),
        None,
        Some("close"),
        None,
    );
}
