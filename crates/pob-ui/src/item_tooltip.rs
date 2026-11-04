//! `ItemsTab:AddItemTooltip` (ItemsTab.lua:4274-4719), `AddItemStatDifferences` (:4724-5079), `SetTooltipHeaderInfluence`, `FormatItemSource`, `GetSocketDescriptionLine`, `itemLib.formatModLine` (ItemTools.lua:431-452).
//! No control state here: the slots to compare with are the caller's (`CompareSlot`), everything else is the engine's.

use std::collections::HashMap;
use std::rc::Rc;

use pob_calc::env::CalcOverride;
use pob_engine::engine::Engine;
use pob_engine::lua::lua_sort;
use pob_gfx::Gfx;
use pob_items::Item;
use pob_items::item::{INFLUENCE_INFO, ModLine};
use pob_lua::format::{Arg, format};
use pob_mods::ModType;

use crate::colors::color_code;
use crate::common::{not_supported_mod_tooltips, show_flavour_text};
use crate::modules::main::{Main, main};
use crate::sg;
use crate::tooltip::{Tooltip, TooltipColor};

fn cc(name: &str) -> String {
    color_code(name).unwrap_or_default()
}

/// `s_format` with numbers and strings.
fn fmt(f: &str, args: &[Arg<'_>]) -> String {
    format(f, args).unwrap_or_else(|e| panic!("{e}"))
}

/// A slot control as the tooltip needs it.
#[derive(Clone)]
pub struct CompareSlot {
    pub slot_name: String,
    pub label: String,
    pub sel_item_id: f64,
    pub node_id: Option<u32>,
    pub slot_num: Option<f64>,
}

/// What the caller knows of the tab: `slot` of `AddItemTooltip` and the slots for `AddItemStatDifferences`.
pub struct ItemTooltipArgs<'a> {
    pub slot: Option<&'a CompareSlot>,
    pub db_mode: bool,
    pub max_width: Option<f64>,
    /// `self.slots` passing the test of :4975, in `orderedSlots` order; filled by the tab per item type.
    pub compare_slots: Vec<CompareSlot>,
    /// `self.build.viewMode == "ITEMS"`.
    pub in_items_tab: bool,
    /// `slotName:find("^Flask") ~= nil and slot.selItemId == 0` of `pairs(self.slots)`.
    pub empty_flask_slots: f64,
}

/// `itemLib.formatModLine(modLine, dbMode)`.
pub fn format_mod_line(ml: &ModLine, db_mode: bool) -> Option<String> {
    let apply = !db_mode && (ml.range.is_some() || ml.corrupted_range.is_some());
    let line = if apply {
        pob_items::apply_range::apply_range(
            &ml.line,
            ml.range.unwrap_or_else(|| main().s.borrow().default_item_affix_quality),
            ml.value_scalar,
            ml.corrupted_range,
            pob_calc::ParseMode::Full,
        )
    } else {
        ml.line.clone()
    };
    let m = |p: &str| pob_lua::find(line.as_bytes(), p.as_bytes(), None, false).ok().flatten().is_some();
    if m("^%+?0%%? ") || (m(" %+?0%%? ") && !m("0 to [1-9]")) || m(" 0%-0 ") || m(" 0 to 0 ") {
        return None;
    }
    if ml.flag("disabled") {
        return Some(format!("{}{line}", cc("DISABLED")));
    }
    if ml.extra.is_some() {
        let tail = if not_supported_mod_tooltips() {
            " ^8(Not supported in PoB yet)"
        } else {
            ""
        };
        // ItemTools.lua:445-447
        let dev = if crate::launch::launch().dev_mode_alt.get() {
            format!("   ^1'{}'", ml.extra.as_deref().unwrap_or(""))
        } else {
            String::new()
        };
        return Some(format!("{}{line}{tail}{dev}", cc("UNSUPPORTED")));
    }
    let code = [
        ("fractured", "FRACTURED"),
        ("crafted", "CRAFTED"),
        ("mutated", "MUTATED"),
        ("scourge", "SCOURGE"),
        ("custom", "CUSTOM"),
        ("crucible", "CRUCIBLE"),
        ("vestigial", "VESTIGIAL"),
    ]
    .iter()
    .find(|(k, _)| ml.flag(k))
    .map_or("MAGIC", |(_, c)| c);
    Some(format!("{}{line}", cc(code)))
}

/// `ItemsTab:GetSocketDescriptionLine` (:4245-4273).
pub fn socket_description_line(item: &Item) -> String {
    let mut group = 0.0;
    let mut line = String::new();
    for (i, s) in item.sockets.iter().enumerate() {
        if i > 0 {
            line.push_str(if s.group == group { "^7=" } else { "  " });
            group = s.group;
        }
        let code = match s.color.as_str() {
            "R" => cc("STRENGTH"),
            "G" => cc("DEXTERITY"),
            "B" => cc("INTELLIGENCE"),
            "W" => cc("SCION"),
            "A" => "^xB0B0B0".to_string(),
            _ => panic!("Classes/ItemsTab.lua:4270: attempt to concatenate local 'code' (a nil value)"),
        };
        line.push_str(&code);
        line.push_str(&s.color);
    }
    line
}

/// `ItemsTab:SetTooltipHeaderInfluence` (:4053-4122).
fn set_header_influence(t: &mut Tooltip, item: &Item) {
    let mut names: Vec<&str> = Vec::new();
    let has = |k: &str| item.influence(k);
    if has("cleansing") && has("tangle") {
        names.extend(["Exarch", "Eater"]);
    } else {
        if item.fractured {
            names.push("Fractured");
        }
        if item.title.as_deref().is_some_and(|x| x.contains("Replica")) {
            names.push("Experimented");
        }
        if item.foulborn {
            names.push("Foulborn");
        }
        if item.veiled {
            names.push("Veiled");
        }
        for (k, n) in [
            ("cleansing", "Exarch"),
            ("tangle", "Eater"),
            ("shaper", "Shaper"),
            ("elder", "Elder"),
            ("crusader", "Crusader"),
            ("eyrie", "Redeemer"),
            ("basilisk", "Hunter"),
            ("adjudicator", "Warlord"),
        ] {
            if has(k) {
                names.push(n);
            }
        }
        if item.vestigial {
            names.push("Vestigial");
        }
        if item.synthesised && names.is_empty() {
            names.push("Synthesis");
        }
        if item.memory_strands.is_some() && names.is_empty() {
            names.push("Memory");
        }
    }
    let first = names.first().map(|s| (*s).to_string());
    let second = names.get(1).map(|s| (*s).to_string()).or_else(|| first.clone());
    t.influence_header = [first, second];
}

/// `ItemsTab:FormatItemSource` (:4124-4129).
fn format_item_source(text: &str) -> String {
    let src = cc("SOURCE");
    let mut s = text.to_string();
    for (key, code) in [
        ("unique", "UNIQUE"),
        ("normal", "NORMAL"),
        ("currency", "CURRENCY"),
        ("prophecy", "PROPHECY"),
    ] {
        let mut out = String::new();
        let mut rest = s.as_str();
        let open = format!("{key}{{");
        while let Some(a) = rest.find(&open) {
            let inner = &rest[a + open.len()..];
            let Some(b) = inner.find('}').filter(|b| *b > 0) else { break };
            out.push_str(&rest[..a]);
            out.push_str(&cc(code));
            out.push_str(&inner[..b]);
            out.push_str(&src);
            rest = &inner[b + 1..];
        }
        out.push_str(rest);
        s = out;
    }
    s
}

