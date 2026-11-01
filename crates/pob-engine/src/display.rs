//! The sidebar stats (R8.3): Build.lua `FormatStat` (:1718), `GetStatBreakdownKey` (:1871), `AddDisplayStatList` (:1913),
//! `InsertItemWarnings` (:2061), `RefreshStatList` (:2078, the `statBox.list` / `warnings.lines` part, with the warnings of
//! `EstimatePlayerProgress` :925-927), and the structure of `engine_displayStatsJSON` (pob-engine-wasm `boot.lua:546-788`: sections of
//! `{ label, value, color, valueColor, statKey }` rows). Number formatting is Common.lua `formatNumSep` with the `main.*` settings
//! ([`DisplaySettings`], the defaults of Main.lua:108-112 = headless).
//!
//! What upstream mutates in place is kept here as a local copy: `table.sort(actor.output.SkillDPS)` (Build.lua:1938, boot.lua:150) sorts
//! the output table itself, so the second sort (the structure, after `RefreshStatList`) starts from the first one's order
//! ([`crate::lua::lua_sort`] = LuaJIT's `auxsort`).
//!
//! Not ported here: the breakdown tables `GetStatBreakdownKey` looks into (`env.player.breakdown`, R8.4/R8.5) -- see [`BreakdownKey`].

use std::cell::Ref;

use pob_calc::breakdown::{BVal, Breakdown};
use pob_calc::{ActorState, CalculateResult, FullDpsSkill, OutTable};
use pob_lua::{Arg, Repl};

use crate::lua::{LuaV, TableRef, eq, ge, gt};
use crate::stats::{DISPLAY_STATS, DisplayStat, MINION_DISPLAY_STATS, Suffix, color};
use crate::view::ActorView;

/// `main.showThousandsSeparators` / `thousandsSeparator` / `decimalSeparator` / `useCompactValues` (Main.lua:108-112 defaults).
#[derive(Clone, Debug, PartialEq)]
pub struct DisplaySettings {
    pub show_thousands_separators: bool,
    pub thousands_separator: String,
    pub decimal_separator: String,
    pub use_compact_values: bool,
}

impl Default for DisplaySettings {
    fn default() -> DisplaySettings {
        DisplaySettings {
            show_thousands_separators: true,
            thousands_separator: ",".into(),
            decimal_separator: ".".into(),
            use_compact_values: false,
        }
    }
}

// --- Common.lua / SimpleGraphic helpers -------------------------------------------------------------------------------------------

fn gsub_str(s: &[u8], pat: &str, repl: &[u8]) -> Vec<u8> {
    pob_lua::gsub(s, pat.as_bytes(), Repl::Str(repl), None).expect("pattern").0
}

impl DisplaySettings {
    /// The separator settings `formatNumSep` reads.
    pub fn number_format(&self) -> pob_calc::power::NumberFormat {
        pob_calc::power::NumberFormat {
            show_thousands_separators: self.show_thousands_separators,
            thousands_separator: self.thousands_separator.clone(),
            decimal_separator: self.decimal_separator.clone(),
        }
    }
}

/// Common.lua:803-829 `formatNumSep(str)` -- the one implementation is [`pob_calc::power::format_num_sep`] (node power and the offence
/// strings use it too).
pub fn format_num_sep(s: &str, st: &DisplaySettings) -> String {
    pob_calc::power::format_num_sep(s, &st.number_format())
}

/// `_SimpleGraphic.def.lua:295-298` `StripEscapes(text)`.
pub fn strip_escapes(s: &str) -> String {
    let a = gsub_str(s.as_bytes(), "%^%d", b"");
    String::from_utf8(gsub_str(&a, "%^x%x%x%x%x%x%x", b"")).expect("UTF-8")
}

/// `Data/Global.lua:89-100` `hexToRGB(hex)`.
pub fn hex_to_rgb(hex: &str) -> Option<[f64; 3]> {
    let h = hex.replace("^x", "").replace("0x", "").replace('#', "");
    if h.len() != 6 {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map(|v| f64::from(v) / 255.0);
    Some([c(0).ok()?, c(2).ok()?, c(4).ok()?])
}

/// Build.lua:38-57 `matchFlags(reqFlags, notFlags, flags)` (one string each in the display data).
pub fn match_flags(flag: Option<&str>, not_flag: Option<&str>, flags: &indexmap::IndexMap<String, bool>) -> bool {
    let set = |f: &str| flags.get(f).copied().unwrap_or(false);
    if flag.is_some_and(|f| !set(f)) {
        return false;
    }
    !not_flag.is_some_and(set)
}

/// The `fmt`/`pc`/`mod`/`label`/`chaosInoc`/`compactValue` of a stat as `FormatStat` reads them.
pub struct FormatSpec<'s> {
    pub fmt: Option<&'s str>,
    pub pc: bool,
    pub mod_: bool,
    pub label: Option<&'s str>,
    pub chaos_inoc: bool,
    pub compact_value: bool,
}

