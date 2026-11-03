//! `Modules/StatDescriber.lua` (`data.describeStats`) over the checkout's `Data/StatDescriptions/*.lua`, read as they are (U5: no conversion into `data/*`; the
//! gem tooltips are the UI's only reader of those files).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use pob_lua::format::{Arg, format};
use pob_lua::{Repl, gsub};

use crate::lua_data::{LuaKey, LuaTab, LuaVal, parse_return};

#[derive(Clone, Copy, Debug)]
enum Limit {
    Any,
    Not,
    Num(f64),
}

struct Lang {
    limit: Vec<(Limit, Limit)>,
    specials: Vec<(String, usize)>,
    gem_quality: bool,
    text: String,
}

struct Desc {
    stats: Vec<String>,
    /// `descriptor[1]`.
    lang: Option<Vec<Lang>>,
}

pub(crate) struct Scope {
    by_stat: HashMap<String, usize>,
    descs: HashMap<usize, Arc<Desc>>,
    parent: Option<String>,
}

thread_local! {
    static SCOPES: RefCell<HashMap<String, Arc<Scope>>> = RefCell::new(HashMap::new());
    static PEARL: RefCell<Option<Rc<LuaVal>>> = const { RefCell::new(None) };
}

fn limit_of(v: Option<&LuaVal>) -> Limit {
    match v {
        Some(LuaVal::Num(n)) => Limit::Num(*n),
        Some(LuaVal::Str(s)) if &**s == "!" => Limit::Not,
        _ => Limit::Any,
    }
}

fn lang_of(t: &LuaTab) -> Lang {
    let limit = t.tab("limit").map_or_else(Vec::new, |l| {
        l.seq()
            .iter()
            .map(|p| {
                let p = p.as_tab().expect("limit pair");
                (limit_of(p.get_num(1.0)), limit_of(p.get_num(2.0)))
            })
            .collect()
    });
    let specials = t
        .seq()
        .iter()
        .filter_map(|s| {
            let s = s.as_tab()?;
            Some((s.str("k")?.to_string(), s.num("v").unwrap_or(1.0) as usize))
        })
        .collect();
    Lang {
        limit,
        specials,
        gem_quality: t
            .get("gem_quality")
            .is_some_and(|v| !matches!(v, LuaVal::Nil | LuaVal::Bool(false))),
        text: t.str("text").unwrap_or("").to_string(),
    }
}

/// The scopes [`read_all_scopes`] read, by name.
pub(crate) type Scopes = HashMap<String, Arc<Scope>>;

