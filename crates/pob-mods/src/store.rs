//! `Classes/ModStore.lua` (the shared base: query wrappers, mutators, GetCondition/GetMultiplier/GetStat), `Classes/ModDB.lua`
//! (mods bucketed by name) and `Classes/ModList.lua` (a flat list). Aggregation semantics: MOD-CORE §2.5.
//!
//! One struct for both stores (orchestrator decision 4): the shared part as in ModStore.lua:40-46 plus `StoreKind`. Parent
//! chains and the actor graph go through `Rc<RefCell<..>>`: queries take shared borrows all the way down (they never mutate
//! anything -- see `tag.div` in eval.rs), mutators take short mutable ones.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_lua::number::{max2, min2, modf, round, tobit};

use crate::data;
use crate::matching::{Query, compare_mod_params, mod_high_precision, mod_matches};
use crate::types::{Actor, ActorRef, Mod, ModCfg, ModType};
use crate::value::{LuaError, LuaResult, Str, Table, Value, arith, check_num, err_bad_arg, err_compare, err_op};

pub type StoreRef = Rc<RefCell<ModStore>>;

/// One table per Sum/More/Tabulate call, shared by every tagged mod of that call (ModDB.lua:146, MOD-CORE §3).
pub type GlobalLimits = IndexMap<Str, f64>;

/// `{ value = value, mod = mod }` (ModDB.lua:296).
#[derive(Clone, Debug)]
pub struct TabEntry {
    pub value: Value,
    pub mod_: Rc<Mod>,
}

/// ModDB.lua: `self.mods[name] = { mod, ... }`. The bucket order is the insertion order of names (Dump sorts them; Sum walks
/// the names it is given), and the order inside a bucket IS the summation order (MINEFIELD L10, MOD-CORE §7.8).
#[derive(Clone, Debug, Default)]
pub struct ModDb {
    pub mods: IndexMap<Str, Vec<Rc<Mod>>>,
}

/// ModList.lua: the mods in the object's own array part.
#[derive(Clone, Debug, Default)]
pub struct ModList {
    pub mods: Vec<Rc<Mod>>,
}

pub enum StoreKind {
    Db(ModDb),
    List(ModList),
}

/// ModStore.lua:40-46 plus the concrete storage.
pub struct ModStore {
    /// Lua keeps `parent or false`; `None` is the same test (L12).
    pub parent: Option<StoreRef>,
    /// ModStore.lua:42: with a parent this IS the parent's actor table, shared.
    pub actor: ActorRef,
    pub multipliers: IndexMap<Str, f64>,
    pub conditions: IndexMap<Str, bool>,
    pub kind: StoreKind,
}

const MS: &str = "Classes/ModStore.lua";

/// Lua `x or 0` on an EvalMod result.
fn or0(v: Value) -> Value {
    if v.truthy() { v } else { Value::Num(0.0) }
}

/// The mods of one queried name: a ModDB bucket, or a linear scan of a ModList (`mod.name == modName` is the first test of
/// the ModList `and`-chain, ModList.lua:107).
enum Named<'a> {
    Db(std::slice::Iter<'a, Rc<Mod>>),
    List(std::slice::Iter<'a, Rc<Mod>>, &'a str),
}

impl<'a> Iterator for Named<'a> {
    type Item = &'a Rc<Mod>;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Named::Db(it) => it.next(),
            Named::List(it, name) => it.find(|m| &*m.name == *name),
        }
    }
}

/// ModStore.lua:165-171, repeated in every public wrapper: `flags = cfg.flags or 0` etc. Flags enter `band` as 32-bit ints.
fn query<'a>(ty: Option<ModType>, cfg: Option<&'a ModCfg>) -> Query<'a> {
    match cfg {
        None => Query {
            ty,
            flags: 0,
            keyword_flags: 0,
            source: None,
        },
        Some(c) => Query {
            ty,
            flags: tobit(c.flags.unwrap_or(0.0)) as u32,
            keyword_flags: tobit(c.keyword_flags.unwrap_or(0.0)) as u32,
            source: c.source.as_deref(),
        },
    }
}

/// Lua `a < b` for the values Min compares (numbers, or strings in C-locale byte order); anything else the compare error.
fn lua_lt(a: &Value, b: &Value, site: &str) -> LuaResult<bool> {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => Ok(x < y),
        (Value::Str(x), Value::Str(y)) => Ok(x.as_bytes() < y.as_bytes()),
        _ => Err(err_compare(site, a, b)),
    }
}