impl<'s> FormatSpec<'s> {
    pub fn of(d: &'s DisplayStat) -> FormatSpec<'s> {
        FormatSpec {
            fmt: d.fmt,
            pc: d.pc,
            mod_: d.mod_,
            label: d.label,
            chaos_inoc: d.chaos_inoc,
            compact_value: d.compact_value,
        }
    }
}

fn s_format(fmt: &str, v: f64) -> Result<String, String> {
    pob_lua::format(fmt, &[Arg::Num(v)])
}

/// Build.lua:1718-1756 `buildMode:FormatStat(statData, statVal, overCapStatVal, colorOverride)` (the `self.last*` UI bookkeeping is
/// not state the output depends on).
pub fn format_stat(
    sd: &FormatSpec<'_>,
    stat_val: &LuaV<'_>,
    over_cap: Option<&LuaV<'_>>,
    color_override: Option<&str>,
    st: &DisplaySettings,
) -> Result<String, String> {
    if matches!(stat_val, LuaV::Table(_)) {
        return Ok(String::new());
    }
    let x = crate::lua::arith(stat_val)?;
    let val = x * (if sd.pc || sd.mod_ { 100.0 } else { 1.0 }) - (if sd.mod_ { 100.0 } else { 0.0 });
    let mut color: &str = match color_override {
        Some(c) => c,
        None => {
            if ge(stat_val, &LuaV::Num(0.0))? {
                "^7"
            } else if sd.chaos_inoc {
                "^8"
            } else {
                color::NEGATIVE
            }
        }
    };
    if sd.label == Some("Unreserved Life") && eq(stat_val, &LuaV::Num(0.0)) {
        color = color::NEGATIVE;
    }
    let mut val_str = None;
    if sd.compact_value && st.use_compact_values && val != f64::INFINITY && val != f64::NEG_INFINITY {
        let abs = val.abs();
        if abs >= 1_000_000_000.0 {
            val_str = Some(s_format("%.1fB", val / 1_000_000_000.0)?);
        } else if abs >= 1_000_000.0 {
            val_str = Some(s_format("%.1fM", val / 1_000_000.0)?);
        } else if abs >= 10000.0 {
            val_str = Some(s_format("%.1fK", val / 1000.0)?);
        }
    }
    let val_str = match val_str {
        Some(v) => v,
        None => {
            let fmt = sd
                .fmt
                .ok_or_else(|| "attempt to concatenate field 'fmt' (a nil value)".to_owned())?;
            let v = s_format(&format!("%{fmt}"), val)?;
            let vb = v.as_bytes();
            match pob_lua::match_(vb, b"^([%+%-]?%d+%.%d+)(%D*)$", None).expect("pattern") {
                Some(c) => {
                    let number = c.get(0).and_then(|x| x.bytes(vb)).unwrap_or(b"");
                    let suffix = c.get(1).and_then(|x| x.bytes(vb)).unwrap_or(b"");
                    let mut t = gsub_str(&gsub_str(number, "0+$", b""), "%.$", b"");
                    t.extend_from_slice(suffix);
                    String::from_utf8(t).expect("UTF-8")
                }
                None => v,
            }
        }
    };
    let mut out = format!("{color}{}", format_num_sep(&val_str, st));
    if let Some(oc) = over_cap
        && gt(oc, &LuaV::Num(0.0))?
    {
        let ocv = crate::lua::arith(oc)?;
        out.push_str(&format!("^x808080 (+{}%)", s_format("%d", ocv)?));
    }
    Ok(out)
}

// --- the statBox list ------------------------------------------------------------------------------------------------------------

/// What `GetStatBreakdownKey` (Build.lua:1871-1903) returns.
#[derive(Clone, Debug, PartialEq)]
pub enum BreakdownKey {
    /// nil: no breakdown popup, the value is not underlined.
    None,
    /// The key: `statData.breakdown`, `<stat>.<childStat>` or `statData.stat`.
    Key(String),
}

/// `{ height, align, x, [1], [2], breakdown, modNames, underline, actorName, ignoredSections, breakdownColour }` -- one entry of
/// `controls.statBox.list`.
#[derive(Clone, Debug, Default)]
pub struct StatBoxEntry {
    pub height: f64,
    pub align: Option<&'static str>,
    pub x: Option<f64>,
    /// `[1]`
    pub text: Option<String>,
    /// `[2]`
    pub value: Option<String>,
    pub breakdown: Option<BreakdownKey>,
    pub mod_names: Option<&'static [&'static str]>,
    /// `underline = { false, not not breakdown }`.
    pub underline: Option<[Option<bool>; 2]>,
    pub actor_name: Option<&'static str>,
    /// The keys of the `ignoredSections` set, in the data's order (upstream: a set).
    pub ignored_sections: Option<&'static [&'static str]>,
    pub breakdown_colour: Option<[f64; 3]>,
}

impl StatBoxEntry {
    fn sep(height: f64) -> StatBoxEntry {
        StatBoxEntry {
            height,
            ..StatBoxEntry::default()
        }
    }

    fn line(height: f64, text: String) -> StatBoxEntry {
        StatBoxEntry {
            height,
            text: Some(text),
            ..StatBoxEntry::default()
        }
    }

    /// `{ height = 14, align = "CENTER_X", x = 140, text }`
    fn centered(text: Option<String>) -> StatBoxEntry {
        StatBoxEntry {
            height: 14.0,
            align: Some("CENTER_X"),
            x: Some(140.0),
            text,
            ..StatBoxEntry::default()
        }
    }
}

/// The result of `RefreshStatList`.
#[derive(Clone, Debug, Default)]
pub struct StatList {
    /// `controls.statBox.list`.
    pub stat_box: Vec<StatBoxEntry>,
    /// `controls.warnings.lines`, in insertion order (the pool warnings walk a 4-key / 2-key table with `pairs`, MINEFIELD-RUST R13:
    /// LuaJIT's order of those lines varies between processes; here the order of the table constructor).
    pub warnings: Vec<String>,
}

