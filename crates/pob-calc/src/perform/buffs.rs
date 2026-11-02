//! CalcPerform.lua:2193-2995 -- combine buffs/debuffs/curses/guards over `env.player.activeSkillList` (the buff loop :2308-2775 with
//! warcries, auras, debuffs, curses, links; Wither :2755; Combustion :2763) -- and :3124-3141 (alternate charges). R6.7, skills on only.
//! The curse slots are [`super::curses`], the guard slots and the application of everything to the actors (:3317-3377) [`super::guards`].
//!
//! What the phase knows it does not do (each recorded in `env.unimplemented` when a build reaches it):
//! * `env.minion` (R7.10): every `if env.minion` half, the minions' own skills (`activeSkill.minion.activeSkillList`, :2777-2993) and the
//!   spectres (:2210-2272) are ported; `minionBuffs` / `minionCurses` go to [`Combined`].
//! * `getCachedOutputValue` (:2281-2301, Blight / Penance Brand / Earthquake stages): a cached run that stopped at a boundary of the port is recorded, its stages skipped.
//! * PartyTab (R-party): `allyBuffs = env.partyMembers["Aura"]` (:2232) is the party tab's Aura list ([`crate::party`]); a stronger ally
//!   aura / aura debuff of the same name wins over the player's (:2491, :2509, :2662, :2886, :2904, :2995), and the party members' buffs
//!   are applied by [`allies`] (:3023-3142). `partyTabEnableExportBuffs` (:1312, ImportTab `exportParty`) is taken as false -- the
//!   buff export and the Link branch it opens (:2737, `partyTabEnableExportBuffs or linksApplyToMinions`) are the recorded :1312 boundary;
//!   `buffExports` is write-only here and not kept.
//! * `activeSkill.buffSkill` / `minionBuffSkill` / `debuffSkill` / `totemBuffSkill` are only read by the CALCS display (Calcs.lua:755) --
//!   not kept. `mult = ...` at :2575 assigns a Lua GLOBAL (no local in scope) that this phase never reads.
//!
//! Hash-order walks (MINEFIELD-RUST R13): `buffs`/`guards`/`debuffs`/`minionBuffs` are keyed by buff NAMES (strings) -- LuaJIT's
//! `pairs` order differs per process; Rust keeps the insertion order ([`Buffs`] is an `IndexMap`). The classification of every walk is in
//! [`super::guards`] (where they are walked). `mergeBuff` itself is order-free per key.

use std::rc::Rc;

use indexmap::IndexMap;
use pob_data::skill_stat_map::MalformedMod;
use pob_lua::number::{max2, min2, modf, round};
use pob_mods::{Mod, ModCfg, ModStore, ModType, StoreRef, Value, compare_mod_params, create_mod};

use super::{flag, flag_cfg, nm, set_cond, skill_cfg_of, skill_guard, skill_list_of, skill_type_u32};
use crate::calc_lib;
use crate::env::Env;
use crate::party::{BuffGroup, PartyBuff, PartyTab};
use crate::setup::{keyword_flag, list_mods, n, ok, s, tag};

const CP: &str = "Modules/CalcPerform.lua";

/// `buffs[name]` / `guards[name]` / `debuffs[name]`: a ModList and the `notBuff` field written onto it (:2333).
#[derive(Default, Clone)]
pub(super) struct BuffMods {
    pub mods: Vec<Rc<Mod>>,
    pub not_buff: bool,
}

/// A name-keyed buff table; insertion order stands in for LuaJIT's per-process string-hash order (R13).
pub(super) type Buffs = IndexMap<String, BuffMods>;

/// One `curse` table (:2620-2627, :3180-3184).
#[derive(Clone, Debug)]
pub(super) struct Curse {
    pub name: String,
    pub priority: f64,
    /// `isMark` -- `activeSkill.skillTypes[SkillType.Mark]` (true or nil).
    pub is_mark: bool,
    pub ignore_hex_limit: bool,
    /// `socketedCursesHexLimit = modDB:Flag(...)`.
    pub socketed_curses_hex_limit: bool,
    pub mod_list: Option<Vec<Rc<Mod>>>,
    pub buff_mod_list: Option<Vec<Rc<Mod>>>,
    /// `curse.minionBuffModList` (:2661-2666, a `CurseBuff` while `env.minion` is set).
    pub minion_buff_mod_list: Option<Vec<Rc<Mod>>>,
}

/// What :2193-2995 leaves for the curse/guard/apply phases.
pub(super) struct Combined {
    pub buffs: Buffs,
    pub guards: Buffs,
    pub debuffs: Buffs,
    /// `curses` (an array: `t_insert` in `activeSkillList` order).
    pub curses: Vec<Curse>,
    /// `minionBuffs` (R7.10).
    pub minion_buffs: Buffs,
    /// `minionCurses` and its `limit` (1, or a spectre's `EnemyCurseLimit` + 1, :2241).
    pub minion_curses: Vec<Curse>,
    pub minion_curse_limit: f64,
    /// `nonUniqueFlasksApplyToMinion = true` written by a Link's `ParentNonUniqueFlasksAppliedToYou` (:2727-2729).
    pub non_unique_flasks_apply_to_minion: bool,
}

impl Default for Combined {
    fn default() -> Self {
        Combined {
            buffs: Buffs::default(),
            guards: Buffs::default(),
            debuffs: Buffs::default(),
            curses: Vec::new(),
            minion_buffs: Buffs::default(),
            minion_curses: Vec::new(),
            minion_curse_limit: 1.0,
            non_unique_flasks_apply_to_minion: false,
        }
    }
}

/// :45-67 `mergeBuff(src, destTable, destKey)` -- the highest value of each `compareModParams`-equal mod.
pub(super) fn merge_buff(src: &[Rc<Mod>], dest_table: &mut Buffs, key: &str) {
    merge_mods(src, &mut dest_table.entry(key.to_owned()).or_default().mods);
}

/// The body of `mergeBuff` over an already-resolved `destTable[destKey]` mod list (shared with the flask phase's plain-`Vec` tables).
pub(super) fn merge_mods(src: &[Rc<Mod>], dest: &mut Vec<Rc<Mod>>) {
    for m in src {
        let mut matched = false;
        if m.ty != Some(ModType::List) {
            for dest_mod in dest.iter_mut() {
                if compare_mod_params(m, dest_mod) {
                    if let Value::Num(dv) = dest_mod.value {
                        // `mod.value > destMod.value` compiles to `destMod.value < mod.value` (operands in VM order).
                        let mv = m
                            .value
                            .as_num()
                            .unwrap_or_else(|| panic!("{CP}:55: attempt to compare number with {}", m.value.type_name()));
                        if mv > dv {
                            *dest_mod = m.clone();
                        }
                    }
                    matched = true;
                    break;
                }
            }
        }
        if !matched {
            dest.push(m.clone());
        }
    }
}

/// `srcList:ScaleAddList(list, scale)` onto a parentless ModList `src`.
pub(super) fn scale_add(src: &StoreRef, list: &[Rc<Mod>], scale: f64) {
    ok(src.borrow_mut().scale_add_list(list, scale, false));
}

/// `new("ModList"):ModList()`.
pub(super) fn new_list() -> StoreRef {
    ModStore::new_list(None)
}

/// `str:gsub(" ", "")`.
pub(super) fn no_spaces(s: &str) -> String {
    s.replace(' ', "")
}

/// Lua arithmetic on a party buff's `effectMult` (a number, a numeric string -- `otherEffects` -- coerced, anything else raises).
pub(super) fn effect_mult_num(v: &Value, line: u32) -> f64 {
    match v {
        Value::Num(n) => *n,
        Value::Str(s) if pob_lua::tonumber(s).is_some() => pob_lua::tonumber(s).unwrap_or_default(),
        _ => panic!(
            "{CP}:{line}: attempt to perform arithmetic on field 'effectMult' (a {} value)",
            v.type_name()
        ),
    }
}

/// `not allyBuffs[ty] or not allyBuffs[ty][name] or allyBuffs[ty][name].effectMult / 100 <= mult` (R-party; :2491, :2509, :2886, :2904).
fn ally_allows(party: &PartyTab, ty: &str, name: &str, mult: f64, line: u32) -> bool {
    match party.aura.group(ty).and_then(|g| g.named.get(name)) {
        None => true,
        Some(b) => effect_mult_num(&b.effect_mult, line) / 100.0 <= mult,
    }
}

/// `allyBuffs[ty] and allyBuffs[ty][name] and allyBuffs[ty][name].effectMult / 100 > mult` (R-party; :2605, :2662, :2995).
fn ally_exceeds(party: &PartyTab, ty: &str, name: &str, mult: f64, line: u32) -> bool {
    party
        .aura
        .group(ty)
        .and_then(|g| g.named.get(name))
        .is_some_and(|b| effect_mult_num(&b.effect_mult, line) / 100.0 > mult)
}

/// `pairs(group)` over a party buff group whose loop body reads `buff.modList` / `buff.effectMult` of every key: a `Vaal` sub-table among
/// the keys (PartyTab.lua:848-851) is a table without those fields -- Lua raises there.
pub(super) fn party_entries(group: &BuffGroup, line: u32) -> impl Iterator<Item = (&String, &PartyBuff)> {
    if group.vaal.is_some() {
        panic!("{CP}:{line}: bad argument #1 to 'ipairs' (table expected, got nil)");
    }
    group.named.iter()
}

/// `str:gsub("%s+", "")` (Lua `%s`: space, \t, \n, \v, \f, \r).
fn strip_space(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'))
        .collect()
}

/// A buff field that is used as a string (`buff.name`, `buff.cond`, ...).
fn str_of<'a>(v: &'a Value, line: u32, what: &str) -> &'a str {
    v.as_str()
        .unwrap_or_else(|| panic!("{CP}:{line}: {what} is a {} (string expected)", v.type_name()))
}

/// The queries perform makes of one skill's `skillModList`, each behind the malformed-mod guard (R6.4).
struct SkillQ<'a> {
    list: StoreRef,
    malformed: &'a [MalformedMod],
}