impl ModStore {
    fn new(parent: Option<StoreRef>, kind: StoreKind) -> StoreRef {
        // ModStore.lua:42 `self.actor = parent and parent.actor or { }`
        let actor = parent
            .as_ref()
            .map_or_else(|| Rc::new(RefCell::new(Actor::default())), |p| p.borrow().actor.clone());
        Rc::new(RefCell::new(ModStore {
            parent,
            actor,
            multipliers: IndexMap::new(),
            conditions: IndexMap::new(),
            kind,
        }))
    }

    /// `new("ModDB"):ModDB(parent)` (ModDB.lua:23).
    pub fn new_db(parent: Option<StoreRef>) -> StoreRef {
        ModStore::new(parent, StoreKind::Db(ModDb::default()))
    }

    /// `new("ModList"):ModList(parent)` (ModList.lua:22).
    pub fn new_list(parent: Option<StoreRef>) -> StoreRef {
        ModStore::new(parent, StoreKind::List(ModList::default()))
    }

    pub fn is_db(&self) -> bool {
        matches!(self.kind, StoreKind::Db(_))
    }

    fn named<'a>(&'a self, name: &'a str) -> Named<'a> {
        match &self.kind {
            StoreKind::Db(db) => Named::Db(db.mods.get(name).map_or([].iter(), |b| b.iter())),
            StoreKind::List(l) => Named::List(l.mods.iter(), name),
        }
    }