/// One actor as the sidebar code sees it (`mainEnv.player` / `mainEnv.minion`).
pub struct DisplayActor<'a> {
    pub name: &'static str,
    pub view: ActorView<'a>,
    /// `actor.mainSkill.skillFlags`.
    pub skill_flags: &'a indexmap::IndexMap<String, bool>,
    /// `actor.mainSkill.infoMessage` / `infoMessage2` / `disableReason`.
    pub info_message: Option<&'a str>,
    pub info_message2: Option<&'a str>,
    pub disable_reason: Option<&'a str>,
    /// `actor.modDB`.
    pub mod_db: pob_mods::StoreRef,
    /// `actor.output.SkillDPS` as the in-place sorts leave it (`None` = nil).
    pub skill_dps: Option<Vec<FullDpsSkill>>,
    /// `env.<actor>.breakdown` of the MAIN env (`None` = nil).
    pub breakdown: Option<&'a Breakdown>,
}

/// `build.calcsTab.mainEnv` for the sidebar: the player, the minion (`mainEnv.minion`), the env.
pub struct DisplayEnv<'a> {
    pub res: &'a CalculateResult,
    pub player: DisplayActor<'a>,
    pub minion: Option<DisplayActor<'a>>,
}

/// Holds the minion's perform state borrowed for the lifetime of a [`DisplayEnv`].
pub struct MinionBorrow<'a>(Option<Ref<'a, ActorState>>);

impl<'a> MinionBorrow<'a> {
    pub fn new(res: &'a CalculateResult) -> MinionBorrow<'a> {
        MinionBorrow(res.perform.minion.as_ref().map(|m| m.borrow()))
    }
}

fn skill_dps_of(st: &ActorState) -> Option<Vec<FullDpsSkill>> {
    match st.output.table("SkillDPS") {
        Some(OutTable::SkillDps(v)) => Some(v.clone()),
        _ => None,
    }
}

impl<'a> DisplayEnv<'a> {
    /// The MAIN env of `res` (the skill phase must have run: `mainSkill` is read).
    pub fn new(res: &'a CalculateResult, minion: &'a MinionBorrow<'a>) -> Result<DisplayEnv<'a>, String> {
        let env = &res.env;
        let skills = env
            .skills
            .as_ref()
            .ok_or("display: env.player.mainSkill needs the skill phase (CalculateOptions.skills)")?;
        let main = skills.skill(skills.main_skill.ok_or("display: env.player.mainSkill is nil")?);
        let player = DisplayActor {
            name: "player",
            view: ActorView::new(&res.perform.player, env),
            skill_flags: &main.skill_flags,
            info_message: main.info_message.as_deref(),
            info_message2: main.info_message2.as_deref(),
            disable_reason: main.disable_reason.as_deref(),
            mod_db: env.player.borrow().mod_db.clone(),
            skill_dps: skill_dps_of(&res.perform.player),
            breakdown: res.perform.player.breakdown.as_ref(),
        };
        // `mainEnv.minion` = `env.player.mainSkill.minion` (CalcPerform.lua:1316)
        let minion = match (env.minion.as_ref(), minion.0.as_deref()) {
            (Some(actor), Some(st)) => {
                let m = main
                    .minion
                    .as_deref()
                    .ok_or("display: env.minion without env.player.mainSkill.minion")?;
                let ms = m.main();
                Some(DisplayActor {
                    name: "minion",
                    view: ActorView::new(st, env),
                    skill_flags: &ms.skill_flags,
                    info_message: ms.info_message.as_deref(),
                    info_message2: ms.info_message2.as_deref(),
                    disable_reason: ms.disable_reason.as_deref(),
                    mod_db: actor.borrow().mod_db.clone(),
                    skill_dps: skill_dps_of(st),
                    breakdown: st.breakdown.as_ref(),
                })
            }
            (None, _) => None,
            (Some(_), None) => return Err("display: env.minion is set but perform left no minion output".into()),
        };
        Ok(DisplayEnv { res, player, minion })
    }
}

/// Build.lua:24-30 `InsertIfNew(t, val)`.
fn insert_if_new(t: &mut Vec<String>, v: &str) {
    if !t.iter().any(|x| x == v) {
        t.push(v.to_owned());
    }
}

/// `table.sort(SkillDPS, function(a,b) return (a.dps * a.count) > (b.dps * b.count) end)`.
fn sort_skill_dps(v: &mut [FullDpsSkill]) {
    crate::lua::lua_sort(v, &mut |a, b| (a.dps * a.count) > (b.dps * b.count));
}

/// Build.lua:1871-1903 `GetStatBreakdownKey(statData, actorName)` over `env.<actor>.breakdown` (`bd`, CalcPerform.lua:1518/:1520 in MAIN;
/// filled by perform / defence / estimations / offence / triggers, R8.4/R8.5).
fn stat_breakdown_key(sd: &DisplayStat, bd: Option<&Breakdown>) -> BreakdownKey {
    // :1874-1877
    let Some(bd) = bd else { return BreakdownKey::None };
    let truthy = |v: Option<&BVal>| !matches!(v, None | Some(BVal::Bool(false)));
    // `t[k]` of a breakdown value that is a table (a plain table or a nested module); anything else answers nil here
    let field = |v: Option<&BVal>, k: &str| -> Option<BVal> {
        match v {
            Some(BVal::Table(t)) => t.fields.get(k).cloned(),
            Some(BVal::Module(m)) => m.get(k).cloned(),
            _ => None,
        }
    };
    let Some(stat) = sd.stat else { return BreakdownKey::None };
    // :1878-1888
    let key: Option<String> = if let Some(b) = sd.breakdown {
        Some(b.to_owned())
    } else if let Some(c) = sd.child_stat {
        let parent = bd.get(stat);
        (truthy(parent) && truthy(field(parent, c).as_ref())).then(|| format!("{stat}.{c}"))
    } else if truthy(bd.get(stat)) {
        Some(stat.to_owned())
    } else {
        None
    };
    let Some(key) = key else { return BreakdownKey::None };
    // :1891-1893 "Modifier-only popups do not require a calculated breakdown"
    if sd.mod_names.is_some() {
        return BreakdownKey::Key(key);
    }
    // :1894-1902 `key:match("^(%a+)%.(%a+)$")`
    let ns_name = key.split_once('.').filter(|(ns, name)| {
        !ns.is_empty() && !name.is_empty() && ns.bytes().all(|b| b.is_ascii_alphabetic()) && name.bytes().all(|b| b.is_ascii_alphabetic())
    });
    // `namespace and breakdown[namespace] and breakdown[namespace][name] or breakdown[key]`
    let data: Option<BVal> = match ns_name {
        Some((ns, name)) if truthy(bd.get(ns)) && truthy(field(bd.get(ns), name).as_ref()) => field(bd.get(ns), name),
        _ => bd.get(&key).cloned(),
    };
    match data {
        Some(d) if Breakdown::is_populated(&d) => BreakdownKey::Key(key),
        _ => BreakdownKey::None,
    }
}

fn lua_err(line: u32, e: String) -> String {
    format!("Modules/Build.lua:{line}: {e}")
}

fn suffix_text<'a>(sd: &DisplayStat, v: &LuaV<'a>, o: &ActorView<'a>) -> Result<Option<String>, String> {
    let Some(sfx) = sd.suffix else { return Ok(None) };
    if let Some(c) = sd.suffix_cond_func
        && !c(v, o)?.truthy()
    {
        return Ok(None);
    }
    let r = match sfx {
        Suffix::Str(s) => LuaV::str(s),
        Suffix::Fn(f) => f(v, o)?,
    };
    match r {
        LuaV::Nil | LuaV::Bool(false) => Ok(None),
        LuaV::Str(s) => Ok(Some(s.to_string())),
        LuaV::Num(x) => Ok(Some(pob_lua::tostring_num(x))),
        other => Err(format!("attempt to concatenate a {} value", other.type_name())),
    }
}

/// `actor.output[statData.stat]`, then `[statData.childStat]` (Build.lua:1923-1927 / boot.lua:144-145).
fn stat_value<'a>(sd: &DisplayStat, o: &ActorView<'a>) -> Result<LuaV<'a>, String> {
    let v = o.get(sd.stat.expect("stat row"));
    match sd.child_stat {
        Some(c) if v.truthy() => v.index(c),
        _ => Ok(v),
    }
}