thread_local! {
    /// `flavourLookup` (:61-68): `name` -> `id` -> text, read from `Data/FlavourText.lua`: `data/` has no such table.
    static FLAVOUR: std::cell::OnceCell<HashMap<String, Vec<(String, Vec<String>)>>> = const { std::cell::OnceCell::new() };
}

fn flavour_table(name: &str) -> Option<Vec<(String, Vec<String>)>> {
    FLAVOUR.with(|f| {
        f.get_or_init(|| {
            let root = crate::launch::launch().cfg.script_path.clone();
            let text = std::fs::read_to_string(format!("{root}Data/FlavourText.lua")).unwrap_or_default();
            let mut out: HashMap<String, Vec<(String, Vec<String>)>> = HashMap::new();
            let (mut id, mut nm, mut lines) = (None, None, Vec::new());
            let quoted = |s: &str| {
                let a = s.find('"')?;
                let b = s.rfind('"')?;
                (b > a).then(|| s[a + 1..b].replace("\\\"", "\"").replace("\\\\", "\\"))
            };
            let mut in_text = false;
            for l in text.lines() {
                let t = l.trim();
                if let Some(v) = t.strip_prefix("id = ") {
                    id = quoted(v);
                } else if let Some(v) = t.strip_prefix("name = ") {
                    nm = quoted(v);
                } else if t.starts_with("text = {") {
                    in_text = true;
                    lines = Vec::new();
                } else if in_text {
                    if t.starts_with('}') {
                        in_text = false;
                        if let (Some(n), Some(i)) = (nm.take(), id.take()) {
                            out.entry(n).or_default().push((i, std::mem::take(&mut lines)));
                        }
                    } else if let Some(q) = quoted(t) {
                        lines.push(q);
                    }
                }
            }
            out
        })
        .get(name)
        .cloned()
    })
}

/// `self.build:AddRequirementsToTooltip` (Build.lua:2185-2221).
fn add_requirements(t: &mut Tooltip, g: &Gfx, e: &Engine, item: &Item) {
    let r = &item.requirements;
    let env = &e.result().env;
    let o = |k: &str| e.output_value(k).unwrap_or(0.0);
    let mut req: Vec<String> = Vec::new();
    let level = r.level;
    if let Some(l) = level.filter(|l| *l > 0.0) {
        req.push(fmt(
            "^x7F7F7FLevel %s%d",
            &[Arg::Str(&Main::stat_color(l, None, Some(e.model().level))), Arg::Num(l)],
        ));
    }
    let db = env.mod_db.borrow();
    if db.flag(None, &["OmniscienceRequirements"]).ok().flatten() == Some(true) {
        let omni_satisfy = db.sum(ModType::Inc, None, &["OmniAttributeRequirements"]).unwrap_or(0.0);
        let mut highest = 0.0f64;
        for stat in [r.str_mod, r.dex_mod, r.int_mod] {
            if stat.unwrap_or(0.0) > highest {
                highest = stat.unwrap_or(0.0);
            }
        }
        let omni = (highest * (100.0 / omni_satisfy)).floor();
        if omni > 0.0 || omni > o("Omni") {
            req.push(fmt(
                "%s%d ^x7F7F7FOmni",
                &[Arg::Str(&Main::stat_color(omni, Some(0.0), Some(o("Omni")))), Arg::Num(omni)],
            ));
        }
    } else {
        for (stat, base, key) in [(r.str_mod, r.str, "Str"), (r.dex_mod, r.dex, "Dex"), (r.int_mod, r.int, "Int")] {
            if let Some(s) = stat
                && (s > 14.0 || s > o(key))
            {
                req.push(fmt(
                    &format!("%s%d ^x7F7F7F{key}"),
                    &[Arg::Str(&Main::stat_color(s, Some(base.unwrap_or(0.0)), Some(o(key)))), Arg::Num(s)],
                ));
            }
        }
    }
    if !req.is_empty() {
        let big = if show_flavour_text() { 18.0 } else { 16.0 };
        t.add_line_ex(
            g,
            big,
            Some(&format!("^x7F7F7FRequires {}", req.join("^x7F7F7F, "))),
            Some("FONTIN SC"),
            false,
            None,
        );
        t.add_separator(Some(10.0));
    }
}

