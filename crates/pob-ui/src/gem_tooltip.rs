//! `Classes/GemTooltip.lua` (the shared gem tooltip) and `SkillsTab:AddSocketGroupTooltip` (SkillsTab.lua:1310-1395).

use std::cell::RefCell;
use std::collections::HashMap;

use pob_calc::active_skill::SkillEffect;
use pob_calc::calc_tools::{SkillInstance, build_skill_instance_stats, get_gem_stat_requirement};
use pob_data::gems::GemEntry;
use pob_data::skills::{GrantedEffect, SkillLevel, Skills};
use pob_engine::engine::Engine;
use pob_gfx::Gfx;
use pob_lua::format::{Arg, format};

use crate::colors::color_code;
use crate::common::{not_supported_mod_tooltips, show_flavour_text};
use crate::stat_describer::describe_stats;
use crate::tooltip::{Tooltip, TooltipColor};

fn cc(name: &str) -> String {
    color_code(name).unwrap_or_default()
}

/// `data.skillColorMap[color]` (Data.lua:306).
pub fn skill_color(color: Option<f64>) -> String {
    match color {
        Some(1.0) => cc("STRENGTH"),
        Some(2.0) => cc("DEXTERITY"),
        Some(3.0) => cc("INTELLIGENCE"),
        Some(4.0) => cc("NORMAL"),
        // `data.skillColorMap[nil]`: a nil index reads nil
        _ => String::new(),
    }
}

thread_local! {
    static DESCRIPTIONS: RefCell<Option<HashMap<String, String>>> = const { RefCell::new(None) };
    static FLAVOUR: RefCell<Option<HashMap<String, Vec<String>>>> = const { RefCell::new(None) };
    static COSTS: RefCell<Option<Vec<(String, String, f64)>>> = const { RefCell::new(None) };
    static PRELOAD: RefCell<Option<std::thread::JoinHandle<Preloaded>>> = const { RefCell::new(None) };
}

/// The files of the gem tooltips, read off the UI thread: nothing here depends on the build or on when it is read.
struct Preloaded {
    descriptions: HashMap<String, String>,
    flavour: HashMap<String, Vec<String>>,
    scopes: crate::stat_describer::Scopes,
}

/// Starts reading the tooltips' files; the first tooltip waits for what is not ready yet.
pub fn start_preload(root: &str) {
    let root = root.to_string();
    let handle = std::thread::Builder::new()
        .name("gem tooltip data".into())
        .spawn(move || Preloaded {
            descriptions: pob_data::skills::skill_descriptions().unwrap_or_else(|e| panic!("skills.json: {e}")),
            flavour: load_flavour(&root),
            scopes: crate::stat_describer::read_all_scopes(&root),
        });
    // without a thread each reader loads its file itself
    PRELOAD.with_borrow_mut(|p| *p = handle.ok());
}

/// The results of [`start_preload`], once.
pub(crate) fn join_preload() {
    let Some(handle) = PRELOAD.with_borrow_mut(Option::take) else {
        return;
    };
    let p = handle.join().unwrap_or_else(|e| std::panic::resume_unwind(e));
    DESCRIPTIONS.with_borrow_mut(|d| *d = Some(p.descriptions));
    FLAVOUR.with_borrow_mut(|d| *d = Some(p.flavour));
    crate::stat_describer::install_scopes(p.scopes);
}

/// `grantedEffect.description` (pob-data keeps only what the calculation reads).
fn description(id: &str) -> Option<String> {
    join_preload();
    DESCRIPTIONS.with_borrow_mut(|d| {
        d.get_or_insert_with(|| pob_data::skills::skill_descriptions().unwrap_or_else(|e| panic!("skills.json: {e}")))
            .get(id)
            .cloned()
    })
}

