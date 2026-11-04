//! `Classes/TradeHelpers.lua`: the lookups of the trade screens (`getTradeCategory`, `findTradeIdOption`, `findTradeHash`, `B64Gzip*`, `newPlainNumericEdit`), `HashStats` of `Modules/Common.lua`, and the data
//! files they read in place in the checkout (`Data/TradeSiteStats.lua`, `Data/StatDescriptions/stat_descriptions.lua`): no conversion into `data/*`, as `stat_describer.rs`.
//! The drawing helpers of the same file are in `tabs/compare/items.rs`.

use std::cell::RefCell;
use std::io::{Read, Write};
use std::rc::Rc;

use pob_gfx::Gfx;
use pob_items::Item;

use crate::control::{Anchor, RectProps};
use crate::controls::edit::{ChangeFunc, EditArgs, EditControl};
use crate::lua::{gsub, is_match, lmatch, lmatch_all};
use crate::lua_data::{LuaTab, LuaVal};

// ---------------------------------------------------------------------------------------------- Common.lua:332-347, :1129-1146

/// `murmurHash2(key, seed)` (Common.lua:349-385).
pub fn murmur_hash2(key: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1_e995;
    let mut h = seed ^ key.len() as u32;
    let (chunks, r) = key.as_chunks::<4>();
    for c in chunks {
        let mut k = u32::from_le_bytes(*c);
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
    }
    if r.len() == 3 {
        h ^= u32::from(r[2]) << 16;
    }
    if r.len() >= 2 {
        h ^= u32::from(r[1]) << 8;
    }
    if !r.is_empty() {
        h ^= u32::from(r[0]);
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

/// `HashStats(stats, extraStat)` (Common.lua:1135-1146).
pub fn hash_stats<S: AsRef<str>>(stats: &[S], extra: Option<&str>) -> u32 {
    let mut bytes = Vec::with_capacity(stats.len() * 4 + 4);
    for s in stats {
        bytes.extend_from_slice(&murmur_hash2(s.as_ref().as_bytes(), 0xC58F_1A7B).to_le_bytes());
    }
    if let Some(s) = extra {
        bytes.extend_from_slice(&murmur_hash2(s.as_bytes(), 0xC58F_1A7B).to_le_bytes());
    }
    murmur_hash2(&bytes, 0x0231_2233)
}

// ---------------------------------------------------------------------------------------------- Data/TradeSiteStats.lua

#[derive(Clone, Debug)]
pub struct StatOption {
    /// A number or a string in the file.
    pub id: serde_json::Value,
    /// Lowercase (TradeHelpers.lua:133-135).
    pub text: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StatEntry {
    pub id: String,
    pub text: String,
    pub options: Option<Vec<StatOption>>,
}

#[derive(Clone, Debug)]
pub struct StatCat {
    pub id: String,
    pub entries: Vec<StatEntry>,
}

fn opt_of(v: &LuaVal) -> Option<StatOption> {
    let t = v.as_tab()?;
    let id = match t.get("id") {
        Some(LuaVal::Num(n)) => crate::trade::dkjson::n(*n),
        Some(LuaVal::Str(s)) => serde_json::Value::String(s.to_string()),
        _ => serde_json::Value::Null,
    };
    Some(StatOption {
        id,
        text: t.str("text").map(str::to_lowercase),
    })
}

thread_local! {
    static STATS: RefCell<Option<Rc<Vec<StatCat>>>> = const { RefCell::new(None) };
    static OPTIONS: RefCell<Option<Rc<OptionMap>>> = const { RefCell::new(None) };
    static DESCS: RefCell<Option<Rc<Vec<StatDesc>>>> = const { RefCell::new(None) };
}

fn read_data_file(rel: &str) -> LuaVal {
    let root = crate::launch::launch().cfg.script_path.clone();
    crate::src_cache::read_lua(&root, rel).unwrap_or_else(|e| panic!("{e}"))
}

/// `tradeHelpers.getTradeStats()` (:102-104).
pub fn trade_stats() -> Rc<Vec<StatCat>> {
    if let Some(s) = STATS.with(|s| s.borrow().clone()) {
        return s;
    }
    let v = read_data_file("Data/TradeSiteStats.lua");
    let t = v.as_tab().expect("TradeSiteStats");
    let mut out = Vec::new();
    for cat in t.seq() {
        let cat = cat.as_tab().expect("category");
        let mut entries = Vec::new();
        for e in cat.tab("entries").map(|t| t.seq()).unwrap_or_default() {
            let e = e.as_tab().expect("entry");
            let options = e
                .tab("option")
                .and_then(|o| o.tab("options"))
                .map(|o| o.seq().iter().filter_map(|x| opt_of(x)).collect::<Vec<_>>());
            entries.push(StatEntry {
                id: e.str("id").unwrap_or("").to_string(),
                text: e.str("text").unwrap_or("").to_string(),
                options,
            });
        }
        out.push(StatCat {
            id: cat.str("id").unwrap_or("").to_string(),
            entries,
        });
    }
    let s = Rc::new(out);
    STATS.with(|c| *c.borrow_mut() = Some(s.clone()));
    s
}

struct OptionPattern {
    pat: String,
    ty: String,
    options: Vec<StatOption>,
    trade_id: String,
}

#[derive(Default)]
struct OptionMap {
    exact: std::collections::HashMap<String, (String, serde_json::Value)>,
    patterns: Vec<OptionPattern>,
}

/// `getOptionTradeStatMap` (:110-144). `pairs(optionTradeStatMap.patterns)` (R13): the file's order; `self.patterns[matchKey] =` keeps the last one of a key at the place of the first.
fn option_map() -> Rc<OptionMap> {
    if let Some(m) = OPTIONS.with(|s| s.borrow().clone()) {
        return m;
    }
    let mut m = OptionMap::default();
    for cat in trade_stats().iter() {
        if cat.id != "enchant" && cat.id != "explicit" && cat.id != "implicit" {
            continue;
        }
        for e in &cat.entries {
            if let Some(c) = lmatch_all(&e.id, "^(.-)|(.+)$") {
                let key = gsub(&e.text, " Passage", "").to_lowercase();
                let value = match pob_lua::tonumber(&c[1]) {
                    Some(n) => crate::trade::dkjson::n(n),
                    None => serde_json::Value::String(c[1].clone()),
                };
                m.exact.insert(format!("{}\0{key}", cat.id), (c[0].clone(), value));
            } else if let Some(options) = &e.options
                && e.text.contains('#')
            {
                let pat = gsub(&gsub(&e.text, "#", "(.*)"), " Passage", "").to_lowercase();
                let item = OptionPattern {
                    pat: pat.clone(),
                    ty: cat.id.clone(),
                    options: options.clone(),
                    trade_id: e.id.clone(),
                };
                match m.patterns.iter_mut().find(|p| p.pat == pat) {
                    Some(p) => *p = item,
                    None => m.patterns.push(item),
                }
            }
        }
    }
    let m = Rc::new(m);
    OPTIONS.with(|c| *c.borrow_mut() = Some(m.clone()));
    m
}

/// `M.findTradeIdOption(modLine, modType)` (:183-222).
pub fn find_trade_id_option(mod_line: &str, mod_type: &str) -> Option<(String, Option<serde_json::Value>)> {
    let line = mod_line.replace('\n', " ").to_lowercase();
    const TIMELESS: [&str; 8] = [
        "bathed in the blood of (%d+) sacrificed in the name of (.+)",
        "carved to glorify (%d+) new faithful converted by high templar (.+)",
        "commanded leadership over (%d+) warriors under (.+)",
        "commissioned (%d+) coins to commemorate (.+)",
        "denoted service of (%d+) dekhara in the akhara of (.+)",
        "remembrancing (%d+) songworthy deeds by the line of (.+)",
        "subjugating (%d+) souls in the thrall of (.+)",
        "binding (%d+) souls to phylacteries to sustain (.+)",
    ];
    for pat in TIMELESS {
        if let Some(c) = lmatch_all(&line, pat) {
            return Some((
                format!("explicit.pseudo_timeless_jewel_{}", c[1]),
                pob_lua::tonumber(&c[0]).map(crate::trade::dkjson::n),
            ));
        }
    }
    let m = option_map();
    let line = gsub(&line, ".added small passive skills grant: ", " ");
    if let Some((id, v)) = m.exact.get(&format!("{mod_type}\0{line}")) {
        return Some((id.clone(), Some(v.clone())));
    }
    for e in &m.patterns {
        if e.ty == mod_type
            && let Some(found) = lmatch(&line, &e.pat)
        {
            for o in &e.options {
                if o.text.as_deref() == Some(found.as_str()) {
                    return Some((e.trade_id.clone(), Some(o.id.clone())));
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------- stat_descriptions

struct StatForm {
    /// `statForm.text:lower()`.
    lower: String,
    pat: String,
    group_indexes: Vec<f64>,
    negate: bool,
    /// `limit[n][1]`, by position.
    limit: Vec<Option<f64>>,
    /// Every `canonical_stat` flag in the order of the file: the last one of the entry wins.
    canonical_stat: Option<f64>,
    /// What `negate` was at the last `canonical_line` flag of the form (:282-285).
    canonical_line_negate: Option<bool>,
}

struct StatDesc {
    stats: Vec<String>,
    forms: Vec<StatForm>,
}

/// `numberPattern` (:10) as the text it leaves in a pattern.
const NUM: &str = "%d+%.?%d*";

/// `string:gsub("{.-}", ...)` by hand, with no pattern of the replacement to escape.
fn replace_groups(text: &str, with: &str) -> String {
    gsub(text, "{.-}", &with.replace('%', "%%"))
}

fn form_of(t: &LuaTab) -> StatForm {
    let raw_text = t.str("text").unwrap_or("");
    // :26-39
    let mut group_indexes = Vec::new();
    let mut left = 1.0;
    for g in pob_items::lua::gmatch(raw_text, "{.-}") {
        let g = g.first().map_or("", String::as_str);
        match lmatch(g, "{(%d+):?.*}") {
            Some(i) => group_indexes.push(pob_lua::tonumber(&i).unwrap_or(0.0) + 1.0),
            None => group_indexes.push(left),
        }
        left += 1.0;
    }
    let text = raw_text.replace("\nPassage", "");
    // :42-60
    let p = text.to_lowercase();
    let p = gsub(&p, "%-{", "{");
    let p = gsub(&p, "%+{", "{");
    let p = gsub(&p, "([%(%)%.%%%+%-%*%?%[%]%^%$])", "%%%1");
    let p = gsub(&p, "{.-} to {.-}", &format!("({NUM} to {NUM})").replace('%', "%%"));
    let p = replace_groups(&p, &format!("%+?(%-?{NUM})"));
    let pat = format!("^{p}$");
    let mut negate = false;
    let mut canonical_stat = None;
    let mut canonical_line_negate = None;
    for flag in t.seq() {
        let Some(flag) = flag.as_tab() else { continue };
        let k = flag.str("k").unwrap_or("");
        let v = flag.get("v");
        if (k == "negate" || k == "negate_and_double") && matches!(v, Some(LuaVal::Num(n)) if *n == 1.0) {
            negate = true;
        }
        if k == "canonical_stat" {
            canonical_stat = v.and_then(LuaVal::as_num);
        }
        if k == "canonical_line" && !matches!(v, None | Some(LuaVal::Nil | LuaVal::Bool(false))) {
            canonical_line_negate = Some(negate);
        }
    }
    let limit = t.tab("limit").map_or_else(Vec::new, |l| {
        l.seq()
            .iter()
            .map(|p| {
                p.as_tab().and_then(|p| match p.get_num(1.0) {
                    Some(LuaVal::Num(n)) => Some(*n),
                    Some(LuaVal::Str(s)) => pob_lua::tonumber(s),
                    _ => None,
                })
            })
            .collect()
    });
    StatForm {
        lower: text.to_lowercase(),
        pat,
        group_indexes,
        negate,
        limit,
        canonical_stat,
        canonical_line_negate,
    }
}

/// `getStatDescData` (:12-65): the entries in the file's order. `statDescEntry` numbers in `ipairs`; the file has `[n]=`.
fn stat_desc_data() -> Rc<Vec<StatDesc>> {
    if let Some(s) = DESCS.with(|s| s.borrow().clone()) {
        return s;
    }
    let v = read_data_file("Data/StatDescriptions/stat_descriptions.lua");
    let t = v.as_tab().expect("stat_descriptions");
    let mut out = Vec::new();
    for e in t.seq() {
        let Some(e) = e.as_tab() else { continue };
        let stats = e
            .tab("stats")
            .map(|s| s.seq().iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let forms = match e.get_num(1.0).and_then(LuaVal::as_tab) {
            Some(f) => f.seq().iter().filter_map(|x| x.as_tab()).map(|x| form_of(x)).collect(),
            None => Vec::new(),
        };
        out.push(StatDesc { stats, forms });
    }
    let s = Rc::new(out);
    DESCS.with(|c| *c.borrow_mut() = Some(s.clone()));
    s
}

/// `M.modLineValue(line, onlyFromTo)` (:91-99).
pub fn mod_line_value(line: &str) -> Option<f64> {
    if let Some(c) = lmatch_all(line, "(%-?%d+%.?%d*) to (%-?%d+%.?%d*)") {
        return Some((pob_lua::tonumber(&c[0])? + pob_lua::tonumber(&c[1])?) / 2.0);
    }
    pob_lua::tonumber(&lmatch(line, "%-?[%d]+%.?[%d]*")?)
}

/// What `findTradeHash` returns.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TradeHash {
    pub ids: Vec<u32>,
    pub value: Option<f64>,
    pub should_negate: Option<bool>,
}

/// `M.findTradeHash(modLine)` (:244-344).
pub fn find_trade_hash(mod_line: &str) -> TradeHash {
    let mut line = mod_line.to_lowercase();
    let mut out = TradeHash::default();
    let mut extra: Option<&str> = None;
    // `pairs(timeLostJewelLines)`: the two patterns exclude each other
    for (pat, stat) in [
        (
            "^notable passive skills in radius also grant ",
            "local_jewel_mod_stats_added_to_notable_passives",
        ),
        (
            "^small passive skills in radius also grant ",
            "local_jewel_mod_stats_added_to_small_passives",
        ),
    ] {
        if is_match(&line, pat) {
            line = gsub(&line, pat, "");
            extra = Some(stat);
            break;
        }
    }
    let insert = |ids: &mut Vec<u32>, h: u32| {
        if !ids.contains(&h) {
            ids.push(h);
        }
    };
    for entry in stat_desc_data().iter() {
        if entry.forms.is_empty() {
            continue;
        }
        // :271-287: each form's flags are walked in order; a form without a flag does not reach the `canonical_negated` line
        let mut canonical_stat = None;
        let mut canonical_negated = false;
        for (i, form) in entry.forms.iter().enumerate() {
            if let Some(c) = form.canonical_stat {
                canonical_stat = Some(c);
            }
            if i == 0 {
                canonical_negated = form.negate;
            } else if let Some(n) = form.canonical_line_negate {
                canonical_negated = n;
            }
        }
        let implied = |form: &StatForm| -> Option<f64> { form.limit.get(canonical_stat.unwrap_or(1.0) as usize - 1).copied().flatten() };
        for form in &entry.forms {
            if line == form.lower {
                insert(&mut out.ids, hash_stats(&entry.stats, extra));
                out.should_negate = Some(false);
                out.value = implied(form);
                break;
            }
            if let Some(matches) = lmatch_all(&line, &form.pat) {
                let mut matched_canonical = false;
                for (idx, m) in matches.into_iter().enumerate() {
                    let number = pob_lua::tonumber(&m).or_else(|| mod_line_value(&m));
                    // `number and (not canonical_stat) or (groupIndexes[idx] == canonical_stat)`
                    let hit = (number.is_some() && canonical_stat.is_none()) || form.group_indexes.get(idx).copied() == canonical_stat;
                    if hit {
                        out.should_negate = Some(form.negate != canonical_negated);
                        insert(&mut out.ids, hash_stats(&entry.stats, extra));
                        out.value = number;
                        matched_canonical = true;
                        break;
                    }
                }
                if canonical_stat.is_some()
                    && !matched_canonical
                    && let Some(v) = implied(form)
                {
                    out.should_negate = Some(form.negate != canonical_negated);
                    insert(&mut out.ids, hash_stats(&entry.stats, extra));
                    out.value = Some(v);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------- categories

/// `M.getTradeCategory(slotName, item)` (:351-387).
pub fn get_trade_category(slot_name: &str, item: Option<&Item>) -> (Option<&'static str>, Option<&'static str>) {
    let ty = item.and_then(|i| i.ty.clone().or_else(|| i.base.map(|b| b.ty.clone())));
    let some = |a, b| (Some(a), Some(b));
    if is_match(slot_name, "^Weapon %d") {
        let Some(t) = ty else { return some("weapon.one", "1HWeapon") };
        return match t.as_str() {
            "Shield" => some("armour.shield", "Shield"),
            "Quiver" => some("armour.quiver", "Quiver"),
            "Bow" => some("weapon.bow", "Bow"),
            "Staff" => some("weapon.staff", "Staff"),
            "Two Handed Sword" => some("weapon.twosword", "2HSword"),
            "Two Handed Axe" => some("weapon.twoaxe", "2HAxe"),
            "Two Handed Mace" => some("weapon.twomace", "2HMace"),
            "Fishing Rod" => some("weapon.rod", "FishingRod"),
            "One Handed Sword" => some("weapon.onesword", "1HSword"),
            "One Handed Axe" => some("weapon.oneaxe", "1HAxe"),
            "One Handed Mace" => some("weapon.onemace", "1HMace"),
            "Sceptre" => some("weapon.sceptre", "Sceptre"),
            "Wand" => some("weapon.wand", "Wand"),
            "Dagger" => some("weapon.dagger", "Dagger"),
            "Claw" => some("weapon.claw", "Claw"),
            x if x.contains("Two Handed") => some("weapon.twomelee", "2HWeapon"),
            x if x.contains("One Handed") => some("weapon.one", "1HWeapon"),
            _ => some("weapon", "1HWeapon"),
        };
    }
    match slot_name {
        "Body Armour" => some("armour.chest", "Chest"),
        "Helmet" => some("armour.helmet", "Helmet"),
        "Gloves" => some("armour.gloves", "Gloves"),
        "Boots" => some("armour.boots", "Boots"),
        "Amulet" => some("accessory.amulet", "Amulet"),
        "Ring 1" | "Ring 2" | "Ring 3" => some("accessory.ring", "Ring"),
        "Belt" => some("accessory.belt", "Belt"),
        s if s.contains("Abyssal") => some("jewel.abyss", "AbyssJewel"),
        s if s.contains("Jewel") => some("jewel", "Jewel"),
        s if s.contains("Flask") => some("flask", "Flask"),
        _ => (None, None),
    }
}

/// `M.getTradeCategoryLabel(slotName, item)` (:392-396).
pub fn get_trade_category_label(item: &Item) -> String {
    match item.base {
        None => "Item".to_string(),
        Some(b) => b.ty.clone(),
    }
}

// ---------------------------------------------------------------------------------------------- controls, base64

/// `M.newPlainNumericEdit(anchor, rect, init, prompt, limit, integer, changeFunc)` (:612-622).
pub fn new_plain_numeric_edit(
    g: &Gfx,
    anchor: Option<Anchor>,
    rect: RectProps,
    init: &str,
    prompt: &str,
    limit: usize,
    integer: bool,
    change_func: Option<ChangeFunc>,
) -> Rc<EditControl> {
    let ctrl = EditControl::new(
        g,
        anchor,
        rect,
        EditArgs {
            init: Some(init.to_string()),
            prompt: Some(prompt.to_string()),
            filter: Some(if integer { "%D" } else { "^%d." }.to_string()),
            limit: Some(limit),
            change_func,
            ..EditArgs::default()
        },
    );
    ctrl.is_numeric.set(false);
    if let Some(b) = &ctrl.button_down {
        b.base.shown.set(false);
    }
    if let Some(b) = &ctrl.button_up {
        b.base.shown.set(false);
    }
    ctrl
}

/// `M.B64GzipEncode(str)` (:627-632): `Deflate(str, true)` makes a gzip stream.
pub fn b64_gzip_encode(s: &str) -> String {
    let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(s.as_bytes()).expect("gzip into a Vec");
    let data = z.finish().expect("gzip into a Vec");
    pob_net::base64_encode(&data).replace('+', "-").replace('/', "_")
}

/// `M.B64GzipDecode(str)` (:636-641): `Inflate` reads gzip and zlib alike.
pub fn b64_gzip_decode(s: &str) -> Option<String> {
    let data = pob_net::base64_decode(&s.replace('-', "+").replace('_', "/"));
    let mut out = Vec::new();
    if flate2::read::GzDecoder::new(&data[..]).read_to_end(&mut out).is_ok() {
        return Some(String::from_utf8_lossy(&out).into_owned());
    }
    out.clear();
    flate2::read::ZlibDecoder::new(&data[..]).read_to_end(&mut out).ok()?;
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// `urlDecode(str)` (Common.lua:1031-1036).
pub fn url_decode(s: &str) -> String {
    let s = s.replace('+', " ");
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `league:gsub("[^%w%-%.%_%~]", "%%%02X")` (TradeQueryRequests.lua:509-511, CompareBuySimilar.lua:196-198).
pub fn encode_league(league: &str) -> String {
    let mut out = String::new();
    for &c in league.as_bytes() {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_' | b'~') {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn murmur_is_the_one_of_common_lua() {
        assert_eq!(murmur_hash2(b"", 0), 0);
        // the id of "+# to maximum Life" in Data/TradeSiteStats.lua
        assert_eq!(hash_stats(&["base_maximum_life"], None), 3_299_347_043);
    }

    #[test]
    fn base64_gzip_goes_both_ways() {
        let s = "{\"a\":\"caf\u{e9}\"}";
        let e = b64_gzip_encode(s);
        assert!(!e.contains('+') && !e.contains('/'));
        assert_eq!(b64_gzip_decode(&e).as_deref(), Some(s));
        assert_eq!(b64_gzip_decode("***"), None);
    }

    #[test]
    fn league_and_url() {
        assert_eq!(encode_league("Hardcore Ruthless"), "Hardcore%20Ruthless");
        assert_eq!(url_decode("Hardcore%20Ruthless"), "Hardcore Ruthless");
        assert_eq!(url_decode("a+b%2"), "a b%2");
    }

    #[test]
    fn categories() {
        assert_eq!(get_trade_category("Ring 2", None), (Some("accessory.ring"), Some("Ring")));
        assert_eq!(get_trade_category("Weapon 1", None), (Some("weapon.one"), Some("1HWeapon")));
        assert_eq!(get_trade_category("Abyssal #1", None), (Some("jewel.abyss"), Some("AbyssJewel")));
        assert_eq!(get_trade_category("Jewel 26725", None), (Some("jewel"), Some("Jewel")));
        assert_eq!(get_trade_category("Nothing", None), (None, None));
    }
}
