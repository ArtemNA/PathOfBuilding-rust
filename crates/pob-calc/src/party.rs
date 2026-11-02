//! The Party tab without its UI (Classes/PartyTab.lua, R-party): `PartyTab:Load` (:516-558) runs `ParseBuffs` (:722-986) over each
//! `<ImportedBuffs>` buffer of the build (`pob_build::BuildModel::party_imported_buffs`, read once by pob-build -- the same reading
//! pob-engine's SaveDB writes back). The result is what the calc reads:
//!
//! * `partyTab.actor` (:22) -> `env.partyMembers` (CalcPerform.lua:1310): its `Aura` / `Curse` / `Warcry` / `Link` buff lists (keyed by
//!   the mod's `GlobalEffect` type, a `Vaal` sub-table for names starting with "Vaal"), its `modDB` (the flag lines of
//!   "PartyMemberStats") and its `output` (the `key=value` lines of "PartyMemberStats");
//! * `partyTab.enemyModList` (:24) -> `env.enemyDB` (CalcSetup.lua:697).
//!
//! Not ported (UI only, no calc reads them): the `label` texts of the simple views (:735-764, :901-984), the edit controls, `lastContent`.
//! Upstream parses once, on load, and every calc shares the same actor; here [`PartyTab::load`] runs per `initEnv` (the parse reads
//! nothing the calc changes). Hash walks over buff NAMES (`pairs`, MINEFIELD-RUST R13) keep insertion order.

use std::rc::Rc;

use indexmap::IndexMap;
use pob_build::{PartyImportedBuffs, PartyText};
use pob_data::globals::globals;
use pob_lua::{Repl, gmatch, gsub, match_};
use pob_mods::{Mod, ModType, Str, Table, Tag, Value, create_mod};

const PT: &str = "Classes/PartyTab.lua";

/// One buff of a party buff list: `{ modList = ModList, effectMult = currentEffect, isMark = true|nil }` (:853-858, :862-865).
#[derive(Clone, Debug)]
pub struct PartyBuff {
    pub mod_list: Vec<Rc<Mod>>,
    /// `currentEffect`: `tonumber(line)` (a number or nil), or for `otherEffects` the matched string (:837).
    pub effect_mult: Value,
    pub is_mark: bool,
}

/// `list[currentModType]` (:846): the buffs by name, plus the `Vaal` sub-table (:848-851).
#[derive(Clone, Debug, Default)]
pub struct BuffGroup {
    pub named: IndexMap<String, PartyBuff>,
    pub vaal: Option<IndexMap<String, PartyBuff>>,
}

/// `self.actor[buffType]` (:22): the groups by mod type, and `limit` (Curse only, :810).
#[derive(Clone, Debug, Default)]
pub struct BuffList {
    pub groups: IndexMap<String, BuffGroup>,
    pub limit: Value,
}

impl BuffList {
    /// `list[modType]`.
    pub fn group(&self, ty: &str) -> Option<&BuffGroup> {
        self.groups.get(ty)
    }
}

/// A value of `partyTab.actor.output` (:770-791): a number, or a table of numbers (`k1.k2=v`).
#[derive(Clone, Debug)]
pub enum PartyOut {
    Num(f64),
    Table(IndexMap<String, f64>),
}

/// What `PartyTab:Load` leaves for the calc.
#[derive(Clone, Debug, Default)]
pub struct PartyTab {
    pub aura: BuffList,
    pub curse: BuffList,
    pub warcry: BuffList,
    pub link: BuffList,
    /// `actor.modDB` (:22): the FLAG mods of "PartyMemberStats" (:793).
    pub member_mods: Vec<Rc<Mod>>,
    /// `actor.output` (:22).
    pub output: IndexMap<String, PartyOut>,
    /// `self.enemyModList` (:24).
    pub enemy_mod_list: Vec<Rc<Mod>>,
}

impl PartyTab {
    /// `actor.output[k]` as a number (nil / a table -> `None`).
    pub fn out_num(&self, k: &str) -> Option<f64> {
        match self.output.get(k) {
            Some(PartyOut::Num(v)) => Some(*v),
            _ => None,
        }
    }

