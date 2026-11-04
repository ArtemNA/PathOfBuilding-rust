//! `Classes/ImportTab.lua`: the three sections, every control and state as the original draws them, the build code both ways, the character import (U8c) over `launch:DownloadPage` and
//! `crate::poe_api`. What a character's JSON does to the build is in `pob_engine::import_tab`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use pob_gfx::{Frame, Gfx};
use serde_json::{Map, Value as Json, json};

use crate::colors::color_code;
use crate::control::{Control, ControlRef, Rect, anchor, anchor_collapse, rect};
use crate::controls::basic::{ButtonControl, CheckBoxControl, LabelControl, SectionControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::host::{ControlHost, InputEvents};
use crate::launch::{PageParams, launch};
use crate::modules::build::BuildMode;
use crate::modules::main::{BareControl, Mode, ModeArg, main, not_ported, sel_by_value};
use crate::sg;

fn colored(code: &str, text: &str) -> String {
    format!("{}{text}", color_code(code).unwrap_or_else(|| panic!("colorCodes.{code}")))
}

fn lua_match(s: &str, pat: &str) -> bool {
    pob_lua::match_(s.as_bytes(), pat.as_bytes(), None).expect("pattern").is_some()
}

fn lua_gsub(s: &str, pat: &str, repl: &str) -> String {
    let out = pob_lua::gsub(s.as_bytes(), pat.as_bytes(), pob_lua::Repl::Str(repl.as_bytes()), None)
        .expect("pattern")
        .0;
    String::from_utf8_lossy(&out).into_owned()
}

/// BuildSiteTools.lua:19-45 `buildSites.websiteList`: `(label, id, matchURL, export)`; `export` = it has `postUrl`, `postFields` and `codeOut` (ImportTab.lua:523-532).
pub(crate) const WEBSITES: [(&str, &str, &str, bool); 8] = [
    ("Maxroll", "Maxroll", "^https://maxroll%.gg/poe/pob/.*", true),
    ("pob.codes", "PoBCodes", "^https://pob%.codes/b/.+", true),
    ("pobb.in", "POBBin", "^https://pobb%.in/.+", true),
    ("PoeNinja", "PoeNinja", "^https://poe%.ninja/?p?o?e?1?/pob/%w+", true),
    ("Pastebin.com", "pastebin", "^https://pastebin%.com/%w+", false),
    ("PastebinP.com", "pastebinProxy", "^https://pastebinp%.com/%w+", false),
    ("Rentry.co", "rentry", "^https://rentry%.co/%w+", false),
    ("poedb.tw", "PoEDB", "^https://poedb%.tw/.+", true),
];

/// ImportTab.lua:18-22 `realmList`.
const REALMS: [(&str, &str); 3] = [("PC", "PC"), ("Xbox", "XBOX"), ("Sony", "SONY")];
/// `realm.hostName`, the same for the three (:19-21).
const HOST_NAME: &str = "https://www.pathofexile.com/";
/// `realm.profileURL`.
const PROFILE_URL: &str = "account/view-profile/";

/// `realm.realmCode` of a realm's `id`.
fn realm_code(id: &str) -> String {
    id.to_ascii_lowercase()
}

fn realm_of(dd: &DropDownControl) -> String {
    dd.get_sel_value()
        .and_then(|v| v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned())
        .unwrap_or_else(|| "PC".to_string())
}

/// `urlEncode(str)` (Common.lua:1024-1029).
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `sub("#", "%%23")` of an account name in a URL.
fn account_in_url(name: &str) -> String {
    name.replace('#', "%23")
}

/// `:gsub("(.*)[#%-]", "%1#")`.
fn account_with_hash(name: &str) -> String {
    lua_gsub(name, "(.*)[#%-]", "%1#")
}

/// The class colours of `BuildCharacterList` (:1039-1067).
fn class_color(class: &str) -> String {
    if class == "?" {
        return color_code("DEFAULT").unwrap_or_else(|| "nil".to_string());
    }
    if let Some(c) = color_code(&class.to_uppercase()) {
        return c;
    }
    let base = match class {
        "Elementalist" | "Necromancer" | "Occultist" | "Harbinger" | "Herald" | "Bog Shaman" => "WITCH",
        "Guardian" | "Inquisitor" | "Hierophant" | "Architect of Chaos" | "Polytheist" | "Puppeteer" => "TEMPLAR",
        "Assassin" | "Trickster" | "Saboteur" | "Surfcaster" | "Servant of Arakaali" | "Blind Prophet" => "SHADOW",
        "Gladiator" | "Slayer" | "Champion" | "Gambler" | "Paladin" | "Aristocrat" => "DUELIST",
        "Raider" | "Pathfinder" | "Deadeye" | "Warden" | "Daughter of Oshabi" | "Whisperer" | "Wildspeaker" => "RANGER",
        "Juggernaut" | "Berserker" | "Chieftain" | "Antiquarian" | "Behemoth" | "Ancestral Commander" => "MARAUDER",
        "Ascendant" | "Reliquarian" | "Luminary" | "Scavenger" => "SCION",
        // `string.format("%s", nil)`
        _ => return "nil".to_string(),
    };
    color_code(base).unwrap_or_else(|| "nil".to_string())
}

/// `FindMatchingStandardLeague(league)` (:888-903).
fn standard_league(league: &str) -> &'static str {
    if league.contains("Hardcore") {
        "Hardcore"
    } else if league.contains("HC SSF") {
        "SSF Hardcore"
    } else if league.contains("SSF") {
        "SSF Standard"
    } else {
        "Standard"
    }
}

/// `ProcessSiteJSON(json)` (:789-800): a table or an error.
fn site_json(text: &str) -> Option<Vec<Json>> {
    serde_json::from_str::<Json>(text).ok()?.as_array().cloned()
}

fn char_of(item: &DropItem) -> Option<Rc<Json>> {
    item.data.clone().and_then(|d| d.downcast::<Json>().ok())
}

fn league_of(item: &DropItem) -> Option<String> {
    item.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned()
}

/// `UrlDecode(url)` (ImportTab.lua:1917-1924).
fn url_decode(url: &str) -> String {
    let s = url.replace('+', " ");
    let mut hex = |subject: &[u8], c: &pob_lua::Captures| -> Option<Vec<u8>> {
        let h = c.get(0).and_then(|x| x.bytes(subject))?;
        let v = u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()?;
        Some(vec![v])
    };
    let out = pob_lua::gsub(s.as_bytes(), b"%%(%x%x)", pob_lua::Repl::Fn(&mut hex), None)
        .expect("pattern")
        .0;
    String::from_utf8_lossy(&out).into_owned()
}

/// What `importCodeHandle` found in the buffer (ImportTab.lua:575-642, PartyTab.lua:120-160).
pub(crate) enum CodeInput {
    Empty,
    Invalid,
    /// `(1-based site, trimmed text)`.
    Url(usize, String),
    Xml(String),
}

/// The part of `importCodeHandle` both tabs share (ImportTab.lua:590-608 and :632-641; the JSON branch :610-630 is the caller's, between the two).
pub(crate) fn url_of(buf: &str) -> (Option<usize>, String) {
    let mut url_text = lua_gsub(&lua_gsub(buf, "^[%s?]+", ""), "[%s?]+$", "");
    if lua_match(&url_text, "youtube%.com/redirect%?") || lua_match(&url_text, "google%.com/url%?") {
        url_text = url_decode(&lua_gsub(&url_text, ".*[?&]q=([^&]+).*", "%1"));
    }
    (WEBSITES.iter().position(|w| lua_match(&url_text, w.2)).map(|i| i + 1), url_text)
}

pub(crate) fn classify_code(buf: &str) -> CodeInput {
    if buf.is_empty() {
        return CodeInput::Empty;
    }
    let (site, url_text) = url_of(buf);
    if let Some(j) = site {
        return CodeInput::Url(j, url_text);
    }
    code_xml(buf)
}

/// `Inflate(common.base64.decode(buf:gsub("-","+"):gsub("_","/")))`.
pub(crate) fn code_xml(buf: &str) -> CodeInput {
    match pob_build::decode_build_code(buf) {
        Some(x) => CodeInput::Xml(String::from_utf8_lossy(&x).into_owned()),
        None => CodeInput::Invalid,
    }
}

/// Every control of a host: what the gate compares (the same fields as `import-runner.lua`).
pub fn dump_host(host: &ControlHost, g: &Gfx) -> Json {
    let mut m = Map::new();
    for (name, c) in host.controls_named() {
        let (x, y) = c.get_pos(g);
        let (w, h) = c.get_size(g);
        let n = |v: f64| pob_lua::tostring_num(v);
        let shown = c.is_shown(g);
        let mut o = Map::new();
        o.insert("shown".into(), json!(shown));
        o.insert("enabled".into(), json!(c.is_enabled(g)));
        // a hidden control may hang off a function that needs what it hides: as in the Lua only its flag counts
        if shown {
            o.insert("rect".into(), json!(format!("{},{},{},{}", n(x), n(y), n(w), n(h))));
        }
        if let Some(v) = c.debug_value(g) {
            o.insert("value".into(), v);
        }
        m.insert(name, Json::Object(o));
    }
    Json::Object(m)
}