/// Every file of `Data/StatDescriptions/`: for a thread of its own. A file it cannot read is left to [`load_scope`], which reports it.
pub(crate) fn read_all_scopes(root: &str) -> Scopes {
    let mut out = HashMap::new();
    for e in std::fs::read_dir(format!("{root}Data/StatDescriptions"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(name) = name.strip_suffix(".lua")
            && let Ok(src) = std::fs::read(e.path())
            && let Ok(v) = parse_return(&src)
        {
            out.insert(name.to_string(), Arc::new(scope_of(&v)));
        }
    }
    out
}

pub(crate) fn install_scopes(scopes: Scopes) {
    SCOPES.with_borrow_mut(|m| m.extend(scopes));
}

fn load_scope(name: &str) -> Arc<Scope> {
    crate::gem_tooltip::join_preload();
    if let Some(s) = SCOPES.with_borrow(|m| m.get(name).cloned()) {
        return s;
    }
    let root = crate::launch::launch().cfg.script_path.clone();
    let path = format!("{root}Data/StatDescriptions/{name}.lua");
    let src = std::fs::read(&path).unwrap_or_else(|e| panic!("Modules/StatDescriber.lua:15: {path}: {e}"));
    let v = parse_return(&src).unwrap_or_else(|e| panic!("{path}: {e}"));
    let scope = Arc::new(scope_of(&v));
    SCOPES.with_borrow_mut(|m| m.insert(name.to_string(), scope.clone()));
    scope
}

fn scope_of(v: &LuaVal) -> Scope {
    let t = v.as_tab().expect("a scope is a table");
    let mut by_stat = HashMap::new();
    let mut descs = HashMap::new();
    let mut add = |order: usize, d: &LuaTab| {
        let stats = d.tab("stats").map_or_else(Vec::new, |s| {
            s.seq().iter().filter_map(|x| x.as_str().map(str::to_string)).collect()
        });
        let lang = d
            .get_num(1.0)
            .and_then(LuaVal::as_tab)
            .map(|l| l.seq().iter().filter_map(|e| e.as_tab().map(|e| lang_of(e))).collect());
        descs.insert(order, Arc::new(Desc { stats, lang }));
    };
    for (i, d) in t.arr.iter().enumerate() {
        if let Some(d) = d.as_tab() {
            add(i + 1, d);
        }
    }
    let mut parent = None;
    for (k, val) in &t.fields {
        match k {
            LuaKey::Num(n) => {
                if let Some(d) = val.as_tab() {
                    add(*n as usize, d);
                }
            }
            LuaKey::Str(s) => {
                if &**s == "parent" {
                    parent = val.as_str().map(str::to_string);
                } else if let Some(n) = val.as_num() {
                    by_stat.insert(s.to_string(), n as usize);
                }
            }
        }
    }
    Scope { by_stat, descs, parent }
}

/// `scope.scopeList`: the scope, then its parents.
fn scope_list(name: &str) -> Vec<Arc<Scope>> {
    let mut out = Vec::new();
    let mut cur = Some(name.to_string());
    while let Some(n) = cur.take() {
        let s = load_scope(&n);
        cur = s.parent.clone();
        out.push(s);
    }
    out
}

#[derive(Clone, Debug)]
enum VVal {
    Num(f64),
    Str(String),
}

#[derive(Clone, Debug)]
struct Val {
    min: VVal,
    max: VVal,
    fmt: &'static str,
}

impl Val {
    fn num(&self) -> (f64, f64) {
        match (&self.min, &self.max) {
            (VVal::Num(a), VVal::Num(b)) => (*a, *b),
            _ => panic!("Modules/StatDescriber.lua: attempt to perform arithmetic on a string value"),
        }
    }
    fn set(&mut self, a: f64, b: f64) {
        self.min = VVal::Num(a);
        self.max = VVal::Num(b);
    }
}

fn round(v: f64, dec: f64) -> f64 {
    let p = 10f64.powf(dec);
    (v * p + 0.5).floor() / p
}

/// `matchLimit` (:31-51).
fn match_limit<'a>(lang: &'a [Lang], val: &[Val], quality: bool) -> Option<&'a Lang> {
    'desc: for desc in lang {
        if quality || !desc.gem_quality {
            for (i, (a, b)) in desc.limit.iter().enumerate() {
                let m = val[i].num().0;
                if let Limit::Not = a {
                    if let Limit::Num(n) = b
                        && m == *n
                    {
                        continue 'desc;
                    }
                } else {
                    if let Limit::Num(n) = b
                        && m > *n
                    {
                        continue 'desc;
                    }
                    if let Limit::Num(n) = a
                        && m < *n
                    {
                        continue 'desc;
                    }
                }
            }
            return Some(desc);
        }
    }
    None
}