/// `ItemsTab:AddItemTooltip(tooltip, item, slot, dbMode, maxWidth)`.
#[allow(clippy::too_many_lines)]
pub fn add_item_tooltip(t: &mut Tooltip, g: &Gfx, e: &Engine, item: &Rc<Item>, a: &ItemTooltipArgs<'_>) {
    let flavour = show_flavour_text();
    let (small, big, title_size) = if flavour { (16.0, 18.0, 22.0) } else { (14.0, 16.0, 20.0) };
    let _ = small;
    let rarity_code = cc(&item.rarity);
    t.max_width = Some(a.max_width.unwrap_or(600.0).min(600.0));
    t.tooltip_header = Some(item.rarity.clone());
    t.foil_type.clone_from(&item.foil_type);
    t.center = true;
    t.color = TooltipColor::Escape(rarity_code.clone());
    let sc: Option<&'static str> = Some("FONTIN SC");
    let (Some(base), Some(base_name)) = (item.base, item.base_name.as_deref()) else {
        let title = item.title.clone().unwrap_or_else(|| {
            if item.name.is_empty() {
                "Unknown Item".to_string()
            } else {
                item.name.clone()
            }
        });
        t.add_line_ex(g, title_size, Some(&format!("{rarity_code}{title}")), sc, false, None);
        t.add_separator(Some(30.0));
        t.add_line_ex(
            g,
            title_size,
            Some(&format!("{}Item base is not supported by the current version.", cc("NEGATIVE"))),
            sc,
            false,
            None,
        );
        return;
    };
    set_header_influence(t, item);
    let plain_base = pob_items::lua::gsub(base_name, " %(.+%)", "", None);
    if let Some(title) = &item.title {
        t.add_line_ex(g, title_size, Some(&format!("{rarity_code}{title}")), sc, false, None);
        t.add_line_ex(g, title_size, Some(&format!("{rarity_code}{plain_base}")), sc, false, None);
    } else {
        t.add_line_ex(
            g,
            title_size,
            Some(&format!("{rarity_code}{}{plain_base}{}", item.name_prefix, item.name_suffix)),
            sc,
            false,
            None,
        );
    }
    if !flavour {
        for (i, (key, display)) in INFLUENCE_INFO.iter().enumerate() {
            let _ = i;
            if item.influence(key) {
                let colour = influence_color(key);
                t.add_line_ex(g, big, Some(&format!("{colour}{display} Item")), sc, false, None);
            }
        }
        if item.fractured {
            t.add_line_ex(g, big, Some(&format!("{}Fractured Item", cc("FRACTURED"))), None, false, None);
        }
        if item.synthesised {
            t.add_line_ex(g, big, Some(&format!("{}Synthesised Item", cc("CRAFTED"))), None, false, None);
        }
    }
    t.add_separator(Some(10.0));

    if a.db_mode {
        let yellow = "^xFFFF30";
        if item.uses_variant_groups {
            if let (Some(v), Some(s)) = (&item.version_list, item.selected_version) {
                t.add_line_ex(g, big, Some(&format!("{yellow}Version: {}", v[s as usize - 1])), sc, false, None);
            }
            let mut selected = Vec::new();
            for gid in item.variant_groups.keys() {
                if let Some(&vid) = item.variant_group_selections.get(gid)
                    && item.is_variant_group_option_eligible(gid.0, vid)
                {
                    selected.push(item.variant_list.as_ref().expect("variantList")[vid as usize - 1].clone());
                }
            }
            if !selected.is_empty() {
                t.add_line_ex(g, big, Some(&format!("{yellow}Variants: {}", selected.join(", "))), sc, false, None);
            }
        } else if let Some(vl) = &item.variant_list {
            if vl.len() == 1 {
                t.add_line_ex(g, big, Some(&format!("{yellow}Variant: {}", vl[0])), sc, false, None);
            } else {
                let v = item.variant.expect("variant");
                t.add_line_ex(
                    g,
                    big,
                    Some(&format!("{yellow}Variant: {} ({} variants)", vl[v as usize - 1], vl.len())),
                    sc,
                    false,
                    None,
                );
            }
        }
        if let Some(l) = &item.league {
            t.add_line_ex(g, big, Some(&format!("^xFF5555Exclusive to: {l}")), sc, false, None);
        }
        if item.unreleased == Some(true) {
            t.add_line_ex(g, big, Some(&format!("{}Not yet available", cc("NEGATIVE"))), sc, false, None);
        }
        if let Some(s) = &item.source {
            t.add_line_ex(
                g,
                big,
                Some(&format!("{}Source: {}", cc("SOURCE"), format_item_source(s))),
                sc,
                false,
                None,
            );
        }
        for path in item.upgrade_paths.iter().flatten() {
            t.add_line_ex(
                g,
                big,
                Some(&format!("{}{}", cc("SOURCE"), format_item_source(path))),
                sc,
                false,
                None,
            );
        }
        t.add_separator(Some(10.0));
    }

    let slot_num = a
        .slot
        .and_then(|s| s.slot_num)
        .unwrap_or(if sg::is_key_down(g, "SHIFT") { 2.0 } else { 1.0 });
    let quality = item.quality.unwrap_or(0.0);
    let magic = cc("MAGIC");
    let grey = "^x7F7F7F";
    let quality_line = |t: &mut Tooltip| {
        t.add_line_ex(
            g,
            big,
            Some(&fmt(&format!("{grey}Quality: {magic}+%d%%"), &[Arg::Num(quality)])),
            sc,
            false,
            None,
        );
    };
    let color = |v: f64, b: Option<f64>| Main::stat_color(v, b, None);
    if let Some(bw) = &base.weapon {
        let wd = item
            .weapon_data
            .as_ref()
            .and_then(|m| m.get(&pob_items::item::NumKey(slot_num)))
            .expect("weaponData");
        let wti = pob_data::rules::rules().weapon_type_info.get(&base.ty);
        let label = wti
            .and_then(|i| i.label.clone())
            .or_else(|| base.sub_type.clone())
            .unwrap_or_else(|| base.ty.clone());
        t.add_line_ex(g, big, Some(&format!("{grey}{label}")), sc, false, None);
        if quality > 0.0 {
            quality_line(t);
        }
        let mut total_damage_types = 0;
        let n = |k: &str| wd.num(k).unwrap_or(0.0);
        if wd.num("PhysicalDPS").is_some() {
            t.add_line_ex(
                g,
                big,
                Some(&fmt(
                    &format!("{grey}Physical Damage: {magic}%d-%d (%.1f DPS)"),
                    &[Arg::Num(n("PhysicalMin")), Arg::Num(n("PhysicalMax")), Arg::Num(n("PhysicalDPS"))],
                )),
                sc,
                false,
                None,
            );
            total_damage_types += 1;
        }
        if wd.num("ElementalDPS").is_some() {
            let mut elem: Option<String> = None;
            for var in ["Fire", "Cold", "Lightning"] {
                if wd.num(&format!("{var}DPS")).is_some() {
                    let mut s = elem.map_or_else(|| format!("{grey}Elemental Damage: "), |x| format!("{x}{grey}, "));
                    s.push_str(&fmt(
                        "%s%d-%d",
                        &[
                            Arg::Str(&cc(&var.to_uppercase())),
                            Arg::Num(n(&format!("{var}Min"))),
                            Arg::Num(n(&format!("{var}Max"))),
                        ],
                    ));
                    elem = Some(s);
                }
            }
            t.add_line_ex(g, big, elem.as_deref(), sc, false, None);
            t.add_line_ex(
                g,
                big,
                Some(&fmt(&format!("{grey}Elemental DPS: {magic}%.1f"), &[Arg::Num(n("ElementalDPS"))])),
                sc,
                false,
                None,
            );
            total_damage_types += 1;
        }
        if wd.num("ChaosDPS").is_some() {
            t.add_line_ex(
                g,
                big,
                Some(&fmt(
                    &format!("{grey}Chaos Damage: {}%d-%d {magic}(%.1f DPS)", cc("CHAOS")),
                    &[Arg::Num(n("ChaosMin")), Arg::Num(n("ChaosMax")), Arg::Num(n("ChaosDPS"))],
                )),
                sc,
                false,
                None,
            );
            total_damage_types += 1;
        }
        if total_damage_types > 1 {
            t.add_line_ex(
                g,
                big,
                Some(&fmt(&format!("{grey}Total DPS: {magic}%.1f"), &[Arg::Num(n("TotalDPS"))])),
                sc,
                false,
                None,
            );
        }
        t.add_line_ex(
            g,
            big,
            Some(&fmt(
                &format!("{grey}Critical Strike Chance: %s%.2f%%"),
                &[
                    Arg::Str(&color(n("CritChance"), Some(bw.CritChanceBase))),
                    Arg::Num(n("CritChance")),
                ],
            )),
            sc,
            false,
            None,
        );
        t.add_line_ex(
            g,
            big,
            Some(&fmt(
                &format!("{grey}Attacks per Second: %s%.2f"),
                &[
                    Arg::Str(&color(n("AttackRate"), Some(bw.AttackRateBase))),
                    Arg::Num(n("AttackRate")),
                ],
            )),
            sc,
            false,
            None,
        );
        if n("range") < 120.0 {
            t.add_line_ex(
                g,
                big,
                Some(&fmt(
                    &format!("{grey}Weapon Range: %s%.1f {grey}metres"),
                    &[Arg::Str(&color(n("range"), Some(bw.Range))), Arg::Num(n("range") / 10.0)],
                )),
                sc,
                false,
                None,
            );
        }
    } else if let Some(ba) = &base.armour {
        let ad = item.armour_data.as_ref().expect("armourData");
        let n = |k: &str| ad.num(k).unwrap_or(0.0);
        if quality > 0.0 {
            quality_line(t);
        }
        if let Some(bc) = ba.BlockChance
            && n("BlockChance") > 0.0
        {
            t.add_line_ex(
                g,
                big,
                Some(&fmt(
                    &format!("{grey}Chance to Block: %s%d%%"),
                    &[Arg::Str(&color(n("BlockChance"), Some(bc))), Arg::Num(n("BlockChance"))],
                )),
                sc,
                false,
                None,
            );
        }
        for (key, label) in [
            ("Armour", "Armour"),
            ("Evasion", "Evasion Rating"),
            ("EnergyShield", "Energy Shield"),
            ("Ward", "Ward"),
        ] {
            if n(key) > 0.0 {
                t.add_line_ex(
                    g,
                    big,
                    Some(&fmt(
                        &format!("{grey}{label}: %s%d"),
                        &[Arg::Str(&color(n(key), ad.num(&format!("{key}Base")))), Arg::Num(n(key))],
                    )),
                    sc,
                    false,
                    None,
                );
            }
        }
    } else if let Some(bf) = &base.flask {
        let fd = item.flask_data.as_ref().expect("flaskData");
        let n = |k: &str| fd.num(k).unwrap_or(0.0);
        if quality > 0.0 {
            quality_line(t);
        }
        for (kind, label, b) in [("life", "Life", bf.life), ("mana", "Mana", bf.mana)] {
            if fd.num(&format!("{kind}Total")).is_some() {
                let total = n(&format!("{kind}Total"));
                if n(&format!("{kind}Gradual")) != 0.0 {
                    t.add_line_ex(
                        g,
                        big,
                        Some(&fmt(
                            &format!("{grey}Recovers %s%d {grey}{label} over %s%.1f0 {grey}Seconds"),
                            &[
                                Arg::Str(&color(total, b)),
                                Arg::Num(n(&format!("{kind}Gradual"))),
                                Arg::Str(&color(n("duration"), Some(bf.duration))),
                                Arg::Num(n("duration")),
                            ],
                        )),
                        sc,
                        false,
                        None,
                    );
                }
                if n(&format!("{kind}Instant")) != 0.0 {
                    t.add_line_ex(
                        g,
                        big,
                        Some(&fmt(
                            &format!("{grey}Recovers %s%d {grey}{label} instantly"),
                            &[Arg::Str(&color(total, b)), Arg::Num(n(&format!("{kind}Instant")))],
                        )),
                        sc,
                        false,
                        None,
                    );
                }
            }
        }
        if fd.num("lifeTotal").is_none() && fd.num("manaTotal").is_none() {
            t.add_line_ex(
                g,
                big,
                Some(&fmt(
                    &format!("{grey}Lasts %s%.2f {grey}Seconds"),
                    &[Arg::Str(&color(n("duration"), Some(bf.duration))), Arg::Num(n("duration"))],
                )),
                sc,
                false,
                None,
            );
        }
        t.add_line_ex(
            g,
            big,
            Some(&fmt(
                &format!("{grey}Consumes %s%d {grey}of %s%d {grey}Charges on use"),
                &[
                    Arg::Str(&color(n("chargesUsed"), Some(bf.charges_used))),
                    Arg::Num(n("chargesUsed")),
                    Arg::Str(&color(n("chargesMax"), Some(bf.charges_max))),
                    Arg::Num(n("chargesMax")),
                ],
            )),
            sc,
            false,
            None,
        );
        buff_lines(t, g, item, big);
    } else if let Some(bt) = &base.tincture {
        let td = item.tincture_data.as_ref().expect("tinctureData");
        let n = |k: &str| td.num(k).unwrap_or(0.0);
        if quality > 0.0 {
            quality_line(t);
        }
        t.add_line_ex(
            g,
            big,
            Some(&fmt(
                &format!("{grey}Inflicts Mana Burn every %s%.2f {grey}Seconds"),
                &[Arg::Str(&color(n("manaBurn"), Some(bt.mana_burn))), Arg::Num(n("manaBurn"))],
            )),
            sc,
            false,
            None,
        );
        t.add_line_ex(
            g,
            big,
            Some(&fmt(
                &format!("{grey}%s%.2f {grey}Second Cooldown When Deactivated"),
                &[Arg::Str(&color(n("cooldown"), Some(bt.cooldown))), Arg::Num(n("cooldown"))],
            )),
            sc,
            false,
            None,
        );
        buff_lines(t, g, item, big);
    } else if item.ty.as_deref() == Some("Jewel") {
        if let Some(l) = item.limit {
            t.add_line_ex(
                g,
                big,
                Some(&format!("{grey}Limited to: ^7{}", pob_lua::tostring_num(l))),
                sc,
                false,
                None,
            );
        }
        if let Some(cr) = &item.class_restriction {
            let col = if e.spec_rc().cur_class_name == *cr {
                cc("POSITIVE")
            } else {
                cc("NEGATIVE")
            };
            t.add_line_ex(g, big, Some(&format!("{grey}Requires Class {col}{cr}")), sc, false, None);
        }
        if let Some(r) = &item.jewel_radius_label {
            t.add_line_ex(g, big, Some(&format!("{grey}Radius: ^7{r}")), sc, false, None);
        }
        // :4486-4498 `item.jewelRadiusData[slot.nodeId]`: what the last MAIN run left in `rad.data` (CalcSetup.lua:306-314); of several functions of one jewel, the last
        let radius = a.slot.and_then(|s| s.node_id).and_then(|node| {
            e.result()
                .env
                .radius_jewel_list
                .iter()
                .rev()
                .find(|r| r.node_id == node && r.item.id == item.id)
        });
        if let Some(rad) = radius {
            let mut text: Option<String> = None;
            for (name, code) in [("Str", "STRENGTH"), ("Dex", "DEXTERITY"), ("Int", "INTELLIGENCE")] {
                let v = rad.data.get(name).as_num().unwrap_or(0.0);
                if v != 0.0 {
                    let part = format!("{}{} {name}^7", cc(code), v as i64);
                    text = Some(text.map_or(part.clone(), |l| format!("{l}, {part}")));
                }
            }
            if let Some(text) = text {
                t.add_line_ex(g, big, Some(&format!("^x7F7F7FAttributes in Radius: {text}")), sc, false, None);
            }
        }
    }
    if let (Some(c), Some(q)) = (item.catalyst, item.catalyst_quality)
        && c > 0.0
        && c <= 12.0
        && q > 0.0
    {
        const NAMES: [&str; 12] = [
            "Attack",
            "Speed",
            "Suffix",
            "Life and Mana",
            "Caster",
            "Attribute",
            "Physical and Chaos",
            "Resistance",
            "Prefix",
            "Defence",
            "Elemental",
            "Critical",
        ];
        t.add_line_ex(
            g,
            big,
            Some(&fmt(
                &format!("{grey}Quality ({} Modifiers): {magic}+%d%% (augmented)", NAMES[c as usize - 1]),
                &[Arg::Num(q)],
            )),
            sc,
            false,
            None,
        );
        t.add_separator(Some(10.0));
    }
    if !item.sockets.is_empty() {
        t.add_line_ex(
            g,
            big,
            Some(&format!("{grey}Sockets: {}", socket_description_line(item))),
            sc,
            false,
            None,
        );
    }
    if let Some(tier) = item.talisman_tier {
        t.add_line_ex(
            g,
            big,
            Some(&format!("{grey}Talisman Tier ^xFFFFFF{}", pob_lua::tostring_num(tier))),
            sc,
            false,
            None,
        );
        t.add_separator(Some(10.0));
    }
    if let Some(m) = item.memory_strands {
        if flavour {
            t.add_line(g, 5.0, Some(""));
        }
        t.add_line_ex(
            g,
            big,
            Some(&fmt(&format!("{}Memory Strands: ^7%d", cc("MEMORY")), &[Arg::Num(m)])),
            sc,
            false,
            Some("Assets/memorybg.png"),
        );
        if flavour {
            t.add_line(g, 5.0, Some(""));
        }
    }
    if let Some(m) = item.intangibility {
        if flavour {
            t.add_line(g, 5.0, Some(""));
        }
        t.add_line_ex(
            g,
            big,
            Some(&fmt(&format!("{}Intangibility: ^7%d%%", cc("INTANGIBILITY")), &[Arg::Num(m)])),
            sc,
            false,
            Some("Assets/intangibilitybg.png"),
        );
        if flavour {
            t.add_line(g, 5.0, Some(""));
        }
    }
    t.add_separator(Some(10.0));
    add_requirements(t, g, e, item);

    for (lid, list) in [
        (1u8, &item.enchant_mod_lines),
        (2, &item.scourge_mod_lines),
        (3, &item.implicit_mod_lines),
        (4, &item.explicit_mod_lines),
        (5, &item.crucible_mod_lines),
    ] {
        if list.is_empty() {
            continue;
        }
        for (i, ml) in list.iter().enumerate() {
            let count = item.get_mod_line_variant_count(ml);
            if count > 0
                && let Some(text) = format_mod_line(ml, a.db_mode)
            {
                for _ in 0..count {
                    t.add_mod_line(g, big, &text, sc, ml.flag("disabled"), (lid, i));
                }
            }
        }
        t.add_separator(Some(10.0));
    }

    if item.cluster_jewel.is_some() {
        t.add_separator(Some(10.0));
        let jd = item.jewel_data.as_ref().expect("jewelData");
        let notables = pob_items::item::value_strings(&jd.get("clusterJewelNotables"));
        let spec = e.spec_rc();
        let node_lines = |name: &str, t: &mut Tooltip| {
            if let Some(&i) = spec.tree.cluster_node_map.get(name) {
                let n = &spec.tree.nodes[i];
                t.add_line_ex(g, big, Some(&format!("{magic}{}", n.dn)), sc, false, None);
                for stat in &n.sd {
                    t.add_line_ex(g, big, Some(&format!("{grey}{stat}")), sc, false, None);
                }
            }
        };
        if !notables.is_empty() {
            for name in &notables {
                node_lines(name, t);
            }
        } else if let Some(k) = jd.get("clusterJewelKeystone").as_str() {
            node_lines(k, t);
        }
        t.add_separator(Some(10.0));
    }

    if item.corrupted || item.split || item.mirrored {
        if item.explicit_mod_lines.is_empty() {
            t.add_separator(Some(10.0));
        }
        let neg = cc("NEGATIVE");
        if item.split {
            t.add_line_ex(g, big, Some(&format!("{neg}Split")), sc, false, None);
        }
        if item.mirrored {
            t.add_line_ex(g, big, Some(&format!("{neg}Mirrored")), sc, false, None);
        }
        if item.corrupted {
            t.add_line_ex(g, big, Some(&format!("{neg}Corrupted")), sc, false, None);
        }
        t.add_separator(Some(14.0));
    }

    if (item.rarity == "UNIQUE" || item.rarity == "RELIC" || base.flavour_text.is_some()) && flavour {
        let mut text: Option<Vec<String>> = base.flavour_text.clone();
        if text.is_none() {
            let title = pob_items::lua::gsub(item.title.as_deref().expect("title"), "^Foulborn%s+", "", None);
            if let Some(table) = flavour_table(&title) {
                if title.contains("Grand Spectrum") {
                    let mut id = None;
                    for l in &t.lines {
                        let s = l.text.as_deref().unwrap_or("");
                        id = [
                            ("Power", "UniqueJewel170"),
                            ("Endurance", "UniqueJewel168"),
                            ("Frenzy", "UniqueJewel169"),
                            ("Elemental Damage", "UniqueJewel168"),
                            ("Elemental Ailments", "UniqueJewel166"),
                            ("Elemental Resistances", "UniqueJewel76"),
                            ("Armour", "UniqueJewel76"),
                            ("Maximum Life", "UniqueJewel165"),
                            ("Critical Strike Multiplier", "UniqueJewel167"),
                            ("Critical Strike Chance", "UniqueJewel75"),
                            ("Mana", "UniqueJewel75"),
                        ]
                        .iter()
                        .find(|(k, _)| s.contains(k))
                        .map(|(_, v)| *v);
                        if id.is_some() {
                            break;
                        }
                    }
                    text = id.and_then(|id| table.iter().find(|(i, _)| i == id).map(|(_, l)| l.clone()));
                } else {
                    // R13: `pairs(flavourTable)`: the first of a process; here the file's
                    text = table.first().map(|(_, l)| l.clone());
                }
            }
        }
        if let Some(lines) = text {
            let u = cc("UNIQUE");
            for l in &lines {
                t.add_line_ex(g, big, Some(&format!("{u}{l}")), Some("FONTIN SC ITALIC"), false, None);
            }
            t.add_separator(Some(14.0));
        }
    }

    // :4654-4700 the granted skills
    if t.child_tooltips.is_none() {
        t.child_tooltips = Some(Vec::new());
    }
    for c in t.child_tooltips.as_mut().expect("children") {
        c.clear(false);
    }
    let mut item_skills: Vec<(String, f64)> = item
        .granted_skills
        .iter()
        .map(|s| (s.skill_id.as_str().unwrap_or("").to_string(), s.level.as_num().unwrap_or(1.0)))
        .collect();
    let mods = item
        .mod_list
        .as_ref()
        .or_else(|| item.slot_mod_list.as_ref().and_then(|l| l.get(slot_num as usize - 1)));
    for m in mods.into_iter().flatten() {
        if &*m.m.name == "ExtraSupport" {
            item_skills.push((
                m.m.value.get("skillId").as_str().unwrap_or("").to_string(),
                m.m.value.get("level").as_num().unwrap_or(1.0),
            ));
        }
    }
    if !item_skills.is_empty() {
        t.add_separator(Some(14.0));
        t.add_line(
            g,
            14.0,
            Some(&format!(
                "{}Tip: Hold Shift to display a tooltip for the granted skill{}.",
                cc("TIP"),
                if item_skills.len() > 1 { "s" } else { "" }
            )),
        );
        let shift = sg::is_key_down(g, "SHIFT");
        for (i, (skill_id, level)) in item_skills.iter().enumerate() {
            let children = t.child_tooltips.as_mut().expect("children");
            if children.len() <= i {
                children.push(Tooltip::new());
            }
            let data = pob_data::skills::skills();
            if let (Some(effect), true) = (data.get(skill_id), shift) {
                let gem = pob_data::gems::gems().gems.0.values().find(|g| g.granted_effect_id == *skill_id);
                let inst = crate::gem_tooltip::GemInst {
                    gem,
                    effect,
                    level: *level,
                    quality: 0.0,
                    display: None,
                    skip_requirements: gem.is_none(),
                };
                crate::gem_tooltip::add_gem_tooltip(&mut children[i], g, e, &inst);
            }
        }
    }
    let hint = if a.in_items_tab { "" } else { " in the Items tab" };
    if !e.items_show_stat_differences() {
        t.add_separator(Some(14.0));
        t.add_line(
            g,
            14.0,
            Some(&format!(
                "{}Tip: Press Ctrl+D{hint} to enable the display of stat differences.",
                cc("TIP")
            )),
        );
        return;
    } else if base.flask.is_none() && base.tincture.is_none() {
        t.add_line(
            g,
            14.0,
            Some(&format!(
                "{}Tip: Press Ctrl+D{hint} to disable the display of stat differences.",
                cc("TIP")
            )),
        );
    }
    add_item_stat_differences(t, g, e, item, a);
    // :4712-4718 modifier debugging info
    if crate::launch::launch().dev_mode_alt.get() {
        t.add_separator(Some(10.0));
        for m in mods.into_iter().flatten() {
            t.add_line(g, 14.0, Some(&format!("^7{}", pob_mods::format::format_mod(&m.m))));
        }
    }
}

