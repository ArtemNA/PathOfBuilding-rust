//! `calcs.perform(env)` without the skill half -- Modules/CalcPerform.lua:1286-3792 up to (not including) `calcs.defence`,
//! plus the actor helpers it drives (`doActorLifeMana` :69-136, `doActorAttribsConditions` :138-548,
//! `doActorLifeManaReservation` :551-584, `applyEnemyModifiers` :619-634, `doActorMisc` :636-977, `doActorCharges`
//! :980-1139) and `calcs.defenceForConditionals` (CalcDefence.lua:481-508). Executable spec: pob-ts `src/calc/perform.ts`
//! (KNOWLEDGE/R4-ARCH.md, CALC-ARCH §8); line numbers from `grep -n` on upstream `ed354c2f8`, 2026-10-31.
//!
//! The phase order IS the mod insertion order and the insertion order is the summation order (MINEFIELD L10): nothing here
//! is reordered.
//!
//! Two paths (R6-ARCH §2 p. 5), told apart by `env.skills`:
//!  * skills on (R6.5, the default): `env.player.activeSkillList` / `mainSkill` are the skills initEnv built (R6.3/R6.4); :1296 gives
//!    every skill its perform-time child `skillModList` ([`Env::skill_mod_lists`]); the skill loop (:1334-1512, [`skills_loop`]) and
//!    the flask/tincture phase (:1513-1942, [`flasks`]) run. Every read of `actor.mainSkill` goes through [`MainSkill`].
//!  * skills off (the "= pob-ts" gates): `mainSkill` is the CALC-ARCH §8 stub (no skill flags, no skill types, empty parentless
//!    `skillModList`, `skillCfg = { skillName = "" }`) -- [`MainSkill`] answers "no" for every flag -- and every branch of upstream
//!    this path knowingly skips is recorded in `env.unimplemented` with the same text pob-ts records (so the perform-only fixture
//!    compares them as content).
//!
//! `calcs.defence` and `calcs.buildDefenceEstimations` are parameters ([`PhaseFn`], as in pob-ts): R4.3/R4.4 plug them in.
//! The R6 checkpoints of the LuaJIT probe (`tools/lua-corpus/export-skills.ts`) are a hook ([`Checkpoint`], [`perform_with`]).

mod ailments;
mod ally;
mod buffs;
mod curses;
mod flasks;
mod guards;
mod reservation;
mod skills_loop;

use std::rc::Rc;

use indexmap::IndexMap;
use pob_data::globals::globals;
use pob_data::rules::rules;
use pob_data::skill_stat_map::MalformedMod;
use pob_data::skills::SkillTypeSet;
use pob_items::{DataTable, Item};
use pob_lua::number::{max2, min2, round};
use pob_lua::tostring_num;
use pob_mods::{Actor, ActorRef, Mod, ModCfg, ModStore, ModType, StoreKind, StoreRef, Table, Tag, Value, create_mod};

use crate::active_skill::ActiveSkill;
use crate::calc_lib;
use crate::env::{CalcActor, Env};
use crate::output::Output;
use crate::setup::{keyword_flag, list_mods, merge_keystones, misc, mod_flag, n, new_mod, ok, s, tag};
use crate::skills::SkillsState;

/// Which of `env`'s two actors a helper works on (`actor == env.player` upstream).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Player,
    Enemy,
}

/// What perform hangs on an actor table besides `modDB`/`itemList`/`weaponData` (those live in [`CalcActor`]).
pub struct ActorState {
    /// `actor.output`.
    pub output: Output,
    /// `actor.reserved_LifeBase` / `_LifePercent` / `_ManaBase` / `_ManaPercent` (:1968-1971); nil on the enemy.
    pub reserved_life_base: Option<f64>,
    pub reserved_life_percent: Option<f64>,
    pub reserved_mana_base: Option<f64>,
    pub reserved_mana_percent: Option<f64>,
    /// `actor.uncancellable_LifeReservation` / `_ManaReservation` (:1972-1973).
    pub uncancellable_life_reservation: Option<f64>,
    pub uncancellable_mana_reservation: Option<f64>,
    /// `actor.strDmgBonus` (:525-528).
    pub str_dmg_bonus: Option<f64>,
    /// `actor.appliedEnemyModifiers`: the nested `value.mod` objects already copied to the enemy, by identity.
    applied_enemy_modifiers: Option<Vec<Rc<Mod>>>,
    /// `env.player.allyLifeList` (:3669); nil on the enemy.
    pub(crate) ally_life_list: Option<ally::AllyLifeList>,
    /// `actor.weaponRange1` / `weaponRange2` (CalcOffence.lua:1159/:1162).
    pub weapon_range1: Option<f64>,
    pub weapon_range2: Option<f64>,
}

impl ActorState {
    fn new(actor: &ActorRef) -> ActorState {
        ActorState {
            output: Output::new(actor),
            reserved_life_base: None,
            reserved_life_percent: None,
            reserved_mana_base: None,
            reserved_mana_percent: None,
            uncancellable_life_reservation: None,
            uncancellable_mana_reservation: None,
            str_dmg_bonus: None,
            applied_enemy_modifiers: None,
            ally_life_list: None,
            weapon_range1: None,
            weapon_range2: None,
        }
    }
}

/// `env.build.partyTab.actor` (:1310): PartyTab is P5; with no party member perform only asks it for an empty ModDB and
/// an empty output table.
pub struct PartyStub {
    pub mod_db: StoreRef,
    pub actor: ActorRef,
}

/// The tables perform creates (`env.player.output`, `env.enemy.output`, `env.partyMembers`, `env.aegisItem`) -- kept
/// next to `Env` rather than in it; `calculate` hands them on to defence/estimations and to `output_to_stats`.
pub struct PerformState {
    pub player: ActorState,
    pub enemy: ActorState,
    pub party_members: PartyStub,
    /// `env.aegisItem` (:1315) -- read by the minion half only.
    pub aegis_item: Option<Rc<Item>>,
}

impl PerformState {
    pub fn actor(&mut self, who: Who) -> &mut ActorState {
        match who {
            Who::Player => &mut self.player,
            Who::Enemy => &mut self.enemy,
        }
    }
}

/// `calcs.defence(env, actor)` / `calcs.buildDefenceEstimations(env, actor)` (pob-ts passes them in the same way).
pub type PhaseFn = fn(&mut Env, &mut PerformState, Who);

/// The stub phase for the perform-only fixture: does nothing, records nothing (pob-ts `noop`).
pub fn noop_phase(_: &mut Env, _: &mut PerformState, _: Who) {}

/// The perform checkpoints of the R6.1 LuaJIT probe (`tools/lua-corpus/export-skills.ts`, CalcPerform.lua @ ed354c2f8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checkpoint {
    /// After the first top-level `doActorLifeMana(env.player)` -- the one at the end of `doActorAttribsConditions` (:546), called from
    /// :1944: the state after the skill loop and the flasks.
    Cp1,
    /// After the first `doActorLifeManaReservation(env.player)` (:2092).
    Cp2,
    /// Entry of `calcs.defenceForConditionals(env, env.player)` (:3484).
    Cp3,
    /// Entry of `calcs.defence(env, env.player)` (:3792).
    Cp4,
    /// Entry of `calcs.offence(env, env.player, env.player.mainSkill)` (CalcPerform.lua:3799) -- not a probe checkpoint (timing).
    OcEntry,
    /// `calcs.offence` (CalcOffence.lua) before :1057 -- after the setup and `preSkillTypeFunc` (R7.1 probe OC1).
    Oc1,
    /// `calcs.offence` before :1987 -- after skill type stats, durations, uptime, costs, `preDamageFunc`, explosions (R7.1 probe OC2).
    Oc2,
    /// `calcs.offence` before :2606 -- after conversions, the pass list, speed/accuracy/block, misc DPS multipliers (R7.1 probe OC3).
    Oc3,
}

/// Called at each [`Checkpoint`] with the env and the player's / enemy's perform state (`actor.output`).
pub type CheckpointHook<'a> = &'a mut dyn FnMut(Checkpoint, &Env, &ActorState, &ActorState);

// --- `actor.mainSkill` -----------------------------------------------------------------------------------------------------

/// What perform reads of `actor.mainSkill`: the real `env.player.mainSkill` (skills on) or the CALC-ARCH §8 stub (skills off: no
/// flags, no types, `skillCfg = { skillName = "" }`, an empty parentless list that is also its own `baseSkillModList`).
pub(crate) struct MainSkill {
    /// `mainSkill.skillModList` (perform's child list, :1296).
    pub list: StoreRef,
    /// `mainSkill.baseSkillModList`.
    pub base: StoreRef,
    /// `mainSkill.skillCfg`.
    pub cfg: ModCfg,
    flags: IndexMap<String, bool>,
    types: SkillTypeSet,
    /// `mainSkill.skillData.triggered` is truthy.
    pub triggered: bool,
    /// The malformed data mods of the skill list (R6.4): a query of the list that reaches their name raises.
    malformed: Vec<MalformedMod>,
    /// `mainSkill.minion` is set.
    pub has_minion: bool,
}

impl MainSkill {
    /// `mainSkill.skillFlags[k]` truthy.
    pub fn flag(&self, k: &str) -> bool {
        self.flags.get(k).copied().unwrap_or(false)
    }

    /// `mainSkill.skillTypes[SkillType[name]]`.
    pub fn has_type(&self, name: &str) -> bool {
        self.types.contains(&skill_type_u32(name))
    }

    /// A query of `skillModList`/`baseSkillModList` by these names: the malformed mods' LuaJIT error first.
    pub fn guard(&self, names: &[&str]) {
        skill_guard(&self.malformed, names);
    }
}

/// `SkillType[name]` as the set key.
fn skill_type_u32(name: &str) -> u32 {
    *globals().skill_type.get(name).unwrap_or_else(|| panic!("SkillType.{name} is nil")) as u32
}

/// A skill-list query that reaches a malformed mod's name raises its LuaJIT error (R6.4, `skill_modlist` module doc).
pub(crate) fn skill_guard(malformed: &[MalformedMod], names: &[&str]) {
    if let Some(m) = malformed.iter().find(|m| names.contains(&&*m.name)) {
        panic!("{}", m.query_error());
    }
}

/// `activeSkill.skillCfg` as a ModStore cfg.
pub(crate) fn skill_cfg_of(st: &SkillsState, sk: &ActiveSkill) -> ModCfg {
    sk.skill_cfg
        .as_ref()
        .unwrap_or_else(|| panic!("Modules/CalcPerform.lua: activeSkill.skillCfg is nil (buildActiveSkillModList not run)"))
        .mod_cfg(&st.skills, &sk.skill_types, None)
}

/// `activeSkill.skillModList` after CalcPerform.lua:1296 (perform's child of `baseSkillModList`); before perform -- the base list.
pub(crate) fn skill_list_of(env: &Env, i: usize) -> StoreRef {
    match env.skill_mod_lists.get(i) {
        Some(l) => l.clone(),
        None => env.skills.as_ref().expect("skills on").active_skill_list[i]
            .skill_mod_list
            .clone()
            .unwrap_or_else(|| panic!("Modules/CalcPerform.lua: activeSkill.skillModList is nil")),
    }
}

/// `actor.mainSkill` -- `None` where upstream's field is nil (the enemy).
pub(crate) fn main_skill(env: &Env, who: Who) -> Option<MainSkill> {
    if who == Who::Player
        && let Some(st) = &env.skills
    {
        let i = st
            .main_skill
            .unwrap_or_else(|| panic!("Modules/CalcPerform.lua: attempt to index field 'mainSkill' (a nil value)"));
        let sk = &st.active_skill_list[i];
        return Some(MainSkill {
            list: skill_list_of(env, i),
            base: sk.skill_mod_list.clone().expect("baseSkillModList"),
            cfg: skill_cfg_of(st, sk),
            flags: sk.skill_flags.clone(),
            types: sk.skill_types.clone(),
            triggered: sk.skill_data.get("triggered").is_some_and(Value::truthy),
            malformed: sk.malformed_mods.clone(),
            has_minion: sk.minion.is_some(),
        });
    }
    actor_of(env, who).borrow().main_skill.as_ref().map(|ms| MainSkill {
        list: ms.skill_mod_list.clone(),
        base: ms.skill_mod_list.clone(),
        cfg: stub_skill_cfg(),
        flags: IndexMap::new(),
        types: SkillTypeSet::new(),
        triggered: false,
        malformed: Vec::new(),
        has_minion: false,
    })
}

/// `env.player.mainSkill` -- upstream indexes it unconditionally.
fn player_main_skill(env: &Env, line: u32) -> MainSkill {
    main_skill(env, Who::Player)
        .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:{line}: attempt to index field 'mainSkill' (a nil value)"))
}

// --- small readers / writers (Lua errors stay errors, L18) -------------------------------------------------------------

fn sum(db: &StoreRef, ty: ModType, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
    ok(db.borrow().sum(ty, cfg, names))
}

fn base(db: &StoreRef, names: &[&str]) -> f64 {
    sum(db, ModType::Base, None, names)
}

fn inc(db: &StoreRef, names: &[&str]) -> f64 {
    sum(db, ModType::Inc, None, names)
}

fn more(db: &StoreRef, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
    ok(db.borrow().more(cfg, names))
}

/// `modDB:Flag(cfg, ...)` in a Lua condition.
fn flag_cfg(db: &StoreRef, cfg: Option<&ModCfg>, names: &[&str]) -> bool {
    ok(db.borrow().flag(cfg, names)) == Some(true)
}

fn flag(db: &StoreRef, names: &[&str]) -> bool {
    flag_cfg(db, None, names)
}

fn override_(db: &StoreRef, names: &[&str]) -> Option<Value> {
    ok(db.borrow().override_(None, names))
}

fn max_(db: &StoreRef, cfg: Option<&ModCfg>, names: &[&str]) -> Option<f64> {
    ok(db.borrow().max(cfg, names))
}

fn list(db: &StoreRef, names: &[&str]) -> Vec<Value> {
    ok(db.borrow().list(None, names))
}

/// A mod value used as a number (Override / list element): anything else is Lua's arithmetic error.
fn num_val(v: &Value, site: &str) -> f64 {
    v.as_num()
        .unwrap_or_else(|| panic!("{site}: attempt to perform arithmetic on a {} value", v.type_name()))
}

/// `modDB:Override(...) or fallback` used as a number. Override's "zero values" (L26) adjusts to nil inside `or`.
fn or_override(v: Option<Value>, fallback: f64, site: &str) -> f64 {
    match v {
        Some(v) if v.truthy() => num_val(&v, site),
        _ => fallback,
    }
}

/// `x or fallback` for a `Max` result (`nil` on no match, ModStore.lua:248).
fn or_num(v: Option<f64>, fallback: f64) -> f64 {
    v.unwrap_or(fallback)
}

/// `db:NewMod(name, type, value, source, flags, keywordFlags, tags...)` with a number value (vararg decoded, MOD-CORE §1).
#[allow(clippy::too_many_arguments)]
fn nm(db: &StoreRef, name: &str, ty: ModType, value: f64, source: &str, flags: u32, kw: u32, tags: Vec<Tag>) {
    new_mod(db, name, ty, n(value), Some(source), flags, kw, tags);
}

fn nm_flag(db: &StoreRef, name: &str, source: &str) {
    new_mod(db, name, ModType::Flag, Value::Bool(true), Some(source), 0, 0, vec![]);
}

fn set_cond(db: &StoreRef, key: &str, v: bool) {
    db.borrow_mut().conditions.insert(Rc::from(key), v);
}

fn cond(db: &StoreRef, key: &str) -> Option<bool> {
    db.borrow().conditions.get(key).copied()
}

fn set_mult(db: &StoreRef, key: &str, v: f64) {
    db.borrow_mut().multipliers.insert(Rc::from(key), v);
}

/// `modDB.multipliers[key] = (modDB.multipliers[key] or 0) + by`.
fn bump_mult(db: &StoreRef, key: &str, by: f64) {
    let cur = db.borrow().multipliers.get(key).copied().unwrap_or(0.0);
    set_mult(db, key, cur + by);
}

fn cfg_actor(actor: &str) -> ModCfg {
    ModCfg {
        actor: Some(Rc::from(actor)),
        ..ModCfg::default()
    }
}