/// `statVal and ((statData.condFunc and statData.condFunc(statVal, actor.output)) or (not statData.condFunc and statVal ~= 0))`.
fn row_shown<'a>(sd: &DisplayStat, v: &LuaV<'a>, o: &ActorView<'a>) -> Result<bool, String> {
    if !v.truthy() {
        return Ok(false);
    }
    Ok(match sd.cond_func {
        Some(c) => c(v, o)?.truthy(),
        None => crate::lua::ne(v, &LuaV::Num(0.0)),
    })
}

/// `actor.output[statData.overCapStat] or nil`, cleared by `overCapStatCondFunc` (Build.lua:1928-1931).
fn over_cap_value<'a>(sd: &DisplayStat, v: &LuaV<'a>, o: &ActorView<'a>) -> Result<Option<LuaV<'a>>, String> {
    let Some(k) = sd.over_cap_stat else { return Ok(None) };
    let oc = o.get(k);
    if !oc.truthy() {
        return Ok(None);
    }
    if let Some(c) = sd.over_cap_stat_cond_func
        && !c(v, o)?.truthy()
    {
        return Ok(None);
    }
    Ok(Some(oc))
}

/// `actor.output[statData.stat.."Warning"] or (statData.warnFunc and statData.warnFunc(statVal, actor.output) and statData.warnColor)`.
fn color_override<'a>(sd: &DisplayStat, v: &LuaV<'a>, o: &ActorView<'a>) -> Result<Option<&'static str>, String> {
    let warn_key = format!("{}Warning", sd.stat.expect("stat row"));
    if o.get(&warn_key).truthy() {
        return Ok(Some(color::NEGATIVE));
    }
    if let Some(w) = sd.warn_func
        && w(v, o)?.truthy()
        && sd.warn_color
    {
        return Ok(Some(color::NEGATIVE));
    }
    Ok(None)
}

