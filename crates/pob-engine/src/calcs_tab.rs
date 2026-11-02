//! The engine side of the Calcs tab (U4): `calcsTab.input`, `RefreshSkillSelectControls(.., "Calcs")` (`crate::shell`), the mutators of the tab's
//! controls (CalcsTab.lua:46-140) and the data part of `CalcBreakdownControl` (Classes/CalcBreakdownControl.lua:60-535): which sections a cell
//! description gives over `actor.breakdown` and `modStore:Tabulate`. No graphics: widths and drawing are `pob_ui`'s.

use std::collections::HashMap;

use indexmap::IndexMap;
use pob_calc::breakdown::{BTable, BVal, Breakdown};
use pob_calc::pob_build::InputValue;
use pob_lua::tostring_num;
use pob_mods::{ModCfg, ModType, StoreRef, TabEntry, Value};

use crate::calcs::{CalcsActor, CheckCtx, Entry, check_flag, color_code, combine, minion_actor, player_actor, sections};
use crate::engine::Engine;
use crate::lua::lua_sort;
use crate::shell::MinionEntry;

/// `self.envName or "calcsEnv"` (CalcBreakdownControl.lua:50).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BdEnv {
    Calcs,
    Main,
}

/// What a table cell shows on hover besides its text (`row[col.key.."Tooltip"]` / `row[col.key.."Node"]`).
#[derive(Clone, Debug, PartialEq)]
pub enum BdHover {
    /// `row.sourceTooltip`: "Total from <source>:" and the lines (:417-422).
    SourceTotals(String, Vec<String>),
    /// `itemsTab:AddItemTooltip` (:285, :431) or a `sourceNameTooltip` closure of the calculation (CalcPerform.lua:2139): the item tooltip is U6.
    Item,
    /// `sourceNameNode` (:442): `ttNode.x`, `ttNode.y` (nil for the abstract nodes of cluster jewels, :631).
    Node(Option<(f64, f64)>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BdCol {
    pub label: String,
    pub key: String,
    pub right: bool,
}

/// One row: `row[key]` as the drawing code reads it (`tostring`; only strings and numbers are drawn).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BdRow {
    pub cells: IndexMap<String, String>,
    pub hover: IndexMap<String, BdHover>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BdSection {
    Text {
        lines: Vec<String>,
        text_size: f64,
    },
    Radius {
        radius: f64,
    },
    Table {
        label: Option<String>,
        footer: Option<String>,
        rows: Vec<BdRow>,
        cols: Vec<BdCol>,
    },
}

/// `table.sort(breakdown.slots)` sorts the calculation's own table in place (:271): the unstable sort of the next open starts from this order.
pub type SlotOrders = HashMap<(BdEnv, bool, String), Vec<usize>>;

fn cols(list: &[(&str, &str, bool)]) -> Vec<BdCol> {
    list.iter()
        .map(|(l, k, r)| BdCol {
            label: (*l).into(),
            key: (*k).into(),
            right: *r,
        })
        .collect()
}

fn val_text(v: &BVal) -> Option<String> {
    match v {
        BVal::Str(s) => Some(s.clone()),
        BVal::Num(n) => Some(tostring_num(*n)),
        _ => None,
    }
}

fn row_of(t: &BTable) -> BdRow {
    let mut r = BdRow::default();
    for (k, v) in &t.fields {
        if let Some(s) = val_text(v) {
            r.cells.insert(k.clone(), s);
        } else if k.ends_with("Tooltip") && matches!(v, BVal::Fn) {
            r.hover.insert(k.trim_end_matches("Tooltip").to_owned(), BdHover::Item);
        }
    }
    r
}

/// `string.format("%+g", v)`.
fn plus_g(v: f64) -> String {
    pob_calc::sf!("%+g", v)
}

/// `value.."..."` of a value (numbers `tostring`, strings as they are).
fn concat_val(v: &Value) -> Result<String, String> {
    match v {
        Value::Num(n) => Ok(tostring_num(*n)),
        Value::Str(s) => Ok(s.to_string()),
        other => Err(format!("attempt to concatenate a {} value", other.type_name())),
    }
}

/// CalcBreakdownControl.lua:537-539 `FormatModName`.
pub fn format_mod_name(name: &str) -> String {
    let g = |s: &[u8], p: &[u8]| -> Vec<u8> { pob_lua::gsub(s, p, pob_lua::Repl::Str(b"%1 %2"), None).expect("constant pattern").0 };
    let a = g(name.as_bytes(), b"([%l%d]:?)(%u)");
    String::from_utf8_lossy(&g(&a, b"(%l)(%d)")).into_owned()
}

/// :541-543 `FormatVarNameOrList`.
fn format_var_or_list(var: &Value, list: &Value) -> Result<String, String> {
    if var.truthy() {
        return Ok(format_mod_name(&concat_val(var)?));
    }
    let Value::Table(t) = list else {
        return Err("bad argument #1 to 'concat' (table expected, got nil)".into());
    };
    let parts: Vec<String> = t.array.iter().map(concat_val).collect::<Result<_, _>>()?;
    Ok(format_mod_name(&parts.join(" / ")))
}

/// :545-547 `FormatModBase`.
fn format_mod_base(ty: Option<ModType>, base: f64) -> String {
    if ty == Some(ModType::Base) {
        plus_g(base.abs())
    } else {
        format!("{}%", tostring_num(base.abs()))
    }
}

fn type_name(t: Option<ModType>) -> &'static str {
    match t {
        Some(ModType::Base) => "BASE",
        Some(ModType::Inc) => "INC",
        Some(ModType::More) => "MORE",
        Some(ModType::Override) => "OVERRIDE",
        Some(ModType::Flag) => "FLAG",
        Some(ModType::List) => "LIST",
        Some(ModType::Max) => "MAX",
        _ => "",
    }
}

/// :549-577 `FormatModValue`.
fn format_mod_value(value: &Value, ty: &str) -> Result<String, String> {
    let num = || match value {
        Value::Num(n) => Ok(*n),
        other => Err(format!("attempt to compare {} with number", other.type_name())),
    };
    Ok(match ty {
        "BASE" => plus_g(num()?) + " base",
        "INC" | "MORE" => {
            let n = num()?;
            let (pos, neg) = if ty == "INC" { ("increased", "reduced") } else { ("more", "less") };
            if n >= 0.0 {
                format!("{}% {pos}", tostring_num(n))
            } else {
                format!("{}% {neg}", tostring_num(-n))
            }
        }
        "OVERRIDE" => format!("Override: {}", concat_val(value)?),
        "FLAG" => if value.truthy() { "True" } else { "False" }.into(),
        "LIST" => match value {
            Value::Table(t) => match t.get("mod") {
                Value::Mod(m) => format!("Modifier: {}", format_mod_name(&m.name)),
                _ => "?".into(),
            },
            Value::Mod(_) => "?".into(),
            _ => "?".into(),
        },
        _ => concat_val(value)?,
    })
}

/// `string.match(source, "[^:]+")`.
fn source_type(s: &str) -> &str {
    s.split(':').find(|x| !x.is_empty()).unwrap_or("")
}

fn globals_names(table: &str) -> Vec<(String, u32)> {
    let g: serde_json::Value = pob_data::read_json("globals.json").unwrap_or_else(|e| panic!("pob-data: globals.json: {e}"));
    g[table]
        .as_object()
        .map(|m| m.iter().filter_map(|(k, v)| v.as_f64().map(|x| (k.clone(), x as u32))).collect())
        .unwrap_or_default()
}

/// `ModFlag`, `KeywordFlag`, `SkillType` (Data/Global.lua).
type FlagTables = (Vec<(String, u32)>, Vec<(String, u32)>, Vec<(String, u32)>);

thread_local! {
    static FLAG_NAMES: FlagTables = (globals_names("ModFlag"), globals_names("KeywordFlag"), globals_names("SkillType"));
}

/// :465-477 `row.flags`: `pairs({ [row.mod.flags] = ModFlag, [row.mod.keywordFlags] = KeywordFlag })` -- one table when the two numbers are equal.
fn flags_text(flags: u32, kw: u32) -> String {
    FLAG_NAMES.with(|(mf, kf, _)| {
        let mut names: Vec<&str> = Vec::new();
        if flags != kw {
            names.extend(mf.iter().filter(|(_, v)| flags & v == *v).map(|(n, _)| n.as_str()));
        }
        names.extend(kf.iter().filter(|(_, v)| kw & v == *v).map(|(n, _)| n.as_str()));
        names.sort_unstable();
        names.join(", ")
    })
}

/// The `desc` of one mod tag (:483-528).
fn tag_desc(
    m: &pob_mods::Mod,
    tag: &pob_mods::Tag,
    base_val: &mut Value,
    skills: &pob_data::skills::Skills,
) -> Result<Option<String>, String> {
    let ty = tag.ty().clone();
    let ty_s = concat_val(&ty)?;
    let neg = if tag.get("neg").truthy() { "Not " } else { "" };
    let num = |k: &str| tag.get(k).as_num();
    let actor_prefix = |tail: &str| match tag.get("actor") {
        Value::Str(a) => format!("{a}{tail}"),
        _ => String::new(),
    };
    let mod_value = m.value.as_num();
    let base_part = |base_val: &Value| -> Result<String, String> {
        // `tag.base and (FormatModBase(mod, tag.base).." + "..math.abs(mod.value).." ") or baseVal`
        if let Some(b) = num("base") {
            let v = mod_value.ok_or("bad argument #1 to 'abs' (number expected)")?;
            Ok(format!("{} + {} ", format_mod_base(m.ty, b), tostring_num(v.abs())))
        } else {
            concat_or_false(base_val)
        }
    };
    Ok(Some(match ty_s.as_str() {
        "Condition" | "ActorCondition" => {
            let (var, list) = (tag.get("var"), tag.get("varList"));
            let cond = if var.truthy() || list.truthy() {
                format!(": {neg}{}", format_var_or_list(&var, &list)?)
            } else {
                String::new()
            };
            let actor = match tag.get("actor") {
                Value::Str(a) => {
                    let (h, t) = a.split_at(a.len().min(1));
                    format!("{}{t} ", h.to_ascii_uppercase())
                }
                _ => String::new(),
            };
            format!("{actor}Condition{cond}")
        }
        "Multiplier" => {
            let base = base_part(base_val)?;
            let div = match tag.get("div") {
                Value::Nil | Value::Bool(false) => String::new(),
                v => format!("{} ", concat_val(&v)?),
            };
            let d = format!("{base}per {div}{}", format_var_or_list(&tag.get("var"), &tag.get("varList"))?);
            *base_val = Value::Str("".into());
            d
        }
        "PerStat" => {
            let base = base_part(base_val)?;
            let div = match tag.get("div") {
                Value::Nil | Value::Bool(false) => "1".to_owned(),
                v => concat_val(&v)?,
            };
            let d = format!(
                "{base}per {div} {}{}",
                actor_prefix(" "),
                format_var_or_list(&tag.get("stat"), &tag.get("statList"))?
            );
            *base_val = Value::Str("".into());
            d
        }
        "PercentStat" => {
            let v = mod_value.ok_or("attempt to perform arithmetic on field 'value'")?;
            let final_percent = (v * (num("percent").unwrap_or(1.0) / 100.0)) * 100.0;
            let base = match num("base") {
                Some(b) => format!("{} + {} ", format_mod_base(m.ty, b), tostring_num(final_percent.abs())),
                None => format_mod_base(m.ty, final_percent),
            };
            let pv = tag.get("percentVar");
            let var = if pv.truthy() { pv } else { tag.get("stat") };
            let d = format!(
                "{base}% of {}{}",
                actor_prefix(" "),
                format_var_or_list(&var, &tag.get("statList"))?
            );
            *base_val = Value::Str("".into());
            d
        }
        "MultiplierThreshold" | "StatThreshold" => {
            let or = |a: Value, b: Value| if a.truthy() { a } else { b };
            let name = format_var_or_list(&or(tag.get("var"), tag.get("stat")), &or(tag.get("varList"), tag.get("statList")))?;
            let cmp = if tag.get("upper").truthy() { " <= " } else { " >= " };
            let pct = match tag.get("thresholdPercent") {
                Value::Nil | Value::Bool(false) => String::new(),
                v => format!("{}% ", concat_val(&v)?),
            };
            let th = match tag.get("threshold") {
                Value::Nil | Value::Bool(false) => format_mod_name(&concat_val(&or(tag.get("thresholdVar"), tag.get("thresholdStat")))?),
                v => concat_val(&v)?,
            };
            format!("If {name}{cmp}{pct}{th}")
        }
        "SkillName" => match tag.get("skillNameList") {
            Value::Table(t) => {
                let p: Vec<String> = t.array.iter().map(concat_val).collect::<Result<_, _>>()?;
                format!("Skill: {}", p.join(" / "))
            }
            _ => format!("Skill: {}", concat_val(&tag.get("skillName"))?),
        },
        "SkillId" => {
            let id = concat_val(&tag.get("skillId"))?;
            format!("Skill: {}", skill_name(skills, &id)?)
        }
        "SkillType" => {
            let t = num("skillType").map(|x| x as u32);
            let name = FLAG_NAMES.with(|(_, _, st)| st.iter().find(|(_, v)| Some(*v) == t).map(|(n, _)| n.clone()));
            match name {
                Some(n) => format!("Skill type: {neg}{}", format_mod_name(&n)),
                None => format!("Skill type: {neg}?"),
            }
        }
        "BaseFlag" => {
            let b = match tag.get("baseFlag") {
                Value::Nil => "nil".to_owned(),
                Value::Bool(b) => b.to_string(),
                v => concat_val(&v)?,
            };
            format!("Base flag: {neg}{}", format_mod_name(&b))
        }
        "SlotNumber" => format!("When in slot #{}", concat_val(&tag.get("num"))?),
        "GlobalEffect" => format_mod_name(&concat_val(&tag.get("effectType"))?),
        "Limit" => match tag.get("limitVar") {
            Value::Nil | Value::Bool(false) => {
                let l = num("limit").ok_or("bad argument #1 to 'abs' (number expected, got nil)")?;
                format!("Limited to {}", format_mod_base(m.ty, l))
            }
            v => format!("Limited to {}", format_mod_name(&concat_val(&v)?)),
        },
        "MonsterTag" => match tag.get("monsterTagList") {
            Value::Table(t) => {
                let p: Vec<String> = t.array.iter().map(concat_val).collect::<Result<_, _>>()?;
                format!("Monster Tag: {}", p.join("/"))
            }
            _ => format!("Monster Tag: {}", concat_val(&tag.get("monsterTag"))?),
        },
        _ => format_mod_name(&ty_s),
    }))
}

/// `baseVal` as an operand of `..`: the `false` of `type(value) == "number" and ...` is a Lua error there.
fn concat_or_false(v: &Value) -> Result<String, String> {
    match v {
        Value::Bool(false) => Err("attempt to concatenate local 'baseVal' (a boolean value)".into()),
        v => concat_val(v),
    }
}

fn skill_name(skills: &pob_data::skills::Skills, id: &str) -> Result<String, String> {
    skills
        .get(id)
        .map(|e| e.name.clone())
        .ok_or_else(|| format!("Classes/CalcBreakdownControl.lua:456: attempt to index field '?' (data.skills['{id}'] is nil)"))
}

thread_local! {
    /// The gate's sample of a process of the original: the input order of a provided `modList` (see [`set_mod_row_order_sample`]).
    static ROW_ORDER_SAMPLE: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// The order of `env.buffs[name]`'s mods comes from `pairs` over string keys and differs between processes of the original (R13; `coc`: 2 orders in 8 processes), and rows
/// of equal name and value keep what the unstable sort makes of it. The gate records the rows of its oracle's process here: `name|flags|keywordFlags|source`.
pub fn set_mod_row_order_sample(sample: Option<Vec<String>>) {
    ROW_ORDER_SAMPLE.with_borrow_mut(|s| *s = sample);
}

/// A `{ value, mod }` row in the form the sort reads.
#[derive(Clone)]
struct ModRow {
    value: Value,
    m: std::rc::Rc<pob_mods::Mod>,
}

fn name_sort(a: &ModRow, b: &ModRow) -> bool {
    let (x, y) = (a.m.name.as_bytes(), b.m.name.as_bytes());
    x > y
        || (x == y
            && match (&a.value, &b.value) {
                (Value::Num(p), Value::Num(q)) => p > q,
                _ => false,
            })
}

/// The inputs of `SetBreakdownData` a section builder needs.
struct BdCtx<'a, 'b> {
    actor: &'b CalcsActor<'a>,
    bd: Option<&'b Breakdown>,
    spec: &'b pob_tree::PassiveSpec,
    skills: &'b pob_data::skills::Skills,
    out: Vec<BdSection>,
}

impl BdCtx<'_, '_> {
    /// :300-535 `AddModSection(sectionData, modList)`.
    fn add_mod_section(&mut self, sd: &Entry, mod_list: Option<&BTable>) -> Result<(), String> {
        let actor = self.actor;
        let mut cfg = match sd.cfg.as_deref() {
            Some(name) if actor.skill_mod_list.is_some() => (actor.cfg)(name).unwrap_or_default(),
            _ => ModCfg::default(),
        };
        cfg.source = sd.mod_source.as_deref().map(pob_mods::Str::from);
        cfg.actor = sd.actor.as_deref().map(pob_mods::Str::from);
        let store: StoreRef = match (&sd.enemy, &actor.enemy_db, &sd.cfg, &actor.skill_mod_list) {
            (Some(true), Some(db), _, _) => db.clone(),
            (Some(true), None, _, _) => return Err("attempt to index field 'enemy' (a nil value)".into()),
            (_, _, Some(_), Some(l)) => l.clone(),
            _ => actor.mod_db.clone(),
        };
        let mod_ty = sd.mod_type.as_deref();
        let mut rows: Vec<ModRow> = match mod_list {
            Some(l) => l
                .list
                .iter()
                .map(|r| match r {
                    BVal::Table(t) => match t.fields.get("mod") {
                        Some(BVal::Mod(m)) => {
                            let value = match t.fields.get("value") {
                                Some(BVal::Num(n)) => Value::Num(*n),
                                Some(BVal::Str(s)) => Value::Str(s.as_str().into()),
                                Some(BVal::Bool(b)) => Value::Bool(*b),
                                _ => m.0.value.clone(),
                            };
                            Ok(ModRow { value, m: m.0.clone() })
                        }
                        _ => Err("a breakdown modList row without a mod".to_owned()),
                    },
                    _ => Err("a breakdown modList entry is not a table".to_owned()),
                })
                .collect::<Result<_, _>>()?,
            None => {
                let ty = match mod_ty {
                    None => None,
                    Some(t) => Some(mod_type(t)?),
                };
                let names: Vec<&str> = sd.mod_name.iter().map(String::as_str).collect();
                let entries: Vec<TabEntry> = store.borrow().tabulate(ty, Some(&cfg), &names).map_err(|e| e.to_string())?;
                entries.into_iter().map(|e| ModRow { value: e.value, m: e.mod_ }).collect()
            }
        };
        if rows.is_empty() {
            return Ok(());
        }
        if mod_list.is_some() {
            ROW_ORDER_SAMPLE.with_borrow(|sample| {
                if let Some(sample) = sample {
                    let place = |r: &ModRow| {
                        let key = format!(
                            "{}|{}|{}|{}",
                            r.m.name,
                            r.m.flags,
                            r.m.keyword_flags,
                            r.m.source.as_deref().unwrap_or("")
                        );
                        sample.iter().position(|k| *k == key).unwrap_or(usize::MAX)
                    };
                    rows.sort_by_key(place);
                }
            });
        }
        let mut section_cols = cols(&[
            ("Value", "displayValue", false),
            ("Stat", "name", false),
            ("Skill types", "flags", false),
            ("Notes", "tags", false),
            ("Source", "source", false),
            ("Source Name", "sourceName", false),
        ]);
        // :339-352
        if mod_list.is_none() && mod_ty.is_none() {
            lua_sort(&mut rows, &mut |a, b| {
                if a.m.ty == b.m.ty {
                    name_sort(a, b)
                } else {
                    type_name(a.m.ty).as_bytes() < type_name(b.m.ty).as_bytes()
                }
            });
        } else {
            lua_sort(&mut rows, &mut name_sort);
        }
        // :354-398 the totals of each modifier source
        let mut source_totals: HashMap<String, Vec<String>> = HashMap::new();
        if mod_list.is_none() && sd.mod_source.is_none() {
            let mut type_list: Vec<&'static str> = Vec::new();
            for r in &rows {
                let t = type_name(r.m.ty);
                if !type_list.contains(&t) {
                    type_list.push(t);
                }
                let src = r.m.source.as_deref().ok_or("attempt to index field 'source' (a nil value)")?;
                source_totals.entry(source_type(src).to_owned()).or_default();
            }
            for (st, lines) in &mut source_totals {
                let mut c = cfg.clone();
                c.source = Some(st.as_str().into());
                for ty in &type_list {
                    let names: Vec<&str> = sd.mod_name.iter().map(String::as_str).collect();
                    let list: Vec<&[&str]> = if sd.mod_name_is_list {
                        names.iter().map(std::slice::from_ref).collect()
                    } else {
                        vec![&names]
                    };
                    for one in list {
                        let total = if *ty == "LIST" {
                            Value::Bool(true) // `self:List(...)`: a table, `value.mod` nil -> "?"
                        } else {
                            combine(&store, ty, &c, one)?
                        };
                        let total = if *ty == "MORE" {
                            match total {
                                Value::Num(n) => Value::Num(((n - 1.0) * 100.0 + 0.5).floor()),
                                other => other,
                            }
                        } else {
                            total
                        };
                        let zero = matches!(total, Value::Num(n) if n == 0.0);
                        if total.truthy() && !zero {
                            let v = format_mod_value(&total, ty)?;
                            if sd.mod_name_is_list {
                                let spaced = pob_lua::gsub(one[0].as_bytes(), b"(%l)(%u)", pob_lua::Repl::Str(b"%1 %2"), None)
                                    .expect("constant pattern")
                                    .0;
                                lines.push(format!("{v} {}", String::from_utf8_lossy(&spaced)));
                            } else {
                                lines.push(v);
                            }
                        }
                    }
                }
            }
        }
        // :400-534
        let mut out_rows = Vec::with_capacity(rows.len());
        for r in &rows {
            let mut row = BdRow::default();
            let m = &*r.m;
            if mod_ty.is_none() {
                row.cells
                    .insert("displayValue".into(), format_mod_value(&r.value, type_name(m.ty))?);
            } else {
                section_cols[0].right = true;
                let v = r
                    .value
                    .as_num()
                    .ok_or_else(|| format!("attempt to perform arithmetic on field 'value' (a {} value)", r.value.type_name()))?;
                row.cells
                    .insert("displayValue".into(), tostring_num((v * 100.0 + 0.5).floor() / 100.0));
            }
            if mod_list.is_some() || sd.mod_name_is_list {
                row.cells.insert("name".into(), format_mod_name(&m.name));
            }
            let src = m.source.as_deref();
            let st = src.map_or("", source_type).to_owned();
            row.cells.insert("source".into(), st.clone());
            if mod_list.is_none() && sd.mod_source.is_none() {
                row.hover.insert(
                    "source".into(),
                    BdHover::SourceTotals(st.clone(), source_totals.get(&st).cloned().unwrap_or_default()),
                );
            }
            let src_s = src.unwrap_or("");
            let after = |prefix: &str| src_s.strip_prefix(prefix);
            match st.as_str() {
                "Item" => {
                    // `row.mod.source:match("Item:(%d+):.+")`
                    let id = after("Item:").and_then(|t| {
                        let (d, rest) = t.split_once(':')?;
                        (!d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()) && !rest.is_empty()).then(|| d.parse::<u64>().ok())?
                    });
                    if let Some(item) = id.and_then(|i| actor.env.items.get(&i)) {
                        let col = color_code(&item.rarity).ok_or_else(|| format!("colorCodes.{} is nil", item.rarity))?;
                        row.cells.insert("sourceName".into(), format!("{col}{}", item.name));
                        row.hover.insert("sourceName".into(), BdHover::Item);
                    }
                }
                "Tree" => {
                    let t = after("Tree:").unwrap_or("");
                    let digits: String = t.bytes().take_while(u8::is_ascii_digit).map(char::from).collect();
                    let text: String = t
                        .bytes()
                        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                        .map(char::from)
                        .collect();
                    if !digits.is_empty() {
                        let id: u32 = digits.parse().map_err(|_| "a node id out of range")?;
                        let (dn, pos) = match self.spec.node(id) {
                            Some(n) => (n.dn().to_owned(), n.tree_node().x.zip(n.tree_node().y)),
                            None => {
                                // `or build.latestTree.nodes[id]` (:440)
                                let latest;
                                let n = match self.spec.tree.node(id) {
                                    Some(n) => n,
                                    None => {
                                        let v = pob_data::tree::tree_versions()
                                            .map_err(|e| e.to_string())?
                                            .latest_tree_version
                                            .clone();
                                        latest = pob_tree::load_passive_tree(&v).map_err(|e| e.0)?;
                                        latest
                                            .node(id)
                                            .ok_or("Classes/CalcBreakdownControl.lua:441: attempt to index local 'node' (a nil value)")?
                                    }
                                };
                                (n.dn.clone(), n.x.zip(n.y))
                            }
                        };
                        row.cells.insert("sourceName".into(), dn);
                        row.hover.insert("sourceName".into(), BdHover::Node(pos));
                    } else if !text.is_empty() {
                        let tree = &self.spec.tree;
                        let name = tree
                            .tattoo()
                            .ok()
                            .and_then(|t| t.nodes.values().find(|n| n.id == text).map(|n| n.dn.clone()))
                            .or_else(|| {
                                tree.legion()
                                    .ok()
                                    .and_then(|l| l.nodes.iter().find(|n| n.id == text).map(|n| n.dn.clone()))
                            });
                        if let Some(n) = name {
                            row.cells.insert("sourceName".into(), n);
                        }
                    }
                }
                "Skill" => {
                    let id = after("Skill:").unwrap_or("");
                    row.cells.insert("sourceName".into(), skill_name(self.skills, id)?);
                }
                "Pantheon" | "Spectre" | "Custom" => {
                    if let Some(rest) = src_s.get(st.len() + 1..).filter(|x| !x.is_empty()) {
                        row.cells.insert("sourceName".into(), rest.to_owned());
                    }
                }
                _ => {}
            }
            if m.flags != 0 || m.keyword_flags != 0 {
                row.cells.insert("flags".into(), flags_text(m.flags, m.keyword_flags));
            }
            if !m.tags.is_empty() {
                let mut base_val = match m.value {
                    Value::Num(n) => Value::Str(format!("{} ", format_mod_base(m.ty, n)).as_str().into()),
                    _ => Value::Bool(false),
                };
                let mut tags: Option<String> = None;
                for tag in &m.tags {
                    if let Some(desc) = tag_desc(m, tag, &mut base_val, self.skills)? {
                        tags = Some(match tags {
                            Some(t) => format!("{t}, {desc}"),
                            None => desc,
                        });
                    }
                }
                if let Some(t) = tags {
                    row.cells.insert("tags".into(), t);
                }
            }
            out_rows.push(row);
        }
        self.out.push(BdSection::Table {
            label: sd.label.clone(),
            footer: None,
            rows: out_rows,
            cols: section_cols,
        });
        Ok(())
    }

    /// :147-297 `AddBreakdownSection(sectionData)`.
    fn add_breakdown_section(&mut self, sd: &Entry, env: BdEnv, is_minion: bool, slot_orders: &mut SlotOrders) -> Result<(), String> {
        let key = sd.breakdown.as_deref().unwrap_or("");
        let Some(bd) = self.bd else {
            return Err("attempt to index field 'breakdown' (a nil value)".into());
        };
        // `^(%a+)%.(%a+)$`
        let alpha = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic());
        let found: Option<&BVal> = match key.split_once('.') {
            Some((ns, name)) if alpha(ns) && alpha(name) => match bd.get(ns) {
                Some(BVal::Module(m)) => m.get(name),
                Some(BVal::Table(t)) => t.fields.get(name),
                Some(_) | None => None,
            },
            _ => bd.get(key),
        };
        let Some(BVal::Table(t)) = found else {
            return Ok(());
        };
        if !t.list.is_empty() {
            let lines = t.list.iter().map(|l| val_text(l).unwrap_or_default()).collect();
            self.out.push(BdSection::Text { lines, text_size: 16.0 });
        }
        if let Some(BVal::Num(radius)) = t.fields.get("radius") {
            self.out.push(BdSection::Radius { radius: *radius });
        }
        let table = |k: &str| match t.fields.get(k) {
            Some(BVal::Table(x)) if !x.list.is_empty() => Some(x),
            _ => None,
        };
        if let Some(rl) = table("rowList") {
            let mut rows: Vec<BTable> = rl
                .list
                .iter()
                .map(|r| match r {
                    BVal::Table(x) => x.clone(),
                    _ => BTable::new(),
                })
                .collect();
            let col_list: Vec<BdCol> = match t.fields.get("colList") {
                Some(BVal::Table(c)) => c
                    .list
                    .iter()
                    .filter_map(|c| match c {
                        BVal::Table(c) => Some(BdCol {
                            label: c.fields.get("label").and_then(val_text).unwrap_or_default(),
                            key: c.fields.get("key").and_then(val_text).unwrap_or_default(),
                            right: matches!(c.fields.get("right"), Some(BVal::Bool(true))),
                        }),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let col_key = col_list.first().map(|c| c.key.clone()).unwrap_or_default();
            if t.fields.get("source").is_some_and(|s| !matches!(s, BVal::Bool(false))) {
                let gt = |a: &BVal, b: &BVal| match (a, b) {
                    (BVal::Num(x), BVal::Num(y)) => x > y,
                    (BVal::Str(x), BVal::Str(y)) => x.as_bytes() > y.as_bytes(),
                    _ => panic!("Classes/CalcBreakdownControl.lua:189: attempt to compare two values of different types"),
                };
                lua_sort(&mut rows, &mut |a, b| {
                    if let Some(an) = a.fields.get("reqNum") {
                        return gt(an, b.fields.get("reqNum").expect("reqNum"));
                    }
                    gt(a.fields.get(&col_key).expect("colKey"), b.fields.get(&col_key).expect("colKey"))
                });
            }
            self.out.push(BdSection::Table {
                label: t.fields.get("label").and_then(val_text),
                footer: t.fields.get("footer").and_then(val_text),
                rows: rows.iter().map(row_of).collect(),
                cols: col_list,
            });
        }
        let simple = |x: &BTable| -> Vec<BdRow> {
            x.list
                .iter()
                .map(|r| match r {
                    BVal::Table(r) => row_of(r),
                    _ => BdRow::default(),
                })
                .collect()
        };
        if let Some(x) = table("reservations") {
            self.out.push(BdSection::Table {
                label: None,
                footer: None,
                rows: simple(x),
                cols: cols(&[
                    ("Skill", "skillName", false),
                    ("Base", "base", false),
                    ("MCM", "mult", false),
                    ("More/less", "more", false),
                    ("Inc/red", "inc", false),
                    ("Efficiency", "efficiency", false),
                    ("Efficiency More/less", "efficiencyMore", false),
                    ("Reservation", "total", false),
                ]),
            });
        }
        if let Some(x) = table("damageTypes") {
            self.out.push(BdSection::Table {
                label: None,
                footer: None,
                rows: simple(x),
                cols: cols(&[
                    ("From", "source", true),
                    ("Base", "base", false),
                    ("Inc/red", "inc", false),
                    ("More/less", "more", false),
                    ("Converted Damage", "convSrc", false),
                    ("Total", "total", false),
                    ("Conversion", "convDst", false),
                    ("Gain", "gainDst", false),
                ]),
            });
        }
        if let Some(x) = table("slots") {
            let items = |r: &BTable| match r.fields.get("source") {
                Some(BVal::Str(s)) if matches!(r.fields.get("item"), Some(BVal::Item(_))) => {
                    self.actor.env.player.borrow().item_list.get(s.as_str()).cloned()
                }
                _ => None,
            };
            let row_t = |r: &BVal| match r {
                BVal::Table(t) => t.clone(),
                _ => BTable::new(),
            };
            let base = |r: &BTable| match r.fields.get("base") {
                Some(BVal::Num(n)) => *n,
                _ => panic!("Classes/CalcBreakdownControl.lua:272: attempt to compare nil with number"),
            };
            let (section_cols, list): (Vec<BdCol>, Vec<BTable>) = if sd.gear_only == Some(true) {
                let list: Vec<BTable> = x
                    .list
                    .iter()
                    .map(row_t)
                    .filter(|r| items(r).is_some_and(|i| i.armour_data.is_some()))
                    .collect();
                let mut idx: Vec<usize> = (0..list.len()).collect();
                lua_sort(&mut idx, &mut |a, b| base(&list[*a]) > base(&list[*b]));
                (
                    cols(&[("Value", "base", true), ("Source", "source", false), ("Name", "sourceLabel", false)]),
                    idx.iter().map(|i| list[*i].clone()).collect(),
                )
            } else {
                let all: Vec<BTable> = x.list.iter().map(row_t).collect();
                let order = slot_orders
                    .entry((env, is_minion, key.to_owned()))
                    .or_insert_with(|| (0..all.len()).collect());
                lua_sort(order, &mut |a, b| base(&all[*a]) > base(&all[*b]));
                (
                    cols(&[
                        ("Base", "base", true),
                        ("Inc/red", "inc", false),
                        ("More/less", "more", false),
                        ("Total", "total", true),
                        ("Source", "source", false),
                        ("Name", "sourceLabel", false),
                    ]),
                    order.iter().map(|i| all[*i].clone()).collect(),
                )
            };
            let mut rows = Vec::with_capacity(list.len());
            for r in &list {
                let mut row = row_of(r);
                if let Some(BVal::Item(_)) = r.fields.get("item") {
                    let item = items(r).ok_or("a slot row's item is not in the actor's item list")?;
                    let col = color_code(&item.rarity).ok_or_else(|| format!("colorCodes.{} is nil", item.rarity))?;
                    row.cells.insert("sourceLabel".into(), format!("{col}{}", item.name));
                    row.hover.insert("sourceLabel".into(), BdHover::Item);
                } else if let Some(s) = r.fields.get("sourceName").and_then(val_text) {
                    row.cells.insert("sourceLabel".into(), s);
                }
                rows.push(row);
            }
            self.out.push(BdSection::Table {
                label: None,
                footer: None,
                rows,
                cols: section_cols,
            });
        }
        if let Some(x) = table("modList") {
            let x = x.clone();
            self.add_mod_section(sd, Some(&x))?;
        }
        Ok(())
    }
}

fn mod_type(t: &str) -> Result<ModType, String> {
    Ok(match t {
        "BASE" => ModType::Base,
        "INC" => ModType::Inc,
        "MORE" => ModType::More,
        "OVERRIDE" => ModType::Override,
        "FLAG" => ModType::Flag,
        "LIST" => ModType::List,
        "MAX" => ModType::Max,
        other => return Err(format!("a mod type {other}")),
    })
}

/// Which actor a breakdown shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForceActor {
    Player,
    Minion,
}

/// The cell description with its flags merged over the row's (Build.lua:1786-1789).
#[derive(Clone, Debug, PartialEq)]
pub struct BreakdownIndexEntry {
    /// `colData`: `(section, subsection, row, cell)`.
    pub col: (usize, usize, usize, usize),
    pub mod_sections_by_name: std::rc::Rc<IndexMap<String, Vec<Entry>>>,
}

/// Build.lua:1764-1810 `BuildBreakdownIndex` (a pure function of the data).
pub fn breakdown_index() -> HashMap<String, BreakdownIndexEntry> {
    let mut index: HashMap<String, BreakdownIndexEntry> = HashMap::new();
    for (si, section) in sections().iter().enumerate() {
        for (ssi, sub) in section.sub_sections.iter().enumerate() {
            let mut by_name: IndexMap<String, Vec<Entry>> = IndexMap::new();
            let mut cells: IndexMap<String, (usize, usize, usize, usize)> = IndexMap::new();
            for (ri, row) in sub.rows.iter().enumerate() {
                for (ci, col) in row.cells.iter().enumerate() {
                    for cell in &col.entries {
                        if cell.skip_side_bar_index != Some(true)
                            && let Some(b) = &cell.breakdown
                            && !cells.contains_key(b)
                        {
                            cells.insert(b.clone(), (si, ssi, ri, ci));
                        }
                        if !cell.mod_name.is_empty() {
                            let mut copy = cell.clone();
                            if copy.label.is_none() {
                                copy.label = row.label.clone();
                            }
                            let mf = |a: &mut Option<String>, b: &Option<String>| {
                                if a.is_none() {
                                    a.clone_from(b);
                                }
                            };
                            mf(&mut copy.flags.flag, &row.flags.flag);
                            mf(&mut copy.flags.player_flag, &row.flags.player_flag);
                            mf(&mut copy.flags.not_flag, &row.flags.not_flag);
                            mf(&mut copy.flags.have_output, &row.flags.have_output);
                            if copy.flags.flag_list.is_none() {
                                copy.flags.flag_list.clone_from(&row.flags.flag_list);
                            }
                            if copy.flags.not_flag_list.is_none() {
                                copy.flags.not_flag_list.clone_from(&row.flags.not_flag_list);
                            }
                            for name in &cell.mod_name {
                                by_name.entry(name.clone()).or_default().push(copy.clone());
                            }
                        }
                    }
                }
            }
            let by_name = std::rc::Rc::new(by_name);
            for (key, col) in cells {
                index.entry(key).or_insert_with(|| BreakdownIndexEntry {
                    col,
                    mod_sections_by_name: by_name.clone(),
                });
            }
        }
    }
    index
}

/// `calcsTab.input` as `CalcsTab:Load` leaves it (:35-36 defaults).
pub fn with_default_input(m: &mut pob_calc::pob_build::BuildModel) {
    if !m.calcs_input.contains_key("skill_number") {
        m.calcs_input.insert("skill_number".into(), InputValue::Num(1.0));
    }
    if !m.calcs_input.contains_key("misc_buffMode") {
        m.calcs_input.insert("misc_buffMode".into(), InputValue::Str("EFFECTIVE".into()));
    }
}

impl Engine {
    /// `calcsTab.input`: the model's, with the values the last CALCS run wrote (CalcSetup.lua:1585-1587 clamps `skill_number`; it is one table upstream).
    pub fn calcs_input(&self) -> IndexMap<String, InputValue> {
        let mut input = crate::save::calcs_input(self.model());
        for (k, v) in &self.calcs_run().env.calcs_input {
            input.insert(k.clone(), v.clone());
        }
        input
    }

    /// `self.input[key] = value` (`nil` removes it): the model and the run's table, one table upstream.
    pub fn set_calcs_input(&mut self, key: &str, value: Option<InputValue>) {
        let (m, c) = self.model_and_calcs_input();
        match value {
            Some(v) => {
                m.insert(key.to_owned(), v.clone());
                c.insert(key.to_owned(), v);
            }
            None => {
                m.shift_remove(key);
                c.shift_remove(key);
            }
        }
    }

    /// `CalcsTab:RestoreUndoState(state)` (:768-773).
    pub fn restore_calcs_input(&mut self, state: &IndexMap<String, InputValue>) {
        let (m, c) = self.model_and_calcs_input();
        m.clone_from(state);
        c.clone_from(state);
    }

    /// The gate: `pairs(mainEnv.allocNodes)`, `pairs(calcsEnv.allocNodes)`.
    pub fn debug_alloc_orders(&self) -> (Vec<u32>, Vec<u32>) {
        (
            self.result().env.alloc_nodes.keys().copied().collect(),
            self.calcs_run().env.alloc_nodes.keys().copied().collect(),
        )
    }

    /// `input.showMinion` (truthy).
    pub fn calcs_show_minion(&self) -> bool {
        matches!(self.calcs_input().get("showMinion"), Some(InputValue::Bool(true)))
    }

    /// `subSection.collapsed` the build loaded (CalcsTab.lua:184-195), `None` = `defaultCollapsed`.
    pub fn calcs_collapsed(&self, section: &str, sub: &str) -> Option<bool> {
        self.extras().calcs_collapsed.get(&(section.to_owned(), sub.to_owned())).copied()
    }

    /// `subsection.collapsed = v`, which `CalcsTab:Save` writes (:215-223).
    pub fn set_calcs_collapsed(&mut self, section: &str, sub: &str, v: bool) {
        self.extras_mut().calcs_collapsed.insert((section.to_owned(), sub.to_owned()), v);
    }

    /// The group behind `input.skill_number`, in the model.
    fn calcs_group_index(&self) -> usize {
        match self.calcs_input().get("skill_number") {
            Some(InputValue::Num(n)) => *n as usize,
            _ => 1,
        }
    }

    /// The model's `<Gem>` behind `displaySkillListCalcs[mainActiveSkillCalcs].activeEffect.srcInstance` (CalcsTab.lua:69-71).
    fn calcs_src_gem(&mut self) -> Result<&mut pob_calc::pob_build::GemModel, String> {
        let (g, k) = {
            let st = self.calcs_run().env.skills.as_ref().ok_or("no skills")?;
            let list = st.tab.socket_group_list();
            let group = list
                .get(self.calcs_group_index().wrapping_sub(1))
                .ok_or("Classes/CalcsTab.lua:69: attempt to index local 'mainSocketGroup' (a nil value)")?;
            let display = group.display_skill_list.as_ref().ok_or("displaySkillListCalcs is nil")?;
            let mas = group.main_active_skill_calcs.unwrap_or(1.0);
            let si = *display
                .get((mas as usize).wrapping_sub(1))
                .ok_or("Classes/CalcsTab.lua:70: attempt to index field '?' (a nil value)")?;
            st.skill(si)
                .active_effect
                .borrow()
                .src_instance
                .ok_or("Classes/CalcsTab.lua:70: attempt to index field 'srcInstance' (a nil value)")?
        };
        if self.model_has_runtime_groups() {
            self.materialize()?;
        }
        let set = crate::engine::active_skill_set_mut(self.model_mut()).ok_or("no active skill set")?;
        set.groups
            .get_mut(g)
            .and_then(|grp| grp.gems.get_mut(k))
            .ok_or_else(|| format!("socket group {} gem {} is not in the saved state", g + 1, k + 1))
    }

    /// `controls.mainSkill` selFunc of the tab (:61-65).
    pub fn set_main_active_skill_calcs(&mut self, index: f64) -> Result<(), String> {
        let g = self.calcs_group_index();
        if self.model_has_runtime_groups() {
            self.materialize()?;
        }
        let set = crate::engine::active_skill_set_mut(self.model_mut()).ok_or("no active skill set")?;
        let grp = set
            .groups
            .get_mut(g.wrapping_sub(1))
            .ok_or("Classes/CalcsTab.lua:62: attempt to index local 'mainSocketGroup' (a nil value)")?;
        grp.main_active_skill_calcs = index;
        Ok(())
    }

    /// :68-74 `srcInstance.skillPartCalcs = index`.
    pub fn set_skill_part_calcs(&mut self, index: f64) -> Result<(), String> {
        self.calcs_src_gem()?.skill_part_calcs = Some(index);
        Ok(())
    }

    /// :76-82 `srcInstance.skillStageCountCalcs = tonumber(buf)`.
    pub fn set_skill_stage_count_calcs(&mut self, count: Option<f64>) -> Result<(), String> {
        self.calcs_src_gem()?.skill_stage_count_calcs = count;
        Ok(())
    }

    /// :85-91 `srcInstance.skillMineCountCalcs = tonumber(buf)`.
    pub fn set_skill_mine_count_calcs(&mut self, count: Option<f64>) -> Result<(), String> {
        self.calcs_src_gem()?.skill_mine_count_calcs = count;
        Ok(())
    }

    /// :100-110 `skillMinionItemSetCalcs` / `skillMinionCalcs`.
    pub fn set_skill_minion_calcs(&mut self, value: &MinionEntry) -> Result<(), String> {
        let gem = self.calcs_src_gem()?;
        match value {
            MinionEntry::ItemSet { item_set_id, .. } => gem.skill_minion_item_set_calcs = Some(*item_set_id),
            MinionEntry::Minion { minion_id, .. } => gem.skill_minion_calcs = Some(minion_id.clone()),
            MinionEntry::Text(_) => gem.skill_minion_calcs = None,
        }
        Ok(())
    }

    /// :118-124 `srcInstance.skillMinionSkillCalcs = index`.
    pub fn set_skill_minion_skill_calcs(&mut self, index: f64) -> Result<(), String> {
        self.calcs_src_gem()?.skill_minion_skill_calcs = Some(index);
        Ok(())
    }

    /// `env.<actor>.breakdown[key].breakdownSource or key` (Build.lua:1817-1819, MAIN env).
    pub fn breakdown_source(&self, key: &str, actor_name: Option<&str>) -> String {
        let res = self.result();
        let of = |bd: Option<&Breakdown>| match bd.and_then(|b| b.get(key)) {
            Some(BVal::Table(t)) => match t.fields.get("breakdownSource") {
                Some(BVal::Str(s)) => Some(s.clone()),
                _ => None,
            },
            _ => None,
        };
        let found = match actor_name {
            Some("player") => of(res.perform.player.breakdown.as_ref()),
            Some("minion") => res.perform.minion.as_ref().and_then(|m| of(m.borrow().breakdown.as_ref())),
            _ => None,
        };
        found.unwrap_or_else(|| key.to_owned())
    }

    /// `SetBreakdownData(displayData, pinned, forceActor)`'s data part (CalcBreakdownControl.lua:72-83): the sections of the cell descriptions.
    /// `slot_orders` keeps the in-place sort of `breakdown.slots` of this run (reset by the caller when `outputRevision` moves).
    pub fn breakdown_sections(
        &self,
        env: BdEnv,
        force: Option<ForceActor>,
        entries: &[Entry],
        slot_orders: &mut SlotOrders,
    ) -> Result<Vec<BdSection>, String> {
        let show_minion = self.calcs_show_minion();
        let res = self.result();
        let run = self.calcs_run();
        let (e, st, skills_state) = match env {
            BdEnv::Calcs => (&run.env, &run.state, run.env.skills.as_ref()),
            BdEnv::Main => (&res.env, &res.perform, res.env.skills.as_ref()),
        };
        let skills = skills_state.ok_or("calcs: no skill phase")?.skills.clone();
        let spec = res.env.spec.clone();
        let player = player_actor(e, &st.player)?;
        let want_minion = match force {
            Some(ForceActor::Minion) => true,
            Some(ForceActor::Player) => false,
            None => show_minion && e.minion.is_some(),
        };
        let run_entries = |actor: &CalcsActor<'_>, bd: Option<&Breakdown>, is_minion: bool, slot_orders: &mut SlotOrders| {
            let ctx = CheckCtx {
                actor,
                player_flags: player.skill_flags,
            };
            let mut c = BdCtx {
                actor,
                bd,
                spec: &spec,
                skills: &skills,
                out: Vec::new(),
            };
            for sd in entries {
                if check_flag(&ctx, &sd.flags)? {
                    if sd.breakdown.is_some() {
                        c.add_breakdown_section(sd, env, is_minion, slot_orders)?;
                    } else if !sd.mod_name.is_empty() {
                        c.add_mod_section(sd, None)?;
                    }
                }
            }
            Ok::<_, String>(c.out)
        };
        if want_minion {
            let ms = st.minion.as_ref().ok_or("attempt to index local 'actor' (a nil value)")?.borrow();
            let minion = minion_actor(e, &ms)?.ok_or("attempt to index local 'actor' (a nil value)")?;
            run_entries(&minion, ms.breakdown.as_ref(), true, slot_orders)
        } else {
            run_entries(&player, st.player.breakdown.as_ref(), false, slot_orders)
        }
    }
}
