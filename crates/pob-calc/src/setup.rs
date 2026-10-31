//! Modules/CalcSetup.lua:19-120 `calcs.initModDB`, :123-305 `buildModListForNode(List)`, ModTools.lua:248-259
//! `mergeKeystones`, Common.lua:512-521 `mergeDB`, PantheonTools.lua:6-19 `applySoulMod`, and the small Lua helpers the
//! item phase needs (pob-ts `src/calc/initEnv.ts`, first half).

use std::rc::Rc;

use pob_data::constants::constants;
use pob_data::globals::globals;
use pob_lua::{Repl, gsub};
use pob_mods::{LuaResult, Mod, ModCfg, ModStore, ModType, StoreKind, StoreRef, Table, Tag, Value, create_mod};
use pob_tree::NodeType;

use crate::calc_lib;
use crate::env::{Env, GrantedSkill};

// --- small helpers -----------------------------------------------------------------------------------------------

/// A Lua error the calc does not expect (L18) dies with upstream's text (RUST-ARCH §5).
pub(crate) fn ok<T>(r: LuaResult<T>) -> T {
    r.unwrap_or_else(|e| panic!("{e}"))
}

pub(crate) fn s(x: &str) -> Value {
    Value::str(x)
}

pub(crate) fn n(x: f64) -> Value {
    Value::Num(x)
}

/// A fresh Lua table literal `{ k = v, ... }` as a tag (a new object per call, L14).
pub(crate) fn tag(pairs: &[(&str, Value)]) -> Tag {
    let mut t = Table::default();
    for (k, v) in pairs {
        t.hash.insert(Rc::from(*k), v.clone());
    }
    Tag::new(t)
}

/// `{ type = "Condition", var = var }`.
pub(crate) fn cond(var: &str) -> Tag {
    tag(&[("type", s("Condition")), ("var", s(var))])
}

/// `{ type = "Condition", var = var, neg = true }`.
pub(crate) fn cond_neg(var: &str) -> Tag {
    tag(&[("type", s("Condition")), ("var", s(var)), ("neg", Value::Bool(true))])
}

/// `{ type = "Multiplier", var = var }`.
pub(crate) fn mult(var: &str) -> Tag {
    tag(&[("type", s("Multiplier")), ("var", s(var))])
}

pub(crate) fn mod_flag(name: &str) -> u32 {
    *globals().mod_flag.get(name).unwrap_or_else(|| panic!("ModFlag.{name} is nil")) as u32
}

pub(crate) fn keyword_flag(name: &str) -> u32 {
    *globals()
        .keyword_flag
        .get(name)
        .unwrap_or_else(|| panic!("KeywordFlag.{name} is nil")) as u32
}

/// `data.characterConstants[key]`.
pub(crate) fn cc(key: &str) -> f64 {
    *constants()
        .character_constants
        .get(key)
        .unwrap_or_else(|| panic!("data.characterConstants[\"{key}\"] is nil (CalcSetup.lua)"))
}

/// `data.misc[key]`.
pub(crate) fn misc(key: &str) -> f64 {
    *constants()
        .misc
        .get(key)
        .unwrap_or_else(|| panic!("data.misc[\"{key}\"] is nil (CalcSetup.lua)"))
}

/// `store:NewMod(name, type, value, source, flags, keywordFlags, tags...)` with the vararg already decoded (MOD-CORE §1).
#[allow(clippy::too_many_arguments)]
pub(crate) fn new_mod(
    store: &StoreRef,
    name: &str,
    ty: ModType,
    value: Value,
    source: Option<&str>,
    flags: u32,
    keyword_flags: u32,
    tags: Vec<Tag>,
) {
    store
        .borrow_mut()
        .new_mod(create_mod(name, Some(ty), value, source, flags, keyword_flags, tags));
}

/// The common `NewMod(name, type, value, "Base", tags...)` shape.
pub(crate) fn base(store: &StoreRef, name: &str, ty: ModType, value: Value, tags: Vec<Tag>) {
    new_mod(store, name, ty, value, Some("Base"), 0, 0, tags);
}