impl SkillQ<'_> {
    fn sum(&self, ty: ModType, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
        skill_guard(self.malformed, names);
        ok(self.list.borrow().sum(ty, cfg, names))
    }
    fn more(&self, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
        skill_guard(self.malformed, names);
        ok(self.list.borrow().more(cfg, names))
    }
    fn flag(&self, cfg: Option<&ModCfg>, names: &[&str]) -> bool {
        skill_guard(self.malformed, names);
        ok(self.list.borrow().flag(cfg, names)) == Some(true)
    }
    fn override_(&self, cfg: Option<&ModCfg>, names: &[&str]) -> Option<Value> {
        skill_guard(self.malformed, names);
        ok(self.list.borrow().override_(cfg, names))
    }
    fn calc_mod(&self, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
        skill_guard(self.malformed, names);
        ok(calc_lib::mod_(&self.list.borrow(), cfg, names))
    }
}

/// `modStore` of a buff: the skill's list (`buff.activeSkillBuff`) or `modDB`.
enum Store<'a> {
    Skill(&'a SkillQ<'a>),
    Db(&'a StoreRef),
}

impl Store<'_> {
    fn sum(&self, ty: ModType, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
        match self {
            Store::Skill(q) => q.sum(ty, cfg, names),
            Store::Db(db) => ok(db.borrow().sum(ty, cfg, names)),
        }
    }
    fn more(&self, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
        match self {
            Store::Skill(q) => q.more(cfg, names),
            Store::Db(db) => ok(db.borrow().more(cfg, names)),
        }
    }
}

/// `modDB:List(cfg, name)` folded by `compareModParams` into shallow copies of `value.mod` (:2446-2459, :2546-2558, :2593-2604).
fn extra_effect_list(mod_db: &StoreRef, cfg: Option<&ModCfg>, name: &str, line: u32) -> Vec<Rc<Mod>> {
    let mut out: Vec<Mod> = Vec::new();
    for value in ok(mod_db.borrow().list(cfg, &[name])) {
        let Value::Mod(vm) = value.get("mod") else {
            panic!(
                "{CP}:{line}: attempt to index field 'mod' (a {} value)",
                value.get("mod").type_name()
            )
        };
        let mut add = true;
        for m in &mut out {
            if compare_mod_params(m, &vm) {
                // `mod.value = mod.value + value.mod.value` on the shallow copy (tags stay shared).
                let (a, b) = (m.value.as_num(), vm.value.as_num());
                let (Some(a), Some(b)) = (a, b) else {
                    panic!("{CP}:{}: attempt to perform arithmetic on field 'value'", line + 4)
                };
                m.value = Value::Num(a + b);
                add = false;
                break;
            }
        }
        if add {
            // `copyTable(value.mod, true)`
            out.push((*vm).clone());
        }
    }
    out.into_iter().map(Rc::new).collect()
}

/// :2522-2537 the totem copies of an aura's defensive mods (`Totem<name>`, value scaled with the `round`/`m_modf` rule).
fn totem_aura_mods(lists: [&[Rc<Mod>]; 2], scale: f64, src: &StoreRef) {
    let scale_value = |v: f64| -> f64 {
        // `(m_floor(v) == v) and m_modf(round(v * scale, 2)) or v * scale` -- `m_modf` truncated to its first result.
        if v.floor() == v {
            modf(round(v * scale, Some(2.0))).0
        } else {
            v * scale
        }
    };
    for list in lists {
        for m in list {
            let name: &str = &m.name;
            let resist = ok_pat(name, "Resist?M?a?x?$");
            if name == "EnergyShield" || name == "Armour" || name == "Evasion" || resist {
                let mut tm = m.deep_copy();
                tm.name = Rc::from(format!("Totem{name}").as_str());
                if scale != 1.0 {
                    match &tm.value {
                        Value::Num(v) => tm.value = Value::Num(scale_value(*v)),
                        v @ (Value::Table(_) | Value::Mod(_)) if v.get("mod").truthy() => {
                            let mut sub = v.get("mod");
                            let sv = sub
                                .get("value")
                                .as_num()
                                .unwrap_or_else(|| panic!("{CP}:2531: bad argument #1 to 'floor' (number expected)"));
                            sub.set("value", Value::Num(scale_value(sv)));
                            let mut nv = tm.value.clone();
                            nv.set("mod", sub);
                            tm.value = nv;
                        }
                        _ => {}
                    }
                }
                src.borrow_mut().add_mod(Rc::new(tm));
            }
        }
    }
}

/// `name:match(pattern)` is truthy.
fn ok_pat(name: &str, pattern: &str) -> bool {
    pob_lua::match_(name.as_bytes(), pattern.as_bytes(), None)
        .unwrap_or_else(|e| panic!("{CP}: pattern {pattern:?}: {e:?}"))
        .is_some()
}

/// :587-614 `determineCursePriority(curseName, activeSkill)`; `skill` = (socket group index, has Aura type).
pub(super) fn determine_curse_priority(env: &Env, curse_name: &str, skill: Option<(Option<usize>, bool)>) -> f64 {
    let cp = &pob_data::rules::rules().curse_priority;
    let prio = |k: &str| cp.get(k).copied();
    let mut source = String::new();
    let mut slot = String::new();
    let mut socket = 1.0;
    if let Some((Some(g), _)) = skill {
        let st = env.skills.as_ref().expect("skills on");
        let group = &st.tab.socket_group_list()[g];
        source = group.source.clone().unwrap_or_default();
        slot = group.slot.clone().unwrap_or_default();
        for (k, v) in group.gem_list.iter().enumerate() {
            if let Some(id) = &v.gem_data
                && pob_data::gems::gems().gems[id].name == curse_name
            {
                // :597 a limit of 8 avoids a collision with data.cursePriority["CurseFromEquipment"]
                socket = min2((k + 1) as f64, 8.0);
                break;
            }
        }
    }
    let base_priority = prio(curse_name).unwrap_or(0.0);
    let socket_base = prio("SocketPriorityBase")
        .unwrap_or_else(|| panic!("{CP}:603: attempt to perform arithmetic on field 'SocketPriorityBase' (a nil value)"));
    let socket_priority = socket * socket_base;
    // `slot:gsub(" (Swap)", "")` -- a capture, not literal parentheses: removes " Swap".
    let mut slot_priority = prio(&crate::setup::lua_gsub(&slot, " (Swap)", "")).unwrap_or(0.0);
    let mut source_priority = 0.0;
    if let Some((_, true)) = skill {
        source_priority = prio("CurseFromAura").unwrap_or_else(|| panic!("{CP}:607: data.cursePriority.CurseFromAura is nil"));
    } else if !source.is_empty() {
        source_priority = prio("CurseFromEquipment").unwrap_or_else(|| panic!("{CP}:609: data.cursePriority.CurseFromEquipment is nil"));
    }
    if !source.is_empty() && (Some(slot_priority) == prio("Ring 2") || Some(slot_priority) == prio("Ring 3")) {
        slot_priority = prio("Ring 1").unwrap_or_else(|| panic!("{CP}:612: data.cursePriority['Ring 1'] is nil"));
    }
    base_priority + socket_priority + slot_priority + source_priority
}

/// `env.minion` as the buff phase reads it (R7.10).
pub(super) struct MinionCtx {
    pub db: StoreRef,
    pub hostile: bool,
    pub actor: Rc<std::cell::RefCell<crate::env::CalcActor>>,
}

/// `copyTable(modValue)` with `source = "Spectre:<name>"` for each value of a LIST spectre mod (`pairs(modData.value)`).
fn spectre_copies(v: &Value, name: &str) -> Vec<Rc<Mod>> {
    let mut out = Vec::new();
    let mut one = |x: &Value| match x {
        Value::Mod(m) => {
            let mut c = m.deep_copy();
            c.source = Some(Rc::from(format!("Spectre:{name}").as_str()));
            out.push(Rc::new(c));
        }
        other => panic!("{CP}:2246: spectre modifier value is a {} (a mod expected)", other.type_name()),
    };
    match v {
        Value::Table(t) => {
            // `pairs` over a table holding mods: the array part, then the hash part (R13: one key in the data -- `mod`)
            for x in &t.array {
                one(x);
            }
            for x in t.hash.values() {
                one(x);
            }
        }
        Value::Mod(m) => one(&Value::Mod(m.clone())),
        other => panic!("{CP}:2246: bad argument #1 to 'pairs' (table expected, got {})", other.type_name()),
    }
    out
}

/// :2222-2272 the active spectres: `HaveBeastSpectre`, the minion curse limit, their ally / minion / player modifiers.
fn spectres(env: &Env, mod_db: &StoreRef, active: &[String], out: &mut Combined) {
    // `env.spec.build.spectreList` (Build.lua:989-992 keeps the ids `data.minions` knows)
    let list: Vec<String> = env
        .build
        .spectres
        .iter()
        .filter(|id| crate::skill_modlist::is_minion(id))
        .cloned()
        .collect();
    for id in &list {
        if !active.contains(id) {
            continue;
        }
        let data = crate::skill_modlist::minion_data(id);
        let name = data.get("name");
        let name = name
            .as_str()
            .unwrap_or_else(|| panic!("{CP}:2246: spectreData.name is not a string"));
        if super::cond(mod_db, "HaveBeastSpectre") != Some(true)
            && let Value::Table(tags) = data.get("monsterTags")
            && tags.array.iter().any(|t| t.is_str("beast"))
        {
            set_cond(mod_db, "HaveBeastSpectre", true);
        }
        let Value::Table(mods) = data.get("modList") else {
            panic!("{CP}:2241: spectreData.modList is not a table")
        };
        for m in &mods.array {
            let Value::Mod(md) = m else {
                panic!("{CP}:2242: spectreData.modList entry is not a mod")
            };
            let is_list = md.ty == Some(ModType::List);
            match &*md.name {
                "EnemyCurseLimit" => {
                    out.minion_curse_limit = md
                        .value
                        .as_num()
                        .unwrap_or_else(|| panic!("{CP}:2244: attempt to perform arithmetic on field 'value'"))
                        + 1.0;
                    break;
                }
                "AllyModifier" if is_list => {
                    let copies = spectre_copies(&md.value, name);
                    out.buffs.entry("Spectre".into()).or_default();
                    out.minion_buffs.entry("Spectre".into()).or_default();
                    for c in copies {
                        out.minion_buffs.get_mut("Spectre").expect("made").mods.push(c.clone());
                        out.buffs.get_mut("Spectre").expect("made").mods.push(c);
                    }
                }
                "MinionModifier" if is_list => {
                    let copies = spectre_copies(&md.value, name);
                    out.minion_buffs.entry("Spectre".into()).or_default().mods.extend(copies);
                }
                "PlayerModifier" if is_list => {
                    let copies = spectre_copies(&md.value, name);
                    out.buffs.entry("Spectre".into()).or_default().mods.extend(copies);
                }
                _ => {}
            }
        }
    }
}