    /// PartyTab:Load (:516-558) over the buffers of the build, in document order. `Err` = the Lua error the load raises.
    pub fn load(buffers: &[PartyImportedBuffs]) -> Result<PartyTab, String> {
        let mut p = PartyTab::default();
        for b in buffers {
            // pob-build refuses an element `node[1]` on load (the `gmatch` of `ParseBuffs` fails on a table)
            let PartyText::Text(buf) = &b.text else {
                return Err(format!("{PT}:806: attempt to call method 'gmatch' (a nil value)"));
            };
            match b.name.as_str() {
                "PartyMemberStats" => p.parse_member_stats(buf)?,
                "Aura" => parse_buff_list(&mut p.aura, buf, "Aura")?,
                "Curse" => parse_buff_list(&mut p.curse, buf, "Curse")?,
                "Warcry Skills" => parse_buff_list(&mut p.warcry, buf, "Warcry")?,
                "Link Skills" => parse_buff_list(&mut p.link, buf, "Link")?,
                "EnemyConditions" => p.parse_enemy_conditions(buf),
                "EnemyMods" => p.parse_enemy_mods(buf)?,
                _ => {}
            }
        }
        Ok(p)
    }

    /// :723-728 "EnemyConditions": every line a flag `Condition:Party:<x>` on the enemy.
    fn parse_enemy_conditions(&mut self, buf: &str) {
        for line in lines(buf) {
            if !line.is_empty() {
                // `line:gsub("Condition:", "Condition:Party:")` (no magic characters: a plain replace)
                let name = line.replace("Condition:", "Condition:Party:");
                self.enemy_mod_list
                    .push(Rc::new(create_mod(&name, Some(ModType::Flag), Value::Bool(true), Some("Party"), 0, 0, vec![])));
            }
        }
    }

    /// :729-748 "EnemyMods": a line without `|` names a group, the others are formatted source mods (the `label` is always given by
    /// Load, so a mod line needs its group: `t_insert(enemyModList[currentName], ...)` :744).
    fn parse_enemy_mods(&mut self, buf: &str) -> Result<(), String> {
        let mut current: Option<&str> = None;
        for line in lines(buf) {
            if !line.contains('|') {
                current = Some(line);
            } else if let Some(mut m) = parse_formatted_source_mod(line) {
                let src = m.source.as_deref().unwrap_or("");
                m.source = Some(Rc::from(format!("Party{src}").as_str()));
                self.enemy_mod_list.push(Rc::new(m));
                if current.is_none_or(str::is_empty) {
                    return Err(format!("{PT}:744: bad argument #1 to 'insert' (table expected, got nil)"));
                }
            }
        }
        Ok(())
    }