fn influence_color(key: &str) -> String {
    let code = match key {
        "shaper" => "SHAPER",
        "elder" => "ELDER",
        "adjudicator" => "ADJUDICATOR",
        "basilisk" => "BASILISK",
        "crusader" => "CRUSADER",
        "eyrie" => "EYRIE",
        "cleansing" => "CLEANSING",
        _ => "TANGLE",
    };
    cc(code)
}

fn buff_lines(t: &mut Tooltip, g: &Gfx, item: &Item, big: f64) {
    for ml in &item.buff_mod_lines {
        let text = if ml.extra.is_some() {
            let tail = if not_supported_mod_tooltips() {
                " ^8(Not supported in PoB yet)"
            } else {
                ""
            };
            format!("{}{}{tail}", cc("UNSUPPORTED"), ml.line)
        } else {
            format!("{}{}", cc("MAGIC"), ml.line)
        };
        t.add_line_ex(g, big, Some(&text), Some("FONTIN SC"), false, None);
    }
}

fn sum(e: &Engine, ty: ModType, actor: bool, name: &str) -> f64 {
    let db = e.result().env.mod_db.borrow();
    let cfg = pob_mods::ModCfg {
        actor: Some("player".into()),
        ..Default::default()
    };
    db.sum(ty, actor.then_some(&cfg), &[name]).unwrap_or(0.0)
}