/// Build.lua:1913-2059 `buildMode:AddDisplayStatList(statList, actor, actorName)`.
fn add_display_stat_list(
    out: &mut StatList,
    list: &[DisplayStat],
    actor: &mut DisplayActor<'_>,
    st: &DisplaySettings,
) -> Result<(), String> {
    let o = &actor.view;
    for sd in list {
        if !match_flags(sd.flag, sd.not_flag, actor.skill_flags) {
            continue;
        }
        let label_color = sd.color.unwrap_or("^7");
        if sd.stat.is_some() {
            let v = stat_value(sd, o).map_err(|e| lua_err(1925, e))?;
            if row_shown(sd, &v, o).map_err(|e| lua_err(1927, e))? {
                let over_cap = over_cap_value(sd, &v, o).map_err(|e| lua_err(1928, e))?;
                if sd.stat == Some("SkillDPS") {
                    // :1932-1963
                    let label_color = color::CUSTOM;
                    let list = actor
                        .skill_dps
                        .as_mut()
                        .ok_or_else(|| lua_err(1934, "attempt to sort a non-table output.SkillDPS".into()))?;
                    sort_skill_dps(list);
                    for sk in list.iter() {
                        let trigger = match sk.trigger.as_deref() {
                            Some(t) if !t.is_empty() => format!("{} ({t}){label_color}", color::WARNING),
                            _ => String::new(),
                        };
                        let lhs = if sk.count >= 2.0 {
                            format!("{label_color}{}x {}{trigger}:", pob_lua::tostring_num(sk.count), sk.name)
                        } else {
                            format!("{label_color}{}{trigger}:", sk.name)
                        };
                        let spec = FormatSpec {
                            fmt: Some(".1f"),
                            compact_value: sd.compact_value,
                            pc: false,
                            mod_: false,
                            label: None,
                            chaos_inoc: false,
                        };
                        let value =
                            format_stat(&spec, &LuaV::Num(sk.dps * sk.count), over_cap.as_ref(), None, st).map_err(|e| lua_err(1947, e))?;
                        out.stat_box.push(StatBoxEntry {
                            height: 16.0,
                            text: Some(lhs),
                            value: Some(value),
                            ..StatBoxEntry::default()
                        });
                        if let Some(part) = &sk.skill_part {
                            out.stat_box.push(StatBoxEntry::centered(Some(format!("^8{part}"))));
                        }
                        if let Some(src) = &sk.source {
                            out.stat_box
                                .push(StatBoxEntry::centered(Some(format!("{}from {src}", color::WARNING))));
                        }
                    }
                } else if !sd.hide_stat {
                    // :1964-2005
                    let co = color_override(sd, &v, o).map_err(|e| lua_err(1967, e))?;
                    let mut formatted = format_stat(&FormatSpec::of(sd), &v, over_cap.as_ref(), co, st).map_err(|e| lua_err(1970, e))?;
                    if let Some(sfx) = suffix_text(sd, &v, o).map_err(|e| lua_err(1971, e))? {
                        formatted.push_str(&format!("^x808080 ({sfx})"));
                    }
                    let breakdown = stat_breakdown_key(sd, actor.breakdown);
                    let underline = match breakdown {
                        BreakdownKey::None => Some(false),
                        BreakdownKey::Key(_) => Some(true),
                    };
                    // :1989-1993 `labelColor ~= "^7" and hexToRGB(labelColor) or colourCodeMap[labelColor]`
                    let colour_code_map = |c: &str| match c {
                        "^1" => Some([0.917, 0.0, 0.0]),
                        "^7" => Some([1.0, 1.0, 1.0]),
                        _ => None,
                    };
                    let breakdown_colour = if label_color != "^7" {
                        hex_to_rgb(label_color).or_else(|| colour_code_map(label_color))
                    } else {
                        colour_code_map(label_color)
                    };
                    out.stat_box.push(StatBoxEntry {
                        height: 16.0,
                        text: Some(format!("{label_color}{}:", sd.label.expect("label"))),
                        value: Some(formatted),
                        breakdown: Some(breakdown),
                        mod_names: sd.mod_names,
                        underline: Some([Some(false), underline]),
                        actor_name: Some(actor.name),
                        ignored_sections: sd.ignored_sections,
                        breakdown_colour,
                        ..StatBoxEntry::default()
                    });
                }
            }
            // :2007-2012
            if let Some(w) = sd.warn_func
                && v.truthy()
            {
                let gate = match sd.cond_func {
                    Some(c) => c(&v, o).map_err(|e| lua_err(2007, e))?.truthy(),
                    None => true,
                };
                if gate {
                    match w(&v, o).map_err(|e| lua_err(2008, e))? {
                        LuaV::Str(s) => insert_if_new(&mut out.warnings, &s),
                        LuaV::Nil | LuaV::Bool(false) => {}
                        other => return Err(lua_err(2010, format!("display: a warnFunc returned a {} value", other.type_name()))),
                    }
                }
            }
        } else if sd.label.is_some() && sd.cond_func.is_some() {
            // :2013-2016 `statData.condFunc(actor.output)`: a label-only row (`labelStat` / `val`); BuildDisplayStats.lua has none.
            return Err("Modules/Build.lua:2013 label-only display row (condFunc(actor.output) / labelStat): unimplemented, no such row in BuildDisplayStats.lua".into());
        } else if out.stat_box.last().is_none_or(|e| e.text.is_some()) {
            // :2017-2018
            out.stat_box.push(StatBoxEntry::sep(6.0));
        }
    }
    // :2022-2031 pairs({ Life, Mana, Rage, Energy Shield }) -- R13: order of the constructor
    for (pool, flag) in [
        ("Life", "LifeCostWarningList"),
        ("Mana", "ManaCostWarningList"),
        ("Rage", "RageCostWarningList"),
        ("Energy Shield", "ESCostWarningList"),
    ] {
        if let Some(names) = warning_list(o, flag) {
            let what = if o.get("EnergyShieldProtectsMana").truthy() && pool == "Mana" {
                "Energy Shield and Mana"
            } else {
                pool
            };
            let mut line = format!("You do not have enough {what} to use: ");
            for sk in names {
                line.push_str(sk);
                line.push_str(", ");
            }
            line.truncate(line.len() - 2); // `line:sub(1, -3)`
            insert_if_new(&mut out.warnings, &line);
        }
    }
    // :2032-2041
    for (pool, flag) in [
        ("Unreserved life", "LifePercentCostPercentCostWarningList"),
        ("Unreserved Mana", "ManaPercentCostPercentCostWarningList"),
    ] {
        if let Some(names) = warning_list(o, flag) {
            let mut line = format!("You do not have enough {pool}% to use: ");
            for sk in names {
                line.push_str(sk);
                line.push_str(", ");
            }
            line.truncate(line.len() - 2);
            insert_if_new(&mut out.warnings, &line);
        }
    }
    // :2042-2047
    if o.get("VixensTooMuchCastSpeedWarn").truthy() {
        insert_if_new(
            &mut out.warnings,
            "You may have too much cast speed or too little cooldown reduction to effectively use Vixen's Curse replacement",
        );
    }
    if o.get("VixenModeNoVixenGlovesWarn").truthy() {
        insert_if_new(
            &mut out.warnings,
            "Vixen's calculation mode for Doom Blast is selected but you do not have Vixen's Entrapment Embroidered Gloves equipped",
        );
    }
    // :2049-2057
    let flag = |name: &str| -> Result<bool, String> {
        let r = actor
            .mod_db
            .borrow()
            .flag(None, &[name])
            .map_err(|e| lua_err(2052, e.to_string()))?;
        Ok(r == Some(true))
    };
    let zero = LuaV::Num(0.0);
    let mut aspect = 0;
    if gt(&o.get("CrabBarriersMax"), &zero).map_err(|e| lua_err(2051, e))?
        && gt(&o.get("CrabBarriers"), &zero).map_err(|e| lua_err(2051, e))?
    {
        aspect += 1;
    }
    if aspect < 2 && flag("Condition:AspectOfTheSpiderActive")? {
        aspect += 1;
    }
    if aspect < 2 && (flag("Condition:CatsAgilityActive")? || flag("Condition:CatsStealthActive")?) {
        aspect += 1;
    }
    if aspect < 2 && (flag("Condition:AviansFlightActive")? || flag("Condition:AviansMightActive")?) {
        aspect += 1;
    }
    if aspect > 1 {
        insert_if_new(&mut out.warnings, "You have more than one Aspect skill active");
    }
    Ok(())
}