    /// `select(ModDB, ModList)` of an upstream site string.
    fn site(&self, db: &'static str, list: &'static str) -> &'static str {
        if self.is_db() { db } else { list }
    }

    // --- storage (ModDB.lua:29-134, ModList.lua:27-99) ----------------------------------------------------------------

    /// ModDB.lua:29 / ModList.lua:27 -- the mod object itself, not a copy.
    pub fn add_mod(&mut self, m: Rc<Mod>) {
        match &mut self.kind {
            StoreKind::Db(db) => db.mods.entry(m.name.clone()).or_default().push(m),
            StoreKind::List(l) => l.mods.push(m),
        }
    }

    /// ModDB.lua:112 / ModList.lua:88 -- the same objects, in order.
    pub fn add_list(&mut self, mods: &[Rc<Mod>]) {
        for m in mods {
            self.add_mod(m.clone());
        }
    }

    /// ModDB.lua:123 `AddDB`: every bucket of the source appended to ours, same objects. `pairs(modDB.mods)` order only
    /// decides which bucket is touched first -- never the order inside one (L10). ModList has no AddDB.
    pub fn add_db(&mut self, src: &ModDb) -> LuaResult<()> {
        let StoreKind::Db(db) = &mut self.kind else {
            return Err(missing_method("AddDB"));
        };
        for (name, list) in &src.mods {
            db.mods.entry(name.clone()).or_default().extend(list.iter().cloned());
        }
        Ok(())
    }

    /// ModDB.lua:42 / ModList.lua:36. ModDB marks the NEW mod `replaced` and skips already-replaced ones, so replacing twice
    /// hits the second slot (§7.3), and creates the bucket even when nothing is replaced; ModList has no marker.
    pub fn replace_mod_internal(&mut self, m: &Rc<Mod>) -> bool {
        let found = match &mut self.kind {
            StoreKind::Db(db) => {
                let list = db.mods.entry(m.name.clone()).or_default();
                match list
                    .iter()
                    .position(|c| m.name == c.name && m.same_identity_but_name(c) && !c.replaced.get())
                {
                    Some(i) => {
                        m.replaced.set(true);
                        list[i] = m.clone();
                        true
                    }
                    None => false,
                }
            }
            StoreKind::List(l) => match l.mods.iter().position(|c| m.name == c.name && m.same_identity_but_name(c)) {
                Some(i) => {
                    l.mods[i] = m.clone();
                    true
                }
                None => false,
            },
        };
        if found {
            return true;
        }
        match &self.parent {
            Some(p) => p.borrow_mut().replace_mod_internal(m),
            None => false,
        }
    }

    /// ModDB.lua:80 / ModList.lua:58. ModDB `t_remove`s from the old bucket, marks the mod `converted` and appends it to the
    /// new one; ModList replaces in place without a marker.
    pub fn convert_mod_internal(&mut self, old_name: &str, m: &Rc<Mod>) -> bool {
        let found = match &mut self.kind {
            StoreKind::Db(db) => match db.mods.get_mut(old_name) {
                // ModDB.lua:81-86: no bucket -> straight to the parent.
                None => false,
                Some(old) => match old
                    .iter()
                    .position(|c| &*c.name == old_name && m.same_identity_but_name(c) && !c.converted.get())
                {
                    Some(i) => {
                        old.remove(i);
                        m.converted.set(true);
                        db.mods.entry(m.name.clone()).or_default().push(m.clone());
                        true
                    }
                    None => false,
                },
            },
            StoreKind::List(l) => match l.mods.iter().position(|c| &*c.name == old_name && m.same_identity_but_name(c)) {
                Some(i) => {
                    l.mods[i] = m.clone();
                    true
                }
                None => false,
            },
        };
        if found {
            return true;
        }
        match &self.parent {
            Some(p) => p.borrow_mut().convert_mod_internal(old_name, m),
            None => false,
        }
    }

    /// ModList.lua:73 `MergeMod`: BASE/INC/MORE fold into the first `compareModParams`-equal entry, replaced by a SHALLOW
    /// copy (tags shared) with the summed value -- keeping the OLD mod's source (§2.2). ModDB has no MergeMod.
    pub fn merge_mod(&mut self, m: Rc<Mod>, skip_non_additive: bool) -> LuaResult<()> {
        let StoreKind::List(l) = &mut self.kind else {
            return Err(missing_method("MergeMod"));
        };
        if matches!(m.ty, Some(ModType::Base | ModType::Inc | ModType::More)) {
            for i in 0..l.mods.len() {
                if compare_mod_params(&l.mods[i], &m) {
                    // ModList.lua:77-78: `copyTable(self[i], true)`, then `.value = .value + mod.value`.
                    let mut merged = (*l.mods[i]).clone();
                    let a = arith(&merged.value, "Classes/ModList.lua:78", Some("field 'value'"))?;
                    let b = arith(&m.value, "Classes/ModList.lua:78", Some("field 'value'"))?;
                    merged.value = Value::Num(a + b);
                    l.mods[i] = Rc::new(merged);
                    return Ok(());
                }
            }
        }
        if !skip_non_additive {
            self.add_mod(m);
        }
        Ok(())
    }

    // --- mutators (ModStore.lua:56-143) -------------------------------------------------------------------------------

    /// ModStore.lua:56-88. Two upstream oddities (§7.4): `replace` that finds nothing adds NOTHING, and a value without a
    /// precision goes through `m_modf(round(v * scale, 2))` -- only its integer part survives.
    pub fn scale_add_mod(&mut self, m: &Rc<Mod>, scale: f64, replace: bool) -> LuaResult<()> {
        // ModStore.lua:57-63
        let unscalable = m.tags.iter().any(|t| t.raw("unscalable").truthy());
        if scale == 1.0 || unscalable {
            self.add_mod(m.clone());
            return Ok(());
        }
        // ModStore.lua:67-76: `scaledMod = copyTable(mod)`; tables inside are copy-on-write (value.rs).
        let mut scaled = m.deep_copy();
        let mut nested = false;
        if scaled.value.is_table() {
            if scaled.value.get("mod").truthy() {
                nested = true;
            } else {
                let key = scaled.value.get("keyOfScaledMod");
                if key.truthy() {
                    let cur = arith(&scaled.value.index(&key), "Classes/ModStore.lua:73", Some("field '?'"))?;
                    scaled.value.set_index(&key, Value::Num(round(cur * scale, Some(2.0))));
                }
            }
        }
        // ModStore.lua:77-85, on `subMod` = the nested `value.mod` or the copy itself.
        let sub = if nested { scaled.value.get("mod") } else { Value::Nil };
        let (sub_value, name, ty) = if nested {
            (sub.get("value"), sub.get("name"), sub.get("type"))
        } else {
            (
                scaled.value.clone(),
                Value::Str(scaled.name.clone()),
                scaled.ty.map_or(Value::Nil, |t| Value::str(t.as_str())),
            )
        };
        if let Value::Num(v) = sub_value {
            let hp = match (name.as_str(), ty.as_str()) {
                (Some(n), Some(t)) => data::high_precision(n, t),
                _ => None,
            };
            let precision = hp.or(if v.floor() != v {
                Some(data::rules().default_high_precision)
            } else {
                None
            });
            let new = match precision {
                Some(p) => {
                    let power = 10f64.powf(p);
                    (v * scale * power).floor() / power
                }
                // `m_modf(...)` in a single-value assignment keeps the integer part.
                None => modf(round(v * scale, Some(2.0))).0,
            };
            if nested {
                let mut sub = sub;
                sub.set("value", Value::Num(new));
                scaled.value.set("mod", sub);
            } else {
                scaled.value = Value::Num(new);
            }
        }
        let scaled = Rc::new(scaled);
        if replace {
            self.replace_mod_internal(&scaled);
        } else {
            self.add_mod(scaled);
        }
        Ok(())
    }

    /// ModStore.lua:90 -- the only mutator that copies.
    pub fn copy_list(&mut self, mods: &[Rc<Mod>]) {
        for m in mods {
            self.add_mod(Rc::new(m.deep_copy()));
        }
    }

    /// ModStore.lua:96
    pub fn scale_add_list(&mut self, mods: &[Rc<Mod>], scale: f64, replace: bool) -> LuaResult<()> {
        if scale == 1.0 {
            self.add_list(mods);
            return Ok(());
        }
        for m in mods {
            self.scale_add_mod(m, scale, replace)?;
        }
        Ok(())
    }

    /// ModStore.lua:107
    pub fn new_mod(&mut self, m: Mod) {
        self.add_mod(Rc::new(m));
    }

    /// ModStore.lua:125 -- falls back to AddMod when nothing matched.
    pub fn replace_mod(&mut self, m: Mod) {
        let m = Rc::new(m);
        if !self.replace_mod_internal(&m) {
            self.add_mod(m);
        }
    }

    /// ModStore.lua:138
    pub fn convert_mod(&mut self, old_name: &str, m: Mod) {
        let m = Rc::new(m);
        if !self.convert_mod_internal(old_name, &m) {
            self.add_mod(m);
        }
    }

    // --- aggregations (ModDB.lua:136-334, ModList.lua:101-243); `ctx` is the store the query started on (L12) -----------

    pub(crate) fn sum_internal(&self, ctx: &ModStore, cfg: Option<&ModCfg>, q: &Query<'_>, names: &[&str]) -> LuaResult<f64> {
        let db = self.is_db();
        let mut result = 0.0;
        let mut gl: Option<GlobalLimits> = None;
        for name in names {
            for m in self.named(name) {
                // ModDB.lua:144 is the only guarded match (`mod.source and ...`); ModList.lua:107 is not.
                if !mod_matches(m, q, db, "Classes/ModList.lua:107")? {
                    continue;
                }
                if m.has_tags() {
                    if db {
                        // ModDB.lua:146-150
                        let gl = gl.get_or_insert_default();
                        let v = or0(ctx.eval_mod(m, cfg, Some(gl))?);
                        result += arith(&v, "Classes/ModDB.lua:150", Some("local 'value'"))?;
                    } else {
                        // ModList.lua:109 -- no globalLimits table here.
                        let v = or0(ctx.eval_mod(m, cfg, None)?);
                        result += arith(&v, "Classes/ModList.lua:109", None)?;
                    }
                } else {
                    result += arith(
                        &m.value,
                        self.site("Classes/ModDB.lua:152", "Classes/ModList.lua:111"),
                        Some("field 'value'"),
                    )?;
                }
            }
        }
        if let Some(p) = &self.parent {
            result += p.borrow().sum_internal(ctx, cfg, q, names)?;
        }
        Ok(result)
    }

    /// ModDB.lua:164 / ModList.lua:122 -- `modPrecision` lives OUTSIDE the name loop and is never reset (§7.1).
    pub(crate) fn more_internal(&self, ctx: &ModStore, cfg: Option<&ModCfg>, q: &Query<'_>, names: &[&str]) -> LuaResult<f64> {
        let db = self.is_db();
        let mut result = 1.0;
        let mut mod_precision: Option<f64> = None;
        let mut gl: Option<GlobalLimits> = None;
        for name in names {
            let mut mod_result = 1.0;
            for m in self.named(name) {
                if !mod_matches(m, q, false, self.site("Classes/ModDB.lua:174", "Classes/ModList.lua:130"))? {
                    continue;
                }
                let v = if db {
                    // ModDB.lua:176-184: `value = EvalMod(...) or 0` / `mod.value or 0`, then `1 + value / 100`.
                    let value = if m.has_tags() {
                        or0(ctx.eval_mod(m, cfg, Some(gl.get_or_insert_default()))?)
                    } else {
                        or0(m.value.clone())
                    };
                    arith(&value, "Classes/ModDB.lua:184", Some("local 'value'"))?
                } else if m.has_tags() {
                    // ModList.lua:132
                    arith(&or0(ctx.eval_mod(m, cfg, None)?), "Classes/ModList.lua:132", None)?
                } else {
                    // ModList.lua:134 -- no `or 0` here, unlike ModDB.lua:182.
                    arith(&m.value, "Classes/ModList.lua:134", Some("field 'value'"))?
                };
                mod_result *= 1.0 + v / 100.0;
                // ModDB.lua:185-189
                let hp = mod_high_precision(m);
                mod_precision = match mod_precision {
                    Some(p) => Some(max2(p, hp.unwrap_or(p))),
                    None => hp,
                };
            }
            // ModDB.lua:192-198
            match mod_precision {
                Some(p) => {
                    let power = 10f64.powf(p);
                    result = (result * mod_result * power).floor() / power;
                }
                None => result *= round(mod_result, Some(2.0)),
            }
        }
        if let Some(p) = &self.parent {
            result *= p.borrow().more_internal(ctx, cfg, q, names)?;
        }
        Ok(result)
    }

    /// ModDB.lua:206 / ModList.lua:156 -- `None` (nil), never `false`, when nothing matched (§7.9).
    pub(crate) fn flag_internal(&self, ctx: &ModStore, cfg: Option<&ModCfg>, q: &Query<'_>, names: &[&str]) -> LuaResult<Option<bool>> {
        for name in names {
            for m in self.named(name) {
                if !mod_matches(m, q, false, self.site("Classes/ModDB.lua:212", "Classes/ModList.lua:161"))? {
                    continue;
                }
                if m.has_tags() {
                    if ctx.eval_mod(m, cfg, None)?.truthy() {
                        return Ok(Some(true));
                    }
                } else if m.value.truthy() {
                    return Ok(Some(true));
                }
            }
        }
        match &self.parent {
            Some(p) => p.borrow().flag_internal(ctx, cfg, q, names),
            None => Ok(None),
        }
    }

    /// ModDB.lua:229 / ModList.lua:177 -- the first truthy value wins (0 is truthy); `None` = no values at all (L26).
    pub(crate) fn override_internal(
        &self,
        ctx: &ModStore,
        cfg: Option<&ModCfg>,
        q: &Query<'_>,
        names: &[&str],
    ) -> LuaResult<Option<Value>> {
        for name in names {
            for m in self.named(name) {
                if !mod_matches(m, q, false, self.site("Classes/ModDB.lua:235", "Classes/ModList.lua:182"))? {
                    continue;
                }
                if m.has_tags() {
                    let v = ctx.eval_mod(m, cfg, None)?;
                    if v.truthy() {
                        return Ok(Some(v));
                    }
                } else if m.value.truthy() {
                    return Ok(Some(m.value.clone()));
                }
            }
        }
        match &self.parent {
            Some(p) => p.borrow().override_internal(ctx, cfg, q, names),
            None => Ok(None),
        }
    }

    /// ModDB.lua:253 / ModList.lua:199 -- appends into the caller's list, this store first, then the parent.
    pub(crate) fn list_internal(
        &self,
        ctx: &ModStore,
        out: &mut Vec<Value>,
        cfg: Option<&ModCfg>,
        q: &Query<'_>,
        names: &[&str],
    ) -> LuaResult<()> {
        for name in names {
            for m in self.named(name) {
                if !mod_matches(m, q, false, self.site("Classes/ModDB.lua:259", "Classes/ModList.lua:204"))? {
                    continue;
                }
                // `EvalMod(...) or nullValue` -- nullValue is an undeclared global, i.e. nil.
                let v = if m.has_tags() {
                    ctx.eval_mod(m, cfg, None)?
                } else {
                    m.value.clone()
                };
                if v.truthy() {
                    out.push(v);
                }
            }
        }
        if let Some(p) = &self.parent {
            p.borrow().list_internal(ctx, out, cfg, q, names)?;
        }
        Ok(())
    }

    /// ModDB.lua:278 / ModList.lua:222 -- no type means every type; a 0 is dropped unless the mod is an OVERRIDE. Only the
    /// ModDB variant hands EvalMod a globalLimits table.
    pub(crate) fn tabulate_internal(
        &self,
        ctx: &ModStore,
        out: &mut Vec<TabEntry>,
        cfg: Option<&ModCfg>,
        q: &Query<'_>,
        names: &[&str],
    ) -> LuaResult<()> {
        let db = self.is_db();
        let mut gl: Option<GlobalLimits> = None;
        for name in names {
            for m in self.named(name) {
                if !mod_matches(m, q, false, self.site("Classes/ModDB.lua:286", "Classes/ModList.lua:227"))? {
                    continue;
                }
                let v = if !m.has_tags() {
                    m.value.clone()
                } else if db {
                    ctx.eval_mod(m, cfg, Some(gl.get_or_insert_default()))?
                } else {
                    ctx.eval_mod(m, cfg, None)?
                };
                // `value and (value ~= 0 or mod.type == "OVERRIDE")`
                let non_zero = !matches!(v, Value::Num(n) if n == 0.0);
                if v.truthy() && (non_zero || m.ty == Some(ModType::Override)) {
                    out.push(TabEntry { value: v, mod_: m.clone() });
                }
            }
        }
        if let Some(p) = &self.parent {
            p.borrow().tabulate_internal(ctx, out, cfg, q, names)?;
        }
        Ok(())
    }

    /// ModDB.lua:315. ModList has NO HasModInternal: the call dies where it is made -- ModStore.lua:286 from `HasMod`, or
    /// ModDB.lua:328 from a ModDB child (§2.2).
    fn has_mod_internal(&self, q: &Query<'_>, names: &[&str], call_site: &str) -> LuaResult<bool> {
        if !self.is_db() {
            return Err(err_op(call_site, "call", Some("method 'HasModInternal'"), &Value::Nil));
        }
        for name in names {
            for m in self.named(name) {
                if mod_matches(m, q, false, "Classes/ModDB.lua:321")? {
                    return Ok(true);
                }
            }
        }
        if let Some(p) = &self.parent
            && p.borrow().has_mod_internal(q, names, "Classes/ModDB.lua:328")?
        {
            return Ok(true);
        }
        Ok(false)
    }

    // --- public queries (ModStore.lua:145-287) ------------------------------------------------------------------------

    /// ModStore.lua:145 -- MAX goes to Max, but MIN is not in the chain and falls through to Sum.
    pub fn combine(&self, ty: ModType, cfg: Option<&ModCfg>, names: &[&str]) -> LuaResult<Value> {
        Ok(match ty {
            ModType::More => Value::Num(self.more(cfg, names)?),
            ModType::Flag => Value::from(self.flag(cfg, names)?),
            ModType::Override => self.override_(cfg, names)?.unwrap_or_default(),
            ModType::List => Value::table(Table {
                array: self.list(cfg, names)?,
                ..Table::default()
            }),
            ModType::Max => Value::from(self.max(cfg, names)?),
            _ => Value::Num(self.sum(ty, cfg, names)?),
        })
    }

    /// ModStore.lua:164
    pub fn sum(&self, ty: ModType, cfg: Option<&ModCfg>, names: &[&str]) -> LuaResult<f64> {
        self.sum_internal(self, cfg, &query(Some(ty), cfg), names)
    }

    /// ModStore.lua:177
    pub fn more(&self, cfg: Option<&ModCfg>, names: &[&str]) -> LuaResult<f64> {
        self.more_internal(self, cfg, &query(Some(ModType::More), cfg), names)
    }

    /// ModStore.lua:189
    pub fn flag(&self, cfg: Option<&ModCfg>, names: &[&str]) -> LuaResult<Option<bool>> {
        self.flag_internal(self, cfg, &query(Some(ModType::Flag), cfg), names)
    }

    /// ModStore.lua:202 -- `None` is Lua's "no values" (L26); the vararg nuance is the callers' (R4).
    pub fn override_(&self, cfg: Option<&ModCfg>, names: &[&str]) -> LuaResult<Option<Value>> {
        self.override_internal(self, cfg, &query(Some(ModType::Override), cfg), names)
    }

    /// ModStore.lua:215
    pub fn list(&self, cfg: Option<&ModCfg>, names: &[&str]) -> LuaResult<Vec<Value>> {
        let mut out = Vec::new();
        self.list_internal(self, &mut out, cfg, &query(Some(ModType::List), cfg), names)?;
        Ok(out)
    }

    /// ModStore.lua:232 -- `ty = None` means every type.
    pub fn tabulate(&self, ty: Option<ModType>, cfg: Option<&ModCfg>, names: &[&str]) -> LuaResult<Vec<TabEntry>> {
        let mut out = Vec::new();
        self.tabulate_internal(self, &mut out, cfg, &query(ty, cfg), names)?;
        Ok(out)
    }

    /// ModStore.lua:248 -- `val > (max or 0)`: a negative maximum is never stored, nothing > 0 answers nil (§7.2).
    pub fn max(&self, cfg: Option<&ModCfg>, names: &[&str]) -> LuaResult<Option<f64>> {
        let mut max: Option<f64> = None;
        for e in self.tabulate(Some(ModType::Max), cfg, names)? {
            let val = self.eval_mod(&e.mod_, cfg, None)?;
            let floor = max.unwrap_or(0.0);
            // `val > x` compiles to `x < val`: the error names the operands in that order.
            let Value::Num(v) = val else {
                return Err(err_compare(&format!("{MS}:252"), &Value::Num(floor), &val));
            };
            if v > floor {
                max = Some(v);
            }
        }
        Ok(max)
    }

    /// ModStore.lua:259 -- the first value always takes the seat (whatever its type); later ones need `val < min`.
    pub fn min(&self, cfg: Option<&ModCfg>, names: &[&str]) -> LuaResult<Value> {
        let mut min = Value::Nil;
        for e in self.tabulate(Some(ModType::Min), cfg, names)? {
            let val = self.eval_mod(&e.mod_, cfg, None)?;
            if min.is_nil() || lua_lt(&val, &min, &format!("{MS}:263"))? {
                min = val;
            }
        }
        Ok(min)
    }

    /// ModStore.lua:278
    pub fn has_mod(&self, ty: ModType, cfg: Option<&ModCfg>, names: &[&str]) -> LuaResult<bool> {
        self.has_mod_internal(&query(Some(ty), cfg), names, "Classes/ModStore.lua:286")
    }

    /// ModStore.lua:293: `conditions[var] or (parent and parent:GetCondition(var, cfg, true)) or (not noMod and
    /// self:Flag(cfg, "Condition:"..var))` -- an `or` chain evaluated lazily; the answer is the last operand when nothing is
    /// truthy: `Some(false)` with noMod, else Flag's `None`/`Some(true)`.
    pub fn get_condition(&self, var: &str, cfg: Option<&ModCfg>, no_mod: bool) -> LuaResult<Option<bool>> {
        if self.conditions.get(var) == Some(&true) {
            return Ok(Some(true));
        }
        if let Some(p) = &self.parent
            && p.borrow().get_condition(var, cfg, true)? == Some(true)
        {
            return Ok(Some(true));
        }
        if no_mod {
            return Ok(Some(false));
        }
        self.flag(cfg, &[&format!("Condition:{var}")])
    }

    /// ModStore.lua:302: `(not noMod and Override(...)) or (multipliers[var] or 0) + (parent and parent:GetMultiplier(var,
    /// cfg, true) or 0) + (not noMod and Sum("BASE", ...) or 0)`. `or` binds looser than `+`, so a truthy Override -- of any
    /// type, 0 included (L4) -- replaces the whole sum (§7.9).
    pub fn get_multiplier(&self, var: &str, cfg: Option<&ModCfg>, no_mod: bool) -> LuaResult<Value> {
        let name = format!("Multiplier:{var}");
        if !no_mod && let Some(v) = self.override_(cfg, &[&name])? {
            return Ok(v);
        }
        let own = self.multipliers.get(var).copied().unwrap_or(0.0);
        let from_parent = match &self.parent {
            // With noMod the parent's answer is always the numeric sum.
            Some(p) => arith(&or0(p.borrow().get_multiplier(var, cfg, true)?), "Classes/ModStore.lua:303", None)?,
            None => 0.0,
        };
        let from_mods = if no_mod { 0.0 } else { self.sum(ModType::Base, cfg, &[&name])? };
        Ok(Value::Num(own + from_parent + from_mods))
    }

    /// ModStore.lua:317-345 -- the same block twice, for mana (:320) and life (:334).
    fn reserved_percent(&self, cfg: Option<&ModCfg>, key: &str, base_key: &str, total_name: &str, line: u32) -> LuaResult<f64> {
        let actor = self.actor.borrow();
        let output = actor
            .output
            .as_ref()
            .ok_or_else(|| err_op(&format!("{MS}:{line}"), "index", Some("field 'output'"), &Value::Nil))?;
        let total = output.get(key).copied();
        if total == Some(0.0) {
            return Ok(0.0);
        }
        let list = actor
            .active_skill_list
            .as_ref()
            .ok_or_else(|| err_bad_arg(&format!("{MS}:{}", line + 2), 1, "ipairs", "table", &Value::Nil))?;
        let has_reservation = data::rules().has_reservation;
        let mut reserved = 0.0;
        for s in list {
            let in_buffs = |name: Option<&Str>| s.buff_list.as_ref().is_some_and(|b| name.is_some_and(|n| b.iter().any(|x| x == n)));
            let buffs_nonempty = s.buff_list.as_ref().is_some_and(|b| !b.is_empty());
            if s.skill_types.contains(&has_reservation)
                && !s.disable.unwrap_or(false)
                && buffs_nonempty
                && let Some(c) = cfg
                && (in_buffs(c.skill_name.as_ref()) || in_buffs(c.summon_skill_name.as_ref()))
            {
                let base = s.skill_data.get(base_key).copied().unwrap_or(0.0);
                let site = format!("{MS}:{}", line + 5);
                let total =
                    total.ok_or_else(|| err_op(&site, "perform arithmetic on", Some(&format!("local '{total_name}'")), &Value::Nil))?;
                reserved = (base / total * 100.0).floor();
                break;
            }
        }
        Ok(min2(reserved, 100.0))
    }

    /// ModStore.lua:308-357 -- three special stats, then `(output and output[stat]) or (cfg.skillStats[stat]) or 0`.
    /// The answer is upstream's Lua value: a number, or nil from the ReservationEfficiency -100 branch (:349) when
    /// `output.Mana` is missing.
    pub fn get_stat(&self, stat: &str, cfg: Option<&ModCfg>) -> LuaResult<Value> {
        if stat == "ManaReservedPercent" {
            return self
                .reserved_percent(cfg, "Mana", "ManaReservedBase", "totalMana", 320)
                .map(Value::Num);
        }
        if stat == "LifeReservedPercent" {
            return self
                .reserved_percent(cfg, "Life", "LifeReservedBase", "totalLife", 334)
                .map(Value::Num);
        }
        let actor = self.actor.borrow();
        let output = actor.output.as_ref();
        if stat == "ManaUnreserved" {
            let site = format!("{MS}:347");
            let output = output.ok_or_else(|| err_op(&site, "index", Some("field 'output'"), &Value::Nil))?;
            let cur = output.get(stat).copied();
            let mana = || {
                arith(
                    &Value::from(output.get("Mana").copied()),
                    &format!("{MS}:353"),
                    Some("field 'Mana'"),
                )
            };
            if let Some(c) = cur
                && c.is_nan()
            {
                // ModStore.lua:349 `return self.actor.output["Mana"]` -- raw, nil included.
                return Ok(Value::from(output.get("Mana").copied()));
            }
            if let Some(c) = cur
                && c < 0.0
            {
                // ModStore.lua:352-353
                let s352 = format!("{MS}:352");
                let pct = check_num(&Value::from(output.get("ManaUnreservedPercent").copied()), &s352, 1, "abs")?;
                let eff = arith(&Value::from(actor.mana_efficiency), &s352, Some("field 'ManaEfficiency'"))?;
                let before = (pct.abs() + 100.0) * ((100.0 + eff) / 100.0);
                return Ok(Value::Num(mana()? * (before.ceil() / 100.0)));
            }
        }
        if let Some(v) = output.and_then(|o| o.get(stat)) {
            return Ok(Value::Num(*v));
        }
        Ok(Value::Num(
            cfg.and_then(|c| c.skill_stats.as_ref())
                .and_then(|s| s.get(stat))
                .copied()
                .unwrap_or(0.0),
        ))
    }
}

/// The Lua error of calling a store method that does not exist on that kind (`AddDB`/`MergeMod` on the wrong store); the
/// caller's file:line is unknown here, so the site is `?` (deviation R2.2, accepted -- no caller in PoB does this).
pub fn missing_method(name: &str) -> LuaError {
    err_op("?", "call", Some(&format!("method '{name}'")), &Value::Nil)
}