/// The mods of a list store.
pub(crate) fn list_mods(store: &StoreRef) -> Vec<Rc<Mod>> {
    match &store.borrow().kind {
        StoreKind::List(l) => l.mods.clone(),
        StoreKind::Db(_) => unreachable!("list_mods on a ModDB"),
    }
}

/// `wipeTable(modList)` on a ModList -- the mods only (see the call sites).
fn clear_list(store: &StoreRef) {
    if let StoreKind::List(l) = &mut store.borrow_mut().kind {
        l.mods.clear();
    }
}

/// `s:gsub(pattern, repl)` with a literal replacement string.
pub(crate) fn lua_gsub(subject: &str, pattern: &str, repl: &str) -> String {
    let (out, _) = gsub(subject.as_bytes(), pattern.as_bytes(), Repl::Str(repl.as_bytes()), None)
        .unwrap_or_else(|e| panic!("gsub({pattern:?}): {e:?}"));
    String::from_utf8(out).expect("gsub of UTF-8 text stays UTF-8")
}

/// `rarity:gsub("(%a)(%u*)", function(a, b) return a..b:lower() end)` -- "UNIQUE" -> "Unique" (:1237). ASCII classes (R3).
pub(crate) fn title_case_rarity(rarity: &str) -> String {
    let b = rarity.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphabetic() {
            out.push(b[i]);
            i += 1;
            while i < b.len() && b[i].is_ascii_uppercase() {
                out.push(b[i].to_ascii_lowercase());
                i += 1;
            }
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).expect("ASCII edits keep UTF-8")
}

// --- CalcSetup.lua:19-120 `calcs.initModDB` -------------------------------------------------------------------------

