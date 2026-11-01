//! Minions as actors (R7.10): `activeSkill.minion` (CalcActiveSkill.lua:704-793, built by [`crate::skill_modlist`]), its skills
//! (`calcs.createMinionSkills`, CalcActiveSkill.lua:878-934), its mod database (`initMinionModDB` CalcPerform.lua:1154-1244,
//! `addMinionModifiers` :1247-1253) and the perform-time actor (`activeSkill.minion.modDB`, :1297-1302).
//!
//! The minion table of upstream is one Lua table shared by `activeSkill.minion` and `env.minion` (:1316): here [`Minion`] lives on its
//! [`ActiveSkill`] and its perform-time actor ([`CalcActor`], `minion.modDB`) is an `Rc` that `Env::minion` shares.
//!
//! Hash-order walks (MINEFIELD-RUST R13): `pairs(env.build.itemsTab.slots)` (:1222) decides the order in which a minion's item mods are
//! added -- the mod order of its DB (a multiset for every sum); Rust takes `orderedSlots`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_data::constants::{constants, monster_table_at};
use pob_items::{DataTable, Item};
use pob_lua::number::{max2, round};
use pob_mods::{Actor, ActorRef, Mod, ModCfg, ModStore, ModType, StoreRef, Value};

use crate::active_skill::ActiveSkill;
use crate::env::{CalcActor, Env};
use crate::setup::{n, new_mod, ok, s, tag};

const CP: &str = "Modules/CalcPerform.lua";

/// Which `env.data.monster*LifeTable` `minion.lifeTable` is (:724-728).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifeTable {
    /// `monsterLifeTable` (hostile, or a spectre).
    Monster,
    /// `monsterLifeTable2` (`lifeScaling == "AltLife1"`).
    Monster2,
    /// `monsterLifeTable3` (`lifeScaling == "AltLife2"`).
    Monster3,
    /// `monsterAllyLifeTable`.
    Ally,
}

impl LifeTable {
    pub fn table(self) -> &'static [f64] {
        let c = constants();
        match self {
            LifeTable::Monster => &c.monster_life_table,
            LifeTable::Monster2 => &c.monster_life_table2,
            LifeTable::Monster3 => &c.monster_life_table3,
            LifeTable::Ally => &c.monster_ally_life_table,
        }
    }
}

/// `activeSkill.minion` (CalcActiveSkill.lua:704-793) plus what perform hangs on it (:1297-1302).
#[derive(Clone)]
pub struct Minion {
    /// `minion.type`.
    pub ty: String,
    /// `minion.minionData` -- the `data.minions` entry (a shared Lua table).
    pub data: Value,
    pub hostile: bool,
    pub level: f64,
    /// `minion.itemList` (filled by `initMinionModDB`, :1213/:1236).
    pub item_list: IndexMap<String, Rc<Item>>,
    /// `minion.uses = activeGrantedEffect.minionUses` -- the slot names it maps to `true`.
    pub uses: Option<Vec<String>>,
    pub life_table: LifeTable,
    pub weapon_data1: DataTable,
    pub weapon_data2: DataTable,
    /// `minion.itemSet` -- index into `build.itemSets` (XML order = `itemSetOrderList`).
    pub item_set: Option<usize>,
    /// The perform-time actor (`minion.modDB = new("ModDB")`, `minion.modDB.actor = minion`, :1297-1298).
    pub actor: Option<Rc<RefCell<CalcActor>>>,
    /// `minion.activeSkillList` (createMinionSkills).
    pub active_skill_list: Vec<ActiveSkill>,
    /// `minion.mainSkill` -- index into `active_skill_list`.
    pub main_skill: Option<usize>,
}

impl std::fmt::Debug for Minion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Minion({}, level {})", self.ty, self.level)
    }
}

impl Minion {
    /// `minionData[k]`.
    pub fn md(&self, k: &str) -> Value {
        self.data.get(k)
    }

    /// `minion.modDB` (perform-time).
    pub fn mod_db(&self) -> StoreRef {
        self.actor
            .as_ref()
            .unwrap_or_else(|| panic!("{CP}: minion.modDB is nil (perform did not build the minion actor)"))
            .borrow()
            .mod_db
            .clone()
    }