pub struct ImportTab {
    pub anchor: Rc<BareControl>,
    pub host: ControlHost,
    build: Weak<BuildMode>,
    me: RefCell<Weak<Self>>,
    pub charimport_mode: RefCell<String>,
    pub charimport_status: RefCell<String>,
    pub import_code_in: Rc<EditControl>,
    pub import_code_mode: Rc<DropDownControl>,
    pub import_code_go: Rc<ButtonControl>,
    pub generate_code_out: Rc<EditControl>,
    pub generate_code: Rc<ButtonControl>,
    pub generate_code_copy: Rc<ButtonControl>,
    pub generate_code_by_link: Rc<ButtonControl>,
    pub enable_party_export_buffs: Rc<CheckBoxControl>,
    pub export_from: Rc<DropDownControl>,
    pub account_realm_fetch_button: Rc<ButtonControl>,
    pub char_select: Rc<DropDownControl>,
    pub char_select_league: Rc<DropDownControl>,
    pub char_import_tree: Rc<ButtonControl>,
    pub char_import_items: Rc<ButtonControl>,
    account_realm: Rc<DropDownControl>,
    site_account_realm: Rc<DropDownControl>,
    clear_jewels: Rc<CheckBoxControl>,
    clear_skills: Rc<CheckBoxControl>,
    clear_items: Rc<CheckBoxControl>,
    ignore_weapon_swap: Rc<CheckBoxControl>,
    site_clear_jewels: Rc<CheckBoxControl>,
    site_clear_skills: Rc<CheckBoxControl>,
    site_clear_items: Rc<CheckBoxControl>,
    site_ignore_weapon_swap: Rc<CheckBoxControl>,
    /// `self.oauthTimer`
    oauth_timer: Cell<Option<f64>>,
    /// `self.rateLimitEndTime`
    rate_limit_end_time: Cell<Option<f64>>,
    /// `self.oauthErrCode`
    oauth_err_code: RefCell<Option<String>>,
    /// `self.oauthLoading`
    oauth_loading: Cell<bool>,
    /// `self.characterList`, by realm code. No name of a character leaves the process but in the request and in the saved hash, as in the original.
    character_list: RefCell<HashMap<String, Vec<Json>>>,
    /// `self.lastCharList`
    last_char_list: RefCell<Vec<Json>>,
    pub site_account_name: Rc<EditControl>,
    pub site_account_history: Rc<DropDownControl>,
    pub site_char_select: Rc<DropDownControl>,
    pub site_char_select_league: Rc<DropDownControl>,
    import_code_site: Cell<Option<usize>>,
    import_code_detail: RefCell<String>,
    import_code_xml: RefCell<Option<String>>,
    import_code_valid: Cell<bool>,
    import_code_json: RefCell<Option<Json>>,
    /// `self.importCodeFetching`
    import_code_fetching: Cell<bool>,
    auto_fetch_attempted: Cell<bool>,
}

fn is_authorized() -> bool {
    crate::poe_api::is_authorized()
}

fn realm_items() -> Vec<DropItem> {
    REALMS
        .iter()
        .map(|(label, id)| {
            let mut it = DropItem::table(label);
            it.data = Some(Rc::new((*id).to_string()) as Rc<dyn std::any::Any>);
            it
        })
        .collect()
}