/// `setSpectreSource(modList, sourceSkill)` (:2780-2791): a spectre's buff mods get `source = "Spectre:[<skill> - ]<minion name>"`. Upstream
/// writes the field in place (on the shared buff mods when the scale is 1); here the list's entries are replaced by re-sourced copies.
fn set_spectre_source(list: &StoreRef, spectre: bool, source_skill: Option<&str>, minion_name: &str) {
    if !spectre {
        return;
    }
    let source = match source_skill {
        Some(s) => format!("Spectre:{s} - {minion_name}"),
        None => format!("Spectre:{minion_name}"),
    };
    if let pob_mods::StoreKind::List(l) = &mut list.borrow_mut().kind {
        for m in &mut l.mods {
            let mut c = (**m).clone();
            c.source = Some(Rc::from(source.as_str()));
            *m = Rc::new(c);
        }
    }
}

/// :2777-2993 the buffs, auras, curses and debuffs of the skills of `env.player.activeSkillList[i]`'s minion (`castingMinion`).
#[allow(clippy::too_many_lines)]
fn minion_skill_buffs(
    env: &Env,
    i: usize,
    env_minion: Option<&MinionCtx>,
    main: &super::MainSkill,
    mod_db: &StoreRef,
    enemy_db: &StoreRef,
    out: &mut Combined,
) {
    let st = env.skills.as_ref().expect("skills on");
    let sk = &st.active_skill_list[i];
    let cm = sk.minion.as_deref().expect("activeSkill.minion");
    let cm_actor = cm.actor.clone().expect("castingMinion.modDB");
    let cm_db = cm_actor.borrow().mod_db.clone();
    // `env.minion == castingMinion` / `env.minion ~= activeSkill.minion` (one table)
    let is_env_minion = env_minion.is_some_and(|m| Rc::ptr_eq(&m.actor, &cm_actor));
    let spectre = sk.flag("spectre");
    let cm_name = cm.md("name");
    let cm_name = cm_name.as_str().unwrap_or("");
    let (mode_buffs, mode_effective) = (env.mode_buffs, env.mode_effective);
    for ms in &cm.active_skill_list {
        let q = SkillQ {
            list: ms.skill_mod_list.clone().expect("minion skill skillModList"),
            malformed: &ms.malformed_mods,
        };
        let skill_cfg = skill_cfg_of(st, ms);
        let data_truthy = |k: &str| ms.skill_data.get(k).is_some_and(Value::truthy);
        let enable = data_truthy("enable");
        let has_ty = |t: &str| ms.skill_types.contains(&skill_type_u32(t));
        for buff in &ms.buff_list {
            let ty = buff.ty.as_str().unwrap_or("");
            let name = || str_of(&buff.name, 2796, "buff.name");
            match ty {
                "Buff" if mode_buffs && enable => {
                    let active = buff.active_skill_buff == Some(true);
                    let cfg = active.then_some(&skill_cfg);
                    let store = if active { Store::Skill(&q) } else { Store::Db(&cm_db) };
                    if buff.apply_allies.truthy() {
                        let name = name();
                        set_cond(mod_db, &format!("AffectedBy{}", no_spaces(name)), true);
                        let src = new_list();
                        let inc =
                            store.sum(ModType::Inc, cfg, &["BuffEffect", "BuffEffectOnPlayer"]) + super::inc(mod_db, &["BuffEffectOnSelf"]);
                        let more =
                            store.more(cfg, &["BuffEffect", "BuffEffectOnPlayer"]) * super::more(mod_db, None, &["BuffEffectOnSelf"]);
                        scale_add(&src, &buff.mod_list, (1.0 + inc / 100.0) * more);
                        merge_buff(&list_mods(&src), &mut out.buffs, name);
                        merge_buff(&buff.mod_list, &mut out.buffs, name);
                        if data_truthy("thisIsNotABuff") {
                            out.buffs.get_mut(name).expect("merged").not_buff = true;
                        }
                    }
                    let env_minion_check = env_minion.is_some() && (is_env_minion || buff.apply_allies.truthy());
                    if buff.apply_minions.truthy() || env_minion_check {
                        let name = name();
                        let target = if env_minion_check {
                            &env_minion.expect("checked").db
                        } else {
                            &cm_db
                        };
                        set_cond(target, &format!("AffectedBy{}", no_spaces(name)), true);
                        let names: &[&str] = if is_env_minion {
                            &["BuffEffect", "BuffEffectOnSelf"]
                        } else {
                            &["BuffEffect"]
                        };
                        let inc = store.sum(ModType::Inc, cfg, names);
                        let more = store.more(cfg, names);
                        let src = new_list();
                        scale_add(&src, &buff.mod_list, (1.0 + inc / 100.0) * more);
                        merge_buff(&list_mods(&src), &mut out.minion_buffs, name);
                        merge_buff(&buff.mod_list, &mut out.minion_buffs, name);
                        if data_truthy("thisIsNotABuff") {
                            out.buffs
                                .get_mut(name)
                                .unwrap_or_else(|| panic!("{CP}:2835: attempt to index field '?' (a nil value)"))
                                .not_buff = true;
                        }
                    }
                }
                "Aura" if mode_buffs && enable => {
                    let name = name();
                    let extra = extra_effect_list(&cm_db, Some(&skill_cfg), "ExtraAuraEffect", 2843);
                    if flag(&cm_db, &["SelfAurasCannotAffectAllies"])
                        || flag(&cm_db, &["SelfAurasOnlyAffectYou"])
                        || flag(&cm_db, &["SelfAuraSkillsCannotAffectAllies"])
                        || q.flag(Some(&skill_cfg), &["SelfAurasAffectYouAndLinkedTarget"])
                    {
                        continue;
                    }
                    let key = format!("AffectedBy{}", no_spaces(name));
                    if !flag(mod_db, &["AlliesAurasCannotAffectSelf"]) && super::cond(mod_db, &key) != Some(true) {
                        let inc = q.sum(
                            ModType::Inc,
                            Some(&skill_cfg),
                            &["AuraEffect", "BuffEffect", "BuffEffectOnPlayer", "AuraBuffEffect"],
                        ) + super::sum(mod_db, ModType::Inc, Some(&skill_cfg), &["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                        let more = q.more(Some(&skill_cfg), &["AuraEffect", "BuffEffect", "AuraBuffEffect"])
                            * super::more(mod_db, Some(&skill_cfg), &["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                        let mult = (1.0 + inc / 100.0) * more;
                        // :2886 unless an ally's same aura is stronger
                        if ally_allows(&env.party, "Aura", name, mult, 2886) {
                            set_cond(mod_db, "AffectedByAura", true);
                            if name.as_bytes().get(..4) == Some(b"Vaal") {
                                set_cond(mod_db, &format!("AffectedBy{}", no_spaces(name.get(5..).unwrap_or(""))), true);
                            }
                            set_cond(mod_db, &key, true);
                            let src = new_list();
                            scale_add(&src, &buff.mod_list, mult);
                            scale_add(&src, &extra, mult);
                            set_spectre_source(&src, spectre, Some(name), cm_name);
                            merge_buff(&list_mods(&src), &mut out.buffs, name);
                        }
                    }
                    if let Some(em) = env_minion
                        && super::cond(&em.db, &key) != Some(true)
                        && (!is_env_minion || !sk.skill_data.get("auraCannotAffectSelf").is_some_and(Value::truthy))
                    {
                        let inc = q.sum(ModType::Inc, Some(&skill_cfg), &["AuraEffect", "BuffEffect"])
                            + super::sum(&em.db, ModType::Inc, Some(&skill_cfg), &["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                        let more = q.more(Some(&skill_cfg), &["AuraEffect", "BuffEffect"])
                            * super::more(&em.db, Some(&skill_cfg), &["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                        let mult = (1.0 + inc / 100.0) * more;
                        // :2904 unless an ally's same aura is stronger
                        if ally_allows(&env.party, "Aura", name, mult, 2904) {
                            set_cond(&em.db, &key, true);
                            set_cond(&em.db, "AffectedByAura", true);
                            let src = new_list();
                            scale_add(&src, &buff.mod_list, mult);
                            scale_add(&src, &extra, mult);
                            set_spectre_source(&src, spectre, Some(name), cm_name);
                            merge_buff(&list_mods(&src), &mut out.minion_buffs, name);
                        }
                    }
                    // :2888-2899 buffExports (write-only)
                    if main.flag("totem") && super::cond(&main.list, &key) != Some(true) {
                        set_cond(&main.list, &key, true);
                        set_cond(&main.list, "AffectedByAura", true);
                        let src = new_list();
                        let names = ["AuraEffect", "BuffEffect", "AuraBuffEffect"];
                        let inc = q.sum(ModType::Inc, Some(&skill_cfg), &names);
                        let more = q.more(Some(&skill_cfg), &names);
                        let scale = max2((1.0 + inc / 100.0) * more, 0.0);
                        totem_aura_mods([&extra, &buff.mod_list], scale, &src);
                        set_spectre_source(&src, spectre, None, cm_name);
                        merge_buff(&list_mods(&src), &mut out.buffs, &format!("Totem {name}"));
                    }
                }
                "Curse" if mode_effective && enable && (!flag(enemy_db, &["Hexproof"]) || has_ty("Mark")) => {
                    let name = name();
                    let priority = determine_curse_priority(env, name, Some((None, has_ty("Aura"))));
                    let inc = q.sum(ModType::Inc, Some(&skill_cfg), &["CurseEffect"]) + super::inc(enemy_db, &["CurseEffectOnSelf"]);
                    let more = q.more(Some(&skill_cfg), &["CurseEffect"]) * super::more(enemy_db, None, &["CurseEffectOnSelf"]);
                    let l = new_list();
                    scale_add(&l, &buff.mod_list, (1.0 + inc / 100.0) * more);
                    out.minion_curses.push(Curse {
                        name: name.to_owned(),
                        priority,
                        is_mark: false,
                        ignore_hex_limit: false,
                        socketed_curses_hex_limit: false,
                        mod_list: Some(list_mods(&l)),
                        buff_mod_list: None,
                        minion_buff_mod_list: None,
                    });
                }
                "Debuff" | "AuraDebuff" => {
                    let stack_count = if buff.stack_var.truthy() {
                        let var = str_of(&buff.stack_var, 2941, "buff.stackVar");
                        let mut c = q.sum(ModType::Base, Some(&skill_cfg), &[&format!("Multiplier:{var}")]);
                        if buff.stack_limit.truthy() {
                            c = min2(c, num_of(&buff.stack_limit, 2943, "stackLimit"));
                        } else if buff.stack_limit_var.truthy() {
                            let var = str_of(&buff.stack_limit_var, 2945, "buff.stackLimitVar");
                            c = min2(c, q.sum(ModType::Base, Some(&skill_cfg), &[&format!("Multiplier:{var}")]));
                        }
                        c
                    } else {
                        match ms.skill_data.get("stackCount") {
                            Some(v) if v.truthy() => num_of(v, 2948, "stackCount"),
                            _ => 1.0,
                        }
                    };
                    if mode_effective && stack_count > 0.0 {
                        let name = name();
                        let key = format!("AffectedBy{}", no_spaces(name));
                        let mut mult = 1.0;
                        if ty == "AuraDebuff" {
                            mult = 0.0;
                            if !q.flag(None, &["SelfAurasOnlyAffectYou"])
                                || q.flag(Some(&skill_cfg), &["SelfAurasAffectYouAndLinkedTarget"])
                            {
                                let names = ["AuraEffect", "BuffEffect", "DebuffEffect"];
                                let inc = q.sum(ModType::Inc, Some(&skill_cfg), &names);
                                let more = q.more(Some(&skill_cfg), &names);
                                mult = (1.0 + inc / 100.0) * more;
                                // :2993-3000 buffExports write-only; a stronger ally aura debuff (:2995) or an applied one wins
                                if super::cond(enemy_db, &key) == Some(true) || ally_exceeds(&env.party, "AuraDebuff", name, mult, 2995) {
                                    mult = 0.0;
                                }
                            }
                        }
                        set_cond(enemy_db, &key, true);
                        if is_env_minion && let Some(em) = env_minion {
                            set_cond(&em.db, &key, true);
                        }
                        if ty == "Debuff" {
                            let inc = q.sum(ModType::Inc, Some(&skill_cfg), &["DebuffEffect"]);
                            let more = q.more(Some(&skill_cfg), &["DebuffEffect"]);
                            mult = (1.0 + inc / 100.0) * more;
                        }
                        let src = new_list();
                        scale_add(&src, &buff.mod_list, mult * stack_count);
                        let sc = ms.skill_data.get("stackCount").cloned().unwrap_or_default();
                        if sc.truthy() || buff.stack_var.truthy() {
                            // `srcList:NewMod(..., activeMinionSkill.skillData.stackCount, buff.name)` -- the skill's stackCount, not `stackCount`
                            src.borrow_mut().new_mod(create_mod(
                                &format!("Multiplier:{name}Stack"),
                                Some(ModType::Base),
                                sc,
                                Some(name),
                                0,
                                0,
                                vec![],
                            ));
                        }
                        merge_buff(&list_mods(&src), &mut out.debuffs, name);
                    }
                }
                _ => {}
            }
        }
    }
}

/// A Lua arithmetic error on a nil local (`m_floor(rate * duration)` with a missing cached output).
fn nil_arith(line: u32, what: &str) -> ! {
    panic!("Modules/CalcPerform.lua:{line}: attempt to perform arithmetic on local '{what}' (a nil value)")
}

/// :2193-2995 -- see the module doc.
#[allow(clippy::too_many_lines)]
pub(super) fn combine(env: &mut Env) -> Combined {
    let mod_db = env.mod_db.clone();
    let enemy_db = env.enemy_db.clone();
    let (mode_buffs, mode_effective) = (env.mode_buffs, env.mode_effective);
    let main = super::player_main_skill(env, 2312);
    let mut notes: Vec<String> = Vec::new();
    let mut out = Combined::default();
    let st = env.skills.as_ref().expect("skills on");
    let skills = st.skills.clone();
    let n_skills = st.active_skill_list.len();
    let has = |i: usize, t: &str| st.active_skill_list[i].skill_types.contains(&skill_type_u32(t));

    // :2208-2272 spectres (R7.10)
    let mut has_active_spectre = false;
    let mut active_spectre_list: Vec<String> = Vec::new();
    for sk in &st.active_skill_list {
        if !sk.flag("disable")
            && skills.effects[sk.active_effect.borrow().granted_effect]
                .id
                .starts_with("RaiseSpectre")
        {
            has_active_spectre = true;
            if let Some(m) = sk.minion.as_deref() {
                active_spectre_list.push(m.ty.clone());
            }
        }
    }
    if has_active_spectre {
        spectres(env, &mod_db, &active_spectre_list, &mut out);
    }
    // `env.minion` (R7.10): its DB, hostility and actor
    let minion: Option<MinionCtx> = env.minion.as_ref().map(|a| MinionCtx {
        db: a.borrow().mod_db.clone(),
        hostile: super::env_minion(env).hostile,
        actor: a.clone(),
    });

    // :2277-2306 maximum sustainable stages
    for (i, sk) in st.active_skill_list.iter().enumerate() {
        // :2279 `not (env.limitedSkills and env.limitedSkills[cacheSkillUUID(activeSkill, env)])`
        if sk.flag("disable") || crate::session::is_limited(env, i) {
            continue;
        }
        let name = skills.effects[sk.active_effect.borrow().granted_effect].name.clone();
        let part2 = sk.skill_part == Some(2.0);
        if !part2 {
            continue;
        }
        // :31-42 `getCachedOutputValue` -- a cached run that stopped at a boundary of the port is recorded, the stages skipped
        let cached = |keys: &[&str], notes: &mut Vec<String>| match crate::session::get_cached_output_value(env, i, keys) {
            Ok(v) => Some(v),
            Err(e) => {
                notes.push(format!("CalcPerform.lua:2281-2301 stages of {name}: {e}"));
                None
            }
        };
        match name.as_str() {
            "Blight" | "Blight of Contagion" | "Blight of Atrophy" => {
                // :2281-2285
                let Some(v) = cached(&["Speed", "Duration"], &mut notes) else {
                    continue;
                };
                let rate = v[0].unwrap_or_else(|| nil_arith(2283, "rate"));
                let duration = v[1].unwrap_or_else(|| nil_arith(2283, "duration"));
                let list = skill_list_of(env, i);
                let base_max = ok(list.borrow().sum(ModType::Base, Some(&main.cfg), &["BlightBaseMaxStages"]));
                let maximum = min2((rate * duration).floor() - 1.0, base_max - 1.0);
                let key = strip_space(&name);
                for suffix in ["MaxStages", "StageAfterFirst"] {
                    nm(
                        &list,
                        &format!("Multiplier:{key}{suffix}"),
                        ModType::Base,
                        maximum,
                        "Base",
                        0,
                        0,
                        vec![],
                    );
                }
            }
            "Penance Brand of Dissipation" => {
                // :2288-2291 (`HitSpeed` is the brand activation frequency)
                let Some(v) = cached(&["HitSpeed", "Duration"], &mut notes) else {
                    continue;
                };
                let freq = v[0].unwrap_or(0.0);
                let duration = v[1].unwrap_or_else(|| nil_arith(2289, "duration"));
                let ticks = max2(min2((freq * duration).floor() - 1.0, 19.0), 0.0);
                let list = skill_list_of(env, i);
                for suffix in ["MaxStages", "StageAfterFirst"] {
                    nm(
                        &list,
                        &format!("Multiplier:PenanceBrandofDissipation{suffix}"),
                        ModType::Base,
                        ticks,
                        "Base",
                        0,
                        0,
                        vec![],
                    );
                }
            }
            "Earthquake of Amplification" => {
                // :2299-2301
                let Some(v) = cached(&["Duration"], &mut notes) else { continue };
                let duration = v[0].unwrap_or_else(|| nil_arith(2300, "duration"));
                let list = skill_list_of(env, i);
                nm(
                    &list,
                    "Multiplier:100msEarthquakeDuration",
                    ModType::Base,
                    (duration * 10.0).floor(),
                    "Skill:EarthquakeAltX",
                    0,
                    0,
                    vec![],
                );
            }
            "Scorching Ray" | "Scorching Ray of Immolation" => {
                let list = skill_list_of(env, i);
                let key = strip_space(&name);
                let maximum = 7.0;
                for suffix in ["MaxStages", "StageAfterFirst"] {
                    nm(
                        &list,
                        &format!("Multiplier:{key}{suffix}"),
                        ModType::Base,
                        maximum,
                        "Base",
                        0,
                        0,
                        vec![],
                    );
                }
            }
            _ => {}
        }
    }

    // :2308-2775 the buff loop
    let mut applied_combustion = false;
    let mut warcry_list: Vec<String> = Vec::new();
    let mut link_skills: Vec<String> = Vec::new();
    // :2709-2710 `linksApplyToMinions` (the same for every Link buff)
    let links_apply_to_minions = minion.as_ref().is_some_and(|mc| {
        flag(&mod_db, &["Condition:CanLinkToMinions"])
            && flag(&mod_db, &["Condition:LinkedToMinion"])
            && !flag(&mc.db, &["Condition:CannotBeDamaged"])
            && !st
                .skill(st.main_skill.expect("mainSkill"))
                .skill_types
                .contains(&skill_type_u32("MinionsAreUndamagable"))
    });
    for i in 0..n_skills {
        let sk = &st.active_skill_list[i];
        let q = SkillQ {
            list: skill_list_of(env, i),
            malformed: &sk.malformed_mods,
        };
        let skill_cfg = skill_cfg_of(st, sk);
        let data_truthy = |k: &str| sk.skill_data.get(k).is_some_and(Value::truthy);
        for buff in &sk.buff_list {
            let ty = buff.ty.as_str().unwrap_or("");
            if buff.cond.truthy()
                && ok(q
                    .list
                    .borrow()
                    .get_condition(str_of(&buff.cond, 2312, "buff.cond"), Some(&skill_cfg), false))
                    != Some(true)
            {
                // Nothing!
                continue;
            }
            if buff.enemy_cond.truthy()
                && ok(enemy_db
                    .borrow()
                    .get_condition(str_of(&buff.enemy_cond, 2314, "buff.enemyCond"), None, false))
                    != Some(true)
            {
                continue;
            }
            let buff_name = || str_of(&buff.name, 2316, "buff.name");
            let active = buff.active_skill_buff == Some(true);
            match ty {
                "GlobalDB" => mod_db.borrow_mut().add_list(&buff.mod_list),
                "Buff" => {
                    if mode_buffs && (!sk.flag("totem") || buff.allow_totem_buff.truthy()) {
                        let cfg = active.then_some(&skill_cfg);
                        let store = if active { Store::Skill(&q) } else { Store::Db(&mod_db) };
                        if !buff.apply_not_player.truthy() {
                            let name = buff_name();
                            set_cond(&mod_db, &format!("AffectedBy{}", no_spaces(name)), true);
                            let src = new_list();
                            let inc = store.sum(ModType::Inc, cfg, &["BuffEffect", "BuffEffectOnSelf", "BuffEffectOnPlayer"])
                                + q.sum(ModType::Inc, cfg, &[&format!("{}Effect", no_spaces(name))]);
                            let more = store.more(cfg, &["BuffEffect", "BuffEffectOnSelf"]);
                            scale_add(&src, &buff.mod_list, (1.0 + inc / 100.0) * more);
                            merge_buff(&list_mods(&src), &mut out.buffs, name);
                            if data_truthy("thisIsNotABuff") {
                                out.buffs.get_mut(name).expect("merged").not_buff = true;
                            }
                        }
                        // :2334-2342 the minion
                        if let Some(mc) = &minion
                            && !mc.hostile
                            && (buff.apply_minions.truthy() || buff.apply_allies.truthy() || q.flag(None, &["BuffAppliesToAllies"]))
                        {
                            let name = buff_name();
                            set_cond(&mc.db, &format!("AffectedBy{}", no_spaces(name)), true);
                            let src = new_list();
                            let inc = store.sum(ModType::Inc, cfg, &["BuffEffect"]) + super::inc(&mc.db, &["BuffEffectOnSelf"]);
                            let more = store.more(cfg, &["BuffEffect"]) * super::more(&mc.db, None, &["BuffEffectOnSelf"]);
                            scale_add(&src, &buff.mod_list, (1.0 + inc / 100.0) * more);
                            merge_buff(&list_mods(&src), &mut out.minion_buffs, name);
                        }
                        // :2343-2350 buff export (partyTabEnableExportBuffs false).
                    }
                }
                "Guard" => {
                    if mode_buffs && (!sk.flag("totem") || buff.allow_totem_buff.truthy()) {
                        let cfg = active.then_some(&skill_cfg);
                        let store = if active { Store::Skill(&q) } else { Store::Db(&mod_db) };
                        if !buff.apply_not_player.truthy() {
                            let src = new_list();
                            let inc = store.sum(ModType::Inc, cfg, &["BuffEffect", "BuffEffectOnSelf", "BuffEffectOnPlayer"]);
                            let more = store.more(cfg, &["BuffEffect", "BuffEffectOnSelf"]);
                            scale_add(&src, &buff.mod_list, (1.0 + inc / 100.0) * more);
                            merge_buff(&list_mods(&src), &mut out.guards, buff_name());
                        }
                    }
                }
                "Warcry" => {
                    if mode_buffs {
                        warcry(
                            env,
                            &q,
                            &skill_cfg,
                            &main.cfg,
                            sk,
                            buff,
                            &mod_db,
                            &enemy_db,
                            &mut warcry_list,
                            &mut out.buffs,
                            minion.as_ref(),
                            &mut out.minion_buffs,
                        );
                    }
                }
                "Aura" => {
                    if mode_buffs {
                        aura(env, &q, &skill_cfg, &main, sk, buff, &mod_db, &mut out, minion.as_ref());
                    }
                }
                "Debuff" | "AuraDebuff" => {
                    let stack_count = if buff.stack_var.truthy() {
                        let var = str_of(&buff.stack_var, 2582, "buff.stackVar");
                        let mut c = q.sum(ModType::Base, Some(&skill_cfg), &[&format!("Multiplier:{var}")]);
                        if buff.stack_limit.truthy() {
                            let lim = buff
                                .stack_limit
                                .as_num()
                                .unwrap_or_else(|| panic!("{CP}:2584: bad argument #2 to 'min' (number expected)"));
                            c = min2(c, lim);
                        } else if buff.stack_limit_var.truthy() {
                            let var = str_of(&buff.stack_limit_var, 2586, "buff.stackLimitVar");
                            c = min2(c, q.sum(ModType::Base, Some(&skill_cfg), &[&format!("Multiplier:{var}")]));
                        }
                        c
                    } else {
                        match sk.skill_data.get("stackCount") {
                            Some(v) if v.truthy() => v
                                .as_num()
                                .unwrap_or_else(|| panic!("{CP}:2592: attempt to compare number with {}", v.type_name())),
                            _ => 1.0,
                        }
                    };
                    if mode_effective && stack_count > 0.0 {
                        let name = buff_name();
                        let key = format!("AffectedBy{}", no_spaces(name));
                        set_cond(&enemy_db, &key, true);
                        set_cond(&mod_db, &key, true);
                        let src = new_list();
                        let mut mult = 1.0;
                        let mut extra: Vec<Rc<Mod>> = Vec::new();
                        if ty == "AuraDebuff" {
                            extra = extra_effect_list(&mod_db, Some(&skill_cfg), "ExtraAuraDebuffEffect", 2602);
                            mult = 0.0;
                            if !flag(&mod_db, &["SelfAurasOnlyAffectYou"]) {
                                let names = ["AuraEffect", "BuffEffect", "DebuffEffect"];
                                let inc = q.sum(ModType::Inc, Some(&skill_cfg), &names);
                                let more = q.more(Some(&skill_cfg), &names);
                                mult = (1.0 + inc / 100.0) * more;
                                // :2661 buffExports (write-only); :2662-2664 a stronger ally aura debuff of the same name wins
                                if ally_exceeds(&env.party, "AuraDebuff", name, mult, 2662) {
                                    mult = 0.0;
                                }
                            }
                        }
                        if ty == "Debuff" {
                            let inc = q.sum(ModType::Inc, Some(&skill_cfg), &["DebuffEffect"]);
                            let more = q.more(Some(&skill_cfg), &["DebuffEffect"]);
                            mult = (1.0 + inc / 100.0) * more;
                        }
                        scale_add(&src, &buff.mod_list, mult * stack_count);
                        scale_add(&src, &extra, mult * stack_count);
                        if sk.skill_data.get("stackCount").is_some_and(Value::truthy) || buff.stack_var.truthy() {
                            nm(
                                &src,
                                &format!("Multiplier:{name}Stack"),
                                ModType::Base,
                                stack_count,
                                name,
                                0,
                                0,
                                vec![],
                            );
                        }
                        merge_buff(&list_mods(&src), &mut out.debuffs, name);
                    }
                }
                "Curse" | "CurseBuff" => {
                    if let Some(c) = curse(env, &q, &skill_cfg, i, buff, &mod_db, &enemy_db, minion.as_ref()) {
                        out.curses.push(c);
                    }
                }
                // :2708-2752 Link: `env.mode_buffs and #linkSkills < 1 and (partyTabEnableExportBuffs or linksApplyToMinions)` --
                // `partyTabEnableExportBuffs` is false (see the module doc).
                "Link" if mode_buffs && link_skills.is_empty() && links_apply_to_minions => {
                    let mc = minion.as_ref().expect("linksApplyToMinions implies env.minion");
                    let extra = extra_effect_list(&mod_db, Some(&skill_cfg), "ExtraLinkEffect", 2715);
                    if extra.iter().any(|m| &*m.name == "ParentNonUniqueFlasksAppliedToYou") {
                        out.non_unique_flasks_apply_to_minion = true;
                    }
                    let name = buff_name();
                    let mut inc = q.sum(ModType::Inc, Some(&skill_cfg), &["LinkEffect", "BuffEffect"]);
                    let mut more = q.more(Some(&skill_cfg), &["LinkEffect", "BuffEffect"]);
                    set_cond(&mc.db, &format!("AffectedBy{}", no_spaces(name)), true);
                    set_cond(&mc.db, "AffectedByLink", true);
                    inc += super::inc(&mc.db, &["BuffEffectOnSelf", "LinkEffectOnSelf"]);
                    more *= super::more(&mc.db, None, &["BuffEffectOnSelf", "LinkEffectOnSelf"]);
                    let mult = (1.0 + inc / 100.0) * more;
                    let src = new_list();
                    scale_add(&src, &buff.mod_list, mult);
                    scale_add(&src, &extra, mult);
                    merge_buff(&list_mods(&src), &mut out.minion_buffs, name);
                    link_skills.push(name.to_owned());
                }
                _ => {}
            }
        }
        // :2755-2762 Wither
        if q.flag(None, &["Condition:CanWither"])
            || (sk.minion.is_some() && minion.as_ref().is_some_and(|mc| flag(&mc.db, &["Condition:CanWither"])))
        {
            let has_minion = sk.minion.is_some();
            let mut wither_effect = super::inc(&mod_db, &[if has_minion { "MinionWitherEffect" } else { "WitherEffect" }]);
            if !has_minion && flag(&mod_db, &["UnholyMight"]) {
                wither_effect += super::inc(&mod_db, &["UnholyMightAlsoWitherEffect"]);
            }
            let effect = (6.0 * (1.0 + wither_effect / 100.0)).floor();
            mod_db
                .borrow_mut()
                .new_mod(create_mod("WitherEffectStack", Some(ModType::Max), n(effect), None, 0, 0, vec![]));
        }
        // :2763-2776 Combustion
        if flag(&enemy_db, &["Condition:Ignited"]) && (has(i, "Damage") || has(i, "Attack")) && !applied_combustion {
            for support in &sk.support_list {
                if skills.effects[support.borrow().granted_effect].name == "Combustion" {
                    if !q.flag(Some(&skill_cfg), &["CannotIgnite"]) {
                        let value = q.sum(ModType::Base, Some(&skill_cfg), &["CombustionFireResist"]);
                        nm(
                            &enemy_db,
                            "FireResist",
                            ModType::Base,
                            value,
                            "Combustion",
                            0,
                            0,
                            vec![
                                tag(&[
                                    ("type", s("GlobalEffect")),
                                    ("effectType", s("Debuff")),
                                    ("effectName", s("Combustion")),
                                ]),
                                tag(&[("type", s("Condition")), ("var", s("Ignited"))]),
                            ],
                        );
                        applied_combustion = true;
                    }
                    break;
                }
            }
        }
        // :2777-2993 the minion's own skills (R7.10)
        if sk.minion.as_ref().is_some_and(|m| m.actor.is_some()) {
            minion_skill_buffs(env, i, minion.as_ref(), &main, &mod_db, &enemy_db, &mut out);
        }
    }
    // :3023-3142 allies (`allyBuffs.otherEffects/Aura/AuraDebuff`, `partyMembers.Warcry/Link`, R-party)
    allies(env, &mod_db, &enemy_db, minion.as_ref(), &mut out);
    env.unimplemented.extend(notes);
    out
}

/// `ScaleAddList(list, scale)` onto a fresh ModList, merged into `dest[name]` (`mergeBuff`).
fn merge_scaled(list: &[Rc<Mod>], scale: f64, dest: &mut Buffs, name: &str) {
    let src = new_list();
    scale_add(&src, list, scale);
    merge_buff(&list_mods(&src), dest, name);
}

/// :3023-3142 the party members' buffs (R-party): `allyBuffs.otherEffects` (:3023-3040), `allyBuffs.Aura` and its Vaal auras
/// (:3041-3081), `allyBuffs.AuraDebuff` (:3082-3107, effective mode), `partyMembers.Warcry.Warcry` (:3108-3130) and
/// `partyMembers.Link.Link` (:3131-3142). `pairs` over buff names: insertion order (R13; each name writes its own key).
fn allies(env: &Env, mod_db: &StoreRef, enemy_db: &StoreRef, minion: Option<&MinionCtx>, out: &mut Combined) {
    let party = env.party.clone();
    let aura = &party.aura;
    if let Some(g) = aura.group("otherEffects") {
        for (buff_name, buff) in party_entries(g, 3029) {
            let key = format!("AffectedBy{}", no_spaces(buff_name));
            set_cond(mod_db, &key, true);
            let inc = super::inc(mod_db, &["BuffEffectOnSelf", "AuraEffectOnSelf"]);
            let more = super::more(mod_db, None, &["BuffEffectOnSelf", "AuraEffectOnSelf"]);
            let eff = effect_mult_num(&buff.effect_mult, 3029);
            merge_scaled(&buff.mod_list, (eff + inc) / 100.0 * more, &mut out.buffs, buff_name);
            if let Some(mc) = minion {
                set_cond(&mc.db, &key, true);
                let inc = super::inc(&mc.db, &["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                let more = super::more(&mc.db, None, &["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                merge_scaled(&buff.mod_list, (eff + inc) / 100.0 * more, &mut out.minion_buffs, buff_name);
            }
        }
    }
    if let Some(g) = aura.group("Aura") {
        // :3042-3060 (the `Vaal` key is skipped here and walked below)
        for (aura_name, a) in &g.named {
            let compressed = no_spaces(aura_name);
            let key = format!("AffectedBy{compressed}");
            if !flag(mod_db, &["AlliesAurasCannotAffectSelf"]) && super::cond(mod_db, &key) != Some(true) {
                set_cond(mod_db, "AffectedByAura", true);
                set_cond(mod_db, &key, true);
                merge_scaled(
                    &a.mod_list,
                    effect_mult_num(&a.effect_mult, 3049) / 100.0,
                    &mut out.buffs,
                    aura_name,
                );
            }
            if let Some(mc) = minion
                && super::cond(&mc.db, &key) != Some(true)
            {
                set_cond(&mc.db, "AffectedByAura", true);
                set_cond(&mc.db, &key, true);
                merge_scaled(
                    &a.mod_list,
                    effect_mult_num(&a.effect_mult, 3056) / 100.0,
                    &mut out.minion_buffs,
                    aura_name,
                );
            }
        }
        // :3061-3080 the Vaal auras
        for (aura_name, a) in g.vaal.iter().flatten() {
            let compressed = no_spaces(aura_name);
            let key = format!("AffectedBy{compressed}");
            if !flag(mod_db, &["AlliesAurasCannotAffectSelf"]) && super::cond(mod_db, &key) != Some(true) {
                set_cond(mod_db, "AffectedByAura", true);
                // `auraName:sub(6):gsub(" ","")`
                set_cond(mod_db, &format!("AffectedBy{}", no_spaces(aura_name.get(5..).unwrap_or(""))), true);
                set_cond(mod_db, &key, true);
                merge_scaled(
                    &a.mod_list,
                    effect_mult_num(&a.effect_mult, 3069) / 100.0,
                    &mut out.buffs,
                    aura_name,
                );
            }
            if let Some(mc) = minion
                && super::cond(&mc.db, &key) != Some(true)
            {
                set_cond(&mc.db, "AffectedByAura", true);
                set_cond(&mc.db, &key, true);
                merge_scaled(
                    &a.mod_list,
                    effect_mult_num(&a.effect_mult, 3076) / 100.0,
                    &mut out.minion_buffs,
                    aura_name,
                );
            }
        }
    }
    if let Some(g) = aura.group("AuraDebuff")
        && env.mode_effective
    {
        // :3083-3106 (the `Vaal` key skipped, then the Vaal aura debuffs)
        for (aura_name, a) in g.named.iter().chain(g.vaal.iter().flatten()) {
            let key = format!("AffectedBy{}", no_spaces(aura_name));
            if super::cond(enemy_db, &key) != Some(true) {
                set_cond(enemy_db, &key, true);
                set_cond(mod_db, &key, true);
                merge_scaled(
                    &a.mod_list,
                    effect_mult_num(&a.effect_mult, 3090) / 100.0,
                    &mut out.debuffs,
                    aura_name,
                );
            }
        }
    }
    if let Some(g) = party.warcry.group("Warcry") {
        // :3108-3130 each warcry buff scaled by its own `warcryPowerBonus` (written onto the mod's first tag by a player warcry)
        let scaled = |w: &PartyBuff| -> StoreRef {
            let src = new_list();
            let eff = if w.effect_mult.is_nil() {
                100.0
            } else {
                effect_mult_num(&w.effect_mult, 3116)
            };
            for wb in &w.mod_list {
                let bonus = match wb.tags.first() {
                    Some(t) => match t.get("warcryPowerBonus") {
                        Value::Nil | Value::Bool(false) => 1.0,
                        v => effect_mult_num(&v, 3116),
                    },
                    None => panic!("{CP}:3116: attempt to index field '?' (a nil value)"),
                };
                scale_add(&src, std::slice::from_ref(wb), eff / 100.0 * bonus);
            }
            src
        };
        for (warcry_name, w) in party_entries(g, 3115) {
            let key = format!("AffectedBy{}", no_spaces(warcry_name));
            if super::cond(mod_db, &key) != Some(true) {
                set_cond(mod_db, "AffectedByWarcry", true);
                set_cond(mod_db, &key, true);
                merge_buff(&list_mods(&scaled(w)), &mut out.buffs, warcry_name);
            }
            if let Some(mc) = minion
                && super::cond(&mc.db, &key) != Some(true)
            {
                set_cond(&mc.db, "AffectedByWarcry", true);
                set_cond(&mc.db, &key, true);
                merge_buff(&list_mods(&scaled(w)), &mut out.minion_buffs, warcry_name);
            }
        }
    }
    if let Some(g) = party.link.group("Link") {
        // :3131-3142
        for (link_name, l) in party_entries(g, 3138) {
            let key = format!("AffectedBy{}", no_spaces(link_name));
            if super::cond(mod_db, &key) != Some(true) {
                set_cond(mod_db, "AffectedByLink", true);
                set_cond(mod_db, &key, true);
                let eff = if l.effect_mult.is_nil() {
                    100.0
                } else {
                    effect_mult_num(&l.effect_mult, 3138)
                };
                merge_scaled(&l.mod_list, eff / 100.0, &mut out.buffs, link_name);
            }
        }
    }
}

/// :2363-2441 a `Warcry` buff.
#[allow(clippy::too_many_arguments)]
fn warcry(
    env: &Env,
    q: &SkillQ<'_>,
    skill_cfg: &ModCfg,
    main_cfg: &ModCfg,
    sk: &crate::active_skill::ActiveSkill,
    buff: &crate::active_skill::Buff,
    mod_db: &StoreRef,
    enemy_db: &StoreRef,
    warcry_list: &mut Vec<String>,
    buffs: &mut Buffs,
    minion: Option<&MinionCtx>,
    minion_buffs: &mut Buffs,
) {
    let buff_name = str_of(&buff.name, 2367, "buff.name");
    // `buff.name:gsub(" Cry", ""):gsub("'s",""):gsub(" ","")` -- plain text in all three patterns.
    let warcry_name = buff_name.replace(" Cry", "").replace("'s", "").replace(' ', "");
    let base_exerts = q.sum(ModType::Base, Some(main_cfg), &[&format!("{warcry_name}ExertedAttacks")]);
    if base_exerts > 0.0 {
        let extra_exertions = q.sum(ModType::Base, None, &["ExtraExertedAttacks"]);
        let exert_multiplier = q.more(None, &["ExtraExertedAttacks"]);
        mod_db.borrow_mut().new_mod(create_mod(
            &format!("Num{warcry_name}Exerts"),
            Some(ModType::Base),
            n(((base_exerts + extra_exertions) * exert_multiplier).floor()),
            None,
            0,
            0,
            vec![],
        ));
        if !warcry_list.iter().any(|w| w == buff_name) {
            nm(
                mod_db,
                "Multiplier:ExertingWarcryCount",
                ModType::Base,
                1.0,
                buff_name,
                0,
                0,
                vec![],
            );
            warcry_list.push(buff_name.to_owned());
        }
    }
    if q.flag(None, &["CannotShareWarcryBuffs"]) {
        return;
    }
    // :2380 `Override or m_max(Sum("BASE") * (1 + Sum("INC") / 100), Sum("BASE", "MinimumWarcryPower"))` -- `or 0` on a Sum is dead.
    let warcry_power = super::or_override(
        super::override_(mod_db, &["WarcryPower"]),
        max2(
            super::base(mod_db, &["WarcryPower"]) * (1.0 + super::inc(mod_db, &["WarcryPower"]) / 100.0),
            super::base(mod_db, &["MinimumWarcryPower"]),
        ),
        "Modules/CalcPerform.lua:2380",
    );
    // :2381-2385 the write into the shared tag (R6-ARCH §6, `Tag::set_warcry_power_bonus`).
    for wb in &buff.mod_list {
        let Some(t0) = wb.tags.first() else { continue };
        if t0.get("effectType").as_str() == Some("Warcry") && t0.get("div").truthy() {
            let limit = t0.get("limit");
            let power = if limit.truthy() {
                min2(warcry_power, num_of(&limit, 2382, "limit"))
            } else {
                warcry_power
            };
            let div = num_of(&t0.get("div"), 2382, "div");
            t0.set_warcry_power_bonus(Value::Num((power / div).floor()));
        }
    }
    let full_duration = crate::offence::calc_skill_duration(&q.list, q.malformed, skill_cfg, &sk.skill_data, env.mode_effective, enemy_db);
    let actual_cooldown = match q.override_(Some(skill_cfg), &["CooldownRecovery"]) {
        Some(v) if v.truthy() => num_of(&v, 2387, "cooldownOverride"),
        _ => {
            let cooldown = match sk.skill_data.get("cooldown") {
                Some(Value::Num(c)) => *c,
                other => panic!(
                    "{CP}:2387: attempt to perform arithmetic on field 'cooldown' (a {} value)",
                    other.map_or("nil", Value::type_name)
                ),
            };
            (cooldown + q.sum(ModType::Base, Some(skill_cfg), &["CooldownRecovery"])) / q.calc_mod(Some(skill_cfg), &["CooldownRecovery"])
        }
    };
    let uptime = if flag(mod_db, &["Condition:WarcryMaxHit"]) {
        1.0
    } else {
        min2(full_duration / actual_cooldown, 1.0)
    };
    // :2389 `extraWarcryModList` -- only the minion half and the export read it.
    if !flag(mod_db, &["CannotGainWarcryBuffs"]) && !buff.apply_not_player.truthy() {
        set_cond(mod_db, &format!("AffectedBy{warcry_name}"), true);
        let src = new_list();
        let inc = q.sum(
            ModType::Inc,
            Some(skill_cfg),
            &["BuffEffect", "BuffEffectOnSelf", "BuffEffectOnPlayer"],
        );
        let more = q.more(Some(skill_cfg), &["BuffEffect", "BuffEffectOnSelf"]);
        for wb in &buff.mod_list {
            // `warcryBuff[1].warcryPowerBonus` -- the first tag, whatever it is (indexing a nil tag is the Lua error).
            let t0 = wb
                .tags
                .first()
                .unwrap_or_else(|| panic!("{CP}:2398: attempt to index a nil value (field '?')"));
            let bonus = t0.get("warcryPowerBonus");
            let bonus = if bonus.truthy() {
                num_of(&bonus, 2398, "warcryPowerBonus")
            } else {
                1.0
            };
            let mult = (1.0 + inc / 100.0) * more * bonus * uptime;
            scale_add(&src, std::slice::from_ref(wb), mult);
        }
        merge_buff(&list_mods(&src), buffs, buff_name);
    }
    // :2404-2433 the minion (R7.10); :2434-2441 export (false).
    if let Some(mc) = minion {
        set_cond(&mc.db, &format!("AffectedBy{warcry_name}"), true);
        let src = new_list();
        let inc = q.sum(ModType::Inc, Some(skill_cfg), &["BuffEffect"])
            + super::sum(&mc.db, ModType::Inc, Some(skill_cfg), &["BuffEffectOnSelf"]);
        let more = q.more(Some(skill_cfg), &["BuffEffect"]) * super::more(&mc.db, Some(skill_cfg), &["BuffEffectOnSelf"]);
        for wb in &buff.mod_list {
            let t0 = wb
                .tags
                .first()
                .unwrap_or_else(|| panic!("{CP}:2411: attempt to index a nil value (field '?')"));
            let bonus = t0.get("warcryPowerBonus");
            let bonus = if bonus.truthy() {
                num_of(&bonus, 2411, "warcryPowerBonus")
            } else {
                1.0
            };
            scale_add(&src, std::slice::from_ref(wb), (1.0 + inc / 100.0) * more * bonus * uptime);
        }
        // :2415-2431 Rallying Cry: the flat weapon damage for the minion
        if buff_name == "Rallying Cry" {
            let extra = new_list();
            let power_bonus = (min2(warcry_power, 30.0) / 5.0).floor();
            let effect = (q.sum(ModType::Base, Some(main_cfg), &["RallyingCryAllyDamageBonusPer5Power"]) * power_bonus).floor();
            let inc = q.sum(ModType::Inc, Some(skill_cfg), &["BuffEffect"])
                + super::sum(&mc.db, ModType::Inc, Some(skill_cfg), &["BuffEffectOnSelf"]);
            let more_mult = 1.0 + q.sum(ModType::Base, Some(main_cfg), &["RallyingCryMinionDamageBonusMultiplier"]);
            let wd1 = env.player.borrow().weapon_data1.clone();
            let gtag = || {
                vec![tag(&[
                    ("type", s("GlobalEffect")),
                    ("effectType", s("Warcry")),
                    ("div", n(5.0)),
                    ("limit", n(30.0)),
                ])]
            };
            for dt in ["Physical", "Lightning", "Cold", "Fire", "Chaos"] {
                for suffix in ["Min", "Max"] {
                    let k = format!("{dt}{suffix}");
                    let v = wd1.get(&k);
                    if v.truthy() {
                        let v = num_of(&v, 2425, &k);
                        nm(
                            &extra,
                            &k,
                            ModType::Base,
                            v * effect / 100.0,
                            "Rallying Cry",
                            0,
                            keyword_flag("Attack"),
                            gtag(),
                        );
                    }
                }
            }
            scale_add(&src, &list_mods(&extra), (1.0 + inc / 100.0) * more_mult * uptime);
        }
        merge_buff(&list_mods(&src), minion_buffs, buff_name);
    }
}

fn num_of(v: &Value, line: u32, what: &str) -> f64 {
    v.as_num().unwrap_or_else(|| {
        panic!(
            "{CP}:{line}: attempt to perform arithmetic on field '{what}' (a {} value)",
            v.type_name()
        )
    })
}

/// :2442-2577 an `Aura` buff.
#[allow(clippy::too_many_arguments)]
fn aura(
    env: &Env,
    q: &SkillQ<'_>,
    skill_cfg: &ModCfg,
    main: &super::MainSkill,
    sk: &crate::active_skill::ActiveSkill,
    buff: &crate::active_skill::Buff,
    mod_db: &StoreRef,
    out: &mut Combined,
    minion: Option<&MinionCtx>,
) {
    let name = str_of(&buff.name, 2462, "buff.name");
    // :2444-2460 extra modifiers to apply to aura skills
    let extra = extra_effect_list(mod_db, Some(skill_cfg), "ExtraAuraEffect", 2446);
    if !sk.skill_data.get("auraCannotAffectSelf").is_some_and(Value::truthy) {
        let names = [
            "AuraEffect",
            "BuffEffect",
            "BuffEffectOnSelf",
            "AuraEffectOnSelf",
            "AuraBuffEffect",
            "SkillAuraEffectOnSelf",
        ];
        let inc = q.sum(ModType::Inc, Some(skill_cfg), &names);
        let more = q.more(Some(skill_cfg), &names);
        let mult = (1.0 + inc / 100.0) * more;
        // :2491 `AlliesAurasCannotAffectSelf or not allyBuffs["Aura"] or not allyBuffs["Aura"][name] or effectMult / 100 <= mult`
        if flag(mod_db, &["AlliesAurasCannotAffectSelf"]) || ally_allows(&env.party, "Aura", name, mult, 2491) {
            set_cond(mod_db, "AffectedByAura", true);
            if name.as_bytes().get(..4) == Some(b"Vaal") {
                // `buff.name:sub(6)`
                let rest = name.get(5..).unwrap_or("");
                set_cond(mod_db, &format!("AffectedBy{}", no_spaces(rest)), true);
            }
            set_cond(mod_db, &format!("AffectedBy{}", no_spaces(name)), true);
            let src = new_list();
            scale_add(&src, &buff.mod_list, mult);
            scale_add(&src, &extra, mult);
            merge_buff(&list_mods(&src), &mut out.buffs, name);
        }
    }
    if !(flag(mod_db, &["SelfAurasCannotAffectAllies"])
        || flag(mod_db, &["SelfAurasOnlyAffectYou"])
        || flag(mod_db, &["SelfAuraSkillsCannotAffectAllies"]))
    {
        // :2505-2518 the minion (R7.10), unless an ally's same aura is stronger (:2509). :2519-2528 buffExports (write-only).
        if let Some(mc) = minion
            && (!mc.hostile || flag(mod_db, &["AurasAffectEnemies"]))
        {
            let inc = q.sum(ModType::Inc, Some(skill_cfg), &["AuraEffect", "BuffEffect"])
                + super::sum(&mc.db, ModType::Inc, Some(skill_cfg), &["BuffEffectOnSelf", "AuraEffectOnSelf"]);
            let more = q.more(Some(skill_cfg), &["AuraEffect", "BuffEffect"])
                * super::more(&mc.db, Some(skill_cfg), &["BuffEffectOnSelf", "AuraEffectOnSelf"]);
            let mult = (1.0 + inc / 100.0) * more;
            if ally_allows(&env.party, "Aura", name, mult, 2509) {
                set_cond(&mc.db, &format!("AffectedBy{}", no_spaces(name)), true);
                set_cond(&mc.db, "AffectedByAura", true);
                let src = new_list();
                scale_add(&src, &buff.mod_list, mult);
                scale_add(&src, &extra, mult);
                merge_buff(&list_mods(&src), &mut out.minion_buffs, name);
            }
        }
        let names = ["AuraEffect", "BuffEffect"];
        let inc = q.sum(ModType::Inc, Some(skill_cfg), &names);
        let more = q.more(Some(skill_cfg), &names);
        let mult = (1.0 + inc / 100.0) * more;
        if flag(mod_db, &["AurasAffectEnemies"]) && !q.flag(Some(skill_cfg), &["SelfAurasAffectYouAndLinkedTarget"]) {
            let src = new_list();
            scale_add(&src, &buff.mod_list, mult);
            scale_add(&src, &extra, mult);
            merge_buff(&list_mods(&src), &mut out.debuffs, name);
        }
    }
    if main.flag("totem") && !(flag(mod_db, &["SelfAurasCannotAffectAllies"]) || flag(mod_db, &["SelfAuraSkillsCannotAffectAllies"])) {
        set_cond(&main.list, &format!("AffectedBy{}", no_spaces(name)), true);
        set_cond(&main.list, "AffectedByAura", true);
        let src = new_list();
        let names = ["AuraEffect", "BuffEffect", "AuraBuffEffect"];
        let inc = q.sum(ModType::Inc, Some(skill_cfg), &names);
        let more = q.more(Some(skill_cfg), &names);
        let scale = max2((1.0 + inc / 100.0) * more, 0.0);
        totem_aura_mods([&extra, &buff.mod_list], scale, &src);
        merge_buff(&list_mods(&src), &mut out.buffs, &format!("Totem {name}"));
    }
    // :2543-2576 an aura with an extra debuff (`ExtraAuraDebuffEffect`) but no AuraDebuff of its own
    if env.mode_effective
        && !ok(mod_db.borrow().list(Some(skill_cfg), &["ExtraAuraDebuffEffect"])).is_empty()
        && !(flag(mod_db, &["SelfAurasOnlyAffectYou"]) || q.flag(Some(skill_cfg), &["SelfAurasAffectYouAndLinkedTarget"]))
    {
        let found = sk.buff_list.iter().any(|b| b.ty.as_str() == Some("AuraDebuff"));
        if !found {
            let extra = extra_effect_list(mod_db, Some(skill_cfg), "ExtraAuraDebuffEffect", 2556);
            // :2601 `mult = ...` assigns a Lua GLOBAL (no local `mult` in scope; this phase never reads it), buffExports (write-only);
            // :2605 the ally check only writes that global (`mult = 0`) -- evaluated for its errors alone.
            let names = ["AuraEffect", "BuffEffect", "DebuffEffect"];
            let mult = (1.0 + q.sum(ModType::Inc, Some(skill_cfg), &names) / 100.0) * q.more(Some(skill_cfg), &names);
            let _ = ally_exceeds(&env.party, "AuraDebuff", name, mult, 2605);
            // `newModList:AddList(extraAuraModList)` -- the unscaled mods.
            merge_buff(&extra, &mut out.debuffs, name);
        }
    }
}

/// :2610-2708 a `Curse` / `CurseBuff` buff -> the curse table, or nothing.
#[allow(clippy::too_many_arguments)]
fn curse(
    env: &Env,
    q: &SkillQ<'_>,
    skill_cfg: &ModCfg,
    i: usize,
    buff: &crate::active_skill::Buff,
    mod_db: &StoreRef,
    enemy_db: &StoreRef,
    minion: Option<&MinionCtx>,
) -> Option<Curse> {
    let st = env.skills.as_ref().expect("skills on");
    let sk = &st.active_skill_list[i];
    let has = |t: &str| sk.skill_types.contains(&skill_type_u32(t));
    let name = str_of(&buff.name, 2612, "buff.name");
    let mark = has("Mark");
    set_cond(mod_db, &format!("SelfCast{}", no_spaces(name)), !(has("Triggered") || has("Aura")));
    let config_var = format!("balanceOfTerrorSelfCast{}", no_spaces(name));
    let skip_curse = !mark && !matches!(env.config.input.get(&config_var), None | Some(pob_build::ConfigValue::Bool(false)));
    let data_truthy = |k: &str| sk.skill_data.get(k).is_some_and(Value::truthy);
    let applies = (env.mode_effective
        && (!flag(enemy_db, &["Hexproof"])
            || flag(mod_db, &["CursesIgnoreHexproof"])
            || data_truthy("ignoreHexLimit")
            || data_truthy("ignoreHexproof")))
        || mark;
    if skip_curse || !applies {
        return None;
    }
    let has_aura = has("Aura");
    let priority = determine_curse_priority(env, name, Some((sk.socket_group, has_aura)));
    let ignore_hex_limit = (flag_cfg(mod_db, Some(skill_cfg), &["CursesIgnoreHexLimit"]) || data_truthy("ignoreHexLimit")) && !mark;
    let socketed_curses_hex_limit = flag_cfg(mod_db, Some(skill_cfg), &["SocketedCursesAdditionalLimit"]);
    let mut inc = q.sum(ModType::Inc, Some(skill_cfg), &["CurseEffect"]) + super::inc(enemy_db, &["CurseEffectOnSelf"]);
    if has_aura && !has("RemoteMined") {
        inc += q.sum(ModType::Inc, Some(skill_cfg), &["AuraEffect"]);
    }
    let mut more = q.more(Some(skill_cfg), &["CurseEffect"]);
    // :2635 "the only More for enemy is the boss effect"
    if !mark {
        more *= super::more(enemy_db, None, &["CurseEffectOnSelf"]);
    }
    let mut mult = 0.0;
    if !((flag(mod_db, &["SelfAurasOnlyAffectYou"]) || q.flag(Some(skill_cfg), &["SelfAurasAffectYouAndLinkedTarget"])) && has_aura) {
        mult = (1.0 + inc / 100.0) * more;
    }
    let mut c = Curse {
        name: name.to_owned(),
        priority,
        is_mark: mark,
        ignore_hex_limit,
        socketed_curses_hex_limit,
        mod_list: None,
        buff_mod_list: None,
        minion_buff_mod_list: None,
    };
    if buff.ty.as_str() == Some("Curse") {
        let l = new_list();
        scale_add(&l, &buff.mod_list, mult);
        c.mod_list = Some(list_mods(&l));
        // :2650-2652 export (false).
    } else {
        // :2654-2667 the curse applies a buff: scaled by curse effect, then by buff effect.
        let temp = new_list();
        scale_add(&temp, &buff.mod_list, mult);
        let l = new_list();
        let buff_inc = super::sum(mod_db, ModType::Inc, Some(skill_cfg), &["BuffEffectOnSelf"]);
        let buff_more = super::more(mod_db, Some(skill_cfg), &["BuffEffectOnSelf"]);
        scale_add(&l, &list_mods(&temp), (1.0 + buff_inc / 100.0) * buff_more);
        c.buff_mod_list = Some(list_mods(&l));
        // :2661-2666 the minion (R7.10)
        if let Some(mc) = minion {
            let ml = new_list();
            let buff_inc = super::inc(&mc.db, &["BuffEffectOnSelf"]);
            let buff_more = super::more(&mc.db, None, &["BuffEffectOnSelf"]);
            scale_add(&ml, &list_mods(&temp), (1.0 + buff_inc / 100.0) * buff_more);
            c.minion_buff_mod_list = Some(list_mods(&ml));
        }
    }
    Some(c)
}

/// :3124-3141 stats from alternate charges.
pub(super) fn alternate_charges(env: &Env) {
    if !env.mode_combat {
        return;
    }
    let db = &env.mod_db;
    if flag(db, &["UseEnduranceCharges"]) && flag(db, &["EnduranceChargesConvertToBrutalCharges"]) {
        let triple = super::base(db, &["PerBrutalTripleDamageChance"]);
        // `NewMod(name, "BASE", v, { tag })`: a table first in the vararg is a tag, the source stays nil (ModTools.lua:33-49).
        db.borrow_mut().new_mod(create_mod(
            "TripleDamageChance",
            Some(ModType::Base),
            n(triple),
            None,
            0,
            0,
            vec![tag(&[("type", s("Multiplier")), ("var", s("BrutalCharge"))])],
        ));
    }
    if flag(db, &["UseFrenzyCharges"]) && flag(db, &["FrenzyChargesConvertToAfflictionCharges"]) {
        let dmg = super::base(db, &["PerAfflictionAilmentDamage"]);
        let effect = super::base(db, &["PerAfflictionNonDamageEffect"]);
        let aff = || vec![tag(&[("type", s("Multiplier")), ("var", s("AfflictionCharge"))])];
        nm(
            db,
            "Damage",
            ModType::More,
            dmg,
            "Affliction Charges",
            0,
            keyword_flag("Ailment"),
            aff(),
        );
        for name in [
            "EnemyChillEffect",
            "EnemyShockEffect",
            "EnemyFreezeEffect",
            "EnemyScorchEffect",
            "EnemyBrittleEffect",
            "EnemySapEffect",
        ] {
            nm(db, name, ModType::More, effect, "Affliction Charges", 0, 0, aff());
        }
    }
}