/// `applySpecial` (:54-205).
#[allow(clippy::too_many_lines)]
fn apply_special(val: &mut [Val], k: &str, v: usize) {
    let i = v - 1;
    let div = |val: &mut [Val], d: f64, g: bool| {
        let (a, b) = val[i].num();
        val[i].set(a / d, b / d);
        if g {
            val[i].fmt = "g";
        }
    };
    let rnd = |val: &mut [Val], d: f64, dp: f64| {
        let (a, b) = val[i].num();
        val[i].set(round(a / d, dp), round(b / d, dp));
        val[i].fmt = "g";
    };
    let mul = |val: &mut [Val], m: f64| {
        let (a, b) = val[i].num();
        val[i].set(a * m, b * m);
    };
    match k {
        "negate" => {
            let (a, b) = val[i].num();
            val[i].set(-b, -a);
        }
        "invert_chance" => {
            let (a, b) = val[i].num();
            // `val.max, val.min = 100 - min, 100 - max`
            val[i].set(100.0 - b, 100.0 - a);
        }
        "negate_and_double" => {
            let (a, b) = val[i].num();
            val[i].set(-2.0 * b, -2.0 * a);
        }
        "divide_by_two_0dp" => div(val, 2.0, false),
        "divide_by_ten_0dp" => div(val, 10.0, false),
        "divide_by_fifteen_0dp" => div(val, 15.0, false),
        "divide_by_four" => div(val, 4.0, true),
        "divide_by_five" => div(val, 5.0, true),
        "divide_by_six" => div(val, 6.0, true),
        "divide_by_ten_1dp_if_required" | "divide_by_ten_1dp" => rnd(val, 10.0, 1.0),
        "divide_by_twelve" => div(val, 12.0, true),
        "divide_by_twenty" => div(val, 20.0, true),
        "divide_by_one_hundred" => div(val, 100.0, true),
        "divide_by_one_hundred_1dp" => rnd(val, 100.0, 1.0),
        "divide_by_one_hundred_2dp_if_required" | "divide_by_one_hundred_2dp" => rnd(val, 100.0, 2.0),
        "divide_by_one_hundred_and_negate" => {
            let (a, b) = val[i].num();
            val[i].set(-round(a / 100.0, 1.0), -round(b / 100.0, 1.0));
            val[i].fmt = "g";
        }
        "divide_by_twenty_then_double_0dp" => {
            let (a, b) = val[i].num();
            val[i].set(round(a / 20.0, 0.0) * 2.0, round(b / 20.0, 0.0) * 2.0);
        }
        "divide_by_one_thousand" => div(val, 1000.0, true),
        "per_minute_to_per_second" | "per_minute_to_per_second_1dp" => rnd(val, 60.0, 1.0),
        "permyriad_per_minute_to_%_per_second" => {
            let (a, b) = val[i].num();
            val[i].set(round(a / 60.0 / 100.0, 1.0), round(b / 60.0 / 100.0, 1.0));
            val[i].fmt = "g";
        }
        "per_minute_to_per_second_0dp" => div(val, 60.0, false),
        "per_minute_to_per_second_2dp" | "per_minute_to_per_second_2dp_if_required" => rnd(val, 60.0, 2.0),
        "milliseconds_to_seconds" => div(val, 1000.0, true),
        "milliseconds_to_seconds_0dp" => div(val, 1000.0, false),
        "milliseconds_to_seconds_1dp" => rnd(val, 1000.0, 1.0),
        "milliseconds_to_seconds_2dp" | "milliseconds_to_seconds_2dp_if_required" => rnd(val, 1000.0, 2.0),
        "deciseconds_to_seconds" | "locations_to_metres" => div(val, 10.0, true),
        "30%_of_value" => mul(val, 0.3),
        "60%_of_value" => mul(val, 0.6),
        "multiplicative_damage_modifier" => {
            let (a, b) = val[i].num();
            val[i].set(100.0 + a, 100.0 + b);
        }
        "multiplicative_permyriad_damage_modifier" => {
            let (a, b) = val[i].num();
            val[i].set(100.0 + round(a / 100.0, 1.0), 100.0 + round(b / 100.0, 1.0));
            val[i].fmt = "g";
        }
        "multiply_by_four" => mul(val, 4.0),
        "times_one_point_five" => mul(val, 1.5),
        "times_twenty" => mul(val, 20.0),
        "double" => mul(val, 2.0),
        "plus_two_hundred" => {
            let (a, b) = val[i].num();
            val[i].set(a + 200.0, b + 200.0);
        }
        "reminderstring" | "canonical_line" | "canonical_stat" | "_stat" => {}
        "display_indexable_non_active_support" => {
            let n = val[i].num().0;
            let text = pearl_support_text(n);
            val[i].fmt = "s";
            val[i].min = VVal::Str(text.clone());
            val[i].max = VVal::Str(text);
        }
        // `ConPrintf("Unknown description function: %s", spec.k)`: the console only
        _ => {}
    }
}

/// `indexableNonActiveSupports[n].baseItemName:gsub(" Support", "")` (`Data/PearlSupports.lua`).
fn pearl_support_text(n: f64) -> String {
    let v = PEARL.with_borrow_mut(|p| {
        p.get_or_insert_with(|| {
            let root = crate::launch::launch().cfg.script_path.clone();
            let src = std::fs::read(format!("{root}Data/PearlSupports.lua")).expect("Data/PearlSupports.lua");
            Rc::new(parse_return(&src).expect("Data/PearlSupports.lua"))
        })
        .clone()
    });
    v.as_tab()
        .and_then(|t| t.get_num(n))
        .and_then(LuaVal::as_tab)
        .and_then(|g| g.str("baseItemName").map(|s| s.replace(" Support", "")))
        .unwrap_or_default()
}

fn fmt_one(fmt: &str, v: &VVal) -> String {
    let spec = format!("%{fmt}");
    match v {
        VVal::Num(n) => format(&spec, &[Arg::Num(*n)]).unwrap_or_else(|e| panic!("{e}")),
        VVal::Str(s) => format(&spec, &[Arg::Str(s)]).unwrap_or_else(|e| panic!("{e}")),
    }
}

fn same(a: &VVal, b: &VVal) -> bool {
    match (a, b) {
        (VVal::Num(x), VVal::Num(y)) => x == y,
        (VVal::Str(x), VVal::Str(y)) => x == y,
        _ => false,
    }
}

fn plain(v: &Val) -> String {
    if same(&v.min, &v.max) {
        fmt_one(v.fmt, &v.min)
    } else {
        format!("({}-{})", fmt_one(v.fmt, &v.min), fmt_one(v.fmt, &v.max))
    }
}