/// The CALC-ARCH §8 stub's `skillCfg = { skillName = "" }`.
fn stub_skill_cfg() -> ModCfg {
    ModCfg {
        skill_name: Some(Rc::from("")),
        ..ModCfg::default()
    }
}

fn skill_type(name: &str) -> Value {
    n(*globals().skill_type.get(name).unwrap_or_else(|| panic!("SkillType.{name} is nil")))
}

fn actor_of(env: &Env, who: Who) -> &Rc<std::cell::RefCell<CalcActor>> {
    match who {
        Who::Player => &env.player,
        Who::Enemy => &env.enemy,
    }
}

fn db_of(env: &Env, who: Who) -> StoreRef {
    actor_of(env, who).borrow().mod_db.clone()
}

/// `actor.enemy.modDB`.
fn enemy_db_of(env: &Env, who: Who) -> StoreRef {
    match who {
        Who::Player => env.enemy_db.clone(),
        Who::Enemy => env.mod_db.clone(),
    }
}

/// A `DataTable` as the mod core's table (`actor.weaponData1/2` is ONE Lua table seen by perform and by EvalMod).
fn data_table_to_table(d: &DataTable) -> Table {
    let mut t = Table::default();
    for (k, v) in &d.0 {
        t.hash.insert(Rc::from(k.as_str()), v.clone());
    }
    t
}

fn sync_weapon_data(actor: &CalcActor) {
    let mut a = actor.actor.borrow_mut();
    a.weapon_data1 = Some(Rc::new(data_table_to_table(&actor.weapon_data1)));
    a.weapon_data2 = Some(Rc::new(data_table_to_table(&actor.weapon_data2)));
}

/// `data.weaponTypeInfo[type]`; Lua indexes the nil result straight away (L18).
fn weapon_info(ty: &Value, site: &str) -> &'static pob_data::rules::WeaponTypeInfo {
    ty.as_str()
        .and_then(|t| rules().weapon_type_info.get(t))
        .unwrap_or_else(|| panic!("{site}: attempt to index a nil value (data.weaponTypeInfo[{ty:?}])"))
}

fn info_flag(info: &pob_data::rules::WeaponTypeInfo, site: &str) -> String {
    let f = info
        .flag
        .as_deref()
        .unwrap_or_else(|| panic!("{site}: attempt to concatenate field 'flag' (a nil value)"));
    format!("Using{f}")
}

// --- CalcPerform.lua:69-136 `doActorLifeMana` ---------------------------------------------------------------------------

pub fn do_actor_life_mana(db: &StoreRef, output: &mut Output) {
    let low_life_perc = base(db, &["LowLifePercentage"]);
    output.set_num(
        "LowLifePercentage",
        100.0
            * if low_life_perc > 0.0 {
                low_life_perc
            } else {
                misc("LowPoolThreshold")
            },
    );
    let full_life_perc = base(db, &["FullLifePercentage"]);
    output.set_num(
        "FullLifePercentage",
        100.0 * if full_life_perc > 0.0 { full_life_perc } else { 1.0 },
    );

    // :82 `output.ChaosInoculation = modDB:Flag(nil, "ChaosInoculation")` -- nil (no key) when nothing matched.
    let ci = ok(db.borrow().flag(None, &["ChaosInoculation"]));
    match ci {
        Some(b) => output.set_bool("ChaosInoculation", b),
        None => output.remove("ChaosInoculation"),
    }
    if ci == Some(true) {
        output.set_num("Life", 1.0);
        set_cond(db, "FullLife", true);
    } else {
        // :88-93
        let b = base(db, &["Life"]);
        let i = inc(db, &["Life"]);
        let m = more(db, None, &["Life"]);
        let ov = override_(db, &["Life"]);
        let conv = base(db, &["LifeConvertToEnergyShield"]);
        let life = or_override(
            ov,
            max2(round(b * (1.0 + i / 100.0) * m * (1.0 - conv / 100.0), None), 1.0),
            "Modules/CalcPerform.lua:93",
        );
        output.set_num("Life", life);
    }
    // :112-113; :114-116 read base/inc/more back for the breakdown only.
    let mana_conv = base(db, &["ManaConvertToArmour"]);
    let mana = round(ok(calc_lib::val(&db.borrow(), "Mana", None)) * (1.0 - mana_conv / 100.0), None);
    output.set_num("Mana", mana);
    let (life, mana) = (output.num("Life"), output.num("Mana"));
    output.set_num("LowestOfMaximumLifeAndMaximumMana", min2(life, mana));
}

// --- CalcPerform.lua:138-548 `doActorAttribsConditions` -----------------------------------------------------------------

/// :160-172 / :207-219 -- the `countsAsAll1H` block, identical for both weapons.
fn apply_counts_as_all_1h(wd: &mut DataTable, db: &StoreRef, site: &str) {
    let c = |k: &str| cond(db, k) == Some(true);
    wd.set("AddedUsingAxe", Value::Bool(!c("UsingAxe")));
    set_cond(db, "UsingAxe", true);
    // `weaponData.name:match("Varunastra") or not condList["UsingSword"]` -- Varunastra is a sword.
    let name = wd.get("name");
    let name = name
        .as_str()
        .unwrap_or_else(|| panic!("{site}: attempt to index field 'name' (a nil value)"));
    let added_sword = if name.contains("Varunastra") {
        Value::str("Varunastra")
    } else {
        Value::Bool(!c("UsingSword"))
    };
    wd.set("AddedUsingSword", added_sword);
    set_cond(db, "UsingSword", true);
    wd.set("AddedUsingDagger", Value::Bool(!c("UsingDagger")));
    set_cond(db, "UsingDagger", true);
    wd.set("AddedUsingMace", Value::Bool(!c("UsingMace")));
    set_cond(db, "UsingMace", true);
    wd.set("AddedUsingClaw", Value::Bool(!c("UsingClaw")));
    set_cond(db, "UsingClaw", true);
    set_cond(db, "WieldingDifferentWeaponTypes", true);
}

/// `info.melee` / `info.oneHand` -> the three shared conditions (:173-180 / :220-227).
fn apply_weapon_hand_conditions(info: &pob_data::rules::WeaponTypeInfo, db: &StoreRef) {
    if info.melee == Some(true) {
        set_cond(db, "UsingMeleeWeapon", true);
    }
    if info.one_hand == Some(true) {
        set_cond(db, "UsingOneHandedWeapon", true);
    } else {
        set_cond(db, "UsingTwoHandedWeapon", true);
    }
}

/// :415-427 / :481-493 -- the attribute comparison conditions, written twice upstream.
fn attribute_conditions(db: &StoreRef, output: &mut Output) {
    let (str_, dex, int) = (output.num("Str"), output.num("Dex"), output.num("Int"));
    let mut stats = [str_, dex, int];
    // `table.sort(stats)` with `<` (no NaN can reach here: round(...) of a sum).
    stats.sort_by(|a, b| a.partial_cmp(b).expect("attributes are numbers"));
    output.set_num("LowestAttribute", stats[0]);
    set_cond(db, "TwoHighestAttributesEqual", stats[1] == stats[2]);
    set_cond(db, "DexHigherThanInt", dex > int);
    set_cond(db, "StrHigherThanInt", str_ > int);
    set_cond(db, "IntHigherThanDex", int > dex);
    set_cond(db, "StrHigherThanDex", str_ > dex);
    set_cond(db, "IntHigherThanStr", int > str_);
    set_cond(db, "DexHigherThanStr", dex > str_);
    set_cond(db, "StrHighestAttribute", str_ >= dex && str_ >= int);
    set_cond(db, "IntHighestAttribute", int >= str_ && int >= dex);
    set_cond(db, "DexHighestAttribute", dex >= str_ && dex >= int);
    set_cond(db, "IntSingleHighestAttribute", int > str_ && int > dex);
    set_cond(db, "DexSingleHighestAttribute", dex > str_ && dex > int);
}

const ATTRIBUTES: [&str; 3] = ["Str", "Dex", "Int"];

#[allow(clippy::too_many_lines)]
fn do_actor_attribs_conditions(env: &mut Env, who: Who, st: &mut ActorState) {
    let db = db_of(env, who);

    // --- :146-245 weapon / slot conditions ------------------------------------------------------------------------------
    {
        let actor_rc = actor_of(env, who).clone();
        let mut actor = actor_rc.borrow_mut();
        let weapon2 = actor.item_list.get("Weapon 2").cloned();
        if weapon2.as_ref().is_some_and(|w| w.ty.as_deref() == Some("Shield")) || (who == Who::Player && env.aegis_mod_list.is_some()) {
            set_cond(&db, "UsingShield", true);
        } else if weapon2.is_none() {
            set_cond(&db, "OffHandIsEmpty", true);
        }
        let wd1_type = actor.weapon_data1.get("type");
        if wd1_type.is_str("None") {
            set_cond(&db, "Unarmed", true);
            if !actor.item_list.contains_key("Weapon 2") && !actor.item_list.contains_key("Gloves") {
                set_cond(&db, "Unencumbered", true);
            }
        } else {
            let info = weapon_info(&wd1_type, "Modules/CalcPerform.lua:158");
            set_cond(&db, &info_flag(info, "Modules/CalcPerform.lua:159"), true);
            if actor.weapon_data1.get("countsAsAll1H").truthy() {
                apply_counts_as_all_1h(&mut actor.weapon_data1, &db, "Modules/CalcPerform.lua:163");
            }
            apply_weapon_hand_conditions(info, &db);
        }
        for slot_name in ["Helmet", "Body Armour", "Gloves", "Boots"] {
            if actor.item_list.contains_key(slot_name) {
                set_cond(&db, &format!("Using{slot_name}"), true);
            }
        }
        for graft in ["Graft 1", "Graft 2"] {
            if let Some(base_name) = actor.item_list.get(graft).and_then(|i| i.base_name.clone()) {
                // `baseName:match("%s(%S+)")` -- a miss is `"Using" .. nil`, the concat error.
                let caps = pob_lua::match_(base_name.as_bytes(), b"%s(%S+)", None).expect("pattern");
                let word = caps
                    .as_ref()
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.bytes(base_name.as_bytes()))
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:190: attempt to concatenate a nil value"));
                set_cond(&db, &format!("Using{word}"), true);
            }
        }
        let wd2_type = actor.weapon_data2.get("type");
        if wd2_type.truthy() {
            let info = weapon_info(&wd2_type, "Modules/CalcPerform.lua:196");
            set_cond(&db, &info_flag(info, "Modules/CalcPerform.lua:197"), true);
            if actor.weapon_data2.get("countsAsAll1H").truthy() {
                apply_counts_as_all_1h(&mut actor.weapon_data2, &db, "Modules/CalcPerform.lua:201");
            }
            apply_weapon_hand_conditions(info, &db);
        }
        if wd1_type.truthy() && wd2_type.truthy() {
            set_cond(&db, "DualWielding", true);
            let all1h = |w: &DataTable| w.get("countsAsAll1H").truthy();
            if (actor.weapon_data1.get("type").is_str("Claw") || all1h(&actor.weapon_data1))
                && (actor.weapon_data2.get("type").is_str("Claw") || all1h(&actor.weapon_data2))
            {
                set_cond(&db, "DualWieldingClaws", true);
            }
            if (actor.weapon_data1.get("type").is_str("Dagger") || all1h(&actor.weapon_data1))
                && (actor.weapon_data2.get("type").is_str("Dagger") || all1h(&actor.weapon_data2))
            {
                set_cond(&db, "DualWieldingDaggers", true);
            }
            // :226-241 -- GGG treats thrusting one handed swords as the same class as one handed swords.
            let weapon_type = |w: &DataTable| -> Value {
                if w.get("type").is_str("One Handed Sword") && w.get("subType").is_str("Thrusting") {
                    return Value::str("One Handed Sword");
                }
                let info = weapon_info(&w.get("type"), "Modules/CalcPerform.lua:234");
                match &info.label {
                    Some(l) => Value::str(l),
                    None if w.get("subType").truthy() => w.get("subType"),
                    None => w.get("type"),
                }
            };
            if !weapon_type(&actor.weapon_data1).raw_eq(&weapon_type(&actor.weapon_data2)) {
                let info1 = weapon_info(&actor.weapon_data1.get("type"), "Modules/CalcPerform.lua:237");
                let info2 = weapon_info(&actor.weapon_data2.get("type"), "Modules/CalcPerform.lua:238");
                if info1.one_hand == Some(true) && info2.one_hand == Some(true) {
                    set_cond(&db, "WieldingDifferentWeaponTypes", true);
                }
            }
        }
        sync_weapon_data(&actor);
    }

    // --- :249-380 combat conditions + shrines ---------------------------------------------------------------------------
    if env.mode_combat {
        // :245-285 `actor.mainSkill.skillData.triggered` / `.skillFlags.*` / `.skillTypes[...]` (the stub answers "no" to all).
        let ms =
            main_skill(env, who).unwrap_or_else(|| panic!("Modules/CalcPerform.lua:245: attempt to index field 'mainSkill' (a nil value)"));
        let cond_true = |k: &str| set_cond(&db, k, true);
        if !ms.triggered && !ms.flag("trap") && !ms.flag("mine") && !ms.flag("totem") {
            if ms.flag("attack") {
                cond_true("AttackedRecently");
            } else if ms.flag("spell") {
                cond_true("CastSpellRecently");
            }
            if ms.has_type("Movement") {
                cond_true("UsedMovementSkillRecently");
            }
            if ms.flag("minion") && !ms.flag("permanentMinion") {
                cond_true("UsedMinionSkillRecently");
            }
            if ms.has_type("Vaal") {
                cond_true("UsedVaalSkillRecently");
            }
            if ms.has_type("Channel") {
                cond_true("Channelling");
            }
        }
        if ms.flag("hit") && !ms.flag("trap") && !ms.flag("mine") && !ms.flag("totem") {
            cond_true("HitRecently");
            if ms.flag("spell") {
                cond_true("HitSpellRecently");
            }
        }
        if ms.flag("totem") {
            cond_true("HaveTotem");
            cond_true("SummonedTotemRecently");
            if ms.flag("hit") {
                cond_true("TotemsHitRecently");
                if ms.flag("spell") {
                    cond_true("TotemsSpellHitRecently");
                }
            }
        }
        if ms.flag("mine") {
            cond_true("DetonatedMinesRecently");
        }
        if ms.flag("trap") {
            cond_true("TriggeredTrapsRecently");
        }
        // :286-294 `A or (B and not C) or D` -- `and` binds tighter than `or`; C reads `env.player.mainSkill.skillCfg`.
        let player_cfg = player_main_skill(env, 286).cfg;
        let crit_alt = flag(&db, &["CritAlwaysAltAilments"]) && !flag_cfg(&db, Some(&player_cfg), &["NeverCrit"]);
        if base(&db, &["EnemyScorchChance"]) > 0.0 || crit_alt || flag(&db, &["IgniteCanScorch"]) {
            set_cond(&db, "CanInflictScorch", true);
        }
        if base(&db, &["EnemyBrittleChance"]) > 0.0 || crit_alt {
            set_cond(&db, "CanInflictBrittle", true);
        }
        if base(&db, &["EnemySapChance"]) > 0.0 || crit_alt {
            set_cond(&db, "CanInflictSap", true);
        }
        shrines(&db);
    }
    // --- :398-407 exposure: `env.player.mainSkill.skillModList:Sum("BASE", env.player.mainSkill.skillCfg, ...)` -----------
    if env.mode_effective {
        let ms = player_main_skill(env, 399);
        for element in ["Fire", "Cold", "Lightning"] {
            let name = format!("{element}ExposureChance");
            ms.guard(&[&name]);
            if sum(&ms.list, ModType::Base, Some(&ms.cfg), &[&name]) > 0.0 || base(&db, &[&name]) > 0.0 {
                set_cond(&db, &format!("CanApply{element}Exposure"), true);
            }
        }
    }

    // --- :393-505 attributes --------------------------------------------------------------------------------------------
    let output = &mut st.output;
    if flag(&db, &["Omniscience"]) {
        calculate_omniscience(env, &db, output);
    } else {
        // :395-427 -- twice, because of the circular dependency (X attribute higher than Y attribute). `pairs` over the
        // array `{"Str","Dex","Int"}` walks it in order.
        for _pass in 1..=2 {
            for stat in ATTRIBUTES {
                let v = max2(round(ok(calc_lib::val(&db.borrow(), stat, None)), None), 0.0);
                output.set_num(stat, v);
            }
            attribute_conditions(&db, output);
        }
    }

    // :507-508
    let total = output.num("Str") + output.num("Dex") + output.num("Int");
    output.set_num("TotalAttr", total);
    // :511
    output.set_num("Devotion", base(&db, &["Devotion"]));

    // :514-546 attribute bonuses. The NewMod ORDER here is part of the summation order (L10).
    if !flag(&db, &["NoAttributeBonuses"]) {
        if !flag(&db, &["NoStrengthAttributeBonuses"]) {
            let str_ = st.output.num("Str");
            if !flag(&db, &["NoStrBonusToLife"]) {
                nm(&db, "Life", ModType::Base, (str_ / 2.0).floor(), "Strength", 0, 0, vec![]);
            }
            let ratio_override = base(&db, &["StrDmgBonusRatioOverride"]);
            let bonus = if ratio_override > 0.0 {
                ((str_ + base(&db, &["DexIntToMeleeBonus"])) * ratio_override).floor()
            } else {
                ((str_ + base(&db, &["DexIntToMeleeBonus"])) / 5.0).floor()
            };
            st.str_dmg_bonus = Some(bonus);
            nm(&db, "PhysicalDamage", ModType::Inc, bonus, "Strength", mod_flag("Melee"), 0, vec![]);
        }
        let output = &mut st.output;
        if !flag(&db, &["NoDexterityAttributeBonuses"]) {
            let per_dex = or_override(
                override_(&db, &["DexAccBonusOverride"]),
                misc("AccuracyPerDexBase"),
                "Modules/CalcPerform.lua:531",
            );
            nm(
                &db,
                "Accuracy",
                ModType::Base,
                output.num("Dex") * per_dex,
                "Dexterity",
                0,
                0,
                vec![],
            );
            if !flag(&db, &["NoDexBonusToEvasion"]) {
                nm(
                    &db,
                    "Evasion",
                    ModType::Inc,
                    (output.num("Dex") / 5.0).floor(),
                    "Dexterity",
                    0,
                    0,
                    vec![],
                );
            }
        }
        if !flag(&db, &["NoIntelligenceAttributeBonuses"]) {
            if !flag(&db, &["NoIntBonusToMana"]) {
                nm(
                    &db,
                    "Mana",
                    ModType::Base,
                    (output.num("Int") / 2.0).floor(),
                    "Intelligence",
                    0,
                    0,
                    vec![],
                );
            }
            if !flag(&db, &["NoIntBonusToES"]) {
                nm(
                    &db,
                    "EnergyShield",
                    ModType::Inc,
                    (output.num("Int") / 10.0).floor(),
                    "Intelligence",
                    0,
                    0,
                    vec![],
                );
            }
        }
    }

    // :547
    do_actor_life_mana(&db, &mut st.output);
}