/// `data/skills.json` has no `flavourText`, the source lines of `Data/Skills/*.lua` do.
fn load_flavour(root: &str) -> HashMap<String, Vec<String>> {
    let mut m = HashMap::new();
    for file in [
        "act_str", "act_dex", "act_int", "other", "sup_str", "sup_dex", "sup_int", "glove", "minion", "spectre",
    ] {
        let Ok(src) = std::fs::read(format!("{root}Data/Skills/{file}.lua")) else {
            continue;
        };
        let text = String::from_utf8_lossy(&src);
        let mut cur: Option<String> = None;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("skills[\"") {
                cur = rest.split('"').next().map(str::to_string);
            } else if let (Some(rest), Some(id)) = (line.trim_start().strip_prefix("flavourText = "), &cur) {
                let table = rest.trim_end().trim_end_matches(',');
                if let Ok(v) = crate::lua_data::parse_return(format!("return {table}").as_bytes())
                    && let Some(t) = v.as_tab()
                {
                    m.insert(id.clone(), t.seq().iter().filter_map(|x| x.as_str().map(str::to_string)).collect());
                }
            }
        }
    }
    m
}

/// `grantedEffect.flavourText`.
fn flavour_text(id: &str) -> Option<Vec<String>> {
    join_preload();
    FLAVOUR.with_borrow_mut(|d| {
        d.get_or_insert_with(|| load_flavour(&crate::launch::launch().cfg.script_path))
            .get(id)
            .cloned()
    })
}