    /// :765-796 "PartyMemberStats": `k=v` lines into `actor.output` (`label` is the output table here), the rest flags on `actor.modDB`.
    fn parse_member_stats(&mut self, buf: &str) -> Result<(), String> {
        for line in lines(buf) {
            let lb = line.as_bytes();
            if line.contains('=') {
                let caps = |p: &str| match_(lb, p.as_bytes(), None).map_err(|e| format!("{PT}: {e}"));
                let cap = |c: &pob_lua::Captures, i: usize| -> Option<String> {
                    c.get(i)
                        .and_then(|x| x.bytes(lb))
                        .map(|b| String::from_utf8_lossy(b).into_owned())
                };
                if let Some(c) = caps("^([%w ]-%w+)%.([%w ]-%w+)=(.+)$")? {
                    // :772-776 `label[k1][k2] = tonumber(v)`
                    let (k1, k2, v) = (cap(&c, 0).unwrap_or_default(), cap(&c, 1).unwrap_or_default(), cap(&c, 2).unwrap_or_default());
                    let t = self.output.entry(k1).or_insert_with(|| PartyOut::Table(IndexMap::new()));
                    if !matches!(t, PartyOut::Table(_)) {
                        *t = PartyOut::Table(IndexMap::new());
                    }
                    let PartyOut::Table(t) = t else { unreachable!() };
                    match pob_lua::tonumber(&v) {
                        Some(n) => {
                            t.insert(k2, n);
                        }
                        None => {
                            t.shift_remove(&k2);
                        }
                    }
                } else if line.contains('|') {
                    // :777-787
                    let Some(c) = caps("([%w ]-%w+)|(.+)=(.+)")? else {
                        return Err(format!("{PT}:780: attempt to index local 'tags' (a nil value)"));
                    };
                    let (k, tags, v) = (cap(&c, 0).unwrap_or_default(), cap(&c, 1).unwrap_or_default(), cap(&c, 2).unwrap_or_default());
                    let mut v = pob_lua::tonumber(&v);
                    for tag in gmatch(tags.as_bytes(), b"([^|]*)|?") {
                        let tag = tag.map_err(|e| format!("{PT}: {e}"))?;
                        let tag = tag.get(0).and_then(|x| x.bytes(tags.as_bytes())).unwrap_or_default();
                        if tag == b"percent" {
                            let x = v.ok_or_else(|| format!("{PT}:782: attempt to perform arithmetic on local 'v' (a nil value)"))?;
                            v = Some(x / 100.0);
                        } else if tag == b"max" {
                            let prev = match self.output.get(&k) {
                                None => 1.0,
                                Some(PartyOut::Num(p)) => *p,
                                Some(PartyOut::Table(_)) => {
                                    return Err(format!("{PT}:784: bad argument #2 to 'max' (number expected, got table)"));
                                }
                            };
                            let x = v.ok_or_else(|| format!("{PT}:784: bad argument #1 to 'max' (number expected, got nil)"))?;
                            v = Some(pob_lua::number::max2(x, prev));
                        }
                    }
                    set_out(&mut self.output, k, v);
                } else {
                    // :789-790
                    let Some(c) = caps("([%w ]-%w+)=(.+)")? else {
                        return Err(format!("{PT}:790: table index is nil"));
                    };
                    let (k, v) = (cap(&c, 0).unwrap_or_default(), cap(&c, 1).unwrap_or_default());
                    set_out(&mut self.output, k, pob_lua::tonumber(&v));
                }
            } else if !line.is_empty() {
                // :793 `list:NewMod(line, "FLAG", true, "Party")`
                self.member_mods
                    .push(Rc::new(create_mod(line, Some(ModType::Flag), Value::Bool(true), Some("Party"), 0, 0, vec![])));
            }
        }
        Ok(())
    }
}

/// `label[k] = v` (nil removes the key).
fn set_out(out: &mut IndexMap<String, PartyOut>, k: String, v: Option<f64>) {
    match v {
        Some(n) => {
            out.insert(k, PartyOut::Num(n));
        }
        None => {
            out.shift_remove(&k);
        }
    }
}

/// `buf:gmatch("([^\n]*)\n?")` -- every line, and the empty match at the end.
fn lines(buf: &str) -> Vec<&str> {
    let b = buf.as_bytes();
    gmatch(b, b"([^\n]*)\n?")
        .filter_map(|c| c.ok())
        .map(|c| match c.get(0) {
            Some(pob_lua::Capture::Str { start, end }) => &buf[start..end],
            _ => "",
        })
        .collect()
}

/// `str:gsub(pat, repl)` with a plain replacement string (`%` escapes as Lua).
fn lua_gsub(s: &str, pat: &str, repl: &str) -> String {
    let (out, _) = gsub(s.as_bytes(), pat.as_bytes(), Repl::Str(repl.as_bytes()), None).unwrap_or_else(|e| panic!("{PT}: {e}"));
    String::from_utf8_lossy(&out).into_owned()
}

/// Lua `a ~= b` on two `effectMult`s (raw equality).
fn effect_ne(a: &Value, b: &Value) -> bool {
    !a.raw_eq(b)
}

/// Lua `a < b` on two `effectMult`s (numbers, or strings byte-wise; anything else raises).
fn effect_lt(a: &Value, b: &Value) -> Result<bool, String> {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => Ok(x < y),
        (Value::Str(x), Value::Str(y)) => Ok(x.as_bytes() < y.as_bytes()),
        _ => Err(format!("{PT}:861: attempt to compare {} with {}", a.type_name(), b.type_name())),
    }
}