impl ImportTab {
    /// ImportTab.lua:477-731 (+ `addOAuthControls` :24-321, `addAccountNameControls` :322-471), then `Load` (:742-757).
    #[allow(clippy::too_many_lines)]
    pub fn new(g: &mut Gfx, build: Weak<BuildMode>) -> Rc<Self> {
        let host = ControlHost::new();
        let a = BareControl::new(None, rect(0.0, 0.0, 0.0, 0.0));
        let tab: ControlRef = a.clone();
        let label = |c: &ControlRef, p: &'static str, op: &'static str, rc: [f64; 4], text: &str| {
            LabelControl::new(anchor(p, c, op), rect(rc[0], rc[1], rc[2], rc[3]), text.to_string())
        };
        let button = |c: &ControlRef, p: &'static str, op: &'static str, rc: [f64; 4], text: &str| {
            ButtonControl::new(anchor(p, c, op), rect(rc[0], rc[1], rc[2], rc[3]), text.to_string(), |_| {})
        };
        let check = |g: &Gfx, c: &ControlRef, x: f64, size: f64, text: &str, tip: &str, state: bool| {
            CheckBoxControl::new(g, anchor("LEFT", c, "RIGHT"), [x, 0.0, size], Some(text), None, Some(tip), state)
        };
        let m = main();

        // --- :488-491, addOAuthControls
        let section_oauth = SectionControl::new(
            anchor("TOPLEFT", &tab, "TOPLEFT"),
            rect(10.0, 18.0, 650.0, 200.0),
            "Import From Your Account".to_string(),
        );
        let so: ControlRef = section_oauth.clone();
        host.add("sectionOauthCharImport", section_oauth.clone());
        let label_spacing = 6.0;
        let row_spacing = 6.0;
        let char_import_status_label = label(&so, "TOPLEFT", "TOPLEFT", [label_spacing, 14.0, 200.0, 16.0], "");
        let status_r: ControlRef = char_import_status_label.clone();
        let status_label = char_import_status_label.clone();
        host.add("charImportStatusLabel", char_import_status_label);
        let logout_api_button = ButtonControl::new(
            anchor("TOPLEFT", &status_r, "TOPRIGHT"),
            rect(label_spacing, 0.0, 170.0, 16.0),
            "^7Logout from Path of Exile API".to_string(),
            |_| {
                // :85-88 `main.api:ResetDetails()` (PoEAPI.lua:77-82 + UpdateMain), `main:SaveSettings()`
                crate::poe_api::reset_details();
                main().save_settings();
            },
        );
        logout_api_button.base.shown.set_fn(|_| is_authorized());
        host.add("logoutApiButton", logout_api_button);
        let character_import_anchor = BareControl::new(anchor("TOPLEFT", &so, "TOPLEFT"), rect(label_spacing, 40.0, 200.0, 16.0));
        let cia: ControlRef = character_import_anchor.clone();
        host.add("characterImportAnchor", character_import_anchor);
        section_oauth.base.height.set_fn(|_| if is_authorized() { 200.0 } else { 60.0 });
        let authenticate_button = ButtonControl::new(
            anchor("TOPLEFT", &cia, "TOPLEFT"),
            rect(0.0, 0.0, 200.0, 16.0),
            "^7Authorize with Path of Exile".to_string(),
            |_| {},
        );
        let authenticate = authenticate_button.clone();
        authenticate_button.base.shown.set_fn(|_| !is_authorized());
        host.add("authenticateButton", authenticate_button);
        let char_select_header = label(
            &so,
            "TOPLEFT",
            "TOPLEFT",
            [label_spacing, 40.0, 200.0, 16.0],
            "^7Choose character to import data from:",
        );
        char_select_header.base.shown.set_fn(|_| is_authorized());
        let csh: ControlRef = char_select_header.clone();
        host.add("charSelectHeader", char_select_header);
        // :192-197
        let oauth_error_label = LabelControl::new(anchor("TOPRIGHT", &so, "TOPRIGHT"), rect(-8.0, 40.0, 0.0, 18.0), String::new());
        let oauth_error = oauth_error_label.clone();
        host.add("oauthErrorLabel", oauth_error_label);
        let last_realm = m.accounts.borrow().last_realm.clone().unwrap_or_else(|| "PC".into());
        let account_realm = DropDownControl::new(
            g,
            anchor("TOPLEFT", &csh, "BOTTOMLEFT"),
            rect(0.0, row_spacing, 60.0, 20.0),
            realm_items(),
            None,
            None,
            false,
        );
        sel_by_value(&account_realm, &last_realm);
        let ar: ControlRef = account_realm.clone();
        host.add("accountRealm", account_realm.clone());
        let account_realm_fetch_button = ButtonControl::new(
            anchor("LEFT", &ar, "RIGHT"),
            rect(label_spacing, 0.0, 130.0, 20.0),
            "Fetch Characters".to_string(),
            |_| {},
        );
        host.add("accountRealmFetchButton", account_realm_fetch_button.clone());
        let char_select_league_label = LabelControl::new(
            anchor("TOPLEFT", &ar, "BOTTOMLEFT"),
            rect(0.0, row_spacing, 0.0, 14.0),
            "^7League:".to_string(),
        );
        let csll: ControlRef = char_select_league_label.clone();
        host.add("charSelectLeagueLabel", char_select_league_label);
        let char_select_league = DropDownControl::new(
            g,
            anchor("LEFT", &csll, "RIGHT"),
            rect(label_spacing, 0.0, 150.0, 18.0),
            Vec::new(),
            None,
            None,
            false,
        );
        host.add("charSelectLeague", char_select_league.clone());
        let char_select = DropDownControl::new(
            g,
            anchor("TOPLEFT", &csll, "BOTTOMLEFT"),
            rect(0.0, row_spacing, 400.0, 18.0),
            Vec::new(),
            None,
            None,
            false,
        );
        char_select.base.enabled.set_fn(|_| is_authorized());
        let cs: ControlRef = char_select.clone();
        host.add("charSelect", char_select.clone());
        let char_import_header = label(&cs, "TOPLEFT", "BOTTOMLEFT", [0.0, row_spacing, 200.0, 16.0], "^7Import:");
        let cih: ControlRef = char_import_header.clone();
        host.add("charImportHeader", char_import_header);
        let char_import_tree = button(&cih, "LEFT", "RIGHT", [label_spacing, 0.0, 170.0, 20.0], "Passive Tree and Jewels");
        let sel = char_select.clone();
        char_import_tree
            .base
            .enabled
            .set_fn(move |_| is_authorized() && sel.get_sel_value().is_some());
        let cit: ControlRef = char_import_tree.clone();
        host.add("charImportTree", char_import_tree.clone());
        let clear_jewels = check(
            g,
            &cit,
            90.0,
            18.0,
            "Delete jewels:",
            "Delete all equipped jewels when importing.",
            true,
        );
        host.add("charImportTreeClearJewels", clear_jewels.clone());
        let char_import_items = button(&cit, "TOPLEFT", "BOTTOMLEFT", [0.0, row_spacing, 110.0, 20.0], "Items and Skills");
        let sel = char_select.clone();
        char_import_items
            .base
            .enabled
            .set_fn(move |_| is_authorized() && sel.get_sel_value().is_some());
        let cii: ControlRef = char_import_items.clone();
        host.add("charImportItems", char_import_items.clone());
        let clear_skills = check(
            g,
            &cii,
            85.0,
            18.0,
            "Delete skills:",
            "Delete all existing skills when importing.",
            true,
        );
        host.add("charImportItemsClearSkills", clear_skills.clone());
        let clear_items = check(
            g,
            &cii,
            220.0,
            18.0,
            "Delete equipment:",
            "Delete all equipped items when importing.",
            true,
        );
        host.add("charImportItemsClearItems", clear_items.clone());
        let ignore_weapon_swap = check(
            g,
            &cii,
            380.0,
            18.0,
            "Ignore weapon swap:",
            "Ignore items and skills in weapon swap.",
            false,
        );
        host.add("charImportItemsIgnoreWeaponSwap", ignore_weapon_swap.clone());

        // --- :493-496, addAccountNameControls
        let section_site = SectionControl::new(
            anchor("TOPLEFT", &so, "BOTTOMLEFT"),
            rect(0.0, 18.0, 650.0, 250.0),
            "Import By Account Name".to_string(),
        );
        let ss: ControlRef = section_site.clone();
        host.add("sectionCharSiteImport", section_site);
        let charimport_mode = RefCell::new("GETACCOUNTNAME".to_string());
        let charimport_status = RefCell::new("Idle".to_string());
        let site_status = label(&ss, "TOPLEFT", "TOPLEFT", [6.0, 14.0, 200.0, 16.0], "");
        host.add("siteCharImportStatusLabel", site_status.clone());
        let site_header = label(
            &ss,
            "TOPLEFT",
            "TOPLEFT",
            [6.0, 40.0, 250.0, 16.0],
            "^7To start importing a character, enter the character's account name:",
        );
        let sh: ControlRef = site_header.clone();
        host.add("siteAccountNameHeader", site_header.clone());
        let site_account_realm = DropDownControl::new(
            g,
            anchor("TOPLEFT", &sh, "BOTTOMLEFT"),
            rect(0.0, 4.0, 60.0, 20.0),
            realm_items(),
            None,
            None,
            false,
        );
        sel_by_value(&site_account_realm, &last_realm);
        let sar: ControlRef = site_account_realm.clone();
        host.add("siteAccountRealm", site_account_realm.clone());
        let last_account = m.accounts.borrow().last_account_name.clone();
        let site_account_name = EditControl::new(
            g,
            anchor("LEFT", &sar, "RIGHT"),
            rect(8.0, 0.0, 200.0, 20.0),
            EditArgs {
                init: Some(last_account.clone().unwrap_or_default()),
                filter: Some("%c".into()),
                clearable: true,
                ..EditArgs::default()
            },
        );
        // :342-351
        *site_account_name.paste_filter.borrow_mut() = Some(Rc::new(|text: String| {
            let mut out = String::with_capacity(text.len());
            for b in text.bytes() {
                if b >= 128 {
                    out.push_str(&format!("%{b:02X}"));
                } else {
                    out.push(b as char);
                }
            }
            out
        }));
        let san: ControlRef = site_account_name.clone();
        host.add("siteAccountName", site_account_name.clone());
        // :353-362 `historyList`: the accounts of the settings, sorted
        let mut history: Vec<String> = m.game_accounts.borrow().keys().cloned().collect();
        history.sort_by_key(|a| a.to_ascii_lowercase());
        let site_account_name_go = ButtonControl::new(
            anchor("LEFT", &san, "RIGHT"),
            rect(8.0, 0.0, 60.0, 20.0),
            "Start".to_string(),
            |_| {},
        );
        let site_go = site_account_name_go.clone();
        let e = site_account_name.clone();
        site_account_name_go
            .base
            .enabled
            .set_fn(move |_| lua_match(&e.buf.borrow(), "%S[#%-]%d%d%d%d$"));
        let e = site_account_name.clone();
        site_account_name_go.tip.set_tooltip_func(move |tt, g, ()| {
            tt.clear(false);
            let buf = e.buf.borrow();
            if !lua_match(&buf, "[#%-]%d%d%d%d$") && !buf.is_empty() {
                tt.add_line(g, 16.0, Some(&format!("^7Missing discriminator e.g. {buf}#1234")));
            }
        });
        let sang: ControlRef = site_account_name_go.clone();
        host.add("siteAccountNameGo", site_account_name_go);
        let name = site_account_name.clone();
        let site_account_history = DropDownControl::new(
            g,
            anchor("LEFT", &sang, "RIGHT"),
            rect(8.0, 0.0, 200.0, 20.0),
            history.iter().map(|h| DropItem::str(h)).collect(),
            Some(Rc::new(move |_g: &mut Gfx, _i: usize, v: &DropItem| {
                // :380 the buffer alone
                *name.buf.borrow_mut() = v.label.clone().unwrap_or_default();
            })),
            None,
            false,
        );
        if let Some(l) = &last_account {
            site_account_history.sel_by_label(l);
        }
        site_account_history.check_dropped_width(g, true);
        let sah: ControlRef = site_account_history.clone();
        host.add("siteAccountHistory", site_account_history.clone());
        let hist = site_account_history.clone();
        let site_remove_account = ButtonControl::new(
            anchor("LEFT", &sah, "RIGHT"),
            rect(8.0, 0.0, 20.0, 20.0),
            "X".to_string(),
            move |_g| {
                // :386-393
                let i = hist.sel_index.get();
                let name = hist.list.borrow().get(i.wrapping_sub(1)).and_then(|v| v.label.clone());
                if let Some(name) = name {
                    hist.list.borrow_mut().remove(i - 1);
                    main().game_accounts.borrow_mut().shift_remove(&name);
                }
            },
        );
        site_remove_account.tip.set_tooltip_func(|tt, g, ()| {
            tt.clear(false);
            tt.add_line(g, 16.0, Some("^7Removes account from the dropdown list"));
        });
        host.add("siteRemoveAccount", site_remove_account);
        let missing = LabelControl::new(
            anchor("TOPLEFT", &san, "BOTTOMLEFT"),
            rect(0.0, 8.0, 0.0, 16.0),
            "^1Missing discriminator e.g. #1234".to_string(),
        );
        let e = site_account_name.clone();
        missing.base.shown.set_fn(move |_| {
            let buf = e.buf.borrow();
            !lua_match(&buf, "[#%-]%d%d%d%d$") && !buf.is_empty()
        });
        host.add("siteAccountNameMissingDiscriminator", missing);
        host.add(
            "siteAccountNameUnicode",
            LabelControl::new(
                anchor("TOPLEFT", &sar, "BOTTOMLEFT"),
                rect(0.0, 34.0, 0.0, 14.0),
                "^7Note: if the account name contains non-ASCII characters it must be pasted into the textbox,\nnot typed manually."
                    .to_string(),
            ),
        );
        // Stage: select character and import data (:410-470)
        let site_char_select_header = label(
            &ss,
            "TOPLEFT",
            "TOPLEFT",
            [6.0, 40.0, 200.0, 16.0],
            "^7Choose character to import data from:",
        );
        let schr: ControlRef = site_char_select_header.clone();
        host.add("siteCharSelectHeader", site_char_select_header.clone());
        let site_league_label = label(&schr, "TOPLEFT", "BOTTOMLEFT", [0.0, 6.0, 0.0, 14.0], "^7League:");
        let sll: ControlRef = site_league_label.clone();
        host.add("siteCharSelectLeagueLabel", site_league_label);
        let site_char_select_league = DropDownControl::new(
            g,
            anchor("LEFT", &sll, "RIGHT"),
            rect(4.0, 0.0, 150.0, 18.0),
            Vec::new(),
            None,
            None,
            false,
        );
        host.add("siteCharSelectLeague", site_char_select_league.clone());
        let site_char_select = DropDownControl::new(
            g,
            anchor("TOPLEFT", &schr, "BOTTOMLEFT"),
            rect(0.0, 24.0, 400.0, 18.0),
            Vec::new(),
            None,
            None,
            false,
        );
        let scs: ControlRef = site_char_select.clone();
        host.add("siteCharSelect", site_char_select.clone());
        let site_import_header = label(&scs, "TOPLEFT", "BOTTOMLEFT", [0.0, 16.0, 200.0, 16.0], "^7Import:");
        let sih: ControlRef = site_import_header.clone();
        host.add("siteCharImportHeader", site_import_header);
        let site_tree = button(&sih, "LEFT", "RIGHT", [8.0, 0.0, 170.0, 20.0], "Passive Tree and Jewels");
        let stree: ControlRef = site_tree.clone();
        host.add("siteCharImportTree", site_tree.clone());
        let site_clear_jewels = check(
            g,
            &stree,
            90.0,
            18.0,
            "Delete jewels:",
            "Delete all equipped jewels when importing.",
            true,
        );
        host.add("siteCharImportTreeClearJewels", site_clear_jewels.clone());
        let site_items = button(&stree, "LEFT", "LEFT", [0.0, 36.0, 110.0, 20.0], "Items and Skills");
        let sitems: ControlRef = site_items.clone();
        host.add("siteCharImportItems", site_items.clone());
        let site_clear_skills = check(
            g,
            &sitems,
            85.0,
            18.0,
            "Delete skills:",
            "Delete all existing skills when importing.",
            true,
        );
        host.add("siteCharImportItemsClearSkills", site_clear_skills.clone());
        let site_clear_items = check(
            g,
            &sitems,
            220.0,
            18.0,
            "Delete equipment:",
            "Delete all equipped items when importing.",
            true,
        );
        host.add("siteCharImportItemsClearItems", site_clear_items.clone());
        let site_ignore_weapon_swap = check(
            g,
            &sitems,
            380.0,
            18.0,
            "Ignore weapon swap:",
            "Ignore items and skills in weapon swap.",
            false,
        );
        host.add("siteCharImportItemsIgnoreWeaponSwap", site_ignore_weapon_swap.clone());
        host.add(
            "siteCharBanditNote",
            LabelControl::new(
                anchor("TOPLEFT", &sih, "BOTTOMLEFT"),
                rect(0.0, 50.0, 200.0, 14.0),
                "^7Tip: After you finish importing a character, make sure you update the bandit choice,\nas it can only be imported by logging in above.".to_string(),
            ),
        );
        let site_close = button(&sih, "TOPLEFT", "BOTTOMLEFT", [0.0, 90.0, 60.0, 20.0], "Close");
        host.add("siteCharClose", site_close.clone());

        // --- :500-521 Build sharing
        let section_build = SectionControl::new(
            anchor_collapse("TOPLEFT", &ss, "BOTTOMLEFT"),
            rect(0.0, 18.0, 650.0, 182.0),
            "Build Sharing".to_string(),
        );
        let sb: ControlRef = section_build.clone();
        host.add("sectionBuild", section_build);
        let generate_code_label = label(
            &sb,
            "TOPLEFT",
            "TOPLEFT",
            [6.0, 14.0, 0.0, 16.0],
            "^7Generate a code to share this build with other Path of Building users:",
        );
        let gcl: ControlRef = generate_code_label.clone();
        host.add("generateCodeLabel", generate_code_label);
        let generate_code = button(&gcl, "LEFT", "RIGHT", [4.0, 0.0, 80.0, 20.0], "Generate");
        let gc: ControlRef = generate_code.clone();
        host.add("generateCode", generate_code.clone());
        let enable_party_export_buffs = CheckBoxControl::new(
            g,
            anchor("LEFT", &gc, "RIGHT"),
            [100.0, 0.0, 18.0],
            Some("Export Support"),
            None,
            Some(
                "This is for party play, to export support character, it enables the exporting of auras, curses and modifiers to the enemy",
            ),
            false,
        );
        host.add("enablePartyExportBuffs", enable_party_export_buffs.clone());
        let generate_code_out = EditControl::new(
            g,
            anchor("TOPLEFT", &gcl, "BOTTOMLEFT"),
            rect(0.0, 8.0, 250.0, 20.0),
            EditArgs {
                init: Some(String::new()),
                prompt: Some("Code".into()),
                filter: Some("%Z".into()),
                ..EditArgs::default()
            },
        );
        let e = generate_code_out.clone();
        generate_code_out.base.enabled.set_fn(move |_| !e.buf.borrow().is_empty());
        let gco: ControlRef = generate_code_out.clone();
        host.add("generateCodeOut", generate_code_out.clone());
        let generate_code_copy = button(&gco, "LEFT", "RIGHT", [8.0, 0.0, 60.0, 20.0], "Copy");
        let e = generate_code_out.clone();
        generate_code_copy.base.enabled.set_fn(move |_| !e.buf.borrow().is_empty());
        let e = generate_code_out.clone();
        *generate_code_copy.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| {
            let buf = e.buf.borrow().clone();
            g.copy(&buf);
            e.set_text(g, "", false);
        });
        let gcc: ControlRef = generate_code_copy.clone();
        host.add("generateCodeCopy", generate_code_copy.clone());
        // :523-533 the sites with the fields of an export
        let export_sites: Vec<usize> = WEBSITES.iter().enumerate().filter(|(_, w)| w.3).map(|(i, _)| i).collect();
        let export_items: Vec<DropItem> = export_sites
            .iter()
            .map(|i| {
                let mut it = DropItem::table(WEBSITES[*i].0);
                it.data = Some(Rc::new(WEBSITES[*i].1.to_string()) as Rc<dyn std::any::Any>);
                it
            })
            .collect();
        let export_from = DropDownControl::new(
            g,
            anchor("LEFT", &gcc, "RIGHT"),
            rect(8.0, 0.0, 120.0, 20.0),
            export_items,
            Some(Rc::new(|_g: &mut Gfx, _i: usize, v: &DropItem| {
                // :536-537
                let id = v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned();
                *main().last_export_website.borrow_mut() = id;
            })),
            None,
            false,
        );
        // :539 `SelByValue(self.exportWebsiteSelected or main.lastExportWebsite or "Pastebin", "id")`
        let wanted = m.last_export_website.borrow().clone().unwrap_or_else(|| "Pastebin".into());
        sel_by_value(&export_from, &wanted);
        let ef: ControlRef = export_from.clone();
        host.add("exportFrom", export_from.clone());
        let generate_code_by_link = button(&ef, "LEFT", "RIGHT", [8.0, 0.0, 100.0, 20.0], "Share");
        // :556-571
        let share_enabled = {
            let e = generate_code_out.clone();
            let sites = export_sites.clone();
            move |_: &Gfx| {
                let buf = e.buf.borrow();
                !buf.is_empty() && !sites.iter().any(|i| lua_match(&buf, WEBSITES[*i].2))
            }
        };
        generate_code_by_link.base.enabled.set_fn(share_enabled.clone());
        export_from.base.enabled.set_fn(share_enabled);
        host.add("generateCodeByLink", generate_code_by_link.clone());
        let note = LabelControl::new(
            anchor("TOPLEFT", &gco, "BOTTOMLEFT"),
            rect(0.0, 4.0, 0.0, 14.0),
            "^7Note: this code can be very long; you can use 'Share' to shrink it.".to_string(),
        );
        let nr: ControlRef = note.clone();
        host.add("generateCodeNote", note);
        let import_code_header = label(
            &nr,
            "TOPLEFT",
            "BOTTOMLEFT",
            [0.0, 26.0, 0.0, 16.0],
            "^7To import a build, enter URL or code here:",
        );
        let ich: ControlRef = import_code_header.clone();
        host.add("importCodeHeader", import_code_header);
        let import_code_in = EditControl::new(
            g,
            anchor("TOPLEFT", &ich, "BOTTOMLEFT"),
            rect(0.0, 4.0, 328.0, 20.0),
            EditArgs {
                init: Some(String::new()),
                clearable: true,
                ..EditArgs::default()
            },
        );
        let ici: ControlRef = import_code_in.clone();
        host.add("importCodeIn", import_code_in.clone());
        let import_code_state = LabelControl::new(anchor("LEFT", &ici, "RIGHT"), rect(8.0, 0.0, 0.0, 16.0), String::new());
        let ics = import_code_state.clone();
        host.add("importCodeState", import_code_state);
        let import_code_mode = DropDownControl::new(
            g,
            anchor("TOPLEFT", &ici, "BOTTOMLEFT"),
            rect(0.0, 4.0, 200.0, 20.0),
            ["Import to this build", "Import to a new build", "Import as comparison"]
                .iter()
                .map(|s| DropItem::str(s))
                .collect(),
            None,
            None,
            false,
        );
        let icm: ControlRef = import_code_mode.clone();
        host.add("importCodeMode", import_code_mode.clone());
        let import_code_go = button(&icm, "LEFT", "RIGHT", [8.0, 0.0, 160.0, 20.0], "Import");
        host.add("importCodeGo", import_code_go.clone());