fn flag(e: &Engine, name: &str) -> bool {
    e.result().env.mod_db.borrow().flag(None, &[name]).ok().flatten() == Some(true)
}

/// `ItemsTab:AddItemStatDifferences(tooltip, item, base, slot)`.
#[allow(clippy::too_many_lines)]
pub fn add_item_stat_differences(t: &mut Tooltip, g: &Gfx, e: &Engine, item: &Rc<Item>, a: &ItemTooltipArgs<'_>) {
    let base = item.base.expect("base");
    let lines = |t: &mut Tooltip, ls: Vec<String>| {
        for l in ls {
            t.add_line(g, 14.0, Some(&l));
        }
    };
    if let Some(bf) = &base.flask {
        let fd = item.flask_data.as_ref().expect("flaskData");
        let n = |k: &str| fd.num(k).unwrap_or(0.0);
        let mut stats: Vec<String> = Vec::new();
        let dur_inc = sum(e, ModType::Inc, false, "FlaskDuration");
        let mut effect_inc = sum(e, ModType::Inc, true, "FlaskEffect");
        let (mut life_dur, mut mana_dur) = (0.0, 0.0);
        if item.rarity == "MAGIC" && bf.life.is_none() && bf.mana.is_none() {
            effect_inc += sum(e, ModType::Inc, true, "MagicUtilityFlaskEffect");
        }
        let out = |k: &str| e.output_value(k).unwrap_or(0.0);
        if bf.life.is_some() || bf.mana.is_some() {
            let rate_inc = sum(e, ModType::Inc, false, "FlaskRecoveryRate");
            if bf.life.is_some() {
                let life_inc = sum(e, ModType::Inc, false, "FlaskLifeRecovery");
                let life_more = e.result().env.mod_db.borrow().more(None, &["FlaskLifeRecovery"]).unwrap_or(1.0);
                let life_rate_inc = sum(e, ModType::Inc, false, "FlaskLifeRecoveryRate");
                let instant = n("instantPerc") + sum(e, ModType::Base, false, "LifeFlaskInstantRecovery");
                let low = e
                    .result()
                    .env
                    .mod_db
                    .borrow()
                    .more(None, &["FlaskLifeRecoveryLowLife"])
                    .unwrap_or(1.0);
                let low_multi = if low > 1.0 {
                    (low - 1.0) / (1.0 + effect_inc / 100.0) + 1.0
                } else {
                    1.0
                };
                let inst = n("lifeBase") * instant / 100.0 * (1.0 + life_inc / 100.0) * life_more * (1.0 + effect_inc / 100.0) * low_multi;
                let b = n("lifeBase")
                    * (1.0 - instant / 100.0)
                    * (1.0 + life_inc / 100.0)
                    * life_more
                    * (1.0 + effect_inc / 100.0)
                    * (1.0 + dur_inc / 100.0)
                    * low_multi;
                let grad = b * out("LifeRecoveryRateMod");
                let es_grad = b * out("EnergyShieldRecoveryRateMod");
                life_dur = n("duration") * (1.0 + dur_inc / 100.0) / (1.0 + rate_inc / 100.0) / (1.0 + life_rate_inc / 100.0);
                if n("lifeAdditional") > 0.0 && !e.config_flag_set("conditionFullLife") {
                    let total_add = n("lifeAdditional") / 100.0 * n("lifeTotal") * out("LifeRecoveryRateMod");
                    let add_grad = life_dur / 10.0 * total_add;
                    let left_dur = 10.0 - life_dur;
                    let left_amt = total_add - add_grad;
                    if inst > 0.0 {
                        if grad > 0.0 {
                            stats.push(fmt(
                                "^8Life recovered: ^7%d ^8(^7%d^8 instantly, plus ^7%d ^8over^7 %.2fs^8, and an additional ^7%d ^8over subsequent ^7%.2fs^8)",
                                &[
                                    Arg::Num(inst + grad + total_add),
                                    Arg::Num(inst),
                                    Arg::Num(grad + add_grad),
                                    Arg::Num(life_dur),
                                    Arg::Num(left_amt),
                                    Arg::Num(left_dur),
                                ],
                            ));
                        } else {
                            life_dur = 0.0;
                            stats.push(fmt(
                                "^8Life recovered: ^7%d ^8(^7%d^8 instantly, and an additional ^7%d ^8over ^7%.2fs^8)",
                                &[Arg::Num(inst + total_add), Arg::Num(inst), Arg::Num(total_add), Arg::Num(10.0)],
                            ));
                        }
                    } else {
                        stats.push(fmt(
                            "^8Life recovered: ^7%d ^8(^7%d ^8over ^7%.2fs^8, and an additional ^7%d ^8over subsequent ^7%.2fs^8)",
                            &[
                                Arg::Num(grad + total_add),
                                Arg::Num(grad + add_grad),
                                Arg::Num(life_dur),
                                Arg::Num(left_amt),
                                Arg::Num(left_dur),
                            ],
                        ));
                    }
                } else if inst > 0.0 && grad > 0.0 {
                    stats.push(fmt(
                        "^8Life recovered: ^7%d ^8(^7%d^8 instantly, plus ^7%d ^8over^7 %.2fs^8)",
                        &[Arg::Num(inst + grad), Arg::Num(inst), Arg::Num(grad), Arg::Num(life_dur)],
                    ));
                } else if inst + grad != n("lifeTotal") || (inst == 0.0 && life_dur != n("duration")) {
                    if inst > 0.0 {
                        life_dur = 0.0;
                        stats.push(fmt("^8Life recovered: ^7%d ^8instantly", &[Arg::Num(inst)]));
                    } else if grad > 0.0 {
                        stats.push(fmt("^8Life recovered: ^7%d ^8over ^7%.2fs", &[Arg::Num(grad), Arg::Num(life_dur)]));
                    }
                }
                if flag(e, "LifeFlaskAppliesToEnergyShield") {
                    if inst > 0.0 && es_grad > 0.0 {
                        stats.push(fmt(
                            "^8Energy Shield recovered: ^7%d ^8(^7%d^8 instantly, plus ^7%d ^8over^7 %.2fs^8)",
                            &[Arg::Num(inst + es_grad), Arg::Num(inst), Arg::Num(es_grad), Arg::Num(life_dur)],
                        ));
                    } else if inst > 0.0 && es_grad == 0.0 {
                        stats.push(fmt("^8Energy Shield recovered: ^7%d ^8instantly", &[Arg::Num(inst)]));
                    } else if inst == 0.0 && es_grad > 0.0 {
                        stats.push(fmt(
                            "^8Energy Shield recovered: ^7%d ^8over ^7%.2fs",
                            &[Arg::Num(es_grad), Arg::Num(life_dur)],
                        ));
                    }
                }
            }
            if bf.mana.is_some() {
                let mana_inc = sum(e, ModType::Inc, false, "FlaskManaRecovery");
                let mana_rate_inc = sum(e, ModType::Inc, false, "FlaskManaRecoveryRate");
                let instant = n("instantPerc") + sum(e, ModType::Base, false, "ManaFlaskInstantRecovery");
                let inst = n("manaBase") * instant / 100.0 * (1.0 + mana_inc / 100.0) * (1.0 + effect_inc / 100.0);
                let b = n("manaBase")
                    * (1.0 - instant / 100.0)
                    * (1.0 + mana_inc / 100.0)
                    * (1.0 + effect_inc / 100.0)
                    * (1.0 + dur_inc / 100.0);
                let grad = b * out("ManaRecoveryRateMod");
                let life_grad = b * out("LifeRecoveryRateMod");
                mana_dur = n("duration") * (1.0 + dur_inc / 100.0) / (1.0 + rate_inc / 100.0) / (1.0 + mana_rate_inc / 100.0);
                if inst > 0.0 && grad > 0.0 {
                    stats.push(fmt(
                        "^8Mana recovered: ^7%d ^8(^7%d^8 instantly, plus ^7%d ^8over^7 %.2fs^8)",
                        &[Arg::Num(inst + grad), Arg::Num(inst), Arg::Num(grad), Arg::Num(mana_dur)],
                    ));
                } else if inst + grad != n("manaTotal") || (inst == 0.0 && mana_dur != n("duration")) {
                    if inst > 0.0 {
                        mana_dur = 0.0;
                        stats.push(fmt("^8Mana recovered: ^7%d ^8instantly", &[Arg::Num(inst)]));
                    } else if grad > 0.0 {
                        stats.push(fmt("^8Mana recovered: ^7%d ^8over ^7%.2fs", &[Arg::Num(grad), Arg::Num(mana_dur)]));
                    }
                }
                if flag(e, "ManaFlaskAppliesToLife") && life_grad > 0.0 {
                    stats.push(fmt(
                        "^8Life recovered: ^7%d ^8over ^7%.2fs",
                        &[Arg::Num(life_grad), Arg::Num(mana_dur)],
                    ));
                }
            }
        } else if dur_inc != 0.0 {
            stats.push(fmt(
                "^8Flask effect duration: ^7%.1f0s",
                &[Arg::Num(n("duration") * (1.0 + dur_inc / 100.0))],
            ));
        }
        let effect_mod = 1.0 + (n("effectInc") + effect_inc) / 100.0;
        if effect_mod != 1.0 {
            stats.push(fmt("^8Flask effect modifier: ^7%+d%%", &[Arg::Num(effect_mod * 100.0 - 100.0)]));
        }
        let used_inc = sum(e, ModType::Inc, false, "FlaskChargesUsed");
        if used_inc != 0.0 {
            let used = (n("chargesUsed") * (1.0 + used_inc / 100.0)).floor();
            stats.push(fmt(
                "^8Charges used: ^7%d ^8of ^7%d ^8(^7%d ^8uses)",
                &[
                    Arg::Num(used),
                    Arg::Num(n("chargesMax")),
                    Arg::Num((n("chargesMax") / used).floor()),
                ],
            ));
        }
        let gain_mod = n("gainMod") * (1.0 + sum(e, ModType::Inc, false, "FlaskChargesGained") / 100.0);
        if gain_mod != 1.0 {
            stats.push(fmt("^8Charge gain modifier: ^7%+d%%", &[Arg::Num(gain_mod * 100.0 - 100.0)]));
        }
        let mut generated = sum(e, ModType::Base, false, "FlaskChargesGenerated");
        if bf.life.is_some() {
            generated += sum(e, ModType::Base, false, "LifeFlaskChargesGenerated");
        }
        if bf.mana.is_some() {
            generated += sum(e, ModType::Base, false, "ManaFlaskChargesGenerated");
        }
        if bf.mana.is_none() && bf.life.is_none() {
            generated += sum(e, ModType::Base, false, "UtilityFlaskChargesGenerated");
        }
        let mut on_ward = 0.0;
        if item.base_name.as_deref() == Some("Iron Flask") {
            on_ward += sum(e, ModType::Base, false, "IronFlaskChargesGeneratedOnWardBreak");
        }
        let mut per_flask = sum(e, ModType::Base, false, "FlaskChargesGeneratedPerEmptyFlask") * a.empty_flask_slots;
        generated *= gain_mod;
        on_ward *= gain_mod;
        per_flask *= gain_mod;
        let total = generated + per_flask;
        if total > 0.0 {
            stats.push(fmt("^8Charges generated: ^7%.2f^8 per second", &[Arg::Num(total)]));
        }
        if on_ward > 0.0 {
            stats.push(fmt("^8Charges generated on Ward Break: ^7%.2f", &[Arg::Num(on_ward)]));
        }
        let not_consume = sum(e, ModType::Base, false, "FlaskChanceNotConsumeCharges").min(100.0);
        if not_consume != 0.0 {
            stats.push(fmt("^8Chance to not consume charges: ^7%d%%", &[Arg::Num(not_consume)]));
        }
        let mut has_uptime = bf.life.is_none() && bf.mana.is_none();
        let mut flask_duration = n("duration") * (1.0 + dur_inc / 100.0);
        if bf.life.is_some() && (fd.get("lifeEffectNotRemoved").truthy() || flag(e, "LifeFlaskEffectNotRemoved")) {
            has_uptime = true;
            flask_duration = life_dur;
        } else if bf.mana.is_some() && (fd.get("manaEffectNotRemoved").truthy() || flag(e, "ManaFlaskEffectNotRemoved")) {
            has_uptime = true;
            flask_duration = mana_dur;
        }
        if has_uptime {
            let used = n("chargesUsed") * (1.0 + used_inc / 100.0);
            if used > 0.0 && flask_duration > 0.0 {
                let per3 = flask_duration - lua_mod(flask_duration, 3.0);
                let per5 = flask_duration - lua_mod(flask_duration, 5.0);
                let minimum = per3 * generated + per5 * per_flask;
                let pmin = (minimum / used * 100.0).min(100.0);
                if pmin < 100.0 && not_consume < 100.0 {
                    let avg_gen = (generated + per_flask) * flask_duration;
                    let avg_used = used * (100.0 - not_consume) / 100.0;
                    let pavg = (avg_gen / avg_used * 100.0).min(100.0);
                    stats.push(fmt(
                        "^8Flask uptime: ^7%d%%^8 average, ^7%d%%^8 minimum",
                        &[Arg::Num(pavg), Arg::Num(pmin)],
                    ));
                } else {
                    stats.push("^8Flask uptime: ^7100%^8".to_string());
                }
            }
        }
        if !stats.is_empty() {
            t.add_line(g, 14.0, Some("^7Effective flask stats:"));
            for s in &stats {
                t.add_line(g, 14.0, Some(s));
            }
        }
        let on = e.result().env.flasks.iter().any(|f| Rc::ptr_eq(f, item));
        let header = if on {
            "^7Deactivating this flask will give you:"
        } else {
            "^7Activating this flask will give you:"
        };
        let ov = CalcOverride {
            toggle_flask: Some(item.clone()),
            ..CalcOverride::default()
        };
        lines(t, e.item_compare_lines(ov, header).unwrap_or_else(|er| panic!("{er}")));
    } else if let Some(bt) = &base.tincture {
        let td = item.tincture_data.as_ref().expect("tinctureData");
        let n = |k: &str| td.num(k).unwrap_or(0.0);
        let mut effect_inc = sum(e, ModType::Inc, true, "TinctureEffect");
        if item.rarity == "MAGIC" {
            effect_inc += sum(e, ModType::Inc, true, "MagicTinctureEffect");
        }
        let effect_mod = (1.0 + (n("effectInc") + effect_inc) / 100.0) * (1.0 + item.quality.unwrap_or(0.0) / 100.0);
        let mut stats = Vec::new();
        if effect_mod != 1.0 {
            stats.push(fmt("^8Tincture effect modifier: ^7%+d%%", &[Arg::Num(effect_mod * 100.0 - 100.0)]));
        }
        stats.push(fmt(
            "^8Mana Burn Inflicted Every Second: ^7%.2f",
            &[Arg::Num(
                1.0 / (n("manaBurn")
                    / (1.0 + sum(e, ModType::Inc, true, "TinctureManaBurnRate") / 100.0)
                    / (1.0 + sum(e, ModType::More, true, "TinctureManaBurnRate") / 100.0)),
            )],
        ));
        let not_inflict = sum(e, ModType::Base, false, "TincturesNotInflictManaBurn").min(100.0);
        if not_inflict != 0.0 {
            stats.push(fmt("^8Chance to not inflict Mana Burn: ^7%d%%", &[Arg::Num(not_inflict)]));
        }
        stats.push(fmt(
            "^8Tincture Cooldown when deactivated: ^7%.2f^8 seconds",
            &[Arg::Num(
                bt.cooldown / (1.0 + (sum(e, ModType::Inc, true, "TinctureCooldownRecovery") + n("cooldownInc")) / 100.0),
            )],
        ));
        t.add_line(g, 14.0, Some("^7Effective tincture stats:"));
        for s in &stats {
            t.add_line(g, 14.0, Some(s));
        }
        let on = e.result().env.tinctures.iter().any(|f| Rc::ptr_eq(f, item));
        let header = if on {
            "^7Deactivating this tincture will give you:"
        } else {
            "^7Activating this tincture will give you:"
        };
        let ov = CalcOverride {
            toggle_tincture: Some(item.clone()),
            ..CalcOverride::default()
        };
        lines(t, e.item_compare_lines(ov, header).unwrap_or_else(|er| panic!("{er}")));
    } else {
        struct Entry {
            sel: Option<Rc<Item>>,
            out: [Option<f64>; 3],
            slot: CompareSlot,
            same_unique: bool,
        }
        let run = |slot: &CompareSlot| -> (Option<Rc<Item>>, pob_calc::session::RunRef) {
            let sel = e.item_by_id(slot.sel_item_id);
            let same = sel.as_ref().is_some_and(|s| Rc::ptr_eq(s, item));
            let changes_tree = |i: Option<&Rc<Item>>| i.is_some_and(|i| item_changes_passive_tree(i));
            let mut ov = CalcOverride {
                rep_slot_name: Some(slot.slot_name.clone()),
                rep_item: (!same).then(|| item.clone()),
                ..CalcOverride::default()
            };
            // :4983-4985
            if let Some(n) = slot.node_id
                && (changes_tree(sel.as_ref()) || changes_tree(Some(item)))
            {
                ov.spec = Some(e.spec_for_jewel_comparison(n, ov.rep_item.clone()).unwrap_or_else(|er| panic!("{er}")));
            }
            let out = e.item_override_run(ov).unwrap_or_else(|er| panic!("{er}"));
            (sel, out)
        };
        let add_compare = |t: &mut Tooltip, slot: &CompareSlot, sel: &Option<Rc<Item>>, out: &pob_calc::session::RunRef| {
            let same = sel.as_ref().is_some_and(|s| Rc::ptr_eq(s, item));
            let header = if same {
                format!("^7Removing this item from {} will give you:", slot.label)
            } else {
                let replacing = sel
                    .as_ref()
                    .map(|s| format!("\n(replacing {}{}^7)", cc(&s.rarity), s.name))
                    .unwrap_or_default();
                format!("^7Equipping this item in {} will give you:{replacing}", slot.label)
            };
            lines(t, e.item_compare_run(out, &header).unwrap_or_else(|er| panic!("{er}")));
        };
        if main().s.borrow().slot_only_tooltips
            && let Some(slot) = a.slot
        {
            let (sel, out) = run(slot);
            add_compare(t, slot, &sel, &out);
            return;
        }
        let is_unique = item.rarity == "UNIQUE" || item.rarity == "RELIC";
        let mut same_count = 0.0;
        let mut slots: Vec<Rc<(Entry, pob_calc::session::RunRef)>> = Vec::new();
        for s in &a.compare_slots {
            let (sel, out) = run(s);
            let same_unique = is_unique && sel.as_ref().is_some_and(|x| x.name == item.name);
            if same_unique && item.limit.is_some() {
                same_count += 1.0;
            }
            let o = out.borrow();
            let vals = [
                o.state.player.output.opt_num("FullDPS"),
                o.state.player.output.opt_num("CombinedDPS"),
                o.state.player.output.opt_num("TotalEHP"),
            ];
            drop(o);
            slots.push(Rc::new((
                Entry {
                    sel,
                    out: vals,
                    slot: s.clone(),
                    same_unique,
                },
                out,
            )));
        }
        if item.limit == Some(same_count) {
            for x in &slots {
                if x.0.same_unique {
                    add_compare(t, &x.0.slot, &x.0.sel, &x.1);
                }
            }
            return;
        }
        let similar = |c: &Option<Rc<Item>>, same_unique: bool| -> f64 {
            let Some(c) = c else { return 0.0 };
            let same_type = !is_unique
                && c.rarity != "UNIQUE"
                && c.rarity != "RELIC"
                && base.ty == c.base.expect("base").ty
                && base.sub_type == c.base.expect("base").sub_type;
            if same_type || same_unique { 1.0 } else { 0.0 }
        };
        lua_sort(&mut slots, &mut |a, b| {
            if Rc::ptr_eq(a, b) {
                return false;
            }
            let (x, y) = (&a.0, &b.0);
            let nums = [
                (
                    Some(if x.slot.sel_item_id == 0.0 { 1.0 } else { 0.0 }),
                    Some(if y.slot.sel_item_id == 0.0 { 1.0 } else { 0.0 }),
                ),
                (Some(similar(&x.sel, x.same_unique)), Some(similar(&y.sel, y.same_unique))),
                (x.out[0], y.out[0]),
                (x.out[1], y.out[1]),
                (x.out[2], y.out[2]),
            ];
            for (p, q) in nums {
                if let (Some(p), Some(q)) = (p, q) {
                    if p > q {
                        return true;
                    } else if p < q {
                        return false;
                    }
                }
            }
            for (p, q) in [(&x.slot.label, &y.slot.label), (&x.slot.slot_name, &y.slot.slot_name)] {
                if p.as_bytes() > q.as_bytes() {
                    return true;
                } else if p.as_bytes() < q.as_bytes() {
                    return false;
                }
            }
            false
        });
        for x in &slots {
            add_compare(t, &x.0.slot, &x.0.sel, &x.1);
        }
    }
}

/// `itemChangesPassiveTree(item)` (:4131-4135).
fn item_changes_passive_tree(i: &Item) -> bool {
    if i.ty.as_deref() != Some("Jewel") {
        return false;
    }
    let Some(jd) = &i.jewel_data else { return false };
    jd.get("conqueredBy").truthy()
        || (i.jewel_radius_index.is_some() && (jd.get("intuitiveLeapLike").truthy() || jd.get("impossibleEscapeKeystone").truthy()))
}

/// Lua's `%` on numbers.
fn lua_mod(a: f64, b: f64) -> f64 {
    a - (a / b).floor() * b
}