/// :296-380 Shrine buffs -- before the life pool is calculated, for Massive Shrine.
fn shrines(db: &StoreRef) {
    use ModType::{Base, Inc, List, More, Override};
    let eff = 1.0 + sum(db, Inc, None, &["BuffEffectOnSelf", "ShrineBuffEffect"]) / 100.0;
    let f = |x: f64| (x * eff).floor();
    let fl = |name: &str| flag(db, &[name]);
    let add = |name: &str, ty: ModType, v: f64, src: &str| nm(db, name, ty, v, src, 0, 0, vec![]);
    if fl("AccelerationShrine") {
        add("ActionSpeed", Inc, f(50.0), "Acceleration Shrine");
        add("ProjectileSpeed", Inc, f(80.0), "Acceleration Shrine");
    }
    if fl("BrutalShrine") {
        add("Damage", Inc, f(50.0), "Brutal Shrine");
        add("EnemyStunDuration", Inc, f(30.0), "Brutal Shrine");
        add("EnemyKnockbackChance", Inc, 100.0, "Brutal Shrine");
    }
    if fl("DiamondShrine") {
        add("CritChance", Override, 100.0, "Diamond Shrine");
    }
    if fl("DivineShrine") {
        add("DamageTaken", More, -100.0, "Divine Shrine");
    }
    if fl("EchoingShrine") {
        nm(db, "Speed", More, f(100.0), "Echoing Shrine", mod_flag("Attack"), 0, vec![]);
        nm(db, "Speed", More, f(100.0), "Echoing Shrine", mod_flag("Cast"), 0, vec![]);
        add("RepeatCount", Base, f(1.0), "Echoing Shrine");
    }
    if fl("GloomShrine") {
        add("NonChaosDamageGainAsChaos", Base, f(10.0), "Gloom Shrine");
    }
    if fl("GreaterFreezingShrine") {
        add("PhysicalDamageGainAsCold", Base, f(30.0), "Greater Freezing Shrine");
    }
    if fl("GreaterShockingShrine") {
        add("PhysicalDamageGainAsLightning", Base, f(30.0), "Greater Shocking Shrine");
    }
    if fl("GreaterSkeletalShrine") {
        add("PhysicalDamageGainAsChaos", Base, f(30.0), "Greater Skeletal Shrine");
    }
    if fl("ImpenetrableShrine") {
        add("Armour", Inc, f(100.0), "Impenetrable Shrine");
        add("Evasion", Inc, f(100.0), "Impenetrable Shrine");
        add("EnergyShield", Inc, f(100.0), "Impenetrable Shrine");
    }
    if fl("MassiveShrine") {
        add("Life", Inc, f(40.0), "Massive Shrine");
        add("AreaOfEffect", Inc, f(40.0), "Massive Shrine");
    }
    if fl("ReplenishingShrine") {
        add("ManaRegen", Inc, 200.0 * eff, "Replenishing Shrine");
        add("LifeRegenPercent", Base, 6.7 * eff, "Replenishing Shrine");
    }
    if fl("ResistanceShrine") {
        add("ElementalResist", Base, f(50.0), "Resistance Shrine");
        add("ElementalResistMax", Base, f(10.0), "Resistance Shrine");
        add("ChaosResistMax", Base, f(10.0), "Resistance Shrine");
    }
    if fl("ResonatingShrine") {
        add("PowerChargesMax", Base, f(1.0), "Resonating Shrine");
        add("FrenzyChargesMax", Base, f(1.0), "Resonating Shrine");
        add("EnduranceChargesMax", Base, f(1.0), "Resonating Shrine");
    }
    if fl("LesserAccelerationShrine") && !fl("AccelerationShrine") {
        add("ActionSpeed", Inc, f(10.0), "Lesser Acceleration Shrine");
        add("ProjectileSpeed", Inc, f(30.0), "Lesser Acceleration Shrine");
    }
    if fl("LesserBrutalShrine") {
        add("Damage", Inc, f(20.0), "Lesser Brutal Shrine");
        add("EnemyStunDuration", Inc, f(20.0), "Lesser Brutal Shrine");
        add("EnemyKnockbackChance", Inc, 100.0, "Lesser Brutal Shrine");
    }
    if fl("LesserImpenetrableShrine") {
        add("Armour", Inc, f(50.0), "Lesser Impenetrable Shrine");
        add("Evasion", Inc, f(50.0), "Lesser Impenetrable Shrine");
        add("EnergyShield", Inc, f(50.0), "Lesser Impenetrable Shrine");
    }
    if fl("LesserMassiveShrine") {
        add("Life", Inc, f(20.0), "Lesser Massive Shrine");
        add("AreaOfEffect", Inc, f(20.0), "Lesser Massive Shrine");
    }
    if fl("LesserReplenishingShrine") {
        add("ManaRegen", Inc, 100.0 * eff, "Lesser Replenishing Shrine");
        add("LifeRegenPercent", Base, 3.3 * eff, "Lesser Replenishing Shrine");
    }
    if fl("LesserResistanceShrine") {
        add("ElementalResist", Base, f(25.0), "Lesser Resistance Shrine");
        add("ElementalResistMax", Base, f(2.0), "Lesser Resistance Shrine");
        add("ChaosResistMax", Base, f(2.0), "Lesser Resistance Shrine");
    }
    if fl("BloodShrineOfRats") {
        add("ElementalDamageGainAsChaos", Base, f(30.0), "Blood Shrine of Rats");
    }
    if fl("BloodShrineOfLocusts") {
        add("DotMultiplier", Base, f(30.0), "Blood Shrine of Locusts");
    }
    if fl("BloodShrineOfToads") {
        add("BloodShrineExplodingToadTriggerChance", Base, f(25.0), "Blood Shrine of Toads");
    }
    if fl("BloodShrineOfCrows") {
        add("ChaosResist", Base, f(50.0), "Blood Shrine of Crows");
        add("ElementalDamageFromHitsTakenAsChaos", Base, f(10.0), "Blood Shrine of Crows");
        add("PhysicalDamageFromHitsTakenAsChaos", Base, f(10.0), "Blood Shrine of Crows");
    }
    if fl("BloodShrineOfBats") {
        add("EnemyCurseLimit", Base, f(1.0), "Blood Shrine of Bats");
        let mut t = Table::default();
        t.hash.insert(Rc::from("skillId"), s("Enfeeble"));
        t.hash.insert(Rc::from("level"), n(1.0));
        new_mod(db, "ExtraCurse", List, Value::table(t), Some("Blood Shrine of Bats"), 0, 0, vec![]);
    }
}

/// :429-490 `calculateOmniscience`.
fn calculate_omniscience(env: &Env, db: &StoreRef, output: &mut Output) {
    // :430 `env.spec.tree.characterData and env.spec.tree.characterData[env.classId] or env.spec.tree.classes[env.classId]`
    let class_id = env.class_id;
    let cd = env
        .spec
        .tree
        .json
        .character_data
        .as_ref()
        .and_then(|cd| cd.get(class_id.to_string().as_str()));
    let (bs, bd, bi) = match cd {
        Some(c) => (c.base_str, c.base_dex, c.base_int),
        None => {
            let c = env
                .spec
                .tree
                .classes
                .get(class_id)
                .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:430: no class {class_id} in the tree"));
            (c.base_str, c.base_dex, c.base_int)
        }
    };
    let base_of = |stat: &str| -> f64 {
        match stat {
            "Str" => bs,
            "Dex" => bd,
            _ => bi,
        }
        .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:436: attempt to perform arithmetic on a nil value (base_{stat})"))
    };
    for pass in 1..=2 {
        if pass != 1 {
            for stat in ATTRIBUTES {
                let b = base_of(stat);
                output.set_num(stat, min2(round(ok(calc_lib::val(&db.borrow(), stat, None)), None), b));
                let v = base(db, &[stat]) - b;
                nm(
                    db,
                    "Omni",
                    ModType::Base,
                    v,
                    &format!("{stat} conversion Omniscience"),
                    0,
                    0,
                    vec![],
                );
                let v = inc(db, &[stat]);
                nm(db, "Omni", ModType::Inc, v, "Omniscience", 0, 0, vec![]);
                let v = sum(db, ModType::More, None, &[stat]);
                nm(db, "Omni", ModType::More, v, "Omniscience", 0, 0, vec![]);
            }
        }
        if pass != 2 {
            // :449-462 -- every reduction first, then the three mods (none of the sums reads `Omni`).
            let mut reduction = [0.0; 3];
            for (i, ty) in [ModType::Base, ModType::Inc, ModType::More].into_iter().enumerate() {
                let c = |stat: &str| sum(db, ty, None, &[stat]);
                reduction[i] = c("StrDex") + c("StrInt") + c("DexInt") + 2.0 * c("All");
            }
            const SRC: &str = "Reduction from Double/Triple Dipped attributes to Omniscience";
            nm(db, "Omni", ModType::Base, -reduction[0], SRC, 0, 0, vec![]);
            nm(db, "Omni", ModType::Inc, -reduction[1], SRC, 0, 0, vec![]);
            nm(db, "Omni", ModType::More, -reduction[2], SRC, 0, 0, vec![]);
        }
        for stat in ATTRIBUTES {
            output.set_num(stat, base_of(stat));
        }
        let omni = max2(round(ok(calc_lib::val(&db.borrow(), "Omni", None)), None), 0.0);
        output.set_num("Omni", omni);
        attribute_conditions(db, output);
    }
}

// --- CalcPerform.lua:551-584 `doActorLifeManaReservation` ---------------------------------------------------------------

pub fn do_actor_life_mana_reservation(db: &StoreRef, st: &mut ActorState, add_aura: bool) {
    // `pairs({"Life", "Mana"})` over an array: in order.
    for pool in ["Life", "Mana"] {
        let max = st.output.num(pool);
        let (res_base, res_pct, uncancellable) = if pool == "Life" {
            (st.reserved_life_base, st.reserved_life_percent, st.uncancellable_life_reservation)
        } else {
            (st.reserved_mana_base, st.reserved_mana_percent, st.uncancellable_mana_reservation)
        };
        let reserved;
        if max > 0.0 {
            let low_perc = base(db, &[&format!("Low{pool}Percentage")]);
            reserved = res_base.unwrap_or(0.0) + (max * res_pct.unwrap_or(0.0) / 100.0).ceil();
            // `uncancellableReservation` has no `local` upstream -- a leaked global, assigned before every use.
            let uncancellable = uncancellable.unwrap_or(0.0);
            let out = &mut st.output;
            out.set_num(&format!("{pool}Reserved"), min2(reserved, max));
            out.set_num(&format!("{pool}ReservedPercent"), min2(reserved / max * 100.0, 100.0));
            out.set_num(&format!("{pool}Unreserved"), max - reserved);
            out.set_num(&format!("{pool}UnreservedPercent"), (max - reserved) / max * 100.0);
            out.set_num(&format!("{pool}UncancellableReservation"), min2(uncancellable, 0.0));
            out.set_num(&format!("{pool}CancellableReservation"), 100.0 - uncancellable);
            if (max - reserved) / max <= if low_perc > 0.0 { low_perc } else { misc("LowPoolThreshold") } {
                set_cond(db, &format!("Low{pool}"), true);
            }
        } else {
            reserved = 0.0;
        }
        if add_aura {
            for value in list(db, &[&format!("GrantReserved{pool}AsAura")]) {
                // :578-580 `copyTable(value.mod)`, value scaled, wrapped as an ExtraAura.
                let Value::Mod(m) = value.get("mod") else {
                    panic!("Modules/CalcPerform.lua:578: attempt to index field 'mod' (a non-mod value)")
                };
                let mut aura = m.deep_copy();
                let v = num_val(&aura.value, "Modules/CalcPerform.lua:579");
                aura.value = n((v * min2(reserved, max)).floor());
                let mut t = Table::default();
                t.hash.insert(Rc::from("mod"), Value::Mod(Rc::new(aura)));
                new_mod(db, "ExtraAura", ModType::List, Value::table(t), None, 0, 0, vec![]);
            }
        }
    }
}

// --- CalcPerform.lua:619-634 `applyEnemyModifiers` ----------------------------------------------------------------------

fn apply_enemy_modifiers(db: &StoreRef, enemy_db: &StoreRef, st: &mut ActorState, clear_cache: bool) {
    if clear_cache || st.applied_enemy_modifiers.is_none() {
        st.applied_enemy_modifiers = Some(Vec::new());
    }
    let cache = st.applied_enemy_modifiers.as_mut().expect("set above");
    let entries = ok(db.borrow().tabulate(None, None, &["EnemyModifier"]));
    for e in entries {
        // `local mod = value.value and value.value.mod`
        let Value::Mod(m) = (if e.value.truthy() { e.value.get("mod") } else { Value::Nil }) else {
            continue;
        };
        if cache.iter().any(|c| Rc::ptr_eq(c, &m)) {
            continue;
        }
        let source = m
            .source
            .clone()
            .or_else(|| e.mod_.source.clone())
            .unwrap_or_else(|| panic!("Modules/ModTools.lua:239: setSource with a nil source"));
        // `enemyDB:AddMod(modLib.setSource(mod, source))` re-sources the shared object in place; the copy here carries
        // the same fields (shallow, tags shared) and nothing reads the original's source afterwards.
        let mut c = (*m).clone();
        pob_mods::format::set_source(&mut c, &source);
        enemy_db.borrow_mut().add_mod(Rc::new(c));
        cache.push(m);
    }
}