        let tab = Rc::new(Self {
            anchor: a,
            host,
            build,
            me: RefCell::new(Weak::new()),
            charimport_mode,
            charimport_status,
            import_code_in,
            import_code_mode,
            import_code_go,
            generate_code_out,
            generate_code,
            generate_code_copy,
            generate_code_by_link,
            enable_party_export_buffs,
            export_from,
            account_realm_fetch_button,
            char_select,
            char_select_league,
            char_import_tree,
            char_import_items,
            account_realm,
            site_account_realm,
            clear_jewels,
            clear_skills,
            clear_items,
            ignore_weapon_swap,
            site_clear_jewels,
            site_clear_skills,
            site_clear_items,
            site_ignore_weapon_swap,
            oauth_timer: Cell::new(None),
            rate_limit_end_time: Cell::new(None),
            oauth_err_code: RefCell::new(None),
            oauth_loading: Cell::new(false),
            character_list: RefCell::new(HashMap::new()),
            last_char_list: RefCell::new(Vec::new()),
            site_account_name,
            site_account_history,
            site_char_select: site_char_select.clone(),
            site_char_select_league,
            import_code_site: Cell::new(None),
            import_code_detail: RefCell::new(String::new()),
            import_code_xml: RefCell::new(None),
            import_code_valid: Cell::new(false),
            import_code_json: RefCell::new(None),
            import_code_fetching: Cell::new(false),
            auto_fetch_attempted: Cell::new(false),
        });
        *tab.me.borrow_mut() = Rc::downgrade(&tab);
        let w = Rc::downgrade(&tab);
        let t = move || w.upgrade().expect("importTab");