    /// `minion.mainSkill`.
    pub fn main(&self) -> &ActiveSkill {
        &self.active_skill_list[self.main_skill.unwrap_or_else(|| panic!("{CP}: minion.mainSkill is nil"))]
    }
}

/// `data.minions[id].monsterTags` as the mod core's `actor.minionData.monsterTags`.
fn monster_tags(data: &Value) -> Option<Vec<pob_mods::Str>> {
    match data.get("monsterTags") {
        Value::Table(t) => Some(t.array.iter().filter_map(|v| v.as_str().map(Rc::from)).collect()),
        _ => None,
    }
}

/// A `DataTable` as the mod core's table.
pub(crate) fn data_table_to_table(d: &DataTable) -> pob_mods::Table {
    let mut t = pob_mods::Table::default();
    for (k, v) in &d.0 {
        t.hash.insert(Rc::from(k.as_str()), v.clone());
    }
    t
}

/// CalcPerform.lua:1297-1298: `minion.modDB = new("ModDB")`, `minion.modDB.actor = minion` -- the actor table with `parent` /
/// `enemy` (:710-716) as EvalMod's `getActor` sees them.
pub(crate) fn new_minion_actor(env: &Env, m: &Minion) -> Rc<RefCell<CalcActor>> {
    let db = ModStore::new_db(None);
    let (parent, enemy) = if m.hostile {
        (&env.enemy, &env.player)
    } else {
        (&env.player, &env.enemy)
    };
    let core: ActorRef = Rc::new(RefCell::new(Actor {
        mod_db: Some(db.clone()),
        output: Some(IndexMap::new()),
        active_skill_list: Some(Vec::new()),
        item_list: Some(IndexMap::new()),
        minion_data: Some(monster_tags(&m.data)),
        weapon_data1: Some(Rc::new(data_table_to_table(&m.weapon_data1))),
        weapon_data2: Some(Rc::new(data_table_to_table(&m.weapon_data2))),
        ..Actor::default()
    }));
    core.borrow_mut().actors.insert(Rc::from("parent"), parent.borrow().actor.clone());
    core.borrow_mut().actors.insert(Rc::from("enemy"), enemy.borrow().actor.clone());
    db.borrow_mut().set_actor(&core);
    Rc::new(RefCell::new(CalcActor {
        actor: core,
        mod_db: db,
        level: m.level,
        output: IndexMap::new(),
        item_list: IndexMap::new(),
        weapon_data1: m.weapon_data1.clone(),
        weapon_data2: m.weapon_data2.clone(),
        enemy: Rc::downgrade(enemy),
        player: Weak::new(),
        main_skill: None,
        active_skill_list: Vec::new(),
    }))
}

/// Keeps the mod core's `actor.itemList` of the minion in step with [`CalcActor::item_list`].
fn sync_item_list(actor: &CalcActor) {
    actor.actor.borrow_mut().item_list = Some(
        actor
            .item_list
            .iter()
            .map(|(k, v)| (Rc::from(k.as_str()), v.clone() as Rc<dyn pob_mods::ItemLike>))
            .collect(),
    );
}