// --- CalcPerform.lua:636-977 `doActorMisc` ------------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
fn do_actor_misc(env: &mut Env, who: Who, st: &mut ActorState, party: &PartyStub) {
    use ModType::{Base, Inc, More};
    let db = db_of(env, who);
    let enemy_db = enemy_db_of(env, who);
    let fl = |name: &str| flag(&db, &[name]);

    if env.mode_combat {
        // :646 `env.player.mainSkill.baseSkillModList:Flag(nil, "Cruelty")` (the stub has no baseSkillModList; its empty
        // skillModList answers the same "no").
        let player_ms = player_main_skill(env, 646);
        player_ms.guard(&["Cruelty"]);
        if flag(&player_ms.base, &["Cruelty"]) {
            let v = or_override(override_(&db, &["Cruelty"]), 40.0, "Modules/CalcPerform.lua:647");
            set_mult(&db, "Cruelty", v);
        }
        // :649-651 Minimum Rage
        let min_rage = base(&db, &["MinimumRage"]);
        if min_rage > db.borrow().multipliers.get("Rage").copied().unwrap_or(0.0) {
            set_mult(&db, "Rage", base(&db, &["MinimumRage"]));
        }
        // :653 `A and B or C and D and E or 0`
        let allied_fortify = if fl("YourFortifyEqualToParent") {
            panic!("Modules/CalcPerform.lua:653: attempt to index field 'parent' (a nil value)")
        } else if flag(&party.mod_db, &["PartyMemberFortifyEqualToYours"]) {
            party_output(party, "FortificationStacks").unwrap_or(0.0)
        } else {
            0.0
        };
        if base(&db, &["MinimumFortification"]) > 0.0 || allied_fortify > 0.0 {
            set_cond(&db, "Fortified", true);
        }
        if fl("Fortified") || base(&db, &["Multiplier:Fortification"]) > 0.0 {
            // :660-661 `actor.mainSkill and actor.mainSkill.skillModList or actor.modDB`
            let (skill_mod_list, skill_cfg) = match main_skill(env, who) {
                Some(ms) => {
                    ms.guard(&["FortifyDuration"]);
                    (ms.list, Some(ms.cfg))
                }
                None => (db.clone(), None),
            };
            let max_stacks = max2(
                or_override(
                    override_(&db, &["MaximumFortification"]),
                    sum(&db, Base, skill_cfg.as_ref(), &["MaximumFortification"]),
                    "Modules/CalcPerform.lua:662",
                ),
                allied_fortify,
            );
            let min_stacks = min2(
                if fl("Condition:HaveMaxFortification") {
                    max_stacks
                } else {
                    base(&db, &["MinimumFortification"])
                },
                max_stacks,
            );
            let stack_override = override_(&db, &["FortificationStacks"]);
            let stacks = min2(
                or_override(
                    stack_override.clone(),
                    if min_stacks > 0.0 { min_stacks } else { max_stacks },
                    "Modules/CalcPerform.lua:664",
                ),
                max_stacks,
            );
            let increased_duration = sum(&skill_mod_list, Inc, None, &["FortifyDuration"]);
            let out = &mut st.output;
            out.set_num("MaximumFortification", max_stacks);
            out.set_num("MinimumFortification", min_stacks);
            out.set_num(
                "RemovableFortification",
                min2(
                    max_stacks - min_stacks,
                    or_override(stack_override, max_stacks, "Modules/CalcPerform.lua:668") - min_stacks,
                ),
            );
            out.set_num("FortificationStacks", stacks);
            out.set_num("FortificationStacksOver20", min2(max2(0.0, stacks - 20.0), max_stacks - 20.0));
            let dur = or_override(
                ok(skill_mod_list.borrow().override_(None, &["FortifyDuration"])),
                misc("FortifyBaseDuration"),
                "Modules/CalcPerform.lua:671",
            );
            out.set_num("FortifyDuration", dur * (1.0 + increased_duration / 100.0));
            // :672 a string, so Willowgift's mod can be shown instead
            out.set_str("FortificationEffect", "0");
            if !fl("Condition:NoFortificationMitigation") {
                st.output.set_num("FortificationEffect", stacks);
                nm(&db, "DamageTakenWhenHit", More, -stacks, "Fortification", 0, 0, vec![]);
            }
            if stacks >= max_stacks {
                nm_flag(&db, "Condition:HaveMaximumFortification", "");
            }
            bump_mult(&db, "BuffOnSelf", 1.0);
        }
        if fl("Onslaught") {
            // :681-706 -- a Silver Flask granting Onslaught brings its flask effect into the calculation.
            let mut onslaught_from_flask = false;
            let mut flask_effect_inc = -100.0;
            let player_cfg = cfg_actor("player");
            for item in &env.flasks {
                let base_name = item
                    .base_name
                    .as_deref()
                    .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:689: attempt to index field 'baseName' (a nil value)"));
                if base_name.contains("Silver Flask") {
                    onslaught_from_flask = true;
                    let effect_inc = item.flask_data.as_ref().and_then(|d| d.num("effectInc"));
                    let effect_inc = effect_inc
                        .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:692: attempt to perform arithmetic on field 'effectInc'"));
                    let mut cur = effect_inc + sum(&db, Inc, Some(&player_cfg), &["FlaskEffect"]);
                    if item.rarity == "MAGIC" {
                        cur += sum(&db, Inc, Some(&player_cfg), &["MagicUtilityFlaskEffect"]);
                    }
                    if flask_effect_inc < cur / 100.0 {
                        flask_effect_inc = cur / 100.0;
                    }
                }
            }
            let onslaught_effect_inc = inc(&db, &["OnslaughtEffect", "BuffEffectOnSelf"]) / 100.0;
            let effect = if onslaught_from_flask {
                (20.0 * (1.0 + flask_effect_inc + onslaught_effect_inc)).floor()
            } else {
                (20.0 * (1.0 + onslaught_effect_inc)).floor()
            };
            nm(&db, "Speed", Inc, effect, "Onslaught", mod_flag("Attack"), 0, vec![]);
            nm(&db, "Speed", Inc, effect, "Onslaught", mod_flag("Cast"), 0, vec![]);
            nm(&db, "MovementSpeed", Inc, effect, "Onslaught", 0, 0, vec![]);
        }
        if cond(&db, "AffectedByArcaneSurge") == Some(true) || fl("Condition:ArcaneSurge") {
            set_cond(&db, "AffectedByArcaneSurge", true);
            let player_cfg = cfg_actor("player");
            let mut effect = 1.0 + inc(&db, &["ArcaneSurgeEffect", "BuffEffectOnSelf"]) / 100.0;
            effect += sum(&db, Inc, Some(&player_cfg), &["FlaskEffect"]) / 100.0 * base(&db, &["FlaskEffectToArcaneSurgeEffect"]) / 100.0;
            let v = or_num(max_(&db, None, &["ArcaneSurgeManaRegen"]), 30.0) * effect;
            nm(&db, "ManaRegen", Inc, v, "Arcane Surge", 0, 0, vec![]);
            let cast_speed = or_num(max_(&db, None, &["ArcaneSurgeCastSpeed"]), 20.0) * effect;
            nm(&db, "Speed", Inc, cast_speed, "Arcane Surge", mod_flag("Cast"), 0, vec![]);
            if fl("ArcaneSurgeCastSpeedToMovementSpeed") {
                nm(&db, "MovementSpeed", Inc, cast_speed, "Arcane Surge", 0, 0, vec![]);
            }
            let damage = or_num(max_(&db, None, &["ArcaneSurgeDamage"]), 0.0);
            if damage != 0.0 {
                nm(&db, "Damage", More, damage * effect, "Arcane Surge", mod_flag("Spell"), 0, vec![]);
            }
            // `pairs(additionalMods)` over three string keys -- a hash order upstream (L10); the three mods have different
            // names, so only their insertion order into modDB could differ (pob-ts order, D13).
            for (m, ty) in [("LifeRegen", Inc), ("ManaRecoup", Base), ("ManaCostEfficiency", Inc)] {
                let value = sum(&db, ty, None, &[&format!("ArcaneSurgeAlso{m}")]);
                if value != 0.0 {
                    nm(&db, m, ty, value * effect, "Arcane Surge", 0, 0, vec![]);
                }
            }
        }
        // :735-739 Fanaticism -- `actor.mainSkill and actor.mainSkill.skillFlags.selfCast` (never set on the stub).
        if fl("Fanaticism") && main_skill(env, who).is_some_and(|ms| ms.flag("selfCast")) {
            let effect = (75.0 * (1.0 + inc(&db, &["BuffEffectOnSelf"]) / 100.0)).floor();
            nm(&db, "Speed", More, effect, "Fanaticism", mod_flag("Cast"), 0, vec![]);
            nm(&db, "Cost", More, -effect, "Fanaticism", mod_flag("Cast"), 0, vec![]);
            nm(&db, "AreaOfEffect", Inc, effect, "Fanaticism", mod_flag("Cast"), 0, vec![]);
        }
        if fl("Condition:CanGainSpiritInfusion") {
            let global_effect = || {
                tag(&[
                    ("type", s("GlobalEffect")),
                    ("effectType", s("Buff")),
                    ("effectName", s("Spirit Infusion")),
                    ("unscalable", Value::Bool(true)),
                ])
            };
            let multiplier = || tag(&[("type", s("Multiplier")), ("var", s("SpiritInfusion"))]);
            let channel = || tag(&[("type", s("SkillType")), ("skillType", skill_type("Channel"))]);
            let spell = keyword_flag("Spell");
            nm(
                &db,
                "EnergyShieldRechargeFaster",
                Inc,
                15.0,
                "Spirit Infusion",
                0,
                0,
                vec![multiplier(), global_effect()],
            );
            nm(
                &db,
                "Damage",
                More,
                5.0,
                "Spirit Infusion",
                0,
                spell,
                vec![channel(), multiplier(), global_effect()],
            );
            nm(
                &db,
                "Cost",
                More,
                10.0,
                "Spirit Infusion",
                0,
                spell,
                vec![channel(), multiplier(), global_effect()],
            );
        }
        if fl("UnholyMight") {
            let effect = 1.0 + inc(&db, &["BuffEffectOnSelf"]) / 100.0;
            nm(
                &db,
                "PhysicalDamageConvertToChaos",
                Base,
                (100.0 * effect).floor(),
                "Unholy Might",
                0,
                0,
                vec![],
            );
            nm_flag(&db, "Condition:CanWither", "Unholy Might");
            let value = base(&db, &["UnholyMightAlsoChaosPenetration"]);
            if value != 0.0 {
                nm(&db, "ChaosPenetration", Base, value * effect, "Unholy Might", 0, 0, vec![]);
            }
        }
        if fl("ShepherdOfSouls") {
            let vaal = || tag(&[("type", s("SkillType")), ("skillType", skill_type("Vaal"))]);
            let not_aura = || {
                tag(&[
                    ("type", s("SkillType")),
                    ("skillType", skill_type("Aura")),
                    ("neg", Value::Bool(true)),
                ])
            };
            nm(&db, "SoulCost", More, -80.0, "Shepherd of Souls", 0, 0, vec![vaal(), not_aura()]);
            let past8 = tag(&[("type", s("Multiplier")), ("var", s("VaalSkillsUsedInPast8Seconds"))]);
            nm(
                &db,
                "SoulCost",
                Inc,
                100.0,
                "Shepherd of Souls",
                0,
                0,
                vec![vaal(), not_aura(), past8],
            );
        }
        if fl("ChaoticMight") {
            let v = (30.0 * (1.0 + inc(&db, &["BuffEffectOnSelf"]) / 100.0)).floor();
            nm(&db, "PhysicalDamageGainAsChaos", Base, v, "Chaotic Might", 0, 0, vec![]);
        }
        if fl("Tailwind") {
            let v = (8.0 * (1.0 + inc(&db, &["TailwindEffectOnSelf", "BuffEffectOnSelf"]) / 100.0)).floor();
            nm(&db, "ActionSpeed", Inc, v, "Tailwind", 0, 0, vec![]);
        }
        if fl("Condition:TotemTailwind") {
            nm(&db, "TotemActionSpeed", Inc, 8.0, "Tailwind", 0, 0, vec![]);
        }
        if fl("Adrenaline") {
            let em = 1.0 + inc(&db, &["BuffEffectOnSelf"]) / 100.0;
            nm(&db, "Damage", Inc, (100.0 * em).floor(), "Adrenaline", 0, 0, vec![]);
            nm(&db, "Speed", Inc, (25.0 * em).floor(), "Adrenaline", mod_flag("Attack"), 0, vec![]);
            nm(&db, "Speed", Inc, (25.0 * em).floor(), "Adrenaline", mod_flag("Cast"), 0, vec![]);
            nm(&db, "MovementSpeed", Inc, (25.0 * em).floor(), "Adrenaline", 0, 0, vec![]);
            nm(
                &db,
                "PhysicalDamageReduction",
                Base,
                (10.0 * em).floor(),
                "Adrenaline",
                0,
                0,
                vec![],
            );
        }
        if fl("Condition:WildSavagery") && fl("WildSavagery") {
            nm(&db, "PhysicalDamage", Inc, 100.0, "Wild Savagery", 0, 0, vec![]);
            nm(&db, "ActionSpeed", Inc, 10.0, "Wild Savagery", 0, 0, vec![]);
            nm_flag(&db, "IgnoreEnemyPhysicalDamageReduction", "Wild Savagery");
            nm_flag(&db, "StunImmune", "Wild Savagery");
        }
        if fl("Convergence") {
            let v = (30.0 * (1.0 + inc(&db, &["BuffEffectOnSelf"]) / 100.0)).floor();
            nm(&db, "ElementalDamage", More, v, "Convergence", 0, 0, vec![]);
        }
        if fl("HerEmbrace") {
            set_cond(&db, "HerEmbrace", true);
            nm(&db, "AvoidStun", Base, 100.0, "Her Embrace", 0, 0, vec![]);
            nm(
                &db,
                "PhysicalDamageGainAsFire",
                Base,
                123.0,
                "Her Embrace",
                mod_flag("Sword"),
                0,
                vec![],
            );
            nm(&db, "AvoidFreeze", Base, 100.0, "Her Embrace", 0, 0, vec![]);
            nm(&db, "AvoidChill", Base, 100.0, "Her Embrace", 0, 0, vec![]);
            nm(&db, "AvoidIgnite", Base, 100.0, "Her Embrace", 0, 0, vec![]);
            nm(&db, "Speed", Inc, 20.0, "Her Embrace", mod_flag("Attack"), 0, vec![]);
            nm(&db, "Speed", Inc, 20.0, "Her Embrace", mod_flag("Cast"), 0, vec![]);
            nm(&db, "MovementSpeed", Inc, 20.0, "Her Embrace", 0, 0, vec![]);
        }
        if fl("Condition:OnConsecratedGround") {
            let effect = 1.0 + inc(&db, &["ConsecratedGroundEffect"]) / 100.0;
            nm(&db, "LifeRegenPercent", Base, 5.0 * effect, "Consecrated Ground", 0, 0, vec![]);
            let v = (inc(&db, &["ConsecratedGroundAlsoAccuracy"]) * effect).floor();
            nm(&db, "Accuracy", Inc, v, "Consecrated Ground", 0, 0, vec![]);
        }
        if fl("Condition:PhantasmalMight") {
            // :810 a slight hack upstream, to not double-count the initial buff.
            let limit = match st.output.get("ActivePhantasmLimit") {
                None => 1.0,
                Some(crate::output::OutValue::Bool(false)) => 1.0,
                Some(crate::output::OutValue::Num(v)) => *v,
                Some(other) => panic!("Modules/CalcPerform.lua:810: attempt to perform arithmetic on {other:?}"),
            };
            // `(multipliers.BuffOnSelf or 0) + (output.ActivePhantasmLimit or 1) - 1`, left to right.
            let cur = db.borrow().multipliers.get("BuffOnSelf").copied().unwrap_or(0.0);
            set_mult(&db, "BuffOnSelf", cur + limit - 1.0);
        }
        if fl("Elusive") {
            elusive(env, who, &db, st);
        }
        if let Some(wither_stack) = max_(&db, None, &["WitherEffectStack"]) {
            nm_flag(&db, "Condition:CanWither", "Config");
            let t = tag(&[("type", s("Multiplier")), ("var", s("WitheredStack")), ("limit", n(15.0))]);
            nm(&enemy_db, "ChaosDamageTaken", Inc, wither_stack, "Withered", 0, 0, vec![t]);
        }
        if fl("Condition:CanBeWithered") {
            let effect = 6.0 * (100.0 + inc(&db, &["WitherEffectOnSelf"])) / 100.0 * more(&db, None, &["WitherEffectOnSelf"]);
            let t = tag(&[("type", s("Multiplier")), ("var", s("WitheredStack")), ("limit", n(15.0))]);
            nm(&db, "ChaosDamageTaken", Inc, effect, "Withered", 0, 0, vec![t]);
        }
        if fl("Excommunicated") {
            nm(&db, "ChaosDamage", More, -100.0, "Excommunicated", 0, 0, vec![]);
        }
        if fl("Blind") && !fl("CannotBeBlinded") && !fl("UnaffectedByBlind") {
            let mut effect = 1.0 + inc(&db, &["BlindEffect", "BuffEffectOnSelf"]) / 100.0;
            let blind_override = override_(&db, &["BlindEffect"]);
            if let Some(v) = blind_override.filter(Value::truthy) {
                effect = min2(num_val(&v, "Modules/CalcPerform.lua:862") / 100.0, effect);
            }
            nm(&db, "Accuracy", More, (-20.0 * effect).floor(), "Blind", 0, 0, vec![]);
            nm(&db, "Evasion", More, (-20.0 * effect).floor(), "Blind", 0, 0, vec![]);
        }
        // :868-904 the three non-damaging self-ailments + Freeze.
        if fl("Chill") {
            let effect = self_ailment(&db, "Chill", &["AvoidChill", "AvoidAilments", "AvoidElementalAilments"], true);
            if fl("SkitterbotBonechill") {
                let sign = if fl("SelfChillEffectIsReversed") { -1.0 } else { 1.0 };
                nm(&db, "ColdDamageTaken", Inc, effect * sign, "Bonechill", 0, 0, vec![]);
            }
            let sign = if fl("SelfChillEffectIsReversed") { 1.0 } else { -1.0 };
            nm(&db, "ActionSpeed", Inc, effect * sign, "Chill", 0, 0, vec![]);
        }
        if fl("Shock") {
            let effect = self_ailment(&db, "Shock", &["AvoidShock", "AvoidAilments", "AvoidElementalAilments"], false);
            nm(&db, "DamageTaken", Inc, effect, "Shock", 0, 0, vec![]);
        }
        if fl("Scorch") {
            let effect = self_ailment(&db, "Scorch", &["AvoidScorch", "AvoidAilments", "AvoidElementalAilments"], true);
            nm(&db, "ElementalResist", Base, -effect, "Scorch", 0, 0, vec![]);
        }
        if fl("Freeze") {
            let effect = max2((70.0 * ok(calc_lib::mod_(&db.borrow(), None, &["SelfChillEffect"]))).floor(), 0.0);
            nm(&db, "ActionSpeed", Inc, -effect, "Freeze", 0, 0, vec![]);
        }
        if fl("CanLeechLifeOnFullLife") && !fl("GhostReaver") {
            set_cond(&db, "Leeching", true);
            set_cond(&db, "LeechingLife", true);
        }
        if fl("CanLeechEnergyShieldOnFullEnergyShield") {
            set_cond(&db, "Leeching", true);
            set_cond(&db, "LeechingEnergyShield", true);
        }
        if fl("Condition:CanGainRage") || base(&db, &["RageRegen"]) > 0.0 {
            // :915 `skillCfg` is an undeclared GLOBAL here (the Fortify block's local is out of scope) -- nil (D20).
            let max_stacks = (base(&db, &["MaximumRage"]) * more(&db, None, &["MaximumRage"])).floor();
            let min_stacks = min2(base(&db, &["MinimumRage"]), max_stacks);
            let rage_config = base(&db, &["Multiplier:RageStack"]);
            let stacks = max2(min2(rage_config, max_stacks), if min_stacks > 0.0 { min_stacks } else { 0.0 });
            let rage_effect = (stacks * ok(calc_lib::mod_(&db.borrow(), None, &["RageEffect"]))).floor();
            st.output.set_num("RageEffect", rage_effect);
            nm(&db, "Multiplier:RageEffect", Base, rage_effect, "Base", 0, 0, vec![]);
            st.output.set_num("Rage", stacks);
            st.output.set_num("MaximumRage", max_stacks);
            nm(&db, "Multiplier:Rage", Base, stacks, "Base", 0, 0, vec![]);
            let fl_spell = if fl("Condition:RageSpellDamage") { "Spell" } else { "Attack" };
            nm(&db, "Damage", More, rage_effect, "Rage", mod_flag(fl_spell), 0, vec![]);
            if stacks == max_stacks {
                nm_flag(&db, "Condition:HaveMaximumRage", "");
            }
            st.output
                .set_num("InherentRageLossDelay", 2.0 + base(&db, &["InherentRageLossDelay"]));
            let loss = if fl("InherentRageLossIsPrevented") {
                0.0
            } else {
                10.0 * (1.0 + inc(&db, &["InherentRageLoss"]) / 100.0)
            };
            st.output.set_num("InherentRageLoss", loss);
        }
        // :935 `(env.configInput.multiplierManaBurnStacks or 0) > 0`
        let mana_burn_config = match env.config.input.get("multiplierManaBurnStacks") {
            None | Some(pob_build::ConfigValue::Bool(false)) => 0.0,
            Some(pob_build::ConfigValue::Num(v)) => *v,
            Some(other) => panic!("Modules/CalcPerform.lua:935: attempt to compare number with {other:?}"),
        };
        if mana_burn_config > 0.0 {
            let mut max_mana_burn = base(&db, &["MaxManaBurnStacks"]);
            if max_mana_burn == 0.0 {
                max_mana_burn = 9999.0;
            }
            let mut stacks = min2(mana_burn_config, max_mana_burn);
            nm(&db, "Multiplier:ManaBurnStacks", Base, stacks, "Config", 0, 0, vec![]);
            stacks += sum(&db, Base, Some(&cfg_actor("player")), &["EffectiveManaBurnStacks"]);
            if fl("Condition:WeepingWoundsInsteadOfManaBurn") {
                nm(&db, "Multiplier:WeepingWoundsStacks", Base, stacks, "Config", 0, 0, vec![]);
            } else {
                nm(&db, "Multiplier:EffectiveManaBurnStacks", Base, stacks, "Config", 0, 0, vec![]);
            }
        }
        if base(&db, &["CoveredInAshEffect"]) > 0.0 {
            let effect = base(&db, &["CoveredInAshEffect"]);
            nm(
                &enemy_db,
                "FireDamageTaken",
                Inc,
                min2(effect, 20.0),
                "Covered in Ash",
                0,
                0,
                vec![],
            );
        }
        if base(&db, &["CoveredInFrostEffect"]) > 0.0 {
            let effect = base(&db, &["CoveredInFrostEffect"]);
            nm(
                &enemy_db,
                "ColdDamageTaken",
                Inc,
                min2(effect, 20.0),
                "Covered in Frost",
                0,
                0,
                vec![],
            );
        }
        if fl("HasMalediction") {
            nm(&db, "DamageTaken", Inc, 10.0, "Malediction", 0, 0, vec![]);
            nm(&db, "Damage", Inc, -10.0, "Malediction", 0, 0, vec![]);
        }
        if fl("HasMaddeningPresence") {
            nm(&db, "ActionSpeed", Inc, -10.0, "Maddening Presence", 0, 0, vec![]);
            nm(&db, "Damage", Inc, -10.0, "Maddening Presence", 0, 0, vec![]);
        }
        if fl("HasShapersPresence") {
            nm(&db, "BuffExpireFaster", More, -20.0, "Shapers Presence", 0, 0, vec![]);
        }
        if fl("Condition:CanHaveSoulEater") {
            let max = or_override(
                override_(&db, &["SoulEaterMax"]),
                base(&db, &["SoulEaterMax"]),
                "Modules/CalcPerform.lua:969",
            );
            let t = || tag(&[("type", s("Multiplier")), ("var", s("SoulEaterStack")), ("limit", n(max))]);
            nm(&db, "Speed", Inc, 5.0, "Base", mod_flag("Attack"), 0, vec![t()]);
            nm(&db, "Speed", Inc, 5.0, "Base", mod_flag("Cast"), 0, vec![t()]);
        }
    }

    // :976
    apply_enemy_modifiers(&db, &enemy_db, st, false);
}