        // the closures over the tab's own state
        // :43-71, :79-82
        let f = t.clone();
        status_label
            .label
            .set_fn(move |_| format!("^7Character import status: {}", f().char_import_status()));
        // :194-197
        let f = t.clone();
        oauth_error.label.set_fn(move |_| {
            f().oauth_err_code.borrow().as_ref().map_or(String::new(), |e| {
                format!("{}Error: {e}", color_code("NEGATIVE").unwrap_or_default())
            })
        });
        // :165-180
        let f = t.clone();
        *authenticate.on_click.borrow_mut() = Rc::new(move |_g: &mut Gfx| {
            let tab = f();
            let w = tab.me.borrow().clone();
            crate::poe_api::fetch_auth_token(move |g, err| {
                let Some(tab) = w.upgrade() else { return };
                let failed = err.is_some();
                *tab.oauth_err_code.borrow_mut() = err;
                tab.oauth_timer.set(None);
                if !failed {
                    // successful login -> fetch
                    tab.fetch_characters(g);
                }
            });
            tab.oauth_timer.set(Some(crate::poe_api::os_time()));
        });
        // :199-202
        let f = t.clone();
        *tab.account_realm.sel_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, _i: usize, _v: &DropItem| {
            f().set_leagues_from_char_list(g)
        }));
        // :205-214
        let f = t.clone();
        tab.account_realm_fetch_button.label.set_fn(move |_| {
            if f().fetch_button_enabled() {
                "Fetch Characters".to_string()
            } else {
                "Fetched".to_string()
            }
        });
        let f = t.clone();
        *tab.account_realm_fetch_button.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| f().fetch_characters(g));
        let f = t.clone();
        tab.account_realm_fetch_button
            .base
            .enabled
            .set_fn(move |_| f().fetch_button_enabled());
        // :218-225 `onLeagueChange`
        let f = t.clone();
        *tab.char_select_league.sel_func.borrow_mut() = Some(Rc::new(move |_g: &mut Gfx, _i: usize, v: &DropItem| {
            let tab = f();
            let realm = realm_code(&realm_of(&tab.account_realm));
            let league = v.label.as_deref().filter(|l| *l != "Any");
            let list = tab.character_list.borrow().get(&realm).cloned();
            tab.build_character_list(&realm, league, list.as_deref(), &tab.char_select);
        }));
        // :250-280
        let f = t.clone();
        *tab.char_import_tree.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| f().on_oauth_tree(g));
        // :287-311
        let f = t.clone();
        *tab.char_import_items.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| f().on_oauth_items(g));
        // :363-367
        let f = t.clone();
        *site_go.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| {
            let tab = f();
            let realm = realm_of(&tab.site_account_realm);
            tab.download_site_character_list(g, &realm);
        });
        // :418-421
        let f = t.clone();
        *tab.site_char_select_league.sel_func.borrow_mut() = Some(Rc::new(move |_g: &mut Gfx, _i: usize, v: &DropItem| {
            let tab = f();
            let realm = realm_code(&realm_of(&tab.site_account_realm));
            let list = tab.last_char_list.borrow().clone();
            tab.build_character_list(&realm, league_of(v).as_deref(), Some(&list), &tab.site_char_select);
        }));
        // :430-441
        let f = t.clone();
        *site_tree.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| {
            let tab = f();
            let realm = realm_of(&tab.site_account_realm);
            if tab.engine_read(pob_engine::engine::Engine::import_has_alloc_nodes) {
                let f2 = tab.me.borrow().clone();
                let r2 = realm.clone();
                main().open_confirm_popup(
                    g,
                    "Character Import",
                    "Importing the passive tree will overwrite your current tree.",
                    "Import",
                    Rc::new(move |g: &mut Gfx| {
                        if let Some(tab) = f2.upgrade() {
                            tab.download_passive_tree(g, &r2);
                        }
                    }),
                    None,
                );
            } else {
                tab.download_passive_tree(g, &realm);
            }
            tab.set_predefined_build_name();
        });
        // :448-452
        let f = t.clone();
        *site_items.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| {
            let tab = f();
            let realm = realm_of(&tab.site_account_realm);
            tab.download_items(g, &realm);
            tab.set_predefined_build_name();
        });
        // :540-555
        let f = t.clone();
        *tab.generate_code_by_link.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| f().on_share(g));
        let f = t.clone();
        site_status
            .label
            .set_fn(move |_| format!("^7Character import status: {}", f().charimport_status.borrow()));
        let f = t.clone();
        site_header
            .base
            .shown
            .set_fn(move |_| *f().charimport_mode.borrow() == "GETACCOUNTNAME");
        let f = t.clone();
        site_char_select_header.base.shown.set_fn(move |_| {
            let tab = f();
            let m = tab.charimport_mode.borrow();
            *m == "SELECTCHAR" || *m == "IMPORTING"
        });
        for en in [site_char_select.base(), site_tree.base(), site_items.base()] {
            let f = t.clone();
            en.enabled.set_fn(move |_| *f().charimport_mode.borrow() == "SELECTCHAR");
        }
        let f = t.clone();
        *site_close.on_click.borrow_mut() = Rc::new(move |_g: &mut Gfx| {
            let tab = f();
            *tab.charimport_mode.borrow_mut() = "GETACCOUNTNAME".to_string();
            *tab.charimport_status.borrow_mut() = "Idle".to_string();
        });
        let f = t.clone();
        ics.label.set_fn(move |_| f().import_code_detail.borrow().clone());
        let f = t.clone();
        *tab.generate_code.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| f().on_generate(g));
        let f = t.clone();
        *tab.enable_party_export_buffs.change_func.borrow_mut() =
            Some(Rc::new(move |g: &mut Gfx, state: bool| f().on_export_buffs(g, state)));
        let f = t.clone();
        *tab.import_code_in.change_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, buf: &str, _placeholder: bool| {
            f().import_code_handle(g, buf);
        }));
        let f = t.clone();
        *tab.import_code_in.enter_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, _buf: &str| {
            let tab = f();
            if tab.import_code_valid.get() {
                tab.on_import_go(g);
            }
        }));
        let f = t.clone();
        tab.import_code_mode.base.enabled.set_fn(move |_| {
            let tab = f();
            (tab.db_file_name_set() || tab.import_code_mode.sel_index.get() == 3) && tab.import_code_valid.get()
        });
        let f = t.clone();
        *tab.import_code_go.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| f().on_import_go(g));
        // :710-712
        let f = t.clone();
        tab.import_code_go.label.set_fn(move |_| {
            if f().import_code_fetching.get() {
                "Retrieving paste..".to_string()
            } else {
                "Import".to_string()
            }
        });
        let f = t.clone();
        tab.import_code_go.base.enabled.set_fn(move |_| {
            let tab = f();
            tab.import_code_valid.get() && !tab.import_code_fetching.get()
        });
        let f = t.clone();
        *tab.import_code_go.enter_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx| {
            let tab = f();
            if tab.import_code_valid.get() {
                tab.on_import_go(g);
            }
        }));

        // :722-729: a saved login is checked once; past its expiry that is a request
        if is_authorized() {
            let w = Rc::downgrade(&tab);
            crate::poe_api::validate_auth(g, move |_g, _valid, err| {
                if let (Some(err), Some(tab)) = (err, w.upgrade()) {
                    *tab.oauth_err_code.borrow_mut() = Some(format!("Token refresh failed: {err}"));
                }
            });
        }
        // :742-757 Load
        let state = tab.engine_export_party();
        tab.enable_party_export_buffs.state.set(state);
        tab
    }

    fn db_file_name_set(&self) -> bool {
        let b = self.build.upgrade().expect("buildMode");
        b.db_file_name.borrow().path().is_some()
    }

    fn engine_export_party(&self) -> bool {
        let b = self.build.upgrade().expect("buildMode");
        let e = b.engine.borrow();
        e.as_ref().is_some_and(pob_engine::engine::Engine::import_export_party)
    }

    /// :507-510. `PartyTab:exportBuffs` is the step R-party-export.
    fn on_export_buffs(&self, g: &mut Gfx, state: bool) {
        let b = self.build.upgrade().expect("buildMode");
        if state {
            self.enable_party_export_buffs.state.set(false);
            not_ported(
                g,
                "Classes/ImportTab.lua:507-510 enablePartyExportBuffs, Classes/PartyTab.lua:996-1047 exportBuffs (R-party-export)",
            );
            return;
        }
        if let Some(e) = b.engine.borrow_mut().as_mut() {
            e.import_set_export_party(false);
        }
        b.build_flag.set(true);
    }

    /// :504-506
    fn on_generate(&self, g: &mut Gfx) {
        let b = self.build.upgrade().expect("buildMode");
        match b.save_db_text() {
            Ok(xml) => {
                let code = pob_build::encode_build_code(xml.as_bytes());
                self.generate_code_out.set_text(g, &code, false);
            }
            Err(e) if e.contains("unimplemented") => not_ported(g, &e),
            Err(e) => panic!("{e}"),
        }
    }

    /// :575-642 `importCodeHandle`.
    fn import_code_handle(&self, g: &mut Gfx, buf: &str) {
        self.import_code_site.set(None);
        self.import_code_detail.borrow_mut().clear();
        *self.import_code_xml.borrow_mut() = None;
        self.import_code_valid.set(false);
        *self.import_code_json.borrow_mut() = None;
        if buf.is_empty() {
            return;
        }
        if !self.db_file_name_set() {
            self.import_code_mode.sel_index.set(2);
        }
        *self.import_code_detail.borrow_mut() = colored("NEGATIVE", "Invalid input");
        let (site, url_text) = url_of(buf);
        if let Some(j) = site {
            self.import_code_valid.set(true);
            *self.import_code_detail.borrow_mut() = colored("POSITIVE", &format!("URL is valid ({})", WEBSITES[j - 1].0));
            self.import_code_site.set(Some(j));
            if buf != url_text {
                self.import_code_in.set_text(g, &url_text, false);
            }
            return;
        }
        // :611-630 the JSON of a character, dev mode only
        if launch().dev_mode.get() && lua_match(&url_text, "^%{.*%}$") {
            *self.import_code_detail.borrow_mut() = colored("NEGATIVE", "Invalid JSON format (decode error)");
            let Ok(v) = serde_json::from_str::<Json>(&url_text) else {
                return;
            };
            let Some(c) = v.get("character").filter(|c| !c.is_null() && *c != &json!(false)) else {
                *self.import_code_detail.borrow_mut() = colored("NEGATIVE", "Invalid JSON format (character missing)");
                return;
            };
            if c.get("equipment").is_none() || c.get("passives").is_none() {
                *self.import_code_detail.borrow_mut() = colored("NEGATIVE", "Invalid JSON format (equipment or passives missing)");
                return;
            }
            *self.import_code_json.borrow_mut() = Some(c.clone());
            *self.import_code_detail.borrow_mut() = colored("POSITIVE", "JSON is valid");
            self.import_code_valid.set(true);
            return;
        }
        let CodeInput::Xml(xml) = code_xml(buf) else { return };
        if launch().dev_mode.get() && sg::is_key_down(g, "SHIFT") {
            g.copy(&xml);
        }
        self.import_code_valid.set(true);
        *self.import_code_detail.borrow_mut() = colored("POSITIVE", "Code is valid");
        *self.import_code_xml.borrow_mut() = Some(xml);
    }

    /// :685-709 the Import button.
    fn on_import_go(&self, g: &mut Gfx) {
        if let Some(site) = self.import_code_site.get()
            && self.import_code_xml.borrow().is_none()
        {
            self.import_code_fetching.set(true);
            let w = self.me.borrow().clone();
            crate::build_sites::download_build(&self.import_code_in.buf.borrow(), site, move |g, ok, data| {
                let Some(tab) = w.upgrade() else { return };
                tab.import_code_fetching.set(false);
                if ok {
                    tab.import_code_handle(g, &data);
                    tab.import_selected_build(g);
                } else {
                    *tab.import_code_detail.borrow_mut() = colored("NEGATIVE", &data);
                    tab.import_code_valid.set(false);
                }
            });
            return;
        }
        let json = self.import_code_json.borrow().clone();
        if let Some(json) = json {
            self.import_items_and_skills(g, &json, true, true, false);
            self.import_passive_tree_and_jewels(g, &json, true);
            return;
        }
        self.import_selected_build(g);
    }

    /// :644-669 `importSelectedBuild`.
    fn import_selected_build(&self, g: &mut Gfx) {
        if !self.import_code_valid.get() || self.import_code_fetching.get() {
            return;
        }
        let xml = self.import_code_xml.borrow().clone().unwrap_or_default();
        let b = self.build.upgrade().expect("buildMode");
        // :652, :666 `self.importCodeSite and self.controls.importCodeIn.buf or nil`
        let link = self.import_code_site.get().map(|_| self.import_code_in.buf.borrow().clone());
        match self.import_code_mode.sel_index.get() {
            1 => {
                let w = self.build.clone();
                let link = link.clone();
                main().open_confirm_popup(
                    g,
                    "Build Import",
                    &format!(
                        "{}Warning:^7 Importing to the current build will erase ALL existing data for this build.",
                        color_code("WARNING").expect("WARNING")
                    ),
                    "Import",
                    Rc::new(move |g: &mut Gfx| {
                        let b = w.upgrade().expect("buildMode");
                        let (db, name) = b.init_args();
                        b.shutdown(g);
                        let mut args = vec![db, name, ModeArg::Str(xml.clone()), ModeArg::Bool(false)];
                        args.extend(link.clone().map(ModeArg::Str));
                        b.init(g, &args);
                        *b.view_mode.borrow_mut() = "TREE".into();
                    }),
                    None,
                );
            }
            3 => not_ported(g, "Classes/ImportTab.lua:655-663 compareTab:ImportBuild (the Compare tab, U8b)"),
            _ => {
                b.shutdown(g);
                let mut args = vec![
                    ModeArg::Bool(false),
                    ModeArg::Str("Imported build".into()),
                    ModeArg::Str(xml),
                    ModeArg::Bool(false),
                ];
                args.extend(link.map(ModeArg::Str));
                b.init(g, &args);
                *b.view_mode.borrow_mut() = "TREE".into();
            }
        }
    }

    fn engine_read<R>(&self, f: impl FnOnce(&pob_engine::engine::Engine) -> R) -> R {
        let b = self.build.upgrade().expect("buildMode");
        let e = b.engine.borrow();
        f(e.as_ref().expect("engine"))
    }

    fn engine_edit<R>(&self, f: impl FnOnce(&mut pob_engine::engine::Engine) -> Result<R, String>) -> R {
        let b = self.build.upgrade().expect("buildMode");
        let mut e = b.engine.borrow_mut();
        f(e.as_mut().expect("engine")).unwrap_or_else(|e| panic!("{e}"))
    }

    /// :43-46
    fn fetch_button_enabled(&self) -> bool {
        !self
            .character_list
            .borrow()
            .contains_key(&realm_code(&realm_of(&self.account_realm)))
    }

    /// :47-71 `charImportStatus`.
    fn char_import_status(&self) -> String {
        let now = crate::poe_api::os_time();
        let authorized = is_authorized();
        if !authorized {
            let Some(timer) = self.oauth_timer.get() else {
                return colored("WARNING", "Not authenticated");
            };
            let left = f64::max(0.0, timer + 60.0 - now);
            if left < 1.0 {
                self.oauth_timer.set(None);
                return colored("WARNING", "Not authenticated");
            }
            return format!(
                "Logging in... ({left}) - URL copied to clipboard{}",
                self.oauth_err_code.borrow().as_deref().unwrap_or("")
            );
        }
        if let Some(end) = self.rate_limit_end_time.get() {
            let left = f64::max(0.0, end - now);
            if left < 0.5 {
                self.rate_limit_end_time.set(None);
                return "Authenticated".to_string();
            }
            return colored("WARNING", &format!("You're doing that too fast. Please wait ({left})"));
        }
        if self.oauth_loading.get() {
            return if self.fetch_button_enabled() {
                "Fetching..."
            } else {
                "Importing..."
            }
            .to_string();
        }
        "Authenticated".to_string()
    }

    /// :98-136 `setLeaguesFromCharList`.
    fn set_leagues_from_char_list(&self, g: &mut Gfx) {
        let realm = realm_code(&realm_of(&self.account_realm));
        let chars = self.character_list.borrow().get(&realm).cloned().unwrap_or_default();
        if chars.is_empty() {
            self.char_select_league.set_list(g, Some(Vec::new()));
            self.char_select.set_list(g, Some(Vec::new()));
            return;
        }
        let mut leagues: Vec<String> = Vec::new();
        for c in &chars {
            if let Some(l) = c["league"].as_str()
                && !leagues.iter().any(|x| x == l)
            {
                leagues.push(l.to_string());
            }
        }
        leagues.sort_by_key(|a| a.to_ascii_lowercase());
        leagues.push("Any".to_string());
        self.char_select_league
            .set_list(g, Some(leagues.iter().map(|l| DropItem::str(l)).collect()));
        self.char_select_league.sel_index.set(0);
        let last = main().accounts.borrow().last_league.clone();
        if let Some(i) = last.and_then(|last| leagues.iter().position(|v| *v == last)) {
            self.char_select_league.set_sel(g, i as i64 + 1, false);
        }
        if self.char_select_league.sel_index.get() == 0 {
            self.char_select_league.set_sel(g, 1, false);
        }
    }

    /// :138-162 `fetchCharacters`.
    fn fetch_characters(&self, g: &mut Gfx) {
        if !is_authorized() {
            return;
        }
        let realm = realm_code(&realm_of(&self.account_realm));
        self.oauth_loading.set(true);
        let w = self.me.borrow().clone();
        let r2 = realm.clone();
        crate::poe_api::download_character_list(g, &realm, move |g, body, err, time_next| {
            let Some(tab) = w.upgrade() else { return };
            match err {
                None => {
                    let list = body.and_then(|b| b["characters"].as_array().cloned());
                    match list {
                        Some(l) => {
                            tab.character_list.borrow_mut().insert(r2, l);
                        }
                        None => {
                            tab.character_list.borrow_mut().remove(&r2);
                        }
                    }
                    tab.set_leagues_from_char_list(g);
                    *tab.oauth_err_code.borrow_mut() = None;
                }
                Some(e) if e == "Response code: 429" => tab.rate_limit_end_time.set(time_next),
                Some(e) if e.contains("401") => {
                    // token has been invalidated for some reason
                    *tab.oauth_err_code.borrow_mut() = Some("Auth token is invalid. Please login again.".to_string());
                    crate::poe_api::reset_details();
                }
                Some(e) => *tab.oauth_err_code.borrow_mut() = Some(e),
            }
            tab.oauth_loading.set(false);
        });
    }

    /// :239-246 `saveDetails`.
    fn save_details(&self, realm_id: &str, league: Option<&str>, name: &str) {
        {
            let m = main();
            let mut a = m.accounts.borrow_mut();
            a.last_realm = Some(realm_id.to_string());
            a.last_league = league.map(str::to_string);
        }
        self.engine_edit(|e| {
            e.import_save_details(Some(realm_id), league, Some(name));
            Ok(())
        });
    }

    fn oauth_selection(&self) -> (String, Option<String>, String) {
        let realm = realm_of(&self.account_realm);
        let league = self.char_select_league.get_sel_value().and_then(|v| v.label);
        let name = self
            .char_select
            .get_sel_value()
            .and_then(|v| v.label)
            .unwrap_or_else(|| panic!("Classes/ImportTab.lua:253: attempt to index a nil value"));
        (realm, league, name)
    }

    /// `importHandler` (:257-269), the items' (:295-310): the error of a missing character.
    fn oauth_import_failed(&self, err: Option<String>) {
        *self.oauth_err_code.borrow_mut() = Some(match err {
            Some(e) => format!("Could not import: {e}"),
            None => "Could not import character".to_string(),
        });
    }

    /// :250-280
    fn on_oauth_tree(&self, g: &mut Gfx) {
        let (realm, league, name) = self.oauth_selection();
        self.save_details(&realm, league.as_deref(), &name);
        let delete_jewels = self.clear_jewels.state.get();
        let w = self.me.borrow().clone();
        let start = move |g: &mut Gfx, w: Weak<Self>, realm: String, name: String| {
            let Some(tab) = w.upgrade() else { return };
            tab.oauth_loading.set(true);
            let w2 = w.clone();
            crate::poe_api::download_character(g, &realm_code(&realm), &name, move |g, data, err, _| {
                let Some(tab) = w2.upgrade() else { return };
                match data.as_ref().map(|d| &d["character"]).filter(|c| !c.is_null()) {
                    Some(c) => {
                        *tab.oauth_err_code.borrow_mut() = None;
                        tab.import_passive_tree_and_jewels(g, c, delete_jewels);
                    }
                    None => tab.oauth_import_failed(err),
                }
                tab.oauth_loading.set(false);
            });
        };
        if self.engine_read(pob_engine::engine::Engine::import_has_alloc_nodes) {
            let run = RefCell::new(Some((w, realm, name)));
            main().open_confirm_popup(
                g,
                "Character Import",
                "Importing the passive tree will overwrite your current tree.",
                "Import",
                Rc::new(move |g: &mut Gfx| {
                    if let Some((w, realm, name)) = run.borrow_mut().take() {
                        start(g, w, realm, name);
                    }
                }),
                None,
            );
        } else {
            start(g, w, realm, name);
        }
    }

    /// :287-311
    fn on_oauth_items(&self, g: &mut Gfx) {
        let (realm, league, name) = self.oauth_selection();
        self.save_details(&realm, league.as_deref(), &name);
        self.oauth_loading.set(true);
        let w = self.me.borrow().clone();
        crate::poe_api::download_character(g, &realm_code(&realm), &name, move |g, data, err, _| {
            let Some(tab) = w.upgrade() else { return };
            match data.as_ref().map(|d| &d["character"]).filter(|c| !c.is_null()) {
                Some(c) => {
                    *tab.oauth_err_code.borrow_mut() = None;
                    let (items, skills, swap) = (
                        tab.clear_items.state.get(),
                        tab.clear_skills.state.get(),
                        tab.ignore_weapon_swap.state.get(),
                    );
                    tab.import_items_and_skills(g, c, items, skills, swap);
                }
                None => tab.oauth_import_failed(err),
            }
            tab.oauth_loading.set(false);
        });
    }

    /// :540-555 the Share button.
    fn on_share(&self, g: &mut Gfx) {
        let i = self.export_from.sel_index.get();
        let export: Vec<usize> = WEBSITES.iter().enumerate().filter(|(_, w)| w.3).map(|(j, _)| j).collect();
        let Some(site) = i.checked_sub(1).and_then(|i| export.get(i)).copied() else {
            panic!("Classes/ImportTab.lua:47: attempt to index local 'websiteInfo'");
        };
        let code = self.generate_code_out.buf.borrow().clone();
        let w = self.me.borrow().clone();
        let started = crate::build_sites::upload_build(&code, site + 1, move |g, link, err| {
            let Some(tab) = w.upgrade() else { return };
            tab.generate_code_by_link.label.set("Share".to_string());
            match (link, err) {
                (Some(link), _) => {
                    let text = format!("{}{link}", crate::build_sites::SITE_NET[site].2);
                    tab.generate_code_out.set_text(g, &text, false);
                }
                (None, err) => {
                    main().open_message_popup(g, WEBSITES[site].1, &format!("Error creating link:\n{}", err.unwrap_or_default()));
                }
            }
        });
        if started {
            self.generate_code_out.set_text(g, "", false);
            self.generate_code_by_link.label.set("Creating link...".to_string());
        }
    }

    /// `SaveAccountHistory` (:802-812).
    fn save_account_history(&self, g: &mut Gfx) {
        let buf = self.site_account_name.buf.borrow().clone();
        let has = self
            .site_account_history
            .list
            .borrow()
            .iter()
            .any(|v| v.label.as_deref() == Some(&buf));
        if !has {
            {
                let mut l = self.site_account_history.list.borrow_mut();
                l.push(DropItem::str(&buf));
                l.sort_by_key(|v| v.label.clone().unwrap_or_default().to_ascii_lowercase());
            }
            self.site_account_history.check_dropped_width(g, true);
            self.site_account_history.sel_by_label(&buf);
        }
    }

    /// `character-window/get-...?accountName=...&character=...&realm=...` of the selected character.
    fn character_url(&self, what: &str, realm: &str, char_name: &str) -> String {
        let account = self.site_account_name.buf.borrow().clone();
        format!(
            "{HOST_NAME}character-window/{what}?accountName={}&character={}&realm={}",
            account_in_url(&account),
            url_encode(char_name),
            realm_code(realm)
        )
    }

    fn site_selected_char(&self) -> Rc<Json> {
        self.site_char_select
            .get_sel_value()
            .and_then(|v| char_of(&v))
            .unwrap_or_else(|| panic!("Classes/ImportTab.lua:819: attempt to index a nil value"))
    }

    /// `self.lastCharacterHash`, `self.lastLeague` of a downloaded character (:834-837, :872-875).
    fn note_downloaded(&self, name: &str) {
        let league = self.site_char_select_league.get_sel_value().and_then(|v| league_of(&v));
        self.engine_edit(|e| {
            e.import_set_character(name);
            if e.import_last_league().is_none() {
                e.import_set_last_league(league.as_deref());
            }
            Ok(())
        });
    }

    /// The common part of a character download's answer: `true` when the body can be read (:825-833, :863-871).
    fn site_answer_ok(&self, body: &str, err: Option<&str>) -> bool {
        *self.charimport_mode.borrow_mut() = "SELECTCHAR".to_string();
        if let Some(e) = err {
            *self.charimport_status.borrow_mut() = colored(
                "NEGATIVE",
                &format!("Error importing character data, try again ({})", e.replace('\n', " ")),
            );
            return false;
        }
        if body == "false" {
            *self.charimport_status.borrow_mut() = colored("NEGATIVE", "Failed to retrieve character data, try again.");
            return false;
        }
        true
    }

    /// `DownloadPassiveTree(realm)` (:814-850).
    fn download_passive_tree(&self, _g: &mut Gfx, realm: &str) {
        *self.charimport_mode.borrow_mut() = "IMPORTING".to_string();
        *self.charimport_status.borrow_mut() = "Retrieving character passive tree...".to_string();
        let chr = self.site_selected_char();
        let name = chr["name"].as_str().unwrap_or("").to_string();
        let url = self.character_url("get-passive-skills", realm, &name);
        let w = self.me.borrow().clone();
        launch().download_page(&url, PageParams::default(), move |g, response, err| {
            let Some(tab) = w.upgrade() else { return };
            if !tab.site_answer_ok(&response.body, err.as_deref()) {
                return;
            }
            tab.note_downloaded(&name);
            let mut r: Json = serde_json::from_str(&response.body)
                .unwrap_or_else(|_| panic!("Classes/ImportTab.lua:840: attempt to index local 'responseLua' (a nil value)"));
            // Account-name imports omit quest choices, so keep the build's current values.
            for (field, var) in [
                ("bandit_choice", "bandit"),
                ("pantheon_major", "pantheonMajorGod"),
                ("pantheon_minor", "pantheonMinorGod"),
            ] {
                if r[field].is_null() {
                    let cur = tab.engine_read(|e| e.config_input_string(var));
                    r[field] = cur.map_or(Json::Null, Json::String);
                }
            }
            // modify response to be like the oauth API response
            let mut char_data = (*chr).clone();
            char_data["jewels"] = r["items"].clone();
            char_data["passives"] = r;
            let delete_jewels = tab.site_clear_jewels.state.get();
            tab.import_passive_tree_and_jewels(g, &char_data, delete_jewels);
        });
    }

    /// `DownloadItems(realm)` (:852-886).
    fn download_items(&self, _g: &mut Gfx, realm: &str) {
        *self.charimport_mode.borrow_mut() = "IMPORTING".to_string();
        *self.charimport_status.borrow_mut() = "Retrieving character items...".to_string();
        let chr = self.site_selected_char();
        let name = chr["name"].as_str().unwrap_or("").to_string();
        let url = self.character_url("get-items", realm, &name);
        let w = self.me.borrow().clone();
        launch().download_page(&url, PageParams::default(), move |g, response, err| {
            let Some(tab) = w.upgrade() else { return };
            if !tab.site_answer_ok(&response.body, err.as_deref()) {
                return;
            }
            tab.note_downloaded(&name);
            let r: Json = serde_json::from_str(&response.body)
                .unwrap_or_else(|_| panic!("Classes/ImportTab.lua:879: attempt to index local 'responseLua' (a nil value)"));
            let mut char_data = (*chr).clone();
            char_data["equipment"] = r["items"].clone();
            char_data["guardian"] = r["guardian"].clone();
            let (items, skills, swap) = (
                tab.site_clear_items.state.get(),
                tab.site_clear_skills.state.get(),
                tab.site_ignore_weapon_swap.state.get(),
            );
            tab.import_items_and_skills(g, &char_data, items, skills, swap);
        });
    }

    /// `DownloadSiteCharacterList(realm)` (:887-1021).
    fn download_site_character_list(&self, _g: &mut Gfx, realm: &str) {
        let code = realm_code(realm);
        *self.charimport_mode.borrow_mut() = "DOWNLOADCHARLIST".to_string();
        *self.charimport_status.borrow_mut() = "Retrieving character list...".to_string();
        let buf = self.site_account_name.buf.borrow().clone();
        // Handle spaces in the account name
        let account = if code == "pc" {
            lua_gsub(&buf, "%s+", "")
        } else {
            lua_gsub(&lua_gsub(&lua_gsub(&buf, "^[%s?]+", ""), "[%s?]+$", ""), "%s", "+")
        };
        let account = account_with_hash(&account);
        let url = format!(
            "{HOST_NAME}character-window/get-characters?accountName={}&realm={code}",
            account_in_url(&account)
        );
        let w = self.me.borrow().clone();
        let realm = realm.to_string();
        launch().download_page(&url, PageParams::default(), move |_g, response, err| {
            let Some(tab) = w.upgrade() else { return };
            let fail = |text: String| {
                *tab.charimport_status.borrow_mut() = colored("NEGATIVE", &text);
                *tab.charimport_mode.borrow_mut() = "GETACCOUNTNAME".to_string();
            };
            match err.as_deref() {
                Some("Response code: 401" | "Response code: 403") => {
                    return fail("Account profile is private or does not exist.".to_string());
                }
                Some("Response code: 404") => return fail("Account name is incorrect.".to_string()),
                Some(e) => return fail(format!("Error retrieving character list, try again ({})", e.replace('\n', " "))),
                None => {}
            }
            let Some(char_list) = site_json(&response.body) else {
                return fail("Error processing character list, try again later".to_string());
            };
            if char_list.is_empty() {
                return fail("The account has no characters to import.".to_string());
            }
            // GGG's character API: the account name is case-sensitive for /get-passive-skills and /get-items. The profile page has the correct one.
            let w2 = w.clone();
            let realm = realm.clone();
            let url = format!("{HOST_NAME}{PROFILE_URL}{}", account_in_url(&account));
            launch().download_page(&url, PageParams::default(), move |g, response, err| {
                let Some(tab) = w2.upgrade() else { return };
                if let Some(e) = err {
                    *tab.charimport_status.borrow_mut() = colored(
                        "NEGATIVE",
                        &format!("Error retrieving character list, try again ({})", e.replace('\n', " ")),
                    );
                    *tab.charimport_mode.borrow_mut() = "GETACCOUNTNAME".to_string();
                    return;
                }
                tab.site_character_list_done(g, &realm, &response.body, char_list);
            });
        });
    }

    /// :955-1018
    fn site_character_list_done(&self, g: &mut Gfx, realm: &str, profile: &str, char_list: Vec<Json>) {
        let caps = pob_lua::match_(profile.as_bytes(), b"/view%-profile/([^/]+)/characters", None).expect("pattern");
        let Some(found) = caps
            .as_ref()
            .and_then(|c| c.get(0).and_then(|x| x.bytes(profile.as_bytes())))
            .map(<[u8]>::to_vec)
        else {
            panic!("Classes/ImportTab.lua:956: attempt to index a nil value");
        };
        // :956-957 `string.format("%%%2X", c:byte(1))` of what is past ASCII
        let mut real = String::with_capacity(found.len());
        for b in found {
            if b > 127 {
                real.push_str(&format!("%{b:2X}"));
            } else {
                real.push(b as char);
            }
        }
        let account = account_with_hash(&real);
        self.site_account_name.set_text(g, &account, false);
        *self.charimport_status.borrow_mut() = "Character list successfully retrieved.".to_string();
        *self.charimport_mode.borrow_mut() = "SELECTCHAR".to_string();
        self.engine_edit(|e| {
            e.import_set_last_realm(realm);
            e.import_set_account(&account);
            Ok(())
        });
        {
            let m = main();
            {
                let mut a = m.accounts.borrow_mut();
                a.last_realm = Some(realm.to_string());
                a.last_account_name = Some(account.clone());
            }
            m.game_accounts.borrow_mut().insert(account.clone(), None);
        }
        let mut leagues: Vec<String> = Vec::new();
        for c in &char_list {
            if let Some(l) = c["league"].as_str()
                && !leagues.iter().any(|x| x == l)
            {
                leagues.push(l.to_string());
            }
        }
        leagues.sort();
        let league_ctl = &self.site_char_select_league;
        {
            let mut l = league_ctl.list.borrow_mut();
            l.clear();
            for league in &leagues {
                let mut it = DropItem::table(league);
                it.data = Some(Rc::new(league.clone()) as Rc<dyn std::any::Any>);
                l.push(it);
            }
            l.push(DropItem::table("All"));
        }
        // set the league combo to the last used if possible, used for previously imported characters
        let last = self.engine_read(pob_engine::engine::Engine::import_last_league);
        if let Some(last) = last {
            sel_by_value(league_ctl, &last);
            let cur = |c: &DropDownControl| c.get_sel_value().and_then(|v| league_of(&v));
            if cur(league_ctl).as_deref() != Some(last.as_str()) {
                // League maybe over, Character will be in standard
                let standard = standard_league(&last).to_string();
                sel_by_value(league_ctl, &standard);
                if cur(league_ctl).as_deref() != Some(standard.as_str()) {
                    // give up and select the first entry. Ruthless mode may not have Standard equivalents
                    league_ctl.sel_index.set(1);
                } else {
                    self.engine_edit(|e| {
                        e.import_set_last_league(Some(&standard));
                        Ok(())
                    });
                }
            }
        } else if league_ctl.sel_index.get() > league_ctl.list.borrow().len() {
            league_ctl.sel_index.set(1);
        }
        *self.last_char_list.borrow_mut() = char_list.clone();
        let league = league_ctl.get_sel_value().and_then(|v| league_of(&v));
        self.build_character_list(&realm_code(realm), league.as_deref(), Some(&char_list), &self.site_char_select);
        // We only get here if the accountname was correct, found, and not private, so add it to the account history.
        self.save_account_history(g);
    }

    /// `BuildCharacterList(realm, league, characters, control)` (:1027-1095).
    fn build_character_list(&self, realm: &str, league: Option<&str>, characters: Option<&[Json]>, control: &DropDownControl) {
        control.list.borrow_mut().clear();
        let Some(characters) = characters else { return };
        let mut out: Vec<DropItem> = Vec::new();
        for c in characters {
            if c["realm"].as_str() == Some(realm) && (league.is_none() || c["league"].as_str() == league) {
                let lvl = c["level"].as_f64().unwrap_or(0.0);
                let char_league = c["league"].as_str().unwrap_or("?");
                let name = c["name"].as_str().unwrap_or("?");
                let class = c["class"].as_str().unwrap_or("?");
                let color = class_color(class);
                let detail = if league.is_none() {
                    format!("{color}{class} ^x808080lvl {} in {char_league}", lvl as i64)
                } else {
                    format!("{color}{class} ^x808080lvl {}", lvl as i64)
                };
                let mut it = DropItem::table(name);
                it.detail = Some(detail);
                it.search_filter = Some(format!("{name} {class}"));
                it.data = Some(Rc::new(c.clone()) as Rc<dyn std::any::Any>);
                out.push(it);
            }
        }
        out.sort_by_key(|v| v.label.clone().unwrap_or_default().to_lowercase());
        let hash = self.engine_read(pob_engine::engine::Engine::import_last_character_hash);
        let at = hash.and_then(|h| {
            out.iter()
                .position(|v| pob_net::sha1_hex(v.label.as_deref().unwrap_or("").as_bytes()) == h)
        });
        *control.list.borrow_mut() = out;
        control.sel_index.set(at.map_or(1, |i| i + 1));
    }

    /// `SetPredefinedBuildName` (:1926-1932).
    fn set_predefined_build_name(&self) {
        let account = lua_gsub(&lua_gsub(&self.site_account_name.buf.borrow(), "%s+", ""), "#%d+", "");
        let chr = self.site_selected_char();
        *main().predefined_build_name.borrow_mut() = Some(format!("{account} - {}", chr["name"].as_str().unwrap_or("")));
    }

    /// `ImportPassiveTreeAndJewels(charData, deleteJewels)` (:1131-1253).
    pub fn import_passive_tree_and_jewels(&self, g: &mut Gfx, char_data: &Json, delete_jewels: bool) {
        let b = self.build.upgrade().expect("buildMode");
        let tree_tab = b.tree_tab.borrow().clone();
        if let Some(t) = &tree_tab {
            t.ensure_undo();
        }
        let display = b.skills_tab.borrow().as_ref().and_then(|t| t.display_index());
        let extra = self.engine_read(pob_engine::engine::Engine::import_extra_points);
        self.engine_edit(|e| e.import_jewels(char_data, delete_jewels, display));
        let items_tab = b.items_tab.borrow().clone();
        if let Some(t) = &items_tab {
            t.after_import(g);
        }
        let out = self.engine_edit(|e| e.import_passive_tree(char_data, extra));
        if let Some(t) = &tree_tab {
            t.sync_active_spec(g);
            t.add_undo_state();
        }
        // :1213-1216 `SetText` calls no `changeFunc`
        let text = out.level_text.clone().unwrap_or_else(|| "nil".to_string());
        self.engine_edit(|e| {
            e.import_set_level(out.level);
            e.set_character_level_auto_mode(false);
            Ok(())
        });
        b.c().character_level.set_text(g, &text, false);
        let config = b.config_tab.borrow().clone();
        if let Some(c) = config {
            c.import_set_sel(g, "resistancePenalty", out.resistance_penalty_index);
            c.import_set_sel_by_val(g, "bandit", &out.bandit);
            c.import_set_sel_by_val(g, "pantheonMajorGod", &out.major_god);
            c.import_set_sel_by_val(g, "pantheonMinorGod", &out.minor_god);
        }
        let (name, class, league) = &out.title;
        let build_name = b.build_name.borrow().clone().unwrap_or_default();
        main().set_window_title_subtext(g, Some(&format!("{build_name} ({name}, {class}, {league})")));
    }

    /// `ImportItemsAndSkills(charData, clearItems, clearSkills, ignoreWeaponSwap)` (:1381-1491).
    pub fn import_items_and_skills(&self, g: &mut Gfx, char_data: &Json, clear_items: bool, clear_skills: bool, ignore_swap: bool) {
        let b = self.build.upgrade().expect("buildMode");
        let carry = self.engine_edit(|e| e.import_items_begin(clear_items, clear_skills));
        let skills_tab = b.skills_tab.borrow().clone();
        if clear_skills && let Some(t) = &skills_tab {
            t.import_cleared(g);
        }
        let display = skills_tab.as_ref().and_then(|t| t.display_index());
        self.engine_edit(|e| e.import_items_body(char_data, ignore_swap, display, carry));
        if let Some(t) = b.items_tab.borrow().clone() {
            t.after_import(g);
        }
        if let Some(t) = &skills_tab {
            t.after_import(g);
        }
        // :1485-1488 `SetText` calls no `changeFunc`: an auto level stays auto, the next run moves it back
        let level = char_data["level"].as_f64();
        let text = level.map_or("nil".to_string(), pob_lua::tostring_num);
        self.engine_edit(|e| {
            e.import_set_level(level.unwrap_or(1.0));
            Ok(())
        });
        b.c().character_level.set_text(g, &text, false);
        b.build_flag.set(true);
    }

    /// :735-740 `TryFetchCharacterList`.
    pub fn try_fetch_character_list(&self, g: &mut Gfx) {
        if main().accounts.borrow().last_realm.is_some() && !self.auto_fetch_attempted.get() {
            let c: ControlRef = self.account_realm_fetch_button.clone();
            if c.is_shown(g) && c.is_enabled(g) {
                self.account_realm_fetch_button.click(g);
            }
            self.auto_fetch_attempted.set(true);
        }
    }

    /// Build.lua:1182 `importTab:SelectControl(importTab.controls.importCodeIn)`.
    pub fn select_import_code(&self, g: &mut Gfx) {
        self.host.select_control(g, Some(self.import_code_in.clone()));
    }

    /// :776-787
    pub fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, input_events: &mut InputEvents) {
        let a = &self.anchor.base;
        a.x.set(view_port.x);
        a.y.set(view_port.y);
        a.width.set(view_port.width);
        a.height.set(view_port.height);
        self.host.process_controls_input(f, input_events, view_port);
        main().draw_background(f, view_port);
        self.host.draw_controls(f, view_port, None);
    }

    /// The state the gate compares (the same fields as `import-runner.lua`).
    pub fn debug_dump(&self, g: &Gfx) -> Json {
        let controls = dump_host(&self.host, g);
        json!({
            "controls": controls,
            "importCodeIn": self.import_code_in.buf.borrow().clone(),
            "importCodeDetail": self.import_code_detail.borrow().clone(),
            "importCodeValid": self.import_code_valid.get(),
            "importCodeSite": self.import_code_site.get(),
            "hasXml": self.import_code_xml.borrow().is_some(),
            "hasJson": self.import_code_json.borrow().is_some(),
            "generated": generated_xml(&self.generate_code_out.buf.borrow()),
            "mode": *self.charimport_mode.borrow(),
            "status": *self.charimport_status.borrow(),
            "history": self.site_account_history.list.borrow().iter().map(|v| v.label.clone().unwrap_or_default()).collect::<Vec<_>>(),
            "oauthErr": self.oauth_err_code.borrow().clone(),
            "oauthLoading": self.oauth_loading.get(),
            "fetching": self.import_code_fetching.get(),
            "charList": drop_rows(&self.char_select),
            "leagueList": drop_rows(&self.char_select_league),
            "siteCharList": drop_rows(&self.site_char_select),
            "siteLeagueList": drop_rows(&self.site_char_select_league),
        })
    }

    /// The scene's `importMode` step: the character stage of the tab (no request can reach it here).
    pub fn debug_set_mode(&self, mode: &str) {
        *self.charimport_mode.borrow_mut() = mode.to_string();
    }

    /// The field with a given text: the code of the original on the gate's PNG (the two zlib streams differ, the XML behind them is compared).
    pub fn debug_set_generated(&self, g: &mut Gfx, code: &str) {
        self.generate_code_out.set_text(g, code, false);
    }

    /// `generateCodeOut.buf`.
    pub fn generated_code(&self) -> String {
        self.generate_code_out.buf.borrow().clone()
    }
}