/// `actor.output[warningFlag]` when it is a list of skill names; another truthy value is not something perform writes there.
fn warning_list<'a>(o: &ActorView<'a>, flag: &str) -> Option<&'a [String]> {
    match o.get(flag) {
        LuaV::Table(TableRef::Out(OutTable::Names(v))) => Some(v.as_slice()),
        LuaV::Nil | LuaV::Bool(false) => None,
        other => panic!("Modules/Build.lua: output.{flag} is a {} value", other.type_name()),
    }
}

/// Build.lua:2061-2075 `buildMode:InsertItemWarnings()`.
fn insert_item_warnings(out: &mut StatList, res: &CalculateResult) {
    let iw = &res.env.item_warnings;
    if let Some(l) = iw.get("jewelLimitWarning") {
        for w in l {
            insert_if_new(&mut out.warnings, &format!("You are exceeding jewel limit with the jewel {w}"));
        }
    }
    if let Some(l) = iw.get("socketLimitWarning") {
        for w in l {
            insert_if_new(&mut out.warnings, &format!("You have too many gems in your {w} slot"));
        }
    }
    if let Some(l) = iw.get("missingAnointWarning") {
        insert_if_new(
            &mut out.warnings,
            &format!("You have eligible items missing an anoint: {}", l.join(", ")),
        );
    }
}

/// Build.lua:925-927, the warnings of `EstimatePlayerProgress` (the rest of it -- level, point display -- is `calculate`'s / UI).
fn progress_warnings(out: &mut StatList, res: &CalculateResult) {
    let c = res.env.spec.count_alloc_nodes();
    // :901-902
    let extra = res.perform.player.output.opt_num("ExtraPoints").unwrap_or(0.0);
    let used_max = 99.0 + 23.0 + extra;
    if f64::from(c.used) > used_max {
        insert_if_new(&mut out.warnings, "You have too many passive points allocated");
    }
    if c.asc_used > 8 {
        insert_if_new(&mut out.warnings, "You have too many ascendancy points allocated");
    }
    if c.secondary_asc_used > 8 {
        insert_if_new(&mut out.warnings, "You have too many secondary ascendancy points allocated");
    }
}

/// `#msg > 40` -> one line per `[^:]+` run, else the message (Build.lua:2082-2088).
fn push_info(out: &mut StatList, msg: &str) {
    if msg.len() > 40 {
        for line in pob_lua::gmatch(msg.as_bytes(), b"([^:]+)") {
            let caps = line.expect("pattern");
            let b = caps.get(0).and_then(|c| c.bytes(msg.as_bytes())).unwrap_or(b"");
            out.stat_box.push(StatBoxEntry::centered(Some(format!(
                "{}{}",
                color::CUSTOM,
                String::from_utf8_lossy(b)
            ))));
        }
    } else {
        out.stat_box.push(StatBoxEntry::centered(Some(format!("{}{msg}", color::CUSTOM))));
    }
}