/// `env.partyMembers.output[key]` -- PartyTab is P5, its output table is always empty.
fn party_output(_party: &PartyStub, _key: &str) -> Option<f64> {
    None
}

/// :817-848 Elusive.
fn elusive(env: &Env, who: Who, db: &StoreRef, st: &mut ActorState) {
    let max_skill_inc = or_num(max_(db, Some(&crate::setup::cfg_source("Skill")), &["ElusiveEffect"]), 0.0);
    let mut inc_ = inc(db, &["ElusiveEffect", "BuffEffectOnSelf"]);
    // :815 `actor.mainSkill.skillModList:Flag(nil, "SupportedByNightblade")` -- the enemy has no mainSkill, so an Elusive enemy
    // is Lua's index error.
    let ms =
        main_skill(env, who).unwrap_or_else(|| panic!("Modules/CalcPerform.lua:815: attempt to index field 'mainSkill' (a nil value)"));
    ms.guard(&["SupportedByNightblade"]);
    if flag(&ms.list, &["SupportedByNightblade"]) {
        inc_ += inc(db, &["NightbladeSupportedElusiveEffect"]);
    }
    inc_ += max_skill_inc;
    let effect_mod = (1.0 + inc_ / 100.0) * more(db, None, &["ElusiveEffect", "BuffEffectOnSelf"]) * 100.0;
    let min_threshold = or_override(override_(db, &["ElusiveEffectMinThreshold"]), 0.0, "Modules/CalcPerform.lua:820");
    let increase_duration = base(db, &["ElusiveEffectIncreaseDuration"]);
    let mut peak = effect_mod;
    if increase_duration > 0.0 {
        let rate = 20.0 / (1.0 + inc(db, &["ElusiveEffectLossSlower"]) / 100.0);
        peak = effect_mod + rate * increase_duration;
        let decrease_duration = (peak - min_threshold) / rate;
        let total_duration = increase_duration + decrease_duration;
        let avg_increase = (effect_mod + peak) / 2.0;
        let avg_decrease = (peak + min_threshold) / 2.0;
        st.output.set_num(
            "ElusiveEffectMod",
            (avg_increase * increase_duration + avg_decrease * decrease_duration) / total_duration,
        );
    } else {
        st.output.set_num("ElusiveEffectMod", (effect_mod + min_threshold) / 2.0);
    }
    nm(db, "ElusiveEffect", ModType::Inc, max_skill_inc, "Max Skill Effect", 0, 0, vec![]);
    if let Some(v) = override_(db, &["ElusiveEffect"]).filter(Value::truthy) {
        st.output
            .set_num("ElusiveEffectMod", min2(num_val(&v, "Modules/CalcPerform.lua:838"), peak));
    }
    let effect = st.output.num("ElusiveEffectMod") / 100.0;
    set_cond(db, "Elusive", true);
    nm(
        db,
        "AvoidAllDamageFromHitsChance",
        ModType::Base,
        (15.0 * effect).floor(),
        "Elusive",
        0,
        0,
        vec![],
    );
    nm(db, "MovementSpeed", ModType::Inc, (30.0 * effect).floor(), "Elusive", 0, 0, vec![]);
}

/// :868-899 -- one self-ailment's magnitude. Upstream writes
/// `local xValue = m_max(modDB:Sum("BASE", nil, "SelfXOverride"), modDB:Override(nil, "XVal")) or ailmentData.X.default`
/// and both halves are traps (MINEFIELD L11/L26): `Override` returns ZERO values on a miss, which in the last argument
/// slot expand to nothing (`m_max(sum)`), and `m_max` always returns a number, so `or ...default` is dead.
fn self_ailment(db: &StoreRef, name: &str, avoid_names: &[&str], shock_avoid_applies: bool) -> f64 {
    let data = rules()
        .non_damaging_ailment
        .get(name)
        .unwrap_or_else(|| panic!("data.nonDamagingAilment.{name} is nil"));
    let self_override = base(db, &[&format!("Self{name}Override")]);
    let val_override = override_(db, &[&format!("{name}Val")]);
    let value = match val_override {
        None => self_override,
        Some(v) => max2(
            self_override,
            v.as_num().unwrap_or_else(|| {
                panic!(
                    "Modules/CalcPerform.lua {name}Val: bad argument #2 to 'max' (number expected, got {})",
                    v.type_name()
                )
            }),
        ),
    };
    let total = ok(calc_lib::mod_(&db.borrow(), None, &[&format!("Self{name}Effect")]));
    let avoid = if flag(db, &[&format!("{name}Immune"), "ElementalAilmentImmune"]) {
        100.0
    } else {
        let shock_part = if shock_avoid_applies && flag(db, &["ShockAvoidAppliesToElementalAilments"]) {
            base(db, &["AvoidShock"])
        } else {
            0.0
        };
        let supp_part = if flag(db, &["SpellSuppressionAppliesToAilmentAvoidance"]) {
            base(db, &["SpellSuppressionChance"]) / 2.0
        } else {
            0.0
        };
        min2(base(db, avoid_names) + shock_part + supp_part, 100.0).floor()
    };
    if avoid == 100.0 {
        0.0
    } else {
        let cap = or_override(
            override_(db, &[&format!("{name}Max")]),
            data.max,
            &format!("Modules/CalcPerform.lua {name}Max"),
        );
        min2(max2((value * total).floor(), 0.0), cap)
    }
}