/// `label|detail` of a list, what the gate compares.
fn drop_rows(d: &DropDownControl) -> Vec<String> {
    d.list
        .borrow()
        .iter()
        .map(|v| format!("{}|{}", v.label.as_deref().unwrap_or(""), v.detail.as_deref().unwrap_or("")))
        .collect()
}

fn generated_xml(code: &str) -> Json {
    if code.is_empty() {
        return Json::Null;
    }
    match pob_build::decode_build_code(code) {
        Some(x) => json!(String::from_utf8_lossy(&x).replace("\r\n", "\n")),
        None => json!(code),
    }
}

/// The state of the real window (tools/sg-oracle/import-rust-input.ps1; the same fields as `RealState` of import-runner.lua).
pub fn debug_input_state(g: &Gfx) -> Json {
    let b = main().build_mode.clone();
    if main().mode.borrow().as_deref() != Some("BUILD") {
        return Json::Null;
    }
    let (Some(t), Some(p)) = (b.import_tab.borrow().clone(), b.party_tab.borrow().clone()) else {
        return Json::Null;
    };
    let pos = |c: &dyn Control| {
        let (x, y) = c.get_pos(g);
        let (w, h) = c.get_size(g);
        json!({ "x": (x + w / 2.0).floor(), "y": (y + h / 2.0).floor() })
    };
    let shown = |c: &dyn Control| if c.is_shown(g) { pos(c) } else { json!({ "x": 0, "y": 0 }) };
    json!({
        "viewMode": *b.view_mode.borrow(),
        "modeImport": pos(&*b.host.get("modeImport").expect("modeImport")),
        "modeParty": pos(&*b.host.get("modeParty").expect("modeParty")),
        "generate": shown(&*t.generate_code),
        "account": shown(&*t.site_account_name),
        "accountBuf": t.site_account_name.buf.borrow().clone(),
        "hasCode": !t.generate_code_out.buf.borrow().is_empty(),
        "advanced": shown(&*p.show_advance_tools),
        "advancedState": p.show_advance_tools.state.get(),
        "auras": shown(&*p.edit_auras),
        "aurasBuf": p.edit_auras.buf.borrow().clone(),
        "modFlag": p.mod_flag.get(),
    })
}