/// CalcSetup.lua:19-120, in file order. Both the player DB and the enemy DB get all of it.
pub(crate) fn init_mod_db(env_modes: (bool, bool, bool), db: &StoreRef) {
    use ModType::{Base, Flag, Inc, More};
    let b = |name: &str, v: f64| base(db, name, Base, n(v), vec![]);
    let max_res = cc("base_maximum_all_resistances_%");
    for name in [
        "FireResistMax",
        "ColdResistMax",
        "LightningResistMax",
        "ChaosResistMax",
        "TotemFireResistMax",
        "TotemColdResistMax",
        "TotemLightningResistMax",
        "TotemChaosResistMax",
    ] {
        b(name, max_res);
    }
    b("BlockChanceMax", cc("maximum_block_%"));
    b("SpellBlockChanceMax", cc("base_maximum_spell_block_%"));
    b("SpellDodgeChanceMax", 75.0);
    b("ChargeDuration", 10.0);
    b("PowerChargesMax", cc("max_power_charges"));
    b("FrenzyChargesMax", cc("max_frenzy_charges"));
    b("EnduranceChargesMax", cc("max_endurance_charges"));
    b("SiphoningChargesMax", 0.0);
    b("ChallengerChargesMax", 0.0);
    b("BlitzChargesMax", 0.0);
    b("InspirationChargesMax", cc("maximum_righteous_charges"));
    b("CrabBarriersMax", 0.0);
    b("BrutalChargesMax", 0.0);
    b("BrineChargesMax", 0.0);
    base(
        db,
        "PhysicalDamageGainAsCold",
        Base,
        n(cc("physical_damage_%_to_add_as_cold_per_brine_charge")),
        vec![mult("BrineCharge")],
    );
    base(
        db,
        "PhysicalDamageGainAsLightning",
        Base,
        n(cc("physical_damage_%_to_add_as_lightning_per_brine_charge")),
        vec![mult("BrineCharge")],
    );
    b("AbsorptionChargesMax", 0.0);
    b("AfflictionChargesMax", 0.0);
    b("BloodChargesMax", cc("maximum_blood_scythe_charges"));
    b("MaxLifeLeechRate", cc("maximum_life_leech_rate_%_per_minute") / 60.0);
    b("MaxManaLeechRate", cc("maximum_mana_leech_rate_%_per_minute") / 60.0);
    b("ImpaleStacksMax", cc("impaled_debuff_number_of_reflected_hits"));
    b("SoulEaterMax", cc("soul_eater_maximum_stacks"));
    b("BleedStacksMax", 1.0);
    b("MaxEnergyShieldLeechRate", 10.0);
    b("MaxLifeLeechInstance", cc("maximum_life_leech_amount_per_leech_%_max_life"));
    b("MaxManaLeechInstance", cc("maximum_mana_leech_amount_per_leech_%_max_mana"));
    b(
        "MaxEnergyShieldLeechInstance",
        cc("maximum_energy_shield_leech_amount_per_leech_%_max_energy_shield"),
    );
    b("TrapThrowingTime", 0.6);
    b("MineLayingTime", 0.3);
    b("WarcryCastTime", 0.8);
    b("TotemPlacementTime", 0.6);
    b("BallistaPlacementTime", 0.5);
    b("ActiveTotemLimit", cc("base_number_of_totems_allowed"));
    b("ShockStacksMax", 1.0);
    b("ScorchStacksMax", 1.0);
    base(db, "MovementSpeed", Inc, n(-30.0), vec![cond("Maimed")]);
    let attack = mod_flag("Attack");
    let spell = mod_flag("Spell");
    new_mod(db, "DamageTaken", Inc, n(10.0), Some("Base"), attack, 0, vec![cond("Intimidated")]);
    new_mod(
        db,
        "DamageTaken",
        Inc,
        n(10.0),
        Some("Base"),
        attack,
        0,
        vec![cond_neg("Intimidated"), cond("Party:Intimidated")],
    );
    new_mod(db, "DamageTaken", Inc, n(10.0), Some("Base"), spell, 0, vec![cond("Unnerved")]);
    new_mod(
        db,
        "DamageTaken",
        Inc,
        n(10.0),
        Some("Base"),
        spell,
        0,
        vec![cond_neg("Unnerved"), cond("Party:Unnerved")],
    );
    let global_effect = |name: &str| tag(&[("type", s("GlobalEffect")), ("effectName", s(name)), ("effectType", s("Debuff"))]);
    base(
        db,
        "Damage",
        More,
        n(-10.0),
        vec![cond("Debilitated"), global_effect("Debilitated")],
    );
    base(
        db,
        "MovementSpeed",
        More,
        n(-20.0),
        vec![cond("Debilitated"), global_effect("Debilitated")],
    );
    base(
        db,
        "Damage",
        More,
        n(-10.0),
        vec![cond("MalignantMadness"), global_effect("Malignant Madness")],
    );
    base(
        db,
        "ActionSpeed",
        More,
        n(-10.0),
        vec![cond("MalignantMadness"), global_effect("Malignant Madness")],
    );
    let t = Value::Bool(true);
    base(
        db,
        "Condition:Burning",
        Flag,
        t.clone(),
        vec![tag(&[("type", s("IgnoreCond"))]), cond("Ignited")],
    );
    base(
        db,
        "Condition:Poisoned",
        Flag,
        t.clone(),
        vec![
            tag(&[("type", s("IgnoreCond"))]),
            tag(&[("type", s("MultiplierThreshold")), ("var", s("PoisonStack")), ("threshold", n(1.0))]),
        ],
    );
    for (name, var) in [
        ("Blind", "Blinded"),
        ("Chill", "Chilled"),
        ("Freeze", "Frozen"),
        ("Fortify", "Fortify"),
        ("Fortified", "Fortified"),
        ("Excommunicated", "Excommunicated"),
        ("Fanaticism", "Fanaticism"),
        ("Onslaught", "Onslaught"),
        ("UnholyMight", "UnholyMight"),
        ("ChaoticMight", "ChaoticMight"),
        ("Tailwind", "Tailwind"),
        ("Adrenaline", "Adrenaline"),
    ] {
        base(db, name, Flag, t.clone(), vec![cond(var)]);
    }
    // :83-96, one statement each upstream.
    for shrine in [
        "AccelerationShrine",
        "BrutalShrine",
        "DiamondShrine",
        "DivineShrine",
        "EchoingShrine",
        "GloomShrine",
        "GreaterFreezingShrine",
        "GreaterShockingShrine",
        "GreaterSkeletalShrine",
        "ImpenetrableShrine",
        "MassiveShrine",
        "ReplenishingShrine",
        "ResistanceShrine",
        "ResonatingShrine",
    ] {
        base(db, shrine, Flag, t.clone(), vec![cond(shrine)]);
    }
    // :97-102
    for lesser in ["Acceleration", "Brutal", "Impenetrable", "Massive", "Replenishing", "Resistance"] {
        let name = format!("Lesser{lesser}Shrine");
        base(db, &name, Flag, t.clone(), vec![cond(&name), cond_neg(&format!("{lesser}Shrine"))]);
    }
    // :103-107
    for blood in ["Rats", "Locusts", "Toads", "Crows", "Bats"] {
        let name = format!("BloodShrineOf{blood}");
        base(db, &name, Flag, t.clone(), vec![cond(&name)]);
    }
    for name in ["AlchemistsGenius", "LuckyHits", "Convergence"] {
        base(db, name, Flag, t.clone(), vec![cond(name)]);
    }
    base(db, "PhysicalDamageReduction", Base, n(-15.0), vec![cond("Crushed")]);
    b("CritChanceCap", 100.0);
    let (buffs, combat, effective) = env_modes;
    let mut st = db.borrow_mut();
    st.conditions.insert(Rc::from("Buffed"), buffs);
    st.conditions.insert(Rc::from("Combat"), combat);
    st.conditions.insert(Rc::from("Effective"), effective);
}