// --- CalcPerform.lua:980-1139 `doActorCharges` --------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
fn do_actor_charges(env: &Env, who: Who, st: &mut ActorState, party: &PartyStub) {
    let db = db_of(env, who);
    let fl = |name: &str| flag(&db, &[name]);
    let charge_duration = |name: &str| {
        let m = ok(calc_lib::mod_(
            &db.borrow(),
            None,
            &[&format!("{name}ChargesDuration"), "ChargeDuration"],
        ));
        (base(&db, &["ChargeDuration"]) * m).floor()
    };
    let o = &mut st.output;

    o.set_num("PowerChargesMin", max2(base(&db, &["PowerChargesMin"]), 0.0));
    let v = or_override(
        override_(&db, &["PowerChargesMax"]),
        max2(base(&db, &["PowerChargesMax"]), 0.0),
        "Modules/CalcPerform.lua:987",
    );
    o.set_num("PowerChargesMax", v);
    o.set_num("PowerChargesDuration", charge_duration("Power"));
    if fl("MaximumFrenzyChargesIsMaximumPowerCharges") {
        let src = first_mod_source(&db, "MaximumFrenzyChargesIsMaximumPowerCharges", 991);
        let m = create_mod(
            "FrenzyChargesMax",
            Some(ModType::Override),
            n(o.num("PowerChargesMax")),
            src.as_deref(),
            0,
            0,
            vec![],
        );
        db.borrow_mut().replace_mod(m);
    }
    o.set_num("FrenzyChargesMin", max2(base(&db, &["FrenzyChargesMin"]), 0.0));
    let fallback = max2(
        if fl("MaximumFrenzyChargesIsMaximumPowerCharges") {
            o.num("PowerChargesMax")
        } else {
            base(&db, &["FrenzyChargesMax"])
        },
        0.0,
    );
    let v = or_override(override_(&db, &["FrenzyChargesMax"]), fallback, "Modules/CalcPerform.lua:995");
    o.set_num("FrenzyChargesMax", v);
    o.set_num("FrenzyChargesDuration", charge_duration("Frenzy"));
    if fl("MaximumEnduranceChargesIsMaximumFrenzyCharges") {
        let src = first_mod_source(&db, "MaximumEnduranceChargesIsMaximumFrenzyCharges", 998);
        let m = create_mod(
            "EnduranceChargesMax",
            Some(ModType::Override),
            n(o.num("FrenzyChargesMax")),
            src.as_deref(),
            0,
            0,
            vec![],
        );
        db.borrow_mut().replace_mod(m);
    }
    o.set_num("EnduranceChargesMin", max2(base(&db, &["EnduranceChargesMin"]), 0.0));
    // :1001 `env.partyMembers and env.partyMembers.modDB:Flag(nil, "PartyMember...") and env.partyMembers.output.X or ...`
    let party_endurance = if flag(&party.mod_db, &["PartyMemberMaximumEnduranceChargesEqualToYours"]) {
        party_output(party, "EnduranceChargesMax")
    } else {
        None
    };
    let inner = match party_endurance {
        Some(v) => v,
        None if fl("MaximumEnduranceChargesIsMaximumFrenzyCharges") => o.num("FrenzyChargesMax"),
        None => base(&db, &["EnduranceChargesMax"]),
    };
    let v = or_override(
        override_(&db, &["EnduranceChargesMax"]),
        max2(inner, 0.0),
        "Modules/CalcPerform.lua:1001",
    );
    o.set_num("EnduranceChargesMax", v);
    o.set_num("EnduranceChargesDuration", charge_duration("Endurance"));
    for name in [
        "SiphoningChargesMax",
        "ChallengerChargesMax",
        "BlitzChargesMax",
        "InspirationChargesMax",
        "CrabBarriersMax",
    ] {
        o.set_num(name, max2(base(&db, &[name]), 0.0));
    }
    let v = if fl("MinimumEnduranceChargesEqualsMinimumBrutalCharges") {
        if fl("MinimumEnduranceChargesIsMaximumEnduranceCharges") {
            o.num("EnduranceChargesMax")
        } else {
            o.num("EnduranceChargesMin")
        }
    } else {
        0.0
    };
    o.set_num("BrutalChargesMin", max2(v, 0.0));
    let v = if fl("MaximumEnduranceChargesEqualsMaximumBrutalCharges") {
        o.num("EnduranceChargesMax")
    } else {
        0.0
    };
    o.set_num("BrutalChargesMax", max2(v, 0.0));
    let v = o.num("EnduranceChargesMax");
    o.set_num("BrineChargesMax", max2(v, 0.0));
    let v = if fl("MinimumPowerChargesEqualsMinimumAbsorptionCharges") {
        if fl("MinimumPowerChargesIsMaximumPowerCharges") {
            o.num("PowerChargesMax")
        } else {
            o.num("PowerChargesMin")
        }
    } else {
        0.0
    };
    o.set_num("AbsorptionChargesMin", max2(v, 0.0));
    let v = if fl("MaximumPowerChargesEqualsMaximumAbsorptionCharges") {
        o.num("PowerChargesMax")
    } else {
        0.0
    };
    o.set_num("AbsorptionChargesMax", max2(v, 0.0));
    let v = if fl("MinimumFrenzyChargesEqualsMinimumAfflictionCharges") {
        if fl("MinimumFrenzyChargesIsMaximumFrenzyCharges") {
            o.num("FrenzyChargesMax")
        } else {
            o.num("FrenzyChargesMin")
        }
    } else {
        0.0
    };
    o.set_num("AfflictionChargesMin", max2(v, 0.0));
    let v = if fl("MaximumFrenzyChargesEqualsMaximumAfflictionCharges") {
        o.num("FrenzyChargesMax")
    } else {
        0.0
    };
    o.set_num("AfflictionChargesMax", max2(v, 0.0));
    o.set_num("BloodChargesMax", max2(base(&db, &["BloodChargesMax"]), 0.0));
    o.set_num("SpiritChargesMax", max2(base(&db, &["SpiritChargesMax"]), 0.0));
    let extra = if fl("Condition:CanGainSpiritInfusion") { 10.0 } else { 0.0 };
    o.set_num("SpiritInfusionsMax", base(&db, &["SpiritInfusionsMax"]) + extra);

    // :1023-1037 initialise charges to 0
    for name in [
        "PowerCharges",
        "FrenzyCharges",
        "EnduranceCharges",
        "SiphoningCharges",
        "ChallengerCharges",
        "BlitzCharges",
        "InspirationCharges",
        "GhostShrouds",
        "BrutalCharges",
        "BrineCharges",
        "AbsorptionCharges",
        "AfflictionCharges",
        "BloodCharges",
        "SpiritCharges",
        "SpiritInfusions",
    ] {
        o.set_num(name, 0.0);
    }

    // :1039-1110 the current number of charges
    if fl("MinimumFrenzyChargesIsMaximumFrenzyCharges") {
        let v = o.num("FrenzyChargesMax");
        o.set_num("FrenzyChargesMin", v);
    }
    if fl("MinimumEnduranceChargesIsMaximumEnduranceCharges") {
        let v = o.num("EnduranceChargesMax");
        o.set_num("EnduranceChargesMin", v);
    }
    if fl("MinimumPowerChargesIsMaximumPowerCharges") {
        let v = o.num("PowerChargesMax");
        o.set_num("PowerChargesMin", v);
    }
    if fl("UsePowerCharges") {
        let v = or_override(
            override_(&db, &["PowerCharges"]),
            o.num("PowerChargesMax"),
            "Modules/CalcPerform.lua:1047",
        );
        o.set_num("PowerCharges", v);
    }
    if fl("PowerChargesConvertToAbsorptionCharges") {
        let v = max2(
            o.num("PowerCharges"),
            min2(o.num("AbsorptionChargesMax"), o.num("AbsorptionChargesMin")),
        );
        o.set_num("AbsorptionCharges", v);
        o.set_num("PowerCharges", 0.0);
    } else {
        let v = max2(o.num("PowerCharges"), min2(o.num("PowerChargesMax"), o.num("PowerChargesMin")));
        o.set_num("PowerCharges", v);
    }
    let v = max2(o.num("PowerCharges") - o.num("PowerChargesMin"), 0.0);
    o.set_num("RemovablePowerCharges", v);
    if fl("UseFrenzyCharges") {
        let v = or_override(
            override_(&db, &["FrenzyCharges"]),
            o.num("FrenzyChargesMax"),
            "Modules/CalcPerform.lua:1059",
        );
        o.set_num("FrenzyCharges", v);
    }
    if fl("FrenzyChargesConvertToAfflictionCharges") {
        let v = max2(
            o.num("FrenzyCharges"),
            min2(o.num("AfflictionChargesMax"), o.num("AfflictionChargesMin")),
        );
        o.set_num("AfflictionCharges", v);
        o.set_num("FrenzyCharges", 0.0);
    } else {
        let v = max2(o.num("FrenzyCharges"), min2(o.num("FrenzyChargesMax"), o.num("FrenzyChargesMin")));
        o.set_num("FrenzyCharges", v);
    }
    let v = max2(o.num("FrenzyCharges") - o.num("FrenzyChargesMin"), 0.0);
    o.set_num("RemovableFrenzyCharges", v);
    if fl("UseEnduranceCharges") {
        let v = or_override(
            override_(&db, &["EnduranceCharges"]),
            o.num("EnduranceChargesMax"),
            "Modules/CalcPerform.lua:1071",
        );
        o.set_num("EnduranceCharges", v);
        if fl("CanGainBrineCharges") {
            let v = o.num("BrineChargesMax");
            o.set_num("BrineCharges", v);
        }
    }
    if fl("EnduranceChargesConvertToBrutalCharges") {
        let v = max2(
            o.num("EnduranceCharges"),
            min2(o.num("BrutalChargesMax"), o.num("BrutalChargesMin")),
        );
        o.set_num("BrutalCharges", v);
        o.set_num("EnduranceCharges", 0.0);
    } else {
        let v = max2(
            o.num("EnduranceCharges"),
            min2(o.num("EnduranceChargesMax"), o.num("EnduranceChargesMin")),
        );
        o.set_num("EnduranceCharges", v);
    }
    let v = max2(o.num("EnduranceCharges") - o.num("EnduranceChargesMin"), 0.0);
    o.set_num("RemovableEnduranceCharges", v);
    if fl("UseSiphoningCharges") {
        let v = or_override(
            override_(&db, &["SiphoningCharges"]),
            o.num("SiphoningChargesMax"),
            "Modules/CalcPerform.lua:1086",
        );
        o.set_num("SiphoningCharges", v);
    }
    if fl("UseChallengerCharges") {
        let v = or_override(
            override_(&db, &["ChallengerCharges"]),
            o.num("ChallengerChargesMax"),
            "Modules/CalcPerform.lua:1089",
        );
        o.set_num("ChallengerCharges", v);
    }
    if fl("UseBlitzCharges") {
        let v = or_override(
            override_(&db, &["BlitzCharges"]),
            o.num("BlitzChargesMax"),
            "Modules/CalcPerform.lua:1092",
        );
        o.set_num("BlitzCharges", v);
    }
    if who == Who::Player {
        let v = or_override(
            override_(&db, &["InspirationCharges"]),
            o.num("InspirationChargesMax"),
            "Modules/CalcPerform.lua:1095",
        );
        o.set_num("InspirationCharges", v);
    }
    if fl("UseGhostShrouds") {
        let v = or_override(override_(&db, &["GhostShrouds"]), 3.0, "Modules/CalcPerform.lua:1098");
        o.set_num("GhostShrouds", v);
    }
    let v = min2(
        or_override(
            override_(&db, &["BloodCharges"]),
            o.num("BloodChargesMax"),
            "Modules/CalcPerform.lua:1100",
        ),
        o.num("BloodChargesMax"),
    );
    o.set_num("BloodCharges", v);
    let v = min2(
        or_override(override_(&db, &["SpiritCharges"]), 0.0, "Modules/CalcPerform.lua:1101"),
        o.num("SpiritChargesMax"),
    );
    o.set_num("SpiritCharges", v);
    let v = min2(
        or_override(override_(&db, &["SpiritInfusion"]), 0.0, "Modules/CalcPerform.lua:1102"),
        o.num("SpiritInfusionsMax"),
    );
    o.set_num("SpiritInfusions", v);
    let v = min2(
        or_override(
            override_(&db, &["CrabBarriers"]),
            o.num("CrabBarriersMax"),
            "Modules/CalcPerform.lua:1105",
        ),
        o.num("CrabBarriersMax"),
    );
    o.set_num("CrabBarriers", v);
    if fl("HaveMaximumPowerCharges") {
        let v = o.num("PowerChargesMax");
        o.set_num("PowerCharges", v);
    }
    if fl("HaveMaximumFrenzyCharges") {
        let v = o.num("FrenzyChargesMax");
        o.set_num("FrenzyCharges", v);
    }
    if fl("HaveMaximumEnduranceCharges") {
        let v = o.num("EnduranceChargesMax");
        o.set_num("EnduranceCharges", v);
    }
    let v = o.num("PowerCharges") + o.num("FrenzyCharges") + o.num("EnduranceCharges");
    o.set_num("TotalCharges", v);
    let v = o.num("RemovableEnduranceCharges") + o.num("RemovableFrenzyCharges") + o.num("RemovablePowerCharges");
    o.set_num("RemovableTotalCharges", v);

    // :1114-1137 the charge multipliers, in upstream order.
    for (key, from) in [
        ("PowerCharge", "PowerCharges"),
        ("PowerChargeMax", "PowerChargesMax"),
        ("RemovablePowerCharge", "RemovablePowerCharges"),
        ("FrenzyCharge", "FrenzyCharges"),
        ("RemovableFrenzyCharge", "RemovableFrenzyCharges"),
        ("EnduranceCharge", "EnduranceCharges"),
        ("RemovableEnduranceCharge", "RemovableEnduranceCharges"),
        ("TotalCharges", "TotalCharges"),
        ("RemovableTotalCharges", "RemovableTotalCharges"),
        ("SiphoningCharge", "SiphoningCharges"),
        ("ChallengerCharge", "ChallengerCharges"),
        ("BlitzCharge", "BlitzCharges"),
        ("InspirationCharge", "InspirationCharges"),
        ("GhostShroud", "GhostShrouds"),
        ("CrabBarrier", "CrabBarriers"),
        ("BrutalCharge", "BrutalCharges"),
        ("BrineCharge", "BrineCharges"),
        ("AbsorptionCharge", "AbsorptionCharges"),
        ("AfflictionCharge", "AfflictionCharges"),
        ("BloodCharge", "BloodCharges"),
        ("SpiritCharge", "SpiritCharges"),
        ("SpiritInfusion", "SpiritInfusions"),
    ] {
        set_mult(&db, key, o.num(from));
    }
}

/// `modDB.mods[name][1].source` -- upstream indexes both straight away, so a gap is an error.
fn first_mod_source(db: &StoreRef, name: &str, line: u32) -> Option<pob_mods::Str> {
    let d = db.borrow();
    let StoreKind::Db(mdb) = &d.kind else {
        panic!("Modules/CalcPerform.lua:{line}: modDB.mods on a ModList")
    };
    let m = mdb
        .mods
        .get(name)
        .and_then(|b| b.first())
        .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:{line}: attempt to index a nil value (modDB.mods[\"{name}\"][1])"));
    m.source.clone()
}

// --- CalcDefence.lua:481-508 `calcs.defenceForConditionals` -------------------------------------------------------------

fn defence_for_conditionals(env: &Env, who: Who, st: &mut ActorState) {
    let db = db_of(env, who);
    let actor = actor_of(env, who).borrow();
    // `pairs` over the array `{"Helmet","Gloves","Boots","Body Armour","Weapon 2","Weapon 3"}`: in order.
    for slot in ["Helmet", "Gloves", "Boots", "Body Armour", "Weapon 2", "Weapon 3"] {
        let Some(armour_data) = actor.item_list.get(slot).and_then(|i| i.armour_data.as_ref()) else {
            continue;
        };
        for stat in ["Ward", "EnergyShield", "Armour", "Evasion"] {
            // `not modDB:Flag(nil, "GainNo<stat>From<slot>") and armourData.<stat> or 0`
            let base_v = if flag(&db, &[&format!("GainNo{stat}From{slot}")]) {
                0.0
            } else {
                match armour_data.get(stat) {
                    Value::Nil | Value::Bool(false) => 0.0,
                    v => num_val(&v, "Modules/CalcDefence.lua:490"),
                }
            };
            if base_v > 0.0 {
                st.output.set_num(&format!("{stat}On{slot}"), base_v);
            }
        }
    }
}

// --- CalcTools.lua:258-273 `calcLib.getConvertedModTags` (minionMods = nil) --------------------------------------------

fn get_converted_mod_tags(m: &Mod, multiplier: f64) -> Vec<Tag> {
    m.tags
        .iter()
        .map(|t| {
            if t.get("limitTotal").truthy() {
                // LimitTotal can apply to 'per stat' or 'multiplier', so copy the whole and update the limit.
                let mut tbl = (*t.table()).clone();
                let limit = num_val(tbl.get("limit"), "Modules/CalcTools.lua:266");
                tbl.hash.insert(Rc::from("limit"), n(limit * multiplier));
                Tag::new(tbl)
            } else {
                t.deep_copy()
            }
        })
        .collect()
}

// --- CalcPerform.lua:1286-3792 `calcs.perform` --------------------------------------------------------------------------

/// :3395-3410 Foulborn Choir of the Storm: `calcs.resistances` on a temporary actor whose modDB is a child of the player's (the conversion
/// mods it adds stay off the player's modDB) and whose `output` is the player's output table, then life/mana and the reservations again.
///
/// The temporary actor (`modDB`, `output`, `activeSkillList`, `enemy`, `player = itself`) is a mod-core [`Actor`]; its `output` is a copy of
/// the player's at entry -- the writes `calcs.resistances` makes go to the player's [`Output`] (the shared Lua table), but EvalMod reads made
/// THROUGH the temporary actor inside `calcs.resistances` (a `PerStat`/`StatThreshold` tag on a resistance mod) would see the entry values.
/// No golden and no synthetic case reaches this branch (`ManaIncreasedByOvercappedLightningRes` is a Foulborn amulet).
fn choir_of_the_storm(mod_db: &StoreRef, player_actor: &ActorRef, player: &mut ActorState) {
    let temp_db = ModStore::new_db(Some(mod_db.clone()));
    let (output, active_skill_list, enemy) = {
        let p = player_actor.borrow();
        (p.output.clone(), p.active_skill_list.clone(), p.actors.get("enemy").cloned())
    };
    let temp = Rc::new(std::cell::RefCell::new(Actor {
        mod_db: Some(temp_db.clone()),
        output,
        active_skill_list,
        ..Actor::default()
    }));
    {
        let mut t = temp.borrow_mut();
        if let Some(e) = enemy {
            t.actors.insert(Rc::from("enemy"), e);
        }
        t.actors.insert(Rc::from("player"), temp.clone());
    }
    temp_db.borrow_mut().set_actor(&temp);
    crate::defence::resistances(&temp_db, &mut player.output);
    // `tempResistActor.player = tempResistActor` is a cycle: break it (Lua's GC would collect the table).
    temp.borrow_mut().actors.clear();
    // :3408-3409 the reservations again, with the mana from the overcapped lightning resistance.
    do_actor_life_mana(mod_db, &mut player.output);
    do_actor_life_mana_reservation(mod_db, player, true);
}