/// Build.lua:2078-2119 `buildMode:RefreshStatList()`: `statBox.list` and `warnings.lines`. Sorts the actors' `SkillDPS` in place
/// (their copies in `denv`), as upstream sorts `output.SkillDPS`.
pub fn refresh_stat_list(denv: &mut DisplayEnv<'_>, st: &DisplaySettings) -> Result<StatList, String> {
    let mut out = StatList::default();
    let p = &denv.player;
    if let Some(msg) = p.info_message {
        push_info(&mut out, msg);
        if let Some(m2) = p.info_message2 {
            out.stat_box.push(StatBoxEntry::centered(Some(format!("^8{m2}"))));
        }
    }
    if let Some(m) = denv.minion.as_mut() {
        out.stat_box.push(StatBoxEntry::line(18.0, "^7Minion:".into()));
        if let Some(msg) = m.info_message {
            push_info(&mut out, msg);
            if let Some(m2) = m.info_message2 {
                out.stat_box.push(StatBoxEntry::centered(Some(format!("^8{m2}"))));
            }
        }
        add_display_stat_list(&mut out, MINION_DISPLAY_STATS, m, st)?;
        out.stat_box.push(StatBoxEntry::sep(10.0));
        out.stat_box.push(StatBoxEntry::line(18.0, "^7Player:".into()));
    }
    let p = &mut denv.player;
    if p.skill_flags.get("disable").copied().unwrap_or(false) {
        out.stat_box.push(StatBoxEntry {
            height: 16.0,
            text: Some("^7Skill disabled:".into()),
            ..StatBoxEntry::default()
        });
        out.stat_box.push(StatBoxEntry::centered(p.disable_reason.map(str::to_owned)));
    }
    add_display_stat_list(&mut out, DISPLAY_STATS, p, st)?;
    insert_item_warnings(&mut out, denv.res);
    progress_warnings(&mut out, denv.res);
    Ok(out)
}

// --- engine_displayStatsJSON (boot.lua:546-788) -----------------------------------------------------------------------------------

/// One row of a display section.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayRow {
    pub label: String,
    pub value: String,
    pub color: String,
    pub value_color: String,
    pub stat_key: Option<&'static str>,
}

/// One section: `{ actor, title, slot, rows }`.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplaySection {
    pub actor: &'static str,
    pub title: String,
    pub slot: usize,
    pub rows: Vec<DisplayRow>,
}

/// `hexColorNames` (boot.lua): the first `colorCodes` name (in this order) holding a code wins.
const HEX_COLOR_NAMES: &[(&str, &str)] = &[
    ("life", color::LIFE),
    ("mana", color::MANA),
    ("es", color::ES),
    ("rage", color::RAGE),
    ("strength", color::STRENGTH),
    ("dexterity", color::DEXTERITY),
    ("intelligence", color::INTELLIGENCE),
    ("rare", color::RARE),
    ("phys", color::PHYS),
    ("fire", color::FIRE),
    ("cold", color::COLD),
    ("lightning", color::LIGHTNING),
    ("chaos", color::CHAOS),
    ("evasion", color::EVASION),
    ("currency", color::CURRENCY),
    ("custom", color::CUSTOM),
    ("warning", color::WARNING),
    ("negative", color::NEGATIVE),
    ("positive", color::POSITIVE),
    ("ward", color::WARD),
    ("armour", color::ARMOUR),
    ("normal", color::NORMAL),
];

fn is_hex6(b: &[u8]) -> bool {
    b.len() == 6 && b.iter().all(u8::is_ascii_hexdigit)
}

/// boot.lua `colorClassName(code)`.
pub fn color_class_name(code: &str) -> String {
    if code.is_empty() {
        return "default".into();
    }
    if let Some((name, _)) = HEX_COLOR_NAMES.iter().find(|(_, c)| *c == code) {
        return (*name).into();
    }
    if code == "^x808080" {
        return "muted".into();
    }
    let b = code.as_bytes();
    if b.len() == 2 && b[0] == b'^' && b[1].is_ascii_digit() {
        return match b[1] {
            b'7' => "default".into(),
            b'8' => "muted".into(),
            d => format!("code{}", d as char),
        };
    }
    if b.len() == 8 && b.starts_with(b"^x") && is_hex6(&b[2..]) {
        return format!("hex{}", code[2..].to_ascii_lowercase());
    }
    "default".into()
}

/// boot.lua `leadingColorClass(str)`.
pub fn leading_color_class(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 8 && b.starts_with(b"^x") && is_hex6(&b[2..8]) {
        return color_class_name(&s[..8]);
    }
    if b.len() >= 2 && b[0] == b'^' && b[1].is_ascii_digit() {
        return color_class_name(&s[..2]);
    }
    "default".into()
}