// --- CalcSetup.lua:123-305 ------------------------------------------------------------------------------------------

/// CalcSetup.lua:123-257 `buildModListForNode(env, node)` (MAIN: `reuse` is nil -> a fresh list per node); `node` is an
/// index into `env.spec.nodes`. Returns the list and the explode source (the second Lua value, L11). The radius-jewel
/// passes (:144-152, :190-200) are inert: `env.radiusJewelList` is always empty (P5), which also makes the `wipeTable`
/// branches unreachable on real data -- the mods triggering them only reach a node through `rad.func`. Ported anyway;
/// upstream's wipe also nils the list's `conditions`/`multipliers`/`parent`, which only a later EvalMod on that list sees.
pub(crate) fn build_mod_list_for_node(env: &mut Env, node_idx: usize) -> (StoreRef, Option<u32>) {
    let node = &env.spec.nodes[node_idx];
    let node_id = node.id;
    let mut mod_list = ModStore::new_list(None);
    if node.ty() == NodeType::Keystone {
        // `AddMod(node.keystoneMod)`: a Keystone node always has one (PassiveTree.lua).
        let km = node
            .keystone_mod()
            .unwrap_or_else(|| panic!("Modules/CalcSetup.lua:139: node {node_id} is a Keystone with no keystoneMod"));
        mod_list.borrow_mut().add_mod(km.clone());
    } else {
        // L14: the SAME mod objects as `node.modList`, shared with every other list, exactly as upstream.
        mod_list.borrow_mut().add_list(&node.mod_list().mods);
    }

    // :155-171 prefilter
    let (mut has_no_effect, mut has_alloc_no_effect, mut has_scale) = (false, false, false);
    let (mut has_other_effect, mut has_extra_skill, mut has_explode) = (false, false, false);
    for m in list_mods(&mod_list) {
        match &*m.name {
            "PassiveSkillHasNoEffect" => has_no_effect = true,
            "AllocatedPassiveSkillHasNoEffect" => has_alloc_no_effect = true,
            "PassiveSkillEffect" => has_scale = true,
            "PassiveSkillHasOtherEffect" => has_other_effect = true,
            "ExtraSkill" => has_extra_skill = true,
            "CanExplode" => has_explode = true,
            _ => {}
        }
    }

    let flag = |l: &StoreRef, name: &str| ok(l.borrow().flag(None, &[name])) == Some(true);
    if (has_no_effect && flag(&mod_list, "PassiveSkillHasNoEffect"))
        || (env.alloc_nodes.contains_key(&node_id) && has_alloc_no_effect && flag(&mod_list, "AllocatedPassiveSkillHasNoEffect"))
    {
        env.unimplemented
            .push("CalcSetup.lua:174 wipeTable(modList) keeps the mods only (conditions/multipliers/parent survive)".into());
        clear_list(&mod_list);
        has_scale = false;
        has_other_effect = false;
        has_extra_skill = false;
        has_explode = false;
    }

    // :182-190 effect scaling
    if has_scale {
        let scale = ok(calc_lib::mod_(&mod_list.borrow(), None, &["PassiveSkillEffect"]));
        if scale != 1.0 {
            let scaled = ModStore::new_list(None);
            ok(scaled.borrow_mut().scale_add_list(&list_mods(&mod_list), scale, false));
            mod_list = scaled;
        }
    }

    // :192-213: the second radius pass + rescan -- inert without radius jewels.

    if has_other_effect && flag(&mod_list, "PassiveSkillHasOtherEffect") {
        let new_mods = ok(mod_list.borrow().list(None, &["NodeModifier"]));
        for (i, entry) in new_mods.iter().enumerate() {
            let Value::Mod(m) = entry.get("mod") else {
                panic!("Modules/CalcSetup.lua:218: attempt to index a nil value (field 'mod')");
            };
            if i == 0 {
                env.unimplemented
                    .push("CalcSetup.lua:219 wipeTable(modList) keeps the mods only".into());
                clear_list(&mod_list);
                has_extra_skill = false;
                has_explode = false;
            }
            match &*m.name {
                "ExtraSkill" => has_extra_skill = true,
                "CanExplode" => has_explode = true,
                _ => {}
            }
            mod_list.borrow_mut().add_mod(m.clone());
        }
    }

    // :233-247 `node.grantedSkills` -- kept in env (the node table is the tree's, shared).
    let mut granted = Vec::new();
    if has_extra_skill {
        for v in ok(mod_list.borrow().list(None, &["ExtraSkill"])) {
            if !v.get("name").is_str("Unknown") {
                granted.push(GrantedSkill {
                    skill_id: v.get("skillId"),
                    level: v.get("level"),
                    source: format!("Tree:{node_id}"),
                    slot_name: None,
                    source_item: None,
                    source_node: None,
                });
            }
        }
    }
    env.node_granted_skills.insert(node_id, granted);

    if has_explode {
        let explode = flag(&mod_list, "CanExplode").then_some(node_id);
        return (mod_list, explode);
    }
    (mod_list, None)
}