/// `calcs.perform(env)` up to `calcs.defence(env, env.player)` and `buildDefenceEstimations` -- passed in; `None` records
/// the call site as unimplemented (pob-ts `perform(env, defence?, estimations?)`).
pub fn perform(env: &mut Env, defence: Option<PhaseFn>, estimations: Option<PhaseFn>) -> PerformState {
    perform_with(env, defence, estimations, None)
}

/// [`perform`] with a [`CheckpointHook`] called at the R6 checkpoints (the hook only observes; the computation is the same).
pub fn perform_with(
    env: &mut Env,
    defence: Option<PhaseFn>,
    estimations: Option<PhaseFn>,
    hook: Option<CheckpointHook<'_>>,
) -> PerformState {
    perform_skip_ehp(env, defence, estimations, hook, false)
}

/// `calcs.perform(env, skipEHP)` (:1286): [`perform_with`] with upstream's second argument -- `true` skips
/// `buildDefenceEstimations` (:3793-3795), what every nested run (`calcs.buildActiveSkill`, `calcFullDPS`) passes (R7.2).
#[allow(clippy::too_many_lines)]
pub fn perform_skip_ehp(
    env: &mut Env,
    defence: Option<PhaseFn>,
    estimations: Option<PhaseFn>,
    mut hook: Option<CheckpointHook<'_>>,
    skip_ehp: bool,
) -> PerformState {
    let mod_db = env.mod_db.clone();
    let enemy_db = env.enemy_db.clone();
    let skip = |env: &mut Env, what: &str| env.unimplemented.push(format!("CalcPerform.lua:{what}"));
    let skills_on = env.skills.is_some();

    // --- :1290-1292 merge keystone modifiers ----------------------------------------------------------------------------
    env.keystones_added = IndexMap::new();
    merge_keystones(env, &mod_db);

    // :1294-1303 every skill gets a fresh child `skillModList` of its `baseSkillModList`; minions are R7.
    if skills_on {
        skills_loop::child_skill_lists(env);
    } else {
        skip(env, "1294-1303 minion skills (createMinionSkills; needs the skill phase, P5)");
    }

    // :1305-1306
    let player_actor = env.player.borrow().actor.clone();
    let enemy_actor = env.enemy.borrow().actor.clone();
    let mut player = ActorState::new(&player_actor);
    let enemy = ActorState::new(&enemy_actor);

    // :1310-1311 `env.partyMembers = env.build.partyTab.actor` -- PartyTab is P5: an empty stand-in actor.
    let party_db = ModStore::new_db(None);
    let party_actor = Rc::new(std::cell::RefCell::new(Actor {
        mod_db: Some(party_db.clone()),
        output: Some(IndexMap::new()),
        ..Actor::default()
    }));
    party_db.borrow_mut().set_actor(&party_actor);
    player_actor
        .borrow_mut()
        .actors
        .insert(Rc::from("partyMembers"), party_actor.clone());
    let party = PartyStub {
        mod_db: party_db,
        actor: party_actor,
    };
    // :1312 `partyTabEnableExportBuffs` is only read by the buff-export tail (skipped).
    skip(env, "1312 partyTab.enableExportBuffs / buff export (P5)");

    // :1315-1324 aegis / minion
    let aegis_item = if env.aegis_mod_list.is_some() {
        env.player.borrow().item_list.get("Weapon 2").cloned()
    } else {
        None
    };
    // :1316 `env.minion = env.player.mainSkill.minion` -- the minion actor (`initMinionModDB`, its modDB, every `if env.minion` branch
    // below) is R7: perform runs as if `env.minion` were nil and records it (the stub has no minion).
    if player_main_skill(env, 1316).has_minion {
        skip(
            env,
            "1316-1320 env.minion = mainSkill.minion: initMinionModDB and every env.minion branch (R7)",
        );
    }
    if env.aegis_mod_list.is_some() {
        let mut p = env.player.borrow_mut();
        p.item_list.shift_remove("Weapon 2");
        if let Some(il) = p.actor.borrow_mut().item_list.as_mut() {
            il.shift_remove("Weapon 2");
        }
    }

    // :1325-1329 Alchemist's Genius
    if flag(&mod_db, &["AlchemistsGenius"]) {
        let effect_mod = 1.0 + inc(&mod_db, &["BuffEffectOnSelf"]) / 100.0;
        nm(
            &mod_db,
            "FlaskEffect",
            ModType::Inc,
            (10.0 * effect_mod).floor(),
            "Alchemist's Genius",
            0,
            0,
            vec![],
        );
        nm(
            &mod_db,
            "FlaskChargesGained",
            ModType::Inc,
            (20.0 * effect_mod).floor(),
            "Alchemist's Genius",
            0,
            0,
            vec![],
        );
    }

    // :1331-1348 banners and warcry power
    if flag(&mod_db, &["Condition:BannerPlanted"]) {
        let max = base(&mod_db, &["MaximumValour"]);
        let stacks = base(&mod_db, &["Multiplier:ValourStacks"]);
        nm(
            &mod_db,
            "Multiplier:BannerValour",
            ModType::Base,
            min2(stacks, max),
            "Base",
            0,
            0,
            vec![],
        );
    }
    if flag(&mod_db, &["CryWolfMinimumPower"]) && base(&mod_db, &["WarcryPower"]) < 10.0 {
        nm(
            &mod_db,
            "WarcryPower",
            ModType::Override,
            10.0,
            "Minimum Warcry Power from CryWolf",
            0,
            0,
            vec![],
        );
    }
    if flag(&mod_db, &["WarcryInfinitePower"]) {
        nm(
            &mod_db,
            "WarcryPower",
            ModType::Override,
            999_999.0,
            "Warcries have infinite power",
            0,
            0,
            vec![],
        );
    }
    // :1347 `Override or Sum or 0` -- Sum is always a number, so `or 0` is dead.
    let wp = or_override(
        override_(&mod_db, &["WarcryPower"]),
        base(&mod_db, &["WarcryPower"]),
        "Modules/CalcPerform.lua:1347",
    );
    player.output.set_num("WarcryPower", wp);
    set_mult(&mod_db, "WarcryPower", wp);

    // :1350-1354 (`applyEnemyModifiers(env.minion, true)` -- env.minion is nil, see :1316)
    apply_enemy_modifiers(&mod_db, &enemy_db, &mut player, true);
    let mut enemy = enemy;
    apply_enemy_modifiers(&enemy_db, &mod_db, &mut enemy, true);

    // :1334-1489 the activeSkillList loop -- brands, totems, hexes, ailments, minion counts, warcries, triggers.
    let loop_out = if skills_on {
        skills_loop::skills_loop(env, &mut player.output)
    } else {
        skip(
            env,
            "1358-1496 activeSkillList loop (brands/totems/minion counts/warcries/triggers; P5)",
        );
        skills_loop::LoopOut::default()
    };
    let summoned_minions = loop_out.summoned_minions;

    // :1500-1503 Companionship only works while exactly one Minion is summoned.
    let summoned_override = match env.config.input.get("multiplierSummonedMinion") {
        // `x ~= 0 and x or nil` (`false` is not 0, but `false and ...` is false, so `or nil`)
        Some(pob_build::ConfigValue::Num(v)) if *v == 0.0 => None,
        Some(pob_build::ConfigValue::Bool(false)) => None,
        other => other.cloned(),
    };
    let only_minion = match &summoned_override {
        None => summoned_minions == 1.0, // `(nil or summonedMinions) == 1`
        Some(pob_build::ConfigValue::Num(v)) => *v == 1.0,
        Some(_) => false, // a string/true is never `== 1`
    };
    set_cond(&mod_db, "OnlyMinion", only_minion);

    // :1508-1512 Bisco's
    let lqne = inc(&mod_db, &["LootQuantityNormalEnemies"]);
    let v = if lqne > 0.0 { lqne + inc(&mod_db, &["LootQuantity"]) } else { 0.0 };
    player.output.set_num("LootQuantityNormalEnemies", v);
    let lrme = inc(&mod_db, &["LootRarityMagicEnemies"]);
    let v = if lrme > 0.0 { lrme + inc(&mod_db, &["LootRarity"]) } else { 0.0 };
    player.output.set_num("LootRarityMagicEnemies", v);

    // :1514-1522 the breakdown module: `breakdown` is built (MAIN, CalcSetup.lua:500); the flask/tincture breakdown below and :2157
    // read it.
    let breakdown = env.build_breakdown;
    let mut flask_state = None;
    if skills_on {
        // :1524-1942 ConvertArmourESToLife, damage -> aura effect, minion life -> player, Mageblood, flasks, tinctures.
        flasks::conversions(env);
        flasks::mageblood(env);
        let fx = flasks::flask_ctx(env, &mut player.output, breakdown);
        let tx = flasks::tincture_ctx(env, &mut player.output, breakdown);
        if env.mode_combat {
            // :1937 `mergeFlasks(env.flasks, false, true)`
            flasks::merge_flasks(env, &fx, false, true);
            // :1940 merge keystones again, to catch any that were added by flasks.
            merge_keystones(env, &mod_db);
        }
        flask_state = Some((fx, tx));
    } else {
        skip(env, "1524-1935 ConvertArmourESToLife / Mageblood / flask + tincture merging (P5)");
        if env.mode_combat {
            // :1937 `mergeFlasks(env.flasks, false, true)`: with an EMPTY flask set it writes nothing (CalcPerform.lua:
            // 1773-1848); a non-empty set is real work we skip. The tincture skip (`mergeTinctures`, :2094-2096, also a no-op
            // when empty) is recorded here, next to it, as pob-ts does.
            if !env.flasks.is_empty() {
                let mut names: Vec<String> = env
                    .flasks
                    .iter()
                    .map(|i| i.base_name.clone().unwrap_or_else(|| i.name.clone()))
                    .collect();
                names.sort();
                let what = format!("1937 mergeFlasks with {} active flasks: {}", env.flasks.len(), names.join(", "));
                skip(env, &what);
            }
            if !env.tinctures.is_empty() {
                let mut names: Vec<String> = env
                    .tinctures
                    .iter()
                    .map(|i| i.base_name.clone().unwrap_or_else(|| i.name.clone()))
                    .collect();
                names.sort();
                let what = format!("2094 mergeTinctures with {} tinctures: {}", env.tinctures.len(), names.join(", "));
                skip(env, &what);
            }
            // :1940 merge keystones again, to catch any that were added by flasks.
            merge_keystones(env, &mod_db);
        }
    }

    // --- :1944-1945 attributes and life/mana pools ----------------------------------------------------------------------
    do_actor_attribs_conditions(env, Who::Player, &mut player);
    // CP1: `doActorAttribsConditions` ends with the first `doActorLifeMana(env.player)` (:546).
    if let Some(h) = hook.as_mut() {
        h(Checkpoint::Cp1, env, &player, &enemy);
    }
    do_actor_life_mana(&mod_db, &mut player.output);

    // --- :1967-2092 reservations ----------------------------------------------------------------------------------------
    player.reserved_life_base = Some(0.0);
    player.reserved_life_percent = Some(base(&mod_db, &["ExtraLifeReserved"]));
    player.reserved_mana_base = Some(0.0);
    player.reserved_mana_percent = Some(0.0);
    player.uncancellable_life_reservation = Some(base(&mod_db, &["ExtraLifeReserved"]));
    player.uncancellable_mana_reservation = Some(base(&mod_db, &["ExtraManaReserved"]));
    if skills_on {
        // :1978-2090 the per-skill loop ([`reservation`]).
        let mut acc = reservation::Acc {
            life_base: player.reserved_life_base.unwrap_or(0.0),
            life_percent: player.reserved_life_percent.unwrap_or(0.0),
            mana_base: player.reserved_mana_base.unwrap_or(0.0),
            mana_percent: player.reserved_mana_percent.unwrap_or(0.0),
            uncancellable_life: player.uncancellable_life_reservation.unwrap_or(0.0),
            uncancellable_mana: player.uncancellable_mana_reservation.unwrap_or(0.0),
        };
        reservation::reservation_loop(env, &player.output, &mut acc);
        player.reserved_life_base = Some(acc.life_base);
        player.reserved_life_percent = Some(acc.life_percent);
        player.reserved_mana_base = Some(acc.mana_base);
        player.reserved_mana_percent = Some(acc.mana_percent);
        player.uncancellable_life_reservation = Some(acc.uncancellable_life);
        player.uncancellable_mana_reservation = Some(acc.uncancellable_mana);
    } else {
        skip(env, "1978-2089 per-skill reservation loop (activeSkillList is empty; P5)");
    }
    let add_aura = !flag(&mod_db, &["ManaIncreasedByOvercappedLightningRes"]);
    do_actor_life_mana_reservation(&mod_db, &mut player, add_aura);
    if let Some(h) = hook.as_mut() {
        h(Checkpoint::Cp2, env, &player, &enemy);
    }
    // :2094-2096 `mergeTinctures(env.tinctures)` (skills off: recorded with the flask skip above).
    if env.mode_combat
        && let Some((_, tx)) = flask_state.as_mut()
    {
        flasks::merge_tinctures(env, tx, &player.output);
    }

    // --- :2099-2162 attribute requirements ------------------------------------------------------------------------------
    {
        let req_mult = ok(calc_lib::mod_(&mod_db.borrow(), None, &["GlobalAttributeRequirements"]));
        let omni_requirements = if flag(&mod_db, &["OmniscienceRequirements"]) {
            Some(ok(calc_lib::mod_(&mod_db.borrow(), None, &["OmniAttributeRequirements"])))
        } else {
            None
        };
        let ignore_attr_req = flag(&mod_db, &["IgnoreAttributeRequirements"]);
        let attr_table: &[&str] = if omni_requirements.is_some() {
            &["Omni", "Str", "Dex", "Int"]
        } else {
            &["Str", "Dex", "Int"]
        };
        for attr in attr_table {
            let breakdown_attr = if omni_requirements.is_some() { "Omni" } else { attr };
            let mut val = 0.0;
            for req_source in &env.requirements_table {
                let raw = match *attr {
                    "Str" => req_source.str_,
                    "Dex" => req_source.dex,
                    "Int" => req_source.int,
                    _ => None,
                };
                if let Some(raw) = raw.filter(|r| *r > 0.0) {
                    let mut req = (raw * req_mult).floor();
                    if let Some(omni) = omni_requirements {
                        let omni_req_mult = 1.0 / (omni - 1.0);
                        let attribute_req = (raw * req_mult).floor();
                        req = (attribute_req * omni_req_mult).floor();
                    }
                    if req > val {
                        val = req;
                    }
                }
            }
            if ignore_attr_req {
                val = 0.0;
            }
            player.output.set_num(&format!("Req{attr}String"), 0.0);
            let current = player.output.opt_num(&format!("Req{breakdown_attr}")).unwrap_or(0.0);
            if val > current {
                player.output.set_num(&format!("Req{breakdown_attr}String"), val);
                player.output.set_num(&format!("Req{breakdown_attr}"), val);
                // :2156 `output["Req..Item"] = out.source` -- a table value: never in `getStats` (not kept).
                if breakdown {
                    // :2158 the coloured string when the requirement is not met.
                    let have = player.output.opt_num(breakdown_attr).unwrap_or(0.0);
                    if val > have {
                        let negative = globals().color_codes.get("NEGATIVE").expect("colorCodes.NEGATIVE");
                        player
                            .output
                            .set_str(&format!("Req{breakdown_attr}String"), &format!("{negative}{}", tostring_num(val)));
                    } else {
                        player.output.set_num(&format!("Req{breakdown_attr}String"), val);
                    }
                }
            }
        }
    }

    // --- :2164-2178 heralds and auras affecting self (the stub path's `activeSkillList` is empty: nothing counts) -------------
    if env.mode_buffs
        && let Some(st) = &env.skills
    {
        let mut herald_list: Vec<&str> = Vec::new();
        let mut aura_list: Vec<&str> = Vec::new();
        let has = |sk: &ActiveSkill, t: &str| sk.skill_types.contains(&skill_type_u32(t));
        for sk in &st.active_skill_list {
            let name = sk
                .skill_cfg
                .as_ref()
                .map(|c| c.skill_name.as_str())
                .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:2169: attempt to index field 'skillCfg' (a nil value)"));
            if has(sk, "Herald") && !herald_list.contains(&name) {
                herald_list.push(name);
                bump_mult(&mod_db, "Herald", 1.0);
                set_cond(&mod_db, "AffectedByHerald", true);
            } else if has(sk, "Aura")
                && !has(sk, "AuraAffectsEnemies")
                && !sk.skill_data.get("auraCannotAffectSelf").is_some_and(Value::truthy)
                && !aura_list.contains(&name)
            {
                aura_list.push(name);
                bump_mult(&mod_db, "AuraAffectingSelf", 1.0);
            }
        }
    }

    // --- :2180-2188 ManaAppliesToShockEffect (Lightning Mastery) --------------------------------------------------------
    if flag(&mod_db, &["ManaAppliesToShockEffect"]) {
        let multiplier = or_num(max_(&mod_db, None, &["ImprovedManaAppliesToShockEffect"]), 100.0) / 100.0;
        let entries = ok(mod_db.borrow().tabulate(Some(ModType::Inc), None, &["Mana"]));
        for e in entries {
            let m = &e.mod_;
            let modifiers = get_converted_mod_tags(m, multiplier);
            let v = (num_val(&m.value, "Modules/CalcPerform.lua:2186") * multiplier).floor();
            let created = create_mod(
                "EnemyShockEffect",
                Some(ModType::Inc),
                n(v),
                m.source.as_deref(),
                m.flags,
                m.keyword_flags,
                modifiers,
            );
            mod_db.borrow_mut().new_mod(created);
        }
    }

    // --- :2191 charges --------------------------------------------------------------------------------------------------
    do_actor_charges(env, Who::Player, &mut player, &party);

    // --- :2193-3376 buffs / debuffs / curses / guards -------------------------------------------------------------------
    match flask_state.as_ref() {
        Some((fx, _)) => {
            // :2193-2995 combine buffs/debuffs/curses/guards ([`buffs`]).
            let mut combined = buffs::combine(env);
            // :3118-3122 the recovery half of the flasks (`mergeFlasks(env.flasks, true, nonUniqueFlasksApplyToMinion)`).
            if env.mode_combat {
                flasks::merge_flasks(env, fx, true, fx.non_unique_flasks_apply_to_minion);
            }
            // :3124-3141 alternate charges; :3143-3316 extra curses, curse limit, slots ([`curses`]).
            buffs::alternate_charges(env);
            curses::extra_curses(env, &mut combined.curses);
            let slots = curses::curse_slots(env, &mut player.output, &combined.curses);
            // :3317-3376 guards, then apply everything ([`guards`]).
            guards::guard_slots(env, &combined.guards, &mut combined.buffs, &mut player.output);
            guards::apply(env, &combined.buffs, &combined.debuffs, &slots);
        }
        None => {
            skip(env, "2193-3341 buffs/debuffs/curses/guards/links/auras (needs the skill phase, P5)");
            // :3341 -- the one line of the guard block that does not depend on a guard being there.
            let guard = cond(&mod_db, "AffectedByGuardSkill");
            player.output.set_bool("GuardSkillActive", guard.unwrap_or(false));
            // :3344-3359 `buffs`/`minionBuffs`/`debuffs` are all empty on this path. :3361 `#curseSlots` runs regardless.
            set_mult(&mod_db, "CurseOnEnemy", 0.0);
        }
    }
    // :3378-3381 guaranteed chills (after the buffs, so chilling areas benefit from active buff modifiers).
    if let Some(st) = &env.skills {
        let chill = rules()
            .non_damaging_ailment
            .get("Chill")
            .and_then(|a| a.default)
            .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:3379: data.nonDamagingAilment.Chill.default is nil"));
        for &i in &loop_out.guaranteed_chill_skills {
            let sk = &st.active_skill_list[i];
            let sml = skill_list_of(env, i);
            skill_guard(&sk.malformed_mods, &["EnemyChillEffect"]);
            let effect = chill * ok(calc_lib::mod_(&sml.borrow(), Some(&skill_cfg_of(st, sk)), &["EnemyChillEffect"]));
            let name = st.skills.effects[sk.active_effect.borrow().granted_effect].name.clone();
            nm(&mod_db, "ChillOverride", ModType::Base, effect, &name, 0, 0, vec![]);
        }
    }

    // --- :3387-3392 impale stacks on the enemy --------------------------------------------------------------------------
    let max_impale = base(&mod_db, &["ImpaleStacksMax"]) * (1.0 + base(&mod_db, &["ImpaleAdditionalDurationChance"]) / 100.0);
    let combat = || tag(&[("type", s("Condition")), ("var", s("Combat"))]);
    if !ok(enemy_db.borrow().has_mod(ModType::Base, None, &["Multiplier:ImpaleStacks"])) {
        nm(
            &enemy_db,
            "Multiplier:ImpaleStacks",
            ModType::Base,
            max_impale,
            "Config",
            0,
            0,
            vec![combat()],
        );
    } else if base(&enemy_db, &["Multiplier:ImpaleStacks"]) > max_impale {
        let m = create_mod(
            "Multiplier:ImpaleStacks",
            Some(ModType::Base),
            n(max_impale),
            Some("Config"),
            0,
            0,
            vec![combat()],
        );
        enemy_db.borrow_mut().replace_mod(m);
    }

    // --- :3395-3416 Foulborn Choir of the Storm, Hallowing Flame --------------------------------------------------------
    if flag(&mod_db, &["ManaIncreasedByOvercappedLightningRes"]) {
        if skills_on {
            choir_of_the_storm(&mod_db, &player_actor, &mut player);
        } else {
            // Needs `calcs.resistances` on a temporary actor (CalcDefence.lua:512-650, R4.3).
            skip(
                env,
                "3394-3410 ManaIncreasedByOvercappedLightningRes (needs calcs.resistances, P4.4b)",
            );
        }
    }
    // :3411-3415 Hallowing Flame (skills off: recorded, as pob-ts does).
    let player_ms = player_main_skill(env, 3411);
    if flag_cfg(&mod_db, Some(&player_ms.cfg), &["Condition:CanInflictHallowingFlame"]) {
        if skills_on {
            let magnitude = or_override(
                override_(&mod_db, &["HallowingFlameMagnitude"]),
                inc(&mod_db, &["HallowingFlameMagnitude"]),
                "Modules/CalcPerform.lua:3412",
            );
            let val = (25.0 * (1.0 + magnitude / 100.0)).floor();
            let m = create_mod(
                "PhysicalDamageGainAsFire",
                Some(ModType::Base),
                n(val),
                Some("Hallowing Flame"),
                0,
                0,
                vec![
                    tag(&[
                        ("type", s("GlobalEffect")),
                        ("effectType", s("Global")),
                        ("unscalable", Value::Bool(true)),
                    ]),
                    tag(&[("type", s("ActorCondition")), ("actor", s("enemy")), ("var", s("HallowingFlame"))]),
                    tag(&[("type", s("Multiplier")), ("var", s("HallowingFlame")), ("actor", s("enemy"))]),
                ],
            );
            let mut t = Table::default();
            t.hash.insert(Rc::from("mod"), Value::Mod(Rc::new(m)));
            new_mod(&mod_db, "ExtraAura", ModType::List, Value::table(t), None, 0, 0, vec![]);
        } else {
            skip(env, "3412-3415 Hallowing Flame ExtraAura");
        }
    }

    // --- :3418-3460 extra auras -----------------------------------------------------------------------------------------
    // `buffExports["Aura"]["extraAura"]` is only read by the buff-export tail (skipped). `env.minion` is nil.
    for value in list(&mod_db, &["ExtraAura"]) {
        let Value::Mod(m) = value.get("mod") else {
            panic!("Modules/CalcPerform.lua:3419: ExtraAura value without a mod")
        };
        let mod_list = [m];
        if !value.get("onlyAllies").truthy() && !(value.get("fromAllies").truthy() && flag(&mod_db, &["AlliesAurasCannotAffectSelf"])) {
            let i = inc(&mod_db, &["BuffEffectOnSelf", "AuraEffectOnSelf"]);
            let m = more(&mod_db, None, &["BuffEffectOnSelf", "AuraEffectOnSelf"]);
            ok(mod_db.borrow_mut().scale_add_list(&mod_list, (1.0 + i / 100.0) * m, false));
            if !value.get("notBuff").truthy() {
                bump_mult(&mod_db, "BuffOnSelf", 1.0);
            }
        }
        // :3433-3459 `fromAllies or not SelfAurasCannotAffectAllies` -> the minion copy (env.minion is nil, :1316) and the totem
        // copy (`env.player.mainSkill.skillFlags.totem`, never set on the stub).
        if value.get("fromAllies").truthy() || !flag(&mod_db, &["SelfAurasCannotAffectAllies"]) {
            let src = &mod_list[0];
            let name: &str = &src.name;
            let blacklisted = name == "Speed" || name == "CritMultiplier" || name == "CritChance";
            if player_ms.flag("totem") && !blacklisted {
                // `copyTable(value.mod)`; `name:gsub("Condition:", "Condition:Totem")` (a pattern: `:` is literal)
                let mut totem_mod = src.deep_copy();
                let renamed = name.replace("Condition:", "Condition:Totem");
                totem_mod.name = if renamed == name {
                    Rc::from(format!("Totem{name}").as_str())
                } else {
                    Rc::from(renamed.as_str())
                };
                mod_db.borrow_mut().add_mod(Rc::new(totem_mod));
            }
        }
    }
    // :3462-3467 `AffectedByAuraMod`: upstream iterates `affectedByAura`, an undeclared GLOBAL (nil), so `pairs(nil)`
    // throws the moment the build has such a mod (D20, upstream bug).
    if !list(&mod_db, &["AffectedByAuraMod"]).is_empty() {
        panic!("Modules/CalcPerform.lua:3464: bad argument #1 to 'pairs' (table expected, got nil)");
    }

    // --- :3470 merge keystones again, to catch any that were added by buffs ---------------------------------------------
    merge_keystones(env, &mod_db);

    // --- :3473-3481 Dancing Dervish -------------------------------------------------------------------------------------
    if flag(&mod_db, &["DisableWeapons"]) {
        let rec = rules()
            .unarmed_weapon_data_for(env.class_id as f64)
            .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:3474: data.unarmedWeaponData[{}] is nil", env.class_id));
        let mut d = DataTable::default();
        d.set("type", Value::str(&rec.ty));
        d.set("AttackRate", n(rec.AttackRate));
        d.set("CritChance", n(rec.CritChance));
        d.set("PhysicalMin", n(rec.PhysicalMin));
        d.set("PhysicalMax", n(rec.PhysicalMax));
        {
            let mut p = env.player.borrow_mut();
            p.weapon_data1 = d;
            sync_weapon_data(&p);
        }
        set_cond(&mod_db, "Unarmed", true);
        // :3476 `env.player.Gloves` (not `itemList.Gloves`) is never assigned upstream -- always nil (D20).
        set_cond(&mod_db, "Unencumbered", true);
    } else if let Some(wml) = env.weapon_mod_list1.clone() {
        mod_db.borrow_mut().add_list(&list_mods(&wml));
    }

    // --- :3484 prerequisites for conditionals ---------------------------------------------------------------------------
    if let Some(h) = hook.as_mut() {
        h(Checkpoint::Cp3, env, &player, &enemy);
    }
    defence_for_conditionals(env, Who::Player, &mut player);

    // --- :3490-3491 misc buffs / modifiers ------------------------------------------------------------------------------
    do_actor_charges(env, Who::Player, &mut player, &party);
    do_actor_misc(env, Who::Player, &mut player, &party);

    // --- :3497-3622 non-damaging ailments, chill/shock multipliers ([`ailments`]) ---------------------------------------
    if skills_on {
        ailments::ailments(env, &mut player.output, loop_out.has_guaranteed_bonechill);
    } else {
        skip(
            env,
            "3497-3620 non-damaging ailments, exposures, consecrated ground, enemy ailment config (P5)",
        );
    }
    do_actor_charges(env, Who::Enemy, &mut enemy, &party);
    do_actor_misc(env, Who::Enemy, &mut enemy, &party);
    // --- :3627-3665 exposures / Elemental Equilibrium / consecrated ground; :3667-3790 ally life pools ([`ally`]) --------
    if skills_on {
        ailments::exposures_consecrated(env);
        ally::ally_life(env, &mut player, summoned_override.as_ref());
    } else {
        skip(env, "3627-3665 exposures / Elemental Equilibrium (P5)");
        skip(env, "3667-3789 ally life pools (Vaal Rejuvenation Totem, spectres; P5)");
    }

    let mut state = PerformState {
        player,
        enemy,
        party_members: party,
        aegis_item,
    };

    // --- :3792 defence, then the offence tail ---------------------------------------------------------------------------
    if let Some(h) = hook.as_mut() {
        h(Checkpoint::Cp4, env, &state.player, &state.enemy);
    }
    match defence {
        Some(def) => {
            def(env, &mut state, Who::Player);
            // :3793-3795 `if not skipEHP then`
            if !skip_ehp {
                match estimations {
                    Some(est) => est(env, &mut state, Who::Player),
                    None => skip(env, "3793-3795 buildDefenceEstimations (P4.4c)"),
                }
            }
        }
        None => {
            skip(env, "3792 calcs.defence (P4.4b)");
            if !skip_ehp {
                skip(env, "3793-3795 buildDefenceEstimations (P4.4c)");
            }
        }
    }
    // --- :3797-3801 triggers / mirages / offence ([`crate::offence`]) -------------------------------------------------------
    if skills_on {
        match crate::offence::triggers_gate(env) {
            crate::offence::Gate::Unported(what) => env.unimplemented.push(what),
            crate::offence::Gate::Pass => match crate::offence::mirages_gate(env) {
                crate::offence::Gate::Unported(what) => env.unimplemented.push(what),
                crate::offence::Gate::Pass => crate::offence::offence(env, &mut state, hook),
            },
        }
    } else {
        skip(env, "3797-3801 triggers / mirages / offence (P5)");
    }
    skip(env, "3803-3990 minion output, buff export, party export (P5)");
    state
}