/// boot.lua `buildDisplayStatSections(statList, actor)`: `(slot, rows)` per non-empty section.
fn build_sections(
    list: &[DisplayStat],
    actor: &mut DisplayActor<'_>,
    st: &DisplaySettings,
) -> Result<Vec<(usize, Vec<DisplayRow>)>, String> {
    let mut sections = Vec::new();
    let mut current: Option<Vec<DisplayRow>> = None;
    let mut slot = 1usize;
    let o = &actor.view;
    for sd in list {
        if !match_flags(sd.flag, sd.not_flag, actor.skill_flags) {
            continue;
        }
        if sd.stat.is_some() {
            let v = stat_value(sd, o)?;
            if !row_shown(sd, &v, o)? {
                continue;
            }
            let over_cap = over_cap_value(sd, &v, o)?;
            if sd.stat == Some("SkillDPS") {
                let list = actor.skill_dps.as_mut().ok_or("attempt to sort a non-table output.SkillDPS")?;
                sort_skill_dps(list);
                let rows = current.get_or_insert_with(Vec::new);
                for sk in list.iter() {
                    let mut label = sk.name.clone();
                    if sk.count >= 2.0 {
                        label = format!("{}x {label}", pob_lua::tostring_num(sk.count));
                    }
                    if let Some(t) = sk.trigger.as_deref().filter(|t| !t.is_empty()) {
                        label = format!("{label} ({t})");
                    }
                    if let Some(p) = &sk.skill_part {
                        label = format!("{label} -- {p}");
                    }
                    if let Some(s) = &sk.source {
                        label = format!("{label} [{s}]");
                    }
                    let spec = FormatSpec {
                        fmt: Some(".1f"),
                        compact_value: sd.compact_value,
                        pc: false,
                        mod_: false,
                        label: None,
                        chaos_inoc: false,
                    };
                    let formatted = format_stat(&spec, &LuaV::Num(sk.dps * sk.count), over_cap.as_ref(), None, st)?;
                    rows.push(DisplayRow {
                        label,
                        value: strip_escapes(&formatted),
                        color: color_class_name(color::CUSTOM),
                        value_color: leading_color_class(&formatted),
                        stat_key: None,
                    });
                }
            } else if !sd.hide_stat {
                let co = color_override(sd, &v, o)?;
                let mut formatted = format_stat(&FormatSpec::of(sd), &v, over_cap.as_ref(), co, st)?;
                if let Some(sfx) = suffix_text(sd, &v, o)? {
                    formatted.push_str(&format!("^x808080 ({sfx})"));
                }
                current.get_or_insert_with(Vec::new).push(DisplayRow {
                    label: strip_escapes(sd.label.expect("label")),
                    value: strip_escapes(&formatted),
                    color: color_class_name(sd.color.unwrap_or("^7")),
                    value_color: leading_color_class(&formatted),
                    stat_key: if sd.child_stat.is_none() { sd.stat } else { None },
                });
            }
        } else if sd.label.is_some() && sd.cond_func.is_some() {
            return Err("boot.lua buildDisplayStatSections: label-only display row (labelStat): unimplemented, no such row in BuildDisplayStats.lua".into());
        } else {
            // closeSection()
            if let Some(rows) = current.take()
                && !rows.is_empty()
            {
                sections.push((slot, rows));
            }
            slot += 1;
        }
    }
    if let Some(rows) = current.take()
        && !rows.is_empty()
    {
        sections.push((slot, rows));
    }
    Ok(sections)
}

const PLAYER_TITLES: [&str; 17] = [
    "Offence",
    "Attributes",
    "Devotion",
    "Effective Hit Pool",
    "Life",
    "Mana",
    "Energy Shield",
    "Ward",
    "Rage",
    "Recovery / Degen",
    "Evasion",
    "Armour",
    "Block, Dodge & Suppression",
    "Resistances",
    "Misc",
    "Full DPS",
    "Skill DPS",
];

fn titled(rows: Vec<(usize, Vec<DisplayRow>)>, titles: &[&str], actor: &'static str) -> impl Iterator<Item = DisplaySection> {
    rows.into_iter().map(move |(slot, rows)| DisplaySection {
        actor,
        title: titles
            .get(slot - 1)
            .map_or_else(|| format!("More {actor} stats (slot {slot})"), |t| (*t).to_owned()),
        slot,
        rows,
    })
}

/// The structure `engine_displayStatsJSON` encodes (boot.lua `displayStructure`): the minion's sections (title "Minion"), then the player's.
pub fn display_sections(denv: &mut DisplayEnv<'_>, st: &DisplaySettings) -> Result<Vec<DisplaySection>, String> {
    let mut out = Vec::new();
    if let Some(m) = denv.minion.as_mut() {
        out.extend(titled(build_sections(MINION_DISPLAY_STATS, m, st)?, &["Minion"], "minion"));
    }
    out.extend(titled(
        build_sections(DISPLAY_STATS, &mut denv.player, st)?,
        &PLAYER_TITLES,
        "player",
    ));
    Ok(out)
}

/// The sidebar of a calculated build: `RefreshStatList` then the `engine_displayStatsJSON` structure, in that order (the second
/// `SkillDPS` sort starts from the first one's result, as upstream).
#[derive(Clone, Debug)]
pub struct Display {
    pub stat_list: StatList,
    pub sections: Vec<DisplaySection>,
}

pub fn build_display(res: &CalculateResult, st: &DisplaySettings) -> Result<Display, String> {
    let minion = MinionBorrow::new(res);
    let mut denv = DisplayEnv::new(res, &minion)?;
    let stat_list = refresh_stat_list(&mut denv, st)?;
    let sections = display_sections(&mut denv, st)?;
    Ok(Display { stat_list, sections })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d() -> DisplaySettings {
        DisplaySettings::default()
    }

    #[test]
    fn num_sep() {
        assert_eq!(format_num_sep("1234567.891", &d()), "1,234,567.891");
        assert_eq!(format_num_sep("123", &d()), "123");
        assert_eq!(format_num_sep("-1234", &d()), "-1,234");
        assert_eq!(format_num_sep("7", &d()), "7");
        assert_eq!(format_num_sep("100%", &d()), "100%");
        assert_eq!(format_num_sep("inf", &d()), "inf");
    }

    #[test]
    fn colors() {
        assert_eq!(color_class_name("^7"), "default");
        assert_eq!(color_class_name(color::MANA), "mana");
        assert_eq!(color_class_name(color::POSITIVE), "evasion");
        assert_eq!(color_class_name("^x123ABC"), "hex123abc");
        assert_eq!(leading_color_class("^xDD0022-5"), "negative");
        assert_eq!(strip_escapes("^7a^xDD0022b^x808080 (c)"), "ab (c)");
    }
}