/// `data.costs`: `(Resource, ResourceString, Divisor)`.
fn costs() -> Vec<(String, String, f64)> {
    COSTS.with_borrow_mut(|d| {
        d.get_or_insert_with(|| {
            let j: serde_json::Value = pob_data::read_json("costs.json").unwrap_or_else(|e| panic!("costs.json: {e}"));
            j.as_array()
                .map(|a| {
                    a.iter()
                        .map(|c| {
                            (
                                c["Resource"].as_str().unwrap_or("").to_string(),
                                c["ResourceString"].as_str().unwrap_or("").to_string(),
                                c["Divisor"].as_f64().unwrap_or(1.0),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .clone()
    })
}

fn fmt(spec: &str, args: &[Arg<'_>]) -> String {
    format(spec, args).unwrap_or_else(|e| panic!("{e}"))
}

/// A `gemInstance` as the tooltip reads it.
pub struct GemInst<'a> {
    pub gem: Option<&'a GemEntry>,
    /// `gemInstance.gemData.grantedEffect`.
    pub effect: &'a GrantedEffect,
    pub level: f64,
    pub quality: f64,
    /// `gemInstance.displayEffect`.
    pub display: Option<SkillEffect>,
    pub skip_requirements: bool,
}

const FS: Option<&str> = Some("FONTIN SC");

/// The `reservationMap` of the level, as `ResourceString`s (`pairs`: one kind per gem in the data).
fn reservation(lv: Option<&SkillLevel>) -> Option<String> {
    let lv = lv?;
    let c = costs();
    let mut out: Option<String> = None;
    for (v, res) in [
        (lv.mana_reservation_flat, "Mana"),
        (lv.mana_reservation_percent, "ManaPercent"),
        (lv.life_reservation_percent, "LifePercent"),
    ] {
        if let Some(v) = v {
            let s = c
                .iter()
                .find(|x| x.0 == res)
                .unwrap_or_else(|| panic!("Classes/GemTooltip.lua:64: data.costs has no {res}"))
                .1
                .replace("{0}", &fmt("%d", &[Arg::Num(v)]));
            out = Some(match out {
                Some(o) => format!("{o}, {s}"),
                None => s,
            });
        }
    }
    out
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn add_common(
    t: &mut Tooltip,
    g: &Gfx,
    e: &Engine,
    skills: &Skills,
    inst: &GemInst<'_>,
    ge: &GrantedEffect,
    add_level: bool,
    add_req: bool,
    merge_from: Option<&GrantedEffect>,
) {
    let big = if show_flavour_text() { 18.0 } else { 16.0 };
    let line = |t: &mut Tooltip, s: &str| t.add_line_ex(g, big, Some(s), FS.map(|_| "FONTIN SC"), false, None);
    let d = inst.display.as_ref();
    let disp_level = d.map_or(Some(inst.level), |x| x.level);
    let lv = disp_level.and_then(|l| ge.level(l));
    let other = |k: &str| d.and_then(|x| x.other.get(k).copied()).unwrap_or(0.0);
    if add_level {
        let (global, item, support) = (other("globalLevel"), other("itemLevel"), other("supportLevel"));
        let natural = inst.gem.map_or(f64::INFINITY, |x| x.natural_max_level);
        let max_level = if inst.level >= natural { " (Max)" } else { "" };
        if item != 0.0 || global != 0.0 || support != 0.0 {
            let dl = disp_level.expect("displayInstance.level");
            line(t, &format!("^x7F7F7FLevel: {}{}", cc("MAGIC"), pob_lua::tostring_num(dl)));
            line(t, &fmt("^7%d Levels from Gem%s", &[Arg::Num(inst.level), Arg::Str(max_level)]));
            for (v, name) in [(item, "Item"), (global, "Global Modifiers"), (support, "Supports")] {
                if v != 0.0 {
                    let c = if v > 0.0 { cc("MAGIC") } else { cc("WARNING") };
                    line(t, &format!("{c}{}", fmt("%+d Levels from %s", &[Arg::Num(v), Arg::Str(name)])));
                }
            }
            if dl != inst.level + item + global + support {
                line(t, &format!("^x7F7F7FLimited to Level {}", pob_lua::tostring_num(dl)));
            }
        } else {
            line(t, &fmt("^x7F7F7FLevel: ^7%d%s", &[Arg::Num(inst.level), Arg::Str(max_level)]));
        }
    }
    let cooldown = |t: &mut Tooltip, lv: Option<&SkillLevel>| {
        if let Some(cd) = lv.and_then(|l| l.cooldown) {
            let mut s = fmt("^x7F7F7FCooldown Time: ^7%.2f sec", &[Arg::Num(cd)]);
            if let Some(u) = lv.and_then(|l| l.stored_uses).filter(|u| *u > 1.0) {
                s.push_str(&fmt(" (%d uses)", &[Arg::Num(u)]));
            }
            line(t, &s);
        }
    };
    if ge.support {
        if let Some(m) = lv.and_then(|l| l.mana_multiplier) {
            line(t, &fmt("^x7F7F7FCost & Reservation Multiplier: ^7%d%%", &[Arg::Num(m + 100.0)]));
        }
        if let Some(r) = reservation(lv) {
            line(t, &format!("^x7F7F7FReservation Override: ^7{r}"));
        }
        cooldown(t, lv);
    } else {
        if let Some(r) = reservation(lv) {
            line(t, &format!("^x7F7F7FReservation: ^7{r}"));
        }
        let mut cost: Option<String> = None;
        for (res, string, div) in costs() {
            if let Some(v) = lv.and_then(|l| l.cost.as_ref()).and_then(|c| c.get(&res)) {
                let n = ((*v / div) * 100.0 + 0.5).floor() / 100.0;
                let s = string.replace("{0}", &fmt("%g", &[Arg::Num(n)]));
                cost = Some(match cost {
                    Some(o) => format!("{o}, {s}"),
                    None => s,
                });
            }
        }
        if let Some(c) = cost {
            line(t, &format!("^x7F7F7FCost: ^7{c}"));
        }
        cooldown(t, lv);
        if let Some(v) = lv.and_then(|l| l.vaal_stored_uses) {
            let soul = lv
                .and_then(|l| l.cost.as_ref())
                .and_then(|c| c.get("Soul"))
                .copied()
                .expect("Classes/GemTooltip.lua:111: attempt to perform arithmetic on field 'Soul'");
            line(
                t,
                &fmt("^x7F7F7FCan Store ^7%d ^x7F7F7FUse (%d Souls)", &[Arg::Num(v), Arg::Num(v * soul)]),
            );
        }
        if let Some(v) = lv.and_then(|l| l.soul_prevention_duration) {
            line(t, &fmt("^x7F7F7FSoul Gain Prevention: ^7%d sec", &[Arg::Num(v)]));
        }
        let attack = inst.gem.is_some_and(|gm| gm.tags.get("attack").copied() == Some(true));
        if attack {
            if let Some(v) = lv.and_then(|l| l.attack_speed_multiplier) {
                line(t, &fmt("^x7F7F7FAttack Speed: ^7%d%% of base", &[Arg::Num(v + 100.0)]));
            }
            if let Some(v) = lv.and_then(|l| l.attack_time) {
                line(t, &fmt("^x7F7F7FAttack Time: ^7%.2f sec", &[Arg::Num(v / 1000.0)]));
            }
            if let Some(v) = lv.and_then(|l| l.base_multiplier) {
                line(t, &fmt("^x7F7F7FAttack Damage: ^7%g%% of base", &[Arg::Num(v * 100.0)]));
            }
        } else {
            let ct = ge
                .cast_time
                .expect("Classes/GemTooltip.lua:134: attempt to compare nil with number");
            if ct > 0.0 {
                line(t, &fmt("^x7F7F7FCast Time: ^7%.2f sec", &[Arg::Num(ct)]));
            } else {
                line(t, "^x7F7F7FCast Time: ^7Instant");
            }
        }
        if let Some(v) = lv.and_then(|l| l.crit_chance) {
            line(t, &fmt("^x7F7F7FCritical Strike Chance: ^7%.2f%%", &[Arg::Num(v)]));
        }
        if let Some(v) = lv.and_then(|l| l.damage_effectiveness) {
            line(t, &fmt("^x7F7F7FEffectiveness of Added Damage: ^7%d%%", &[Arg::Num(v * 100.0)]));
        }
    }
    if add_req {
        if inst.quality > 0.0 {
            line(
                t,
                &fmt("^x7F7F7FQuality: +%s%d%%", &[Arg::Str(&cc("MAGIC")), Arg::Num(inst.quality)]),
            );
        }
        for (v, name) in [
            (d.and_then(|x| x.item_quality).unwrap_or(0.0), "Item"),
            (d.and_then(|x| x.global_quality).unwrap_or(0.0), "Global Modifiers"),
            (d.and_then(|x| x.support_quality).unwrap_or(0.0), "Supports"),
            (d.and_then(|x| x.socket_quality).unwrap_or(0.0), "Socket Colour"),
        ] {
            if v != 0.0 {
                let c = if v > 0.0 { cc("MAGIC") } else { cc("WARNING") };
                line(t, &format!("{c}{}", fmt("%+d%% Quality from %s", &[Arg::Num(v), Arg::Str(name)])));
            }
        }
    }
    t.add_separator(Some(10.0));
    if add_req {
        let gm = inst.gem.expect("Classes/GemTooltip.lua:170: attempt to index field 'gemData'");
        let req_level = ge.level(inst.level).and_then(|l| l.level_requirement).unwrap_or(1.0);
        let req = |m: f64| get_gem_stat_requirement(req_level, ge.support, m);
        if let Some(s) = e.requirements_line(req_level, req(gm.req_str), req(gm.req_dex), req(gm.req_int)) {
            line(t, &s);
            t.add_separator(Some(10.0));
        }
    }
    if let Some(desc) = description(&ge.id) {
        line(t, &format!("{}{desc}", cc("GEM")));
    }
    t.add_separator(Some(10.0));
    let si = SkillInstance {
        level: disp_level.unwrap_or(inst.level),
        quality: d.map_or(Some(inst.quality), |x| x.quality).unwrap_or(0.0),
        actor_level: d.and_then(|x| x.actor_level),
    };
    let mut stats = build_skill_instance_stats(&si, ge).unwrap_or_else(|er| panic!("{er}"));
    if let Some(m) = merge_from {
        for (k, v) in build_skill_instance_stats(&si, m).unwrap_or_else(|er| panic!("{er}")) {
            *stats.entry(k).or_insert(0.0) += v;
        }
    }
    let list: Vec<(String, f64)> = stats.into_iter().collect();
    let scope = ge
        .stat_description_scope
        .as_deref()
        .expect("Modules/StatDescriber.lua:15: attempt to concatenate local 'scopeName' (a nil value)");
    let (lines, map) = describe_stats(&list, scope, false);
    for l in lines {
        let source = map
            .get(&l)
            .and_then(|s| skills.stat_map(ge, s).unwrap_or_else(|er| panic!("{er}")).map(|e| (s, e)));
        if let Some((stat, source)) = source {
            // GemTooltip.lua:190-199
            let l = if crate::launch::launch().dev_mode_alt.get() {
                format!("{l} ^2{}", source_dev_text(stat, &source))
            } else {
                l
            };
            line(t, &format!("{}{l}", cc("MAGIC")));
        } else {
            // GemTooltip.lua:202-204
            let l = if crate::launch::launch().dev_mode_alt.get() {
                format!("{l} ^1{}", map.get(&l).map_or("", String::as_str))
            } else {
                l
            };
            let mut x = format!("{}{l}", cc("UNSUPPORTED"));
            if not_supported_mod_tooltips() {
                x.push_str(" ^8(Not supported in PoB yet)");
            }
            line(t, &x);
        }
    }
}

/// GemTooltip.lua:191-197. `source[1].value = lineMap[line]` writes to the shared map in the Lua; here a copy is formatted, the map stays as it is (dev mode only).
fn source_dev_text(stat: &str, source: &pob_data::skill_stat_map::StatMapEntry) -> String {
    use pob_data::skill_stat_map::{ModElem, SkillMod};
    let Some(first) = source.mods.first() else {
        return stat.to_string();
    };
    match first {
        ModElem::Mod(SkillMod::Mod(m)) => {
            if m.value.truthy() {
                pob_mods::format::format_mod(m)
            } else {
                let mut m = m.clone();
                m.value = pob_mods::Value::str(stat);
                pob_mods::format::format_mod(&m)
            }
        }
        ModElem::Mod(SkillMod::Malformed(_)) => unimplemented!("Classes/GemTooltip.lua:196 formatMod of a malformed mod"),
        ModElem::Group { .. } => unimplemented!("Classes/GemTooltip.lua:196 formatMod of a group of mods"),
    }
}

/// `GemTooltip.AddGemTooltip(tooltip, build, gemInstance, options)` (GemTooltip.lua:219-265).
pub fn add_gem_tooltip(t: &mut Tooltip, g: &Gfx, e: &Engine, inst: &GemInst<'_>) {
    let flavour = show_flavour_text();
    let (big, title) = if flavour { (18.0, 24.0) } else { (16.0, 20.0) };
    t.center = true;
    t.color = TooltipColor::Escape(cc("GEM"));
    t.max_width = Some(600.0);
    t.tooltip_header = Some("GEM".to_string());
    let skills = pob_data::skills::skills();
    let primary = inst.effect;
    let secondary = inst
        .gem
        .and_then(|x| x.secondary_granted_effect_id.as_deref())
        .and_then(|i| skills.get(i));
    let add = |t: &mut Tooltip, size: f64, s: &str| t.add_line_ex(g, size, Some(s), Some("FONTIN SC"), false, None);
    let gem_col = cc("GEM");
    let sec_name = inst.gem.and_then(|x| x.secondary_effect_name.clone());
    if let Some(sec) = secondary.filter(|s| !s.support || sec_name.is_some()) {
        // `gemData.VaalGem` is not a field of the data (`vaalGem` is): always the primary first
        add(t, title, &format!("{gem_col}{}", primary.name));
        t.add_separator(Some(10.0));
        let tag = inst.gem.map_or("", |x| x.tag_string.as_str());
        add(t, big, &format!("^x7F7F7F{tag}"));
        add_common(t, g, e, &skills, inst, primary, true, !inst.skip_requirements, None);
        t.add_separator(Some(10.0));
        add(t, title, &format!("{gem_col}{}", sec_name.as_deref().unwrap_or(&sec.name)));
        t.add_separator(Some(10.0));
        add_common(t, g, e, &skills, inst, sec, false, false, None);
    } else {
        add(t, title, &format!("{gem_col}{}", primary.name));
        t.add_separator(Some(10.0));
        if primary.legacy {
            add(t, title, &format!("{}Legacy Gem", cc("WARNING")));
            add(t, big, &format!("{}Gem only exists in Standard League", cc("WARNING")));
            t.add_separator(Some(10.0));
        }
        if let Some(gm) = inst.gem {
            add(t, big, &format!("^x7F7F7F{}", gm.tag_string));
        }
        add_common(
            t,
            g,
            e,
            &skills,
            inst,
            primary,
            true,
            !inst.skip_requirements,
            secondary.filter(|s| s.support),
        );
    }
    if flavour && let Some(text) = flavour_text(&primary.id) {
        t.add_separator(Some(10.0));
        for l in text {
            t.add_line_ex(g, big, Some(&format!("{}{l}", cc("UNIQUE"))), Some("FONTIN SC ITALIC"), false, None);
        }
    }
}

/// `string.format("%s%s ^7%d%s/%d%s", ...)` of one effect (SkillsTab.lua:1332-1339).
fn effect_line(e: &Engine, skills: &Skills, fx: &SkillEffect) -> String {
    let ge = &skills.effects[fx.granted_effect];
    let src = fx.src_instance.and_then(|(g, k)| {
        e.result()
            .env
            .skills
            .as_ref()
            .and_then(|s| s.tab.socket_group_list().get(g).and_then(|x| x.gem_list.get(k)))
    });
    let level = fx.level.unwrap_or(0.0);
    let quality = fx.quality.unwrap_or(0.0);
    let bonus = |cur: f64, base: f64| {
        if cur > base {
            format!("{}+{}^7", cc("MAGIC"), pob_lua::tostring_num(cur - base))
        } else {
            String::new()
        }
    };
    let (show_l, extra_l, show_q, extra_q) = match src {
        Some(s) => {
            let (l, q) = (s.level.unwrap_or(0.0), s.quality.unwrap_or(0.0));
            (l, bonus(level, l), q, bonus(quality, q))
        }
        None => (level, String::new(), quality, String::new()),
    };
    fmt(
        "%s%s ^7%d%s/%d%s",
        &[
            Arg::Str(&skill_color(ge.color)),
            Arg::Str(&ge.name),
            Arg::Num(show_l),
            Arg::Str(&extra_l),
            Arg::Num(show_q),
            Arg::Str(&extra_q),
        ],
    )
}

/// `SkillsTab:AddSocketGroupTooltip(tooltip, socketGroup)` (SkillsTab.lua:1310-1395) of group `index` (1-based) of the live list.
#[allow(clippy::too_many_lines)]
pub fn add_socket_group_tooltip(t: &mut Tooltip, g: &Gfx, e: &Engine, index: usize) {
    let groups = e.socket_groups();
    let Some(group) = groups.get(index - 1) else { return };
    let env = &e.result().env;
    let Some(st) = env.skills.as_ref() else { return };
    let skills = &st.skills;
    let source_name = |src: &pob_calc::skills_tab::ExplodeSource| -> (String, String) {
        match src {
            pob_calc::skills_tab::ExplodeSource::Node(id) => {
                let n = env.spec.node(*id).map(|n| n.tree_node());
                (
                    "NORMAL".to_string(),
                    n.map_or_else(|| "???".to_string(), |n| n.name.clone().unwrap_or_else(|| n.dn.clone())),
                )
            }
            pob_calc::skills_tab::ExplodeSource::Item(i) => (i.0.rarity.clone(), i.0.name.clone()),
        }
    };
    if let Some(list) = &group.explode_sources {
        for s in list {
            let (rarity, name) = source_name(s);
            t.add_line(g, 18.0, Some(&format!("^7Source: {}{name}", cc(&rarity))));
        }
        return;
    }
    if group.enabled && group.slot_enabled != Some(true) {
        t.add_line(
            g,
            16.0,
            Some("^7Note: this group is disabled because it is socketed in the inactive weapon set."),
        );
    }
    if let Some(i) = &group.source_item {
        t.add_line(g, 18.0, Some(&format!("^7Source: {}{}", cc(&i.0.rarity), i.0.name)));
        t.add_separator(Some(10.0));
    } else if let Some(id) = group.source_node {
        let n = env.spec.node(id).map(|n| n.tree_node());
        let name = n.map_or_else(String::new, |n| n.name.clone().unwrap_or_else(|| n.dn.clone()));
        t.add_line(g, 18.0, Some(&format!("^7Source: {}{name}", cc("NORMAL"))));
        t.add_separator(Some(10.0));
    }
    let mut shown: Vec<(usize, usize)> = Vec::new();
    let list = group.display_skill_list.as_deref().unwrap_or(&[]);
    for (i, &si) in list.iter().enumerate() {
        let n = i + 1;
        if n > 1 {
            t.add_separator(Some(10.0));
        }
        t.add_line(g, 16.0, Some(&format!("^7Active Skill #{n}:")));
        let skill = st.skill(si);
        for fx in &skill.effect_list {
            let fx = fx.borrow();
            t.add_line(g, 20.0, Some(&effect_line(e, skills, &fx)));
            if let Some(s) = fx.src_instance {
                shown.push(s);
            }
        }
        if let Some(minion) = skill.minion.as_deref()
            && let Some(ms) = minion.main_skill.and_then(|m| minion.active_skill_list.get(m))
        {
            t.add_separator(Some(10.0));
            t.add_line(g, 16.0, Some(&format!("^7Active Skill #{n}'s Main Minion Skill:")));
            if let Some(fx) = ms.effect_list.first() {
                let fx = fx.borrow();
                t.add_line(g, 20.0, Some(&effect_line(e, skills, &fx)));
                if let Some(s) = fx.src_instance {
                    shown.push(s);
                }
            }
        }
    }
    let run_groups = st.tab.socket_group_list();
    let own: Vec<(usize, usize)> = (0..group.gem_list.len()).map(|k| (index - 1, k)).collect();
    let display: &[(usize, usize)] = group.display_gem_list.as_deref().unwrap_or(&own);
    let mut header = true;
    for &(gi, k) in display {
        if shown.contains(&(gi, k)) {
            continue;
        }
        let Some(gem) = run_groups
            .get(gi)
            .and_then(|x| x.gem_list.get(k))
            .or_else(|| groups.get(gi).and_then(|x| x.gem_list.get(k)))
        else {
            continue;
        };
        if header {
            header = false;
            t.add_separator(Some(10.0));
            t.add_line(g, 16.0, Some("^7Inactive Gems:"));
        }
        let data = pob_data::gems::gems();
        let gd = gem.gem_data.as_ref().and_then(|id| data.gems.0.get(id));
        let ge = gd
            .and_then(|d| skills.get(&d.granted_effect_id))
            .or_else(|| gem.granted_effect.as_ref().and_then(|id| skills.get(id)));
        let de = gem.display_effect.as_ref().map(|d| d.borrow().clone());
        let reason = match ge {
            None => "(Unsupported)",
            Some(_) if !gem.enabled => "(Disabled)",
            Some(_) if !group.enabled || group.slot_enabled != Some(true) => "",
            Some(x) if x.support => {
                let superseded = de.as_ref().is_some_and(|d| d.superseded == Some(true));
                let no_target = de.as_ref().is_none_or(|d| d.is_supporting.as_ref().is_none_or(Vec::is_empty));
                if superseded {
                    "(Superseded)"
                } else if no_target && !list.is_empty() {
                    "(Cannot apply to any of the active skills)"
                } else {
                    ""
                }
            }
            Some(_) => "",
        };
        let name = gem
            .granted_effect
            .as_ref()
            .and_then(|id| skills.get(id))
            .map(|x| x.name.clone())
            .or_else(|| gd.map(|d| d.name.clone()))
            .or_else(|| gem.name_spec.clone())
            .unwrap_or_default();
        let (gl, gq) = (gem.level.unwrap_or(0.0), gem.quality.unwrap_or(0.0));
        let (dl, dq) = de.as_ref().map_or((gl, gq), |d| (d.level.unwrap_or(0.0), d.quality.unwrap_or(0.0)));
        // `displayEffect.srcInstance` is this very gem (CalcSetup.lua:1712, :1790); a kept effect's indices are of an older list
        let src = de.as_ref().and_then(|d| d.src_instance).map(|_| gem);
        let bonus = |cur: f64, base: f64| {
            if cur > base {
                format!("{}+{}^7", cc("MAGIC"), pob_lua::tostring_num(cur - base))
            } else {
                String::new()
            }
        };
        t.add_line(
            g,
            20.0,
            Some(&fmt(
                "%s%s ^7%d%s/%d%s %s",
                &[
                    Arg::Str(gem.color.as_deref().unwrap_or("")),
                    Arg::Str(&name),
                    Arg::Num(src.and_then(|s| s.level).unwrap_or(dl)),
                    Arg::Str(&bonus(dl, gl)),
                    Arg::Num(src.and_then(|s| s.quality).unwrap_or(dq)),
                    Arg::Str(&bonus(dq, gq)),
                    Arg::Str(reason),
                ],
            )),
        );
    }
}