// --- Calcs.lua:392-449 -- the MAIN tail of `calcs.buildOutput` ----------------------------------------------------------

/// The part of `calcs.buildOutput` that runs after `perform` and writes into `player.output`.
pub fn finish_output(env: &mut Env, state: &mut PerformState) {
    env.unimplemented
        .push("Calcs.lua:392-398 calcFullDPS -> SkillDPS/FullDPS/FullDotDPS (P5)".into());
    env.unimplemented
        .push("Calcs.lua:400-427 per-skill cost warnings + EnergyShieldProtectsMana (P5)".into());
    finish_output_spec(env, state);
    env.unimplemented
        .push("Calcs.lua:450-464 env.skillsUsed / display stats (P5)".into());
}

/// Calcs.lua:440-449 -- `ExtraPoints` and the tree's `Spec:*Inc` (the MAIN tail of `calcs.buildOutput` after the per-skill loop).
pub(crate) fn finish_output_spec(env: &Env, state: &mut PerformState) {
    let db = env.mod_db.clone();
    let out = &mut state.player.output;
    // :440
    out.set_num("ExtraPoints", base(&db, &["ExtraPoints"]));
    // :442-449
    let spec_cfg = crate::setup::cfg_source("Tree");
    out.set_num("Spec:LifeInc", sum(&db, ModType::Inc, Some(&spec_cfg), &["Life"]));
    out.set_num("Spec:ManaInc", sum(&db, ModType::Inc, Some(&spec_cfg), &["Mana"]));
    out.set_num(
        "Spec:ArmourInc",
        sum(&db, ModType::Inc, Some(&spec_cfg), &["Armour", "ArmourAndEvasion"]),
    );
    out.set_num(
        "Spec:EvasionInc",
        sum(&db, ModType::Inc, Some(&spec_cfg), &["Evasion", "ArmourAndEvasion"]),
    );
    out.set_num("Spec:EnergyShieldInc", sum(&db, ModType::Inc, Some(&spec_cfg), &["EnergyShield"]));
}

/// `env.modDB:Flag(nil, name)` for the buildOutput tail (Calcs.lua:406).
pub(crate) fn mod_db_flag(env: &Env, name: &str) -> bool {
    flag(&env.mod_db, &[name])
}