/// CalcPerform.lua:1247-1253 `addMinionModifiers(modList, skillCfg, minion)`: the `MinionModifier` LIST values whose `type` is nil or
/// the minion's type -- `value.mod` (the shared mod object, R4) into the minion's DB.
pub(crate) fn add_minion_modifiers(list: &StoreRef, cfg: Option<&ModCfg>, minion_db: &StoreRef, minion_type: &str) {
    for v in ok(list.borrow().list(cfg, &["MinionModifier"])) {
        let ty = v.get("type");
        if ty.is_nil() || ty.is_str(minion_type) {
            match v.get("mod") {
                Value::Mod(m) => minion_db.borrow_mut().add_mod(m),
                other => panic!("{CP}:1250: MinionModifier value.mod is not a mod ({})", other.type_name()),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn nm(db: &StoreRef, name: &str, ty: ModType, value: Value, source: &str, flags: u32, kw: u32, tags: Vec<pob_mods::Tag>) {
    new_mod(db, name, ty, value, Some(source), flags, kw, tags);
}

fn mc(key: &str) -> f64 {
    *constants()
        .monster_constants
        .0
        .get(key)
        .unwrap_or_else(|| panic!("data.monsterConstants[{key:?}] is nil"))
}

fn md_num(m: &Minion, k: &str, line: u32) -> f64 {
    m.md(k)
        .as_num()
        .unwrap_or_else(|| panic!("{CP}:{line}: attempt to perform arithmetic on field '{k}' (a nil value)"))
}

/// `env.data.mapLevelLifeMult[env.enemyLevel] or 1`.
fn map_level_life_mult(enemy_level: f64) -> f64 {
    constants()
        .map_level_life_mult
        .0
        .get(&pob_lua::tostring_num(enemy_level))
        .copied()
        .unwrap_or(1.0)
}

/// CalcPerform.lua:1154-1244 `initMinionModDB(env, activeSkill, output)` for `env.player.activeSkillList[i]`: the minion's base mods,
/// its data mods, the skill's extra mods, the Aegis / The Iron Mass / bow / main-hand / item-set mods, Strength from the player.
/// `minion.output` is set by the caller (`output or { }`).
#[allow(clippy::too_many_lines)]
pub(crate) fn init_minion_mod_db(env: &mut Env, i: usize, aegis_item: Option<Rc<Item>>) {
    let mod_db = env.mod_db.clone();
    let (minion, extra, sd_bow, sd_main_hand) = {
        let st = env.skills.as_ref().expect("skills on");
        let sk = &st.active_skill_list[i];
        let m = sk
            .minion
            .as_deref()
            .unwrap_or_else(|| panic!("{CP}:1156: activeSkill.minion is nil"))
            .clone();
        let truthy = |k: &str| sk.skill_data.get(k).is_some_and(Value::truthy);
        (
            m,
            sk.extra_skill_mod_list.clone().unwrap_or_default(),
            truthy("minionUseBowAndQuiver"),
            truthy("minionUseMainHandWeapon"),
        )
    };
    let actor_rc = minion.actor.clone().unwrap_or_else(|| panic!("{CP}:1158: minion.modDB is nil"));
    let db = actor_rc.borrow().mod_db.clone();
    let lvl = minion.level;
    // :1158-1159
    db.borrow_mut().multipliers.insert(Rc::from("Level"), lvl);
    crate::setup::init_mod_db((env.mode_buffs, env.mode_combat, env.mode_effective), &db);
    // :1160-1164 life
    let at = |t: &[f64], what: &str| {
        monster_table_at(t, lvl).unwrap_or_else(|| panic!("{CP}: attempt to perform arithmetic on a nil value ({what}[{lvl}])"))
    };
    let mut base_life = at(minion.life_table.table(), "lifeTable") * md_num(&minion, "life", 1160);
    if minion.hostile {
        base_life *= map_level_life_mult(env.enemy_level);
    }
    nm(&db, "Life", ModType::Base, n(base_life.floor()), "Base", 0, 0, vec![]);
    // :1165-1172 energy shield
    if let Some(es) = minion.md("energyShield").as_num() {
        let es_table = if minion.hostile {
            minion.life_table.table()
        } else {
            &constants().monster_ally_life_table
        };
        let mut base_es = at(es_table, "esTable") * md_num(&minion, "life", 1167) * es;
        if minion.hostile {
            base_es *= map_level_life_mult(env.enemy_level);
        }
        nm(&db, "EnergyShield", ModType::Base, n(base_es.floor()), "Base", 0, 0, vec![]);
    } else if minion.md("energyShield").truthy() {
        panic!("{CP}:1167: attempt to perform arithmetic on field 'energyShield'");
    }
    // :1173-1174 armour / evasion (`minionData.armour or 1`)
    let or1 = |k: &str| match minion.md(k) {
        Value::Nil | Value::Bool(false) => 1.0,
        v => v
            .as_num()
            .unwrap_or_else(|| panic!("{CP}:1173: attempt to perform arithmetic on field '{k}'")),
    };
    let c = constants();
    nm(
        &db,
        "Armour",
        ModType::Base,
        n(round(at(&c.monster_armour_table, "monsterArmourTable") * or1("armour"), None)),
        "Base",
        0,
        0,
        vec![],
    );
    nm(
        &db,
        "Evasion",
        ModType::Base,
        n(round(at(&c.monster_evasion_table, "monsterEvasionTable") * or1("evasion"), None)),
        "Base",
        0,
        0,
        vec![],
    );
    // :1175-1179 accuracy
    if ok(mod_db.borrow().flag(None, &["MinionAccuracyEqualsAccuracy"])) == Some(true) {
        let acc = ok(crate::calc_lib::val(&mod_db.borrow(), "Accuracy", None));
        let dex = ok(crate::calc_lib::val(&mod_db.borrow(), "Dex", None));
        let per = match ok(mod_db.borrow().override_(None, &["DexAccBonusOverride"])) {
            Some(v) if v.truthy() => v
                .as_num()
                .unwrap_or_else(|| panic!("{CP}:1176: attempt to perform arithmetic on a {} value", v.type_name())),
            _ => crate::setup::misc("AccuracyPerDexBase"),
        };
        nm(&db, "Accuracy", ModType::Base, n(acc + dex * per), "Player", 0, 0, vec![]);
    } else {
        nm(
            &db,
            "CannotBeEvaded",
            ModType::Flag,
            n(1.0),
            "Minion Attacks always hit",
            0,
            0,
            vec![],
        );
    }
    let mt = |var: &str| tag(&[("type", s("Multiplier")), ("var", s(var))]);
    let mf = crate::setup::mod_flag;
    let kf = crate::setup::keyword_flag;
    nm(
        &db,
        "CritMultiplier",
        ModType::Base,
        n(mc("base_critical_strike_multiplier") - 100.0),
        "Base",
        0,
        0,
        vec![],
    );
    nm(
        &db,
        "DotMultiplier",
        ModType::Base,
        n(mc("critical_ailment_dot_multiplier_+")),
        "Base",
        0,
        0,
        vec![tag(&[("type", s("Condition")), ("var", s("CriticalStrike"))])],
    );
    for (name, key) in [
        ("FireResist", "fireResist"),
        ("ColdResist", "coldResist"),
        ("LightningResist", "lightningResist"),
        ("ChaosResist", "chaosResist"),
    ] {
        nm(&db, name, ModType::Base, minion.md(key), "Base", 0, 0, vec![]);
    }
    nm(
        &db,
        "CritChance",
        ModType::Inc,
        n(mc("critical_strike_chance_+%_per_power_charge")),
        "Base",
        0,
        0,
        vec![mt("PowerCharge")],
    );
    nm(
        &db,
        "Speed",
        ModType::Inc,
        n(mc("base_attack_speed_+%_per_frenzy_charge")),
        "Base",
        mf("Attack"),
        0,
        vec![mt("FrenzyCharge")],
    );
    nm(
        &db,
        "Speed",
        ModType::Inc,
        n(mc("base_cast_speed_+%_per_frenzy_charge")),
        "Base",
        mf("Cast"),
        0,
        vec![mt("FrenzyCharge")],
    );
    nm(
        &db,
        "Damage",
        ModType::More,
        n(mc("object_inherent_damage_+%_final_per_frenzy_charge")),
        "Base",
        0,
        0,
        vec![mt("FrenzyCharge")],
    );
    nm(
        &db,
        "PhysicalDamageReduction",
        ModType::Base,
        n(mc("physical_damage_reduction_%_per_endurance_charge")),
        "Base",
        0,
        0,
        vec![mt("EnduranceCharge")],
    );
    nm(
        &db,
        "ElementalDamageReduction",
        ModType::Base,
        n(mc("elemental_damage_reduction_%_per_endurance_charge_if_player_minion")),
        "Base",
        0,
        0,
        vec![mt("EnduranceCharge")],
    );
    nm(&db, "ProjectileCount", ModType::Base, n(1.0), "Base", 0, 0, vec![]);
    nm(
        &db,
        "MaximumFortification",
        ModType::Base,
        n(mc("base_max_fortification")),
        "Base",
        0,
        0,
        vec![],
    );
    nm(
        &db,
        "Damage",
        ModType::More,
        n(200.0),
        "Base",
        0,
        kf("Bleed"),
        vec![tag(&[("type", s("ActorCondition")), ("actor", s("enemy")), ("var", s("Moving"))])],
    );
    // :1201-1206 the data's mods, the skill's extra mods (shared objects)
    if let Value::Table(t) = minion.md("modList") {
        for v in &t.array {
            match v {
                Value::Mod(m) => db.borrow_mut().add_mod(m.clone()),
                other => panic!("{CP}:1202: minionData.modList entry is not a mod ({})", other.type_name()),
            }
        }
    }
    for m in extra {
        db.borrow_mut().add_mod(m);
    }
    // :1207-1210 Necromantic Aegis
    let mut item_list = minion.item_list.clone();
    if let Some(aegis) = env.aegis_mod_list.clone() {
        match aegis_item {
            Some(it) => {
                item_list.insert("Weapon 3".into(), it);
            }
            None => {
                item_list.shift_remove("Weapon 3");
            }
        }
        db.borrow_mut().add_list(&crate::setup::list_mods(&aegis));
    }
    // :1211-1213 The Iron Mass
    if let Some(iron) = env.the_iron_mass.clone()
        && minion.ty == "RaisedSkeleton"
    {
        db.borrow_mut().add_list(&crate::setup::list_mods(&iron));
    }
    // :1214-1225 bow and quiver (Mirror / Blink Arrow)
    let p_items = env.player.borrow().item_list.clone();
    let p_w1_type = env.player.borrow().weapon_data1.get("type");
    let slot_list = |it: &Item, slot_num: f64, line: u32| -> Vec<Rc<Mod>> {
        crate::init_env::src_list_of(it, Some(slot_num))
            .unwrap_or_else(|| panic!("{CP}:{line}: bad argument #1 to 'ipairs' (table expected, got nil)"))
    };
    if sd_bow {
        if p_w1_type.is_str("Bow") {
            let w1 = p_items
                .get("Weapon 1")
                .unwrap_or_else(|| panic!("{CP}:1216: attempt to index field 'Weapon 1' (a nil value)"));
            db.borrow_mut().add_list(&slot_list_only(w1, 1216));
        }
        if let Some(w2) = p_items.get("Weapon 2")
            && w2.ty.as_deref() == Some("Quiver")
        {
            let scale = max2(ok(mod_db.borrow().sum(ModType::Base, None, &["WidowHailMultiplier"])), 1.0);
            let list = item_mod_list_only(w2, 1219);
            ok(db.borrow_mut().scale_add_list(&list, scale, false));
        }
        if ok(mod_db.borrow().flag(None, &["BlinkAndMirrorUseGloves"])) == Some(true)
            && let Some(g) = p_items.get("Gloves")
        {
            db.borrow_mut().add_list(&item_mod_list_only(g, 1222));
        }
    }
    // :1226-1228
    if sd_main_hand {
        let w1 = p_items
            .get("Weapon 1")
            .unwrap_or_else(|| panic!("{CP}:1227: attempt to index field 'Weapon 1' (a nil value)"));
        db.borrow_mut().add_list(&slot_list_only(w1, 1227));
    }
    // :1229-1250 item set / used slots (`pairs(env.build.itemsTab.slots)` -- `orderedSlots` order here, see the module doc)
    if minion.item_set.is_some() || minion.uses.is_some() {
        let uses = minion
            .uses
            .clone()
            .unwrap_or_else(|| panic!("{CP}:1231: attempt to index field 'uses' (a nil value)"));
        let set = minion.item_set.map(|k| env.build.item_sets[k].clone());
        for slot in env.ordered_slots.clone() {
            // `pairs(slots)` covers every slot control, `orderedSlots` too (ItemsTab.lua:141-235)
            let mut slot_name = slot.slot_name.clone();
            if !uses.contains(&slot_name) {
                continue;
            }
            let item = match &set {
                Some(set) => {
                    if slot.weapon_set == Some(1.0) && set.use_second_weapon_set {
                        slot_name.push_str(" Swap");
                    }
                    let sel = set.slots.get(&slot_name).map_or(0.0, |x| x.item_id);
                    env.item(sel).cloned()
                }
                None => p_items.get(&slot_name).cloned(),
            };
            if let Some(item) = item {
                item_list.insert(slot_name.clone(), item.clone());
                let list = slot_list(&item, slot.slot_num.unwrap_or(f64::NAN), 1245);
                db.borrow_mut().add_list(&list);
            }
        }
    }
    // :1252-1254 Strength added to minions
    let str_added = ok(mod_db.borrow().sum(ModType::Base, None, &["StrengthAddedToMinions"]));
    if str_added > 0.0 {
        let str_ = ok(crate::calc_lib::val(&mod_db.borrow(), "Str", None));
        nm(
            &db,
            "Str",
            ModType::Base,
            n(round(str_ * str_added / 100.0, None)),
            "Player",
            0,
            0,
            vec![],
        );
    }
    {
        let mut a = actor_rc.borrow_mut();
        a.item_list = item_list.clone();
        sync_item_list(&a);
    }
    let st = env.skills.as_mut().expect("skills on");
    if let Some(m) = st.active_skill_list[i].minion.as_deref_mut() {
        m.item_list = item_list;
    }
}

/// `item.slotModList[1]` (the main hand list of a weapon).
fn slot_list_only(it: &Item, line: u32) -> Vec<Rc<Mod>> {
    let slots = it
        .slot_mod_list
        .as_ref()
        .unwrap_or_else(|| panic!("{CP}:{line}: attempt to index field 'slotModList' (a nil value)"));
    slots
        .first()
        .map(|l| l.iter().map(|m| Rc::new(m.m.clone())).collect())
        .unwrap_or_else(|| panic!("{CP}:{line}: bad argument #1 to 'ipairs' (table expected, got nil)"))
}

/// `item.modList`.
fn item_mod_list_only(it: &Item, line: u32) -> Vec<Rc<Mod>> {
    it.mod_list
        .as_ref()
        .map(|l| l.iter().map(|m| Rc::new(m.m.clone())).collect())
        .unwrap_or_else(|| panic!("{CP}:{line}: bad argument #1 to 'ipairs' (table expected, got nil)"))
}

/// `env.configInput[k] or env.configPlaceholder[k]`.
fn config_or(env: &Env, k: &str) -> Value {
    let conv = |v: &pob_build::ConfigValue| match v {
        pob_build::ConfigValue::Num(x) => Value::Num(*x),
        pob_build::ConfigValue::Str(t) => Value::str(t),
        pob_build::ConfigValue::Bool(b) => Value::Bool(*b),
    };
    let input = env.config.input.get(k).map(conv).unwrap_or_default();
    if input.truthy() {
        input
    } else {
        env.config.placeholder.get(k).map(conv).unwrap_or_default()
    }
}

/// CalcPerform.lua:1297-1302 for `env.player.activeSkillList[i]` (a skill with a minion): `minion.modDB = new("ModDB")` with the minion as
/// its actor, `calcs.createMinionSkills(env, activeSkill)` (CalcActiveSkill.lua:878-934) and `activeSkill.skillPartName =
/// minion.mainSkill.activeEffect.grantedEffect.name`. `skill_list` is the skill's perform-time `skillModList` (:1296).
#[allow(clippy::too_many_lines)]
pub(crate) fn build_minion(env: &mut Env, i: usize, skill_list: &StoreRef, unimplemented: &mut Vec<String>) {
    let mut minion = {
        let st = env.skills.as_mut().expect("skills on");
        *st.active_skill_list[i].minion.take().expect("activeSkill.minion")
    };
    // :1297-1298
    let actor = new_minion_actor(env, &minion);
    {
        let mut a = actor.borrow_mut();
        a.item_list = minion.item_list.clone();
        sync_item_list(&a);
    }
    minion.actor = Some(actor.clone());
    let minion_db = actor.borrow().mod_db.clone();

    let st = env.skills.as_ref().expect("skills on");
    let sk = st.skills.clone();
    let parent = &st.active_skill_list[i];
    let parent_cfg = crate::perform::skill_cfg_of(st, parent);
    // :883-895 the skill ids: the data's (those in `data.skills`), then `ExtraMinionSkill`
    let mut ids: Vec<String> = Vec::new();
    if let Value::Table(t) = minion.md("skillList") {
        for v in &t.array {
            if let Some(id) = v.as_str()
                && sk.index.contains_key(id)
            {
                ids.push(id.to_owned());
            }
        }
    } else {
        panic!("Modules/CalcActiveSkill.lua:885: bad argument #1 to 'ipairs' (table expected)");
    }
    crate::perform::skill_guard(&parent.malformed_mods, &["ExtraMinionSkill"]);
    for v in ok(skill_list.borrow().list(Some(&parent_cfg), &["ExtraMinionSkill"])) {
        let ml = v.get("minionList");
        let listed = match &ml {
            Value::Nil | Value::Bool(false) => true,
            Value::Table(t) => t.array.iter().any(|x| x.is_str(&minion.ty)),
            other => panic!("Modules/CalcActiveSkill.lua:891: isValueInArray of a {}", other.type_name()),
        };
        if listed {
            match v.get("skillId") {
                Value::Str(id) => ids.push(id.to_string()),
                other => panic!("Modules/CalcActiveSkill.lua:892: ExtraMinionSkill skillId is {}", other.type_name()),
            }
        }
    }
    if ids.is_empty() {
        ids.push("Melee".into());
    }
    let support_list = parent.support_list.clone();
    let parent_act = &sk.effects[parent.active_effect.borrow().granted_effect];
    let parent_types = parent.skill_types.clone();
    let parent_minion_types = parent.minion_skill_types.clone();
    let parent_spectre = parent.skill_flags.get("spectre").copied();
    let damage_eff = match parent.skill_data.get("minionDamageEffectiveness") {
        None | Some(Value::Nil | Value::Bool(false)) => 0.0,
        Some(v) => v.as_num().unwrap_or_else(|| {
            panic!("Modules/CalcActiveSkill.lua:916: attempt to perform arithmetic on field 'minionDamageEffectiveness'")
        }),
    };
    let parent_src = parent.active_effect.borrow().src_instance;
    let parent_name = parent_act.name.clone();
    let from_item = st.from_item_marks.clone();
    let spectres: Vec<String> = env
        .build
        .spectres
        .iter()
        .filter(|id| crate::skill_modlist::is_minion(id))
        .cloned()
        .collect();

    let melee_distance = config_or(env, "meleeDistance");
    let projectile_distance = config_or(env, "projectileDistance");
    let p = env.player.borrow();
    let inp = crate::skill_modlist::ModListInputs {
        mod_db: &env.mod_db,
        enemy_db: &env.enemy_db,
        item_list: &p.item_list,
        weapon_data1: &p.weapon_data1,
        weapon_data2: &p.weapon_data2,
        mode_buffs: env.mode_buffs,
        mode_combat: env.mode_combat,
        mode_effective: env.mode_effective,
        melee_distance,
        projectile_distance,
        spectre_list: &spectres,
        character_level: env.build.level,
        enemy_level: env.enemy_level,
        items: &env.items,
        item_sets: &env.build.item_sets,
        the_iron_mass: env.the_iron_mass.is_some(),
        calcs: env.mode == "CALCS",
    };
    let view_of =
        |d: &DataTable| -> (Option<String>, bool) { (d.get("type").as_str().map(str::to_owned), d.get("countsAsAll1H").truthy()) };
    let (w1t, w1a) = view_of(&minion.weapon_data1);
    let (w2t, w2a) = view_of(&minion.weapon_data2);
    // a minion skill has no `srcInstance` / `socketGroup`: the builder never touches the skills tab for it
    let tab = &mut env.skills.as_mut().expect("skills on").tab;
    let mut list = Vec::new();
    for id in &ids {
        let ge_idx = sk.index[id.as_str()];
        let ge = &sk.effects[ge_idx];
        // :897-910 the highest level whose requirement the minion meets (`ipairs(levels)`)
        let mut level = 1.0;
        if ge.levels_len > 1 {
            let mut k = 1.0;
            while let Some(l) = ge.level(k) {
                if l.level_requirement.unwrap_or(0.0) > minion.level {
                    break;
                }
                level = k;
                k += 1.0;
            }
        }
        let effect = crate::active_skill::EffectRef::new(crate::active_skill::SkillEffect {
            granted_effect: ge_idx,
            level: Some(level),
            quality: Some(0.0),
            ..crate::active_skill::SkillEffect::default()
        });
        let view = crate::active_skill::ActorView {
            weapon1: crate::calc_tools::WeaponView {
                ty: w1t.as_deref(),
                counts_as_all_1h: w1a,
            },
            weapon2: crate::calc_tools::WeaponView {
                ty: w2t.as_deref(),
                counts_as_all_1h: w2a,
            },
            is_player: false,
        };
        let mut ms = crate::active_skill::create_active_skill_summoned(
            &sk,
            effect,
            support_list.clone(),
            &view,
            None,
            false,
            &from_item,
            Some(crate::active_skill::Summon {
                index: i,
                view: crate::calc_tools::SummonView {
                    granted_effect: parent_act,
                    skill_types: Some(&parent_types),
                    minion_skill_types: parent_minion_types.as_ref(),
                },
            }),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let actor_view = crate::skill_modlist::SkillActor {
            mod_db: &minion_db,
            item_list: &minion.item_list,
            weapon_data1: &minion.weapon_data1,
            weapon_data2: &minion.weapon_data2,
            minion: Some((&minion.data, minion.level)),
            summon_name: Some(parent_name.clone()),
            calcs_main: false,
        };
        // the minion skill's auxiliary-list entry (:871-874) only feeds the UI's `env.auxSkillList` (Calcs.lua:63)
        let _aux = crate::skill_modlist::build_skill(&sk, tab, &mut ms, &inp, &actor_view, unimplemented).unwrap_or_else(|e| panic!("{e}"));
        // :912-916
        ms.skill_flags.insert("minion".into(), true);
        ms.skill_flags.insert("minionSkill".into(), true);
        ms.skill_flags.insert("haveMinion".into(), true);
        match parent_spectre {
            Some(b) => {
                ms.skill_flags.insert("spectre".into(), b);
            }
            None => {
                ms.skill_flags.shift_remove("spectre");
            }
        }
        ms.skill_data
            .insert("damageEffectiveness".into(), Value::Num(1.0 + damage_eff / 100.0));
        list.push(ms);
    }
    drop(p);
    // :919-929 the minion's main skill (MAIN writes the choice back to the gem)
    let n_skills = list.len() as f64;
    let mode = env.mode;
    let st = env.skills.as_mut().expect("skills on");
    let src = parent_src.unwrap_or_else(|| panic!("Modules/CalcActiveSkill.lua:921: attempt to index field 'srcInstance' (a nil value)"));
    let gi = &mut st.tab.socket_group_list_mut()[src.0].gem_list[src.1];
    let skill_index = if mode == "CALCS" {
        let k = max2(n_skills.min(gi.skill_minion_skill_calcs.unwrap_or(1.0)), 1.0);
        gi.skill_minion_skill_calcs = Some(k);
        k
    } else {
        let k = max2(n_skills.min(gi.skill_minion_skill.unwrap_or(1.0)), 1.0);
        if mode == "MAIN" {
            gi.skill_minion_skill = Some(k);
        }
        k
    };
    minion.active_skill_list = list;
    minion.main_skill = (skill_index >= 1.0 && skill_index.fract() == 0.0 && skill_index <= n_skills).then(|| skill_index as usize - 1);
    let main_name = {
        let m = minion.main();
        sk.effects[m.active_effect.borrow().granted_effect].name.clone()
    };
    let parent = &mut st.active_skill_list[i];
    parent.skill_part_name = Some(main_name);
    parent.minion = Some(Box::new(minion));
}