/// CalcSetup.lua:259-305 `buildModListForNodeList(env, nodeList, finishJewels)`.
///
/// D13: upstream walks `pairs(nodeList)` -- a hash order; this port walks the insertion order of `env.allocNodes` (the XML
/// node order, the order pob-ts' `Map` keeps). The mods land in the same buckets, so only the order INSIDE a bucket can
/// differ from LuaJIT, which the oracle comparison treats as a multiset (L10: it reaches the last bits of a >= 3-term sum).
pub(crate) fn build_mod_list_for_node_list(env: &mut Env, finish_jewels: bool) -> (StoreRef, Vec<u32>) {
    let mod_list = ModStore::new_list(None);
    let mut explode_sources = Vec::new();
    let nodes: Vec<usize> = env.alloc_nodes.values().copied().collect();
    for idx in nodes {
        let (node_mod_list, explode) = build_mod_list_for_node(env, idx);
        // `t_insert(explodeSources, explode)` with a nil value is a no-op.
        if let Some(e) = explode {
            explode_sources.push(e);
        }
        mod_list.borrow_mut().add_list(&list_mods(&node_mod_list));
        let id = env.spec.nodes[idx].id;
        env.node_final_mod_list.insert(id, node_mod_list);
    }
    if finish_jewels {
        // :285-300: extraRadiusNodeList + `rad.func(nil, modList, rad.data)` -- both empty without radius jewels (P5).
        let extra: Vec<usize> = env.extra_radius_node_list.values().copied().collect();
        for idx in extra {
            let (node_mod_list, _) = build_mod_list_for_node(env, idx);
            let id = env.spec.nodes[idx].id;
            env.node_final_mod_list.insert(id, node_mod_list);
        }
    }
    (mod_list, explode_sources)
}