/// :797-900 the buff lists ("Aura", "Curse", "Warcry", "Link") -- the parse, without the `label` text (:901-984, UI only).
#[allow(clippy::too_many_lines)]
fn parse_buff_list(list: &mut BuffList, buf: &str, buff_type: &str) -> Result<(), String> {
    #[derive(PartialEq)]
    enum Mode {
        Name,
        CurseLimit,
        Effect,
        IsMark,
        Stats,
    }
    let mut mode = if buff_type == "Curse" { Mode::CurseLimit } else { Mode::Name };
    let mut current_name: Option<String> = None;
    let mut current_effect = Value::Nil;
    let mut is_mark = false;
    let mut current_mod_type: Option<String> = Some(
        match buff_type {
            "Link" => "Link",
            "Warcry" => "Warcry",
            _ => "Unknown",
        }
        .to_owned(),
    );
    let skills = pob_data::skills::skills();
    for line in lines(buf) {
        let mut line: String = line.to_owned();
        if line != "---" && line.contains("---") {
            // comment but not divider, skip the line
        } else if mode == Mode::CurseLimit && !line.is_empty() {
            list.limit = pob_lua::tonumber(&line).map_or(Value::Nil, Value::Num);
            mode = Mode::Name;
        } else if mode == Mode::Name && !line.is_empty() {
            current_name = Some(line.replace("_Debuff", ""));
            current_effect = Value::Num(0.0);
            if line == "extraAura" || line == "otherEffects" {
                current_mod_type = Some(line.clone());
                mode = Mode::Stats;
            } else {
                mode = Mode::Effect;
                current_mod_type = Some("Unknown".to_owned());
            }
        } else if mode == Mode::Effect {
            current_effect = pob_lua::tonumber(&line).map_or(Value::Nil, Value::Num);
            mode = if buff_type == "Curse" { Mode::IsMark } else { Mode::Stats };
        } else if mode == Mode::IsMark {
            is_mark = line == "true";
            mode = Mode::Stats;
        } else if line == "---" {
            mode = Mode::Name;
        } else if line.contains('|') && current_name.as_deref() != Some("SKIP") && !line.contains("MinionModifier|LIST") {
            if current_mod_type.as_deref() == Some("otherEffects") {
                // :837 `currentName, currentEffect, line = line:match("([%w ']-%w+)|(%w+)|(.+)")`
                let lb = line.as_bytes().to_vec();
                let c = match_(&lb, b"([%w ']-%w+)|(%w+)|(.+)", None).map_err(|e| format!("{PT}: {e}"))?;
                let cap = |i: usize| -> Option<String> {
                    c.as_ref()
                        .and_then(|c| c.get(i))
                        .and_then(|x| x.bytes(&lb))
                        .map(|b| String::from_utf8_lossy(b).into_owned())
                };
                current_name = cap(0);
                current_effect = cap(1).map_or(Value::Nil, |s| Value::str(&s));
                match cap(2) {
                    Some(l) => line = l,
                    None => return Err(format!("{PT}:839: attempt to index local 'line' (a nil value)")),
                }
            }
            let Some(mut m) = parse_formatted_source_mod(&line) else {
                continue;
            };
            for t in &m.tags {
                let ct = current_mod_type.as_deref();
                if t.get("type").is_str("GlobalEffect") && ct != Some("Link") && ct != Some("Warcry") && ct != Some("otherEffects") {
                    current_mod_type = t.get("effectType").as_str().map(str::to_owned);
                }
            }
            let Some(mod_type) = current_mod_type.clone() else {
                return Err(format!("{PT}:846: table index is nil"));
            };
            let Some(name) = current_name.clone() else {
                return Err(format!("{PT}:848: attempt to index local 'currentName' (a nil value)"));
            };
            let group = list.groups.entry(mod_type).or_default();
            let elems = if name.as_bytes().get(..4) == Some(b"Vaal") {
                group.vaal.get_or_insert_with(IndexMap::new)
            } else {
                &mut group.named
            };
            // :852-869 a new entry; a different effect: the higher one wins (a fresh entry, without `isMark`), a lower one is skipped
            let replace = match elems.get(&name) {
                None => Some(is_mark),
                Some(e) if effect_ne(&e.effect_mult, &current_effect) => {
                    if effect_lt(&e.effect_mult, &current_effect)? {
                        Some(false)
                    } else {
                        current_name = Some("SKIP".to_owned());
                        None
                    }
                }
                Some(_) => None,
            };
            if let Some(mark) = replace {
                elems.insert(
                    name.clone(),
                    PartyBuff {
                        mod_list: Vec::new(),
                        effect_mult: current_effect.clone(),
                        is_mark: mark,
                    },
                );
            }
            if current_name.as_deref() != Some("SKIP") {
                fix_source(&mut m, &name, &skills)?;
                if buff_type == "Link" {
                    // :887-893
                    m.name = Rc::from(m.name.replace("Parent", "PartyMember").as_str());
                    m.tags = m
                        .tags
                        .iter()
                        .map(|t| {
                            if t.get("actor").is_str("parent") {
                                let mut tb = (*t.table()).clone();
                                tb.hash.insert(Rc::from("actor"), Value::str("partyMembers"));
                                Tag::new(tb)
                            } else {
                                t.clone()
                            }
                        })
                        .collect();
                }
                elems.get_mut(&name).expect("inserted above").mod_list.push(Rc::new(m));
            }
        }
    }
    Ok(())
}