/// The chain of `gsub`s of :257-291.
fn describe_text(text: &str, val: &[Val]) -> String {
    let cap_str = |s: &[u8], c: &pob_lua::Captures, i: usize| -> String {
        String::from_utf8_lossy(c.get(i).and_then(|x| x.bytes(s)).unwrap_or(b"")).into_owned()
    };
    let run = |s: &[u8], pat: &[u8], f: &mut dyn FnMut(&[u8], &pob_lua::Captures) -> Vec<u8>| -> Vec<u8> {
        let mut g = |a: &[u8], c: &pob_lua::Captures| Some(f(a, c));
        gsub(s, pat, Repl::Fn(&mut g), None).unwrap_or_else(|e| panic!("{e:?}")).0
    };
    let s = run(text.as_bytes(), b"{(%d)}", &mut |a, c| {
        let n: usize = cap_str(a, c, 0).parse().expect("digit");
        plain(&val[n]).into_bytes()
    });
    let s = run(&s, b"{}", &mut |_, _| plain(&val[0]).into_bytes());
    let s = run(&s, b"{:%+?d}", &mut |_, _| plain(&val[0]).into_bytes());
    let s = run(&s, b"{(%d):(%+?)d}", &mut |a, c| {
        let n: usize = cap_str(a, c, 0).parse().expect("digit");
        let plus = cap_str(a, c, 1);
        let v = &val[n];
        if same(&v.min, &v.max) {
            fmt_one(&format!("{plus}{}", v.fmt), &v.min).into_bytes()
        } else if plus == "+" {
            let (lo, hi) = v.num();
            if hi < 0.0 {
                format!(
                    "-({}-{})",
                    format("%d", &[Arg::Num(-lo)]).expect("format"),
                    format("%d", &[Arg::Num(-hi)]).expect("format")
                )
                .into_bytes()
            } else {
                format!(
                    "+({}-{})",
                    format("%d", &[Arg::Num(lo)]).expect("format"),
                    format("%d", &[Arg::Num(hi)]).expect("format")
                )
                .into_bytes()
            }
        } else {
            let f = format!("{plus}{}", v.fmt);
            format!("({}-{})", fmt_one(&f, &v.min), fmt_one(&f, &v.max)).into_bytes()
        }
    });
    let s = gsub(&s, b"%%%%", Repl::Str(b"%%"), None).unwrap_or_else(|e| panic!("{e:?}")).0;
    String::from_utf8_lossy(&s).into_owned()
}

/// `data.describeStats(stats, scopeName, quality)` (:207-299): the lines and `lineMap` (line -> the last described stat of its description).
pub fn describe_stats(stats: &[(String, f64)], scope_name: &str, quality: bool) -> (Vec<String>, HashMap<String, String>) {
    let scopes = scope_list(scope_name);
    let lookup = |s: &str| stats.iter().find(|(k, _)| k == s).map(|(_, v)| *v);
    let mut described: HashMap<String, (usize, usize, Arc<Desc>)> = HashMap::new();
    for (s, v) in stats {
        if *v != 0.0 {
            for (depth, scope) in scopes.iter().enumerate() {
                if let Some(&order) = scope.by_stat.get(s) {
                    if let Some(d) = scope.descs.get(&order)
                        && d.lang.is_some()
                        && let Some(first) = d.stats.first()
                    {
                        described.insert(first.clone(), (depth + 1, order, d.clone()));
                    }
                    break;
                }
            }
        }
    }
    let mut ordered: Vec<(usize, usize, Arc<Desc>)> = described.into_values().collect();
    ordered.sort_by(|a, b| if a.0 == b.0 { a.1.cmp(&b.1) } else { b.0.cmp(&a.0) });
    let mut out = Vec::new();
    let mut line_map = HashMap::new();
    for (_, _, d) in ordered {
        let mut stat = None;
        let mut val: Vec<Val> = d
            .stats
            .iter()
            .map(|s| {
                let n = match lookup(s) {
                    Some(v) => {
                        stat = Some(s.clone());
                        v
                    }
                    None => 0.0,
                };
                Val {
                    min: VVal::Num(n),
                    max: VVal::Num(n),
                    fmt: "d",
                }
            })
            .collect();
        let lang = d.lang.as_ref().expect("checked");
        if let Some(desc) = match_limit(lang, &val, quality) {
            for (k, v) in &desc.specials {
                apply_special(&mut val, k, *v);
            }
            let text = describe_text(&desc.text, &val);
            let joined = format!("{text}\\n");
            let bytes = joined.as_bytes();
            for c in pob_lua::gmatch(bytes, b"([^\\]+)\\n") {
                let c = c.unwrap_or_else(|e| panic!("{e:?}"));
                let line = String::from_utf8_lossy(c.get(0).and_then(|x| x.bytes(bytes)).unwrap_or(b"")).into_owned();
                if let Some(s) = &stat {
                    line_map.insert(line.clone(), s.clone());
                } else {
                    line_map.remove(&line);
                }
                out.push(line);
            }
        }
    }
    (out, line_map)
}