/// ModTools.lua:248-259 `modLib.mergeKeystones(env, modDB)`.
///
/// `fromTree` is TRUE when the source does NOT mention the tree (i.e. the keystone came from an item); upstream then
/// re-sources the keystone's OWN shared mod objects in place (`setSource`, L14). Here the re-sourced mod is a copy --
/// the in-place write would also rewrite the tree's keystone for every later env; in `init_env` the only caller passes
/// `initialNodeModDB`, whose `Keystone` mods all carry a `Tree<id>` source, so the branch is never taken.
pub(crate) fn merge_keystones(env: &mut Env, db: &StoreRef) {
    let entries = ok(db.borrow().tabulate(Some(ModType::List), None, &["Keystone"]));
    for e in entries {
        let Some(value) = e.value.as_str().map(str::to_owned) else {
            continue;
        };
        let keystone = env.spec.tree.keystone_map.get(&value).copied();
        if env.keystones_added.get(&value).copied().unwrap_or(false) {
            continue;
        }
        let Some(k_idx) = keystone else { continue };
        env.keystones_added.insert(value.clone(), true);
        let source = e.mod_.source.clone();
        let from_tree = source.as_deref().is_some_and(|src| !pob_lua::string::lower(src).contains("tree"));
        let mods = env.spec.tree.nodes[k_idx].stats.mod_list.mods.clone();
        for m in mods {
            if from_tree {
                let mut c = m.deep_copy();
                pob_mods::format::set_source(&mut c, source.as_deref().expect("fromTree implies a source"));
                db.borrow_mut().add_mod(Rc::new(c));
            } else {
                db.borrow_mut().add_mod(m);
            }
        }
    }
}

/// Common.lua:512-521 `mergeDB(srcDB, modDB)`.
pub(crate) fn merge_db(dst: &StoreRef, src: &StoreRef) {
    let src = src.borrow();
    let StoreKind::Db(db) = &src.kind else {
        unreachable!("mergeDB of a ModList")
    };
    let mut d = dst.borrow_mut();
    ok(d.add_db(db));
    for (k, v) in &src.conditions {
        d.conditions.insert(k.clone(), *v);
    }
    for (k, v) in &src.multipliers {
        d.multipliers.insert(k.clone(), *v);
    }
}

/// PantheonTools.lua:6-19 `pantheon.applySoulMod(db, parser, god)`: every soul line through `modLib.parseMod`; a line with
/// mods and no `extra` (L4) gets the FIRST soul's name as its source.
pub(crate) fn apply_soul_mod(db: &StoreRef, god: &pob_data::pantheons::Pantheon) {
    for soul in &god.souls {
        for soul_mod in &soul.mods {
            let parsed = pob_parser::parse_mod_line(&soul_mod.line, false);
            if let (Some(list), None) = (parsed.mod_list, parsed.extra) {
                let god_name = &god.souls[0].name;
                let source: Rc<str> = Rc::from(format!("Pantheon:{god_name}"));
                let mods: Vec<Rc<Mod>> = list
                    .into_iter()
                    .map(|mut m| {
                        m.source = Some(source.clone());
                        Rc::new(m)
                    })
                    .collect();
                db.borrow_mut().add_list(&mods);
            }
        }
    }
}

/// `modList:Flag(nil, name)` / `:Sum("INC", nil, name)` over an item's plain mod array (`baseModList`).
pub(crate) fn temp_list(mods: &[Mod]) -> StoreRef {
    let st = ModStore::new_list(None);
    for m in mods {
        st.borrow_mut().add_mod(Rc::new(m.clone()));
    }
    st
}

/// `{ source = "Tree" }`.
pub(crate) fn cfg_source(src: &str) -> ModCfg {
    ModCfg {
        source: Some(Rc::from(src)),
        ..ModCfg::default()
    }
}