/// :871-886 the source of a party buff mod: an item source loses its id, a skill source not in `data.skills` is renamed after the
/// buff (`currentName` with ` x` capitalised and spaces removed) when that names a skill, else it becomes the bare skill id.
fn fix_source(m: &mut Mod, current_name: &str, skills: &pob_data::skills::Skills) -> Result<(), String> {
    let src: String = m.source.as_deref().unwrap_or("").to_owned();
    let mut src: Option<String> = Some(src);
    if let Some(s) = src.as_deref()
        && s.contains("Item")
    {
        // `oldItem, mod.source = mod.source:match("Item:(%d+):(.+)")`; `"Party - "..mod.source`
        let c = match_(s.as_bytes(), b"Item:(%d+):(.+)", None).map_err(|e| format!("{PT}: {e}"))?;
        let rest = c
            .and_then(|c| c.get(1))
            .and_then(|x| x.bytes(s.as_bytes()))
            .map(|b| String::from_utf8_lossy(b).into_owned());
        match rest {
            Some(r) => src = Some(format!("Party - {r}")),
            None => return Err(format!("{PT}:874: attempt to concatenate field 'source' (a nil value)")),
        }
    }
    if let Some(s) = src.clone()
        && s.contains("Skill")
    {
        let c = match_(s.as_bytes(), b"Skill:(.+)", None).map_err(|e| format!("{PT}: {e}"))?;
        let skill_id = c
            .and_then(|c| c.get(0))
            .and_then(|x| x.bytes(s.as_bytes()))
            .map(|b| String::from_utf8_lossy(b).into_owned());
        if !skill_id.as_deref().is_some_and(|id| skills.index.contains_key(id)) {
            // `currentName:gsub(" %l", string.upper):gsub(" ", "")`
            let mut caps_up: Vec<u8> = Vec::new();
            let mut f = |subject: &[u8], c: &pob_lua::Captures| -> Option<Vec<u8>> {
                c.get(0).and_then(|x| x.bytes(subject)).map(<[u8]>::to_ascii_uppercase)
            };
            let (up, _) = gsub(current_name.as_bytes(), b" %l", Repl::Fn(&mut f), None).map_err(|e| format!("{PT}: {e}"))?;
            caps_up.extend_from_slice(&up);
            let minimised = lua_gsub(&String::from_utf8_lossy(&caps_up), " ", "");
            src = if skills.index.contains_key(&minimised) {
                Some(format!("Skill:{minimised}"))
            } else {
                skill_id
            };
        }
    }
    m.source = src.map(|s| Str::from(s.as_str()));
    Ok(())
}

/// ModTools.lua:72-98 `modLib.parseTags(line)`.
fn parse_tags(line: Option<&str>) -> Vec<Tag> {
    let Some(line) = line.filter(|l| *l != "-") else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    let lb = line.as_bytes();
    for group in gmatch(lb, b"([^,]*),?").filter_map(Result::ok) {
        let g = group.get(0).and_then(|x| x.bytes(lb)).unwrap_or_default();
        if g.is_empty() {
            continue;
        }
        let mut t = Table::default();
        for tag in gmatch(g, b"([^/]*)/?").filter_map(Result::ok) {
            let tag = tag.get(0).and_then(|x| x.bytes(g)).unwrap_or_default();
            if tag.is_empty() {
                continue;
            }
            // `tag:match("^(%a+)=(.+)")`; an invalid one is printed and dropped (ConPrintf)
            let Ok(Some(c)) = match_(tag, b"^(%a+)=(.+)", None) else {
                continue;
            };
            let name = String::from_utf8_lossy(c.get(0).and_then(|x| x.bytes(tag)).unwrap_or_default()).into_owned();
            let val = String::from_utf8_lossy(c.get(1).and_then(|x| x.bytes(tag)).unwrap_or_default()).into_owned();
            // only `threshold` is a number; `tagValue == "true" and true or tagValue`
            let v = if name == "threshold" {
                pob_lua::tonumber(&val).map_or(Value::Nil, Value::Num)
            } else if val == "true" {
                Value::Bool(true)
            } else {
                Value::str(&val)
            };
            if v.is_nil() {
                t.hash.shift_remove(name.as_str());
            } else {
                t.hash.insert(Rc::from(name.as_str()), v);
            }
        }
        tags.push(Tag::new(t));
    }
    tags
}

/// ModTools.lua:100-119 `modLib.parseFormattedSourceMod(line)`: `value|source|name|type|flags|keywordFlags|tags`.
pub fn parse_formatted_source_mod(line: &str) -> Option<Mod> {
    let lb = line.as_bytes();
    let parts: Vec<&str> = gmatch(lb, b"([^|]*)|?")
        .filter_map(Result::ok)
        .map(|c| match c.get(0) {
            Some(pob_lua::Capture::Str { start, end }) => &line[start..end],
            _ => "",
        })
        .collect();
    if parts.len() < 4 {
        return None;
    }
    let value = if parts[0] == "true" {
        Value::Bool(true)
    } else {
        Value::Num(pob_lua::tonumber(parts[0]).unwrap_or(0.0))
    };
    let g = globals();
    let flags = parts.get(4).and_then(|f| g.mod_flag.get(*f)).map_or(0, |v| *v as u32);
    let keyword_flags = parts.get(5).and_then(|f| g.keyword_flag.get(*f)).map_or(0, |v| *v as u32);
    Some(create_mod(
        parts[2],
        ModType::parse(parts[3]),
        value,
        Some(parts[1]),
        flags,
        keyword_flags,
        parse_tags(parts.get(6).copied()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatted_source_mod_round_trip() {
        let m = parse_formatted_source_mod(
            "5|Skill:FlameLink|FireMin|BASE|-|-|type=GlobalEffect/effectType=Link,type=PercentStat/actor=parent/percent=1/stat=Life",
        )
        .expect("mod");
        assert_eq!(&*m.name, "FireMin");
        assert_eq!(m.ty, Some(ModType::Base));
        assert_eq!(m.value.as_num(), Some(5.0));
        assert_eq!(m.tags.len(), 2);
        assert!(m.tags[1].get("percent").is_str("1"));
        // `gmatch("([^|]*)|?")` adds the empty match at the end: "a|b|c" has 4 fields (a mod), "a|b" 3 (nil)
        assert!(parse_formatted_source_mod("a|b|c").is_some());
        assert!(parse_formatted_source_mod("a|b").is_none());
    }

    #[test]
    fn member_stats_and_buffs() {
        let bufs = [
            PartyImportedBuffs {
                name: "PartyMemberStats".into(),
                text: PartyText::Text("Life=15342\nMovementSpeedMod|percent|max=273\nSomeFlag".into()),
            },
            PartyImportedBuffs {
                name: "Curse".into(),
                text: PartyText::Text(
                    "--- Curse Limit ---\n1\nAssassin's Mark\n100\ntrue\n80|Skill:AssassinsMark|SelfManaOnKill|BASE|-|-|type=GlobalEffect/effectType=Curse\n---".into(),
                ),
            },
        ];
        let p = PartyTab::load(&bufs).expect("load");
        assert_eq!(p.out_num("Life"), Some(15342.0));
        assert_eq!(p.out_num("MovementSpeedMod"), Some(2.73));
        assert_eq!(p.member_mods.len(), 1);
        assert_eq!(p.curse.limit.as_num(), Some(1.0));
        let c = &p.curse.group("Curse").expect("Curse").named["Assassin's Mark"];
        assert!(c.is_mark);
        assert_eq!(c.effect_mult.as_num(), Some(100.0));
        assert_eq!(c.mod_list.len(), 1);
    }
}
