//! CalcOffence.lua:348-1055 -- the start of `calcs.offence` up to the probe's OC1 (before :1057). `calcAreaOfEffect` (:374-492) and
//! `calcResistForType` (:494-507) are local functions defined here; the first lives in [`super::a2`] (called at :1185), the second is
//! first called after :1936 (R7.5).

use std::rc::Rc;

use pob_lua::number::{max2, min2};
use pob_mods::{Mod, ModCfg, ModType, Tag, Value};

use super::helpers::DMG_TYPE_LIST;
use super::{Off, run_skill_func};
use crate::setup::{cond, misc, mod_flag, ok, tag};

const LUA: &str = "Modules/CalcOffence.lua";

/// `unpack(mod)` -- the mod's own tag tables (shared objects, R4).
fn tags_of(m: &Mod) -> Vec<Tag> {
    m.tags.clone()
}

/// CalcTools.lua:258-273 `calcLib.getConvertedModTags(mod, multiplier, minionMods)`.
fn converted_tags(m: &Mod, multiplier: f64, minion_mods: bool) -> Vec<Tag> {
    m.tags
        .iter()
        .map(|t| {
            if minion_mods && t.get("type").is_str("ActorCondition") && t.get("actor").is_str("parent") {
                tag(&[("type", Value::str("Condition")), ("var", t.get("var"))])
            } else if t.get("limitTotal").truthy() {
                let mut tbl = (*t.table()).clone();
                let limit = tbl
                    .get("limit")
                    .as_num()
                    .unwrap_or_else(|| panic!("Modules/CalcTools.lua:266: attempt to perform arithmetic on field 'limit'"));
                tbl.hash.insert(Rc::from("limit"), Value::Num(limit * multiplier));
                Tag::new(tbl)
            } else {
                t.deep_copy()
            }
        })
        .collect()
}

/// `mod.value` in arithmetic.
fn mv(m: &Mod, line: u32) -> f64 {
    m.value.as_num().unwrap_or_else(|| {
        panic!(
            "{LUA}:{line}: attempt to perform arithmetic on field 'value' (a {} value)",
            m.value.type_name()
        )
    })
}

/// `cfg` with only `flags` / `keywordFlags` (a table literal).
fn flags_cfg(flags: u32, kw: Option<u32>) -> ModCfg {
    ModCfg {
        flags: Some(f64::from(flags)),
        keyword_flags: kw.map(f64::from),
        ..ModCfg::default()
    }
}

/// `env.configInput[key]` as a string (nil / non-string -> `None`).
fn config_str(off: &Off<'_>, key: &str) -> Option<String> {
    match off.env.config.input.get(key) {
        Some(pob_build::ConfigValue::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

/// `actor.itemList[slot] and .weaponData and .weaponData[1]` truthy.
fn has_weapon(off: &Off<'_>, slot: &str) -> bool {
    off.env
        .player
        .borrow()
        .item_list
        .get(slot)
        .and_then(|it| it.weapon_data.as_ref())
        .is_some_and(|wd| wd.contains_key(&pob_items::NumKey(1.0)))
}

/// `actor.weaponData<n>[key] or 0`.
fn wd(off: &Off<'_>, n: u8, key: &str) -> f64 {
    let p = off.env.player.borrow();
    let d = if n == 1 { &p.weapon_data1 } else { &p.weapon_data2 };
    match d.get(key) {
        Value::Nil | Value::Bool(false) => 0.0,
        v => v
            .as_num()
            .unwrap_or_else(|| panic!("{LUA}: attempt to perform arithmetic on field '{key}' (a {} value)", v.type_name())),
    }
}

/// `actor.weaponData<n>[key]` as a Lua value.
fn wd_val(off: &Off<'_>, n: u8, key: &str) -> Value {
    let p = off.env.player.borrow();
    if n == 1 { p.weapon_data1.get(key) } else { p.weapon_data2.get(key) }
}

/// :358-1055. `false` = the skill is disabled (:368-372, offence returns).
#[allow(clippy::too_many_lines)]
pub(super) fn setup(off: &mut Off<'_>) -> bool {
    let q = off.q.clone();
    // :362-366
    if off.sd_truthy("showAverage") {
        off.set_flag("showAverage", true);
    } else {
        off.set_flag("notAverage", true);
    }
    // :368-372
    if off.flag("disable") {
        off.out().set_num("CombinedDPS", 0.0);
        return false;
    }

    // :516
    run_skill_func(off, "initialFunc");

    // :518-525 `skillCfg.skillCond[...] = skillData[...]` (nil removes the key)
    for (k, d, f) in [
        ("SkillIsTriggered", "triggered", "triggered"),
        ("SkillIsFocused", "chanceToTriggerOnFocus", "focused"),
    ] {
        let v = off.sd(d);
        let cfg = off.sk_mut().skill_cfg.as_mut().expect("skillCfg");
        if v.is_nil() {
            cfg.skill_cond.shift_remove(k);
        } else {
            cfg.skill_cond.insert(k.to_owned(), v.truthy());
        }
        if v.truthy() {
            off.set_flag(f, true);
        }
    }
    off.refresh_cfg();
    let cfg = off.cfg.clone();

    // :528-534 update skill data
    for value in q.list(Some(&cfg), &["SkillData"]) {
        let key = value.get("key");
        let key = key
            .as_str()
            .unwrap_or_else(|| panic!("{LUA}:529: SkillData key is a {}", value.get("key").type_name()))
            .to_owned();
        if value.get("merge").is_str("MAX") {
            let cur = match off.sd(&key) {
                Value::Nil | Value::Bool(false) => 0.0,
                v => v
                    .as_num()
                    .unwrap_or_else(|| panic!("{LUA}:530: bad argument #2 to 'max' (number expected)")),
            };
            let new = value
                .get("value")
                .as_num()
                .unwrap_or_else(|| panic!("{LUA}:530: bad argument #1 to 'max' (number expected)"));
            off.set_sd(&key, Value::Num(max2(new, cur)));
        } else {
            off.set_sd(&key, value.get("value"));
        }
    }

    // :537-552 addition stat bonuses
    let str_bonus = off.st.player.str_dmg_bonus.unwrap_or(0.0);
    if q.flag(None, &["IronGrip"]) {
        q.nm(
            "PhysicalDamage",
            ModType::Inc,
            str_bonus,
            "Strength",
            mod_flag("Attack") | mod_flag("Projectile"),
            0,
            vec![],
        );
    }
    if q.flag(None, &["IronWill"]) {
        q.nm("Damage", ModType::Inc, str_bonus, "Strength", mod_flag("Spell"), 0, vec![]);
    }
    let transfig = misc("Transfiguration");
    if q.flag(None, &["TransfigurationOfBody"]) {
        let v = (q.inc(None, &["Life"]) * transfig).floor();
        q.nm("Damage", ModType::Inc, v, "Transfiguration of Body", mod_flag("Attack"), 0, vec![]);
    }
    if q.flag(None, &["TransfigurationOfMind"]) {
        let v = (q.inc(None, &["Mana"]) * transfig).floor();
        q.nm("Damage", ModType::Inc, v, "Transfiguration of Mind", 0, 0, vec![]);
    }
    if q.flag(None, &["TransfigurationOfSoul"]) {
        let v = (q.inc(None, &["EnergyShield"]) * transfig).floor();
        q.nm("Damage", ModType::Inc, v, "Transfiguration of Soul", mod_flag("Spell"), 0, vec![]);
    }
    // :554-558
    if ok(off.mod_db.borrow().flag(None, &["Elusive"])) == Some(true) && q.flag(None, &["SupportedByNightblade"]) {
        let elusive_effect = off.out_num("ElusiveEffectMod", 555) / 100.0;
        let nightblade_multi = q.base(None, &["NightbladeElusiveCritMultiplier"]);
        q.nm(
            "CritMultiplier",
            ModType::Base,
            (nightblade_multi * elusive_effect).floor(),
            "Nightblade",
            0,
            0,
            vec![],
        );
    }

    // :561-562
    let v = q.base(Some(&cfg), &["ActiveTrapLimit"]);
    off.out().set_num("ActiveTrapLimit", v);
    let v = q.base(Some(&cfg), &["ActiveMineLimit"]);
    off.out().set_num("ActiveMineLimit", v);
    // :565-566
    for k in ["LifeFlaskRecovery", "LifeFlaskCharges"] {
        let v = off.env.item_mod_db.borrow().multipliers.get(k).copied();
        off.set_out(k, v.map_or(Value::Nil, Value::Num));
    }

    // :568-583 Energy Blade
    if off.mod_db.borrow().conditions.get("AffectedByEnergyBlade").copied() == Some(true) {
        off.note(format!(
            "{LUA}:568-583 Energy Blade weapon data (data.itemBases weapon stats onto actor.weaponData1/2; not ported)"
        ));
    }

    // :587-594 Battlemage
    if q.flag(None, &["Battlemage"]) && has_weapon(off, "Weapon 1") {
        let multiplier = q.max(Some(&cfg), &["MainHandWeaponDamageAppliesToSpells"]).unwrap_or(100.0) / 100.0;
        for dt in DMG_TYPE_LIST {
            for mm in ["Min", "Max"] {
                let v = (wd(off, 1, &format!("{dt}{mm}")) * multiplier).floor();
                q.nm(&format!("{dt}{mm}"), ModType::Base, v, "Battlemage", mod_flag("Spell"), 0, vec![]);
            }
        }
    }
    // :595-596
    let rules = pob_data::rules::rules();
    let type_info = |n: u8| wd_val(off, n, "type").as_str().and_then(|t| rules.weapon_type_info.get(t));
    let weapon1info = type_info(1);
    let weapon2info = type_info(2);
    // :599-612 Spellblade
    let spellblade_multi = q.max(Some(&cfg), &["OneHandWeaponDamageAppliesToSpells"]);
    if let Some(sbm) = spellblade_multi
        && has_weapon(off, "Weapon 1")
    {
        let info = weapon1info.unwrap_or_else(|| panic!("{LUA}:600: attempt to index local 'weapon1info' (a nil value)"));
        if info.melee == Some(true) && info.one_hand == Some(true) {
            let multiplier = sbm / 100.0 * if weapon2info.is_some() { 0.6 } else { 1.0 };
            for dt in DMG_TYPE_LIST {
                for mm in ["Min", "Max"] {
                    let v = (wd(off, 1, &format!("{dt}{mm}")) * multiplier).floor();
                    q.nm(
                        &format!("{dt}{mm}"),
                        ModType::Base,
                        v,
                        "Spellblade Main Hand",
                        mod_flag("Spell"),
                        0,
                        vec![],
                    );
                }
            }
            if weapon2info.is_some() {
                for dt in DMG_TYPE_LIST {
                    for mm in ["Min", "Max"] {
                        let v = (wd(off, 2, &format!("{dt}{mm}")) * multiplier).floor();
                        q.nm(
                            &format!("{dt}{mm}"),
                            ModType::Base,
                            v,
                            "Spellblade Off Hand",
                            mod_flag("Spell"),
                            0,
                            vec![],
                        );
                    }
                }
            }
        }
    }
    // :613-643 minion stat conversions
    let minion_mods = |q: &super::Q| -> Vec<Rc<Mod>> {
        q.list(Some(&cfg), &["MinionModifier"])
            .into_iter()
            .map(|v| match v.get("mod") {
                Value::Mod(m) => m,
                o => panic!("{LUA}:617: attempt to index field 'mod' (a {} value)", o.type_name()),
            })
            .collect()
    };
    if q.flag(None, &["MinionDamageAppliesToPlayer"]) || q.flag(Some(&cfg), &["MinionDamageAppliesToPlayer"]) {
        let multiplier = q.max(Some(&cfg), &["ImprovedMinionDamageAppliesToPlayer"]).unwrap_or(100.0) / 100.0;
        for m in minion_mods(&q) {
            if &*m.name == "Damage" && m.ty == Some(ModType::Inc) {
                let modifiers = converted_tags(&m, multiplier, true);
                q.new_mod(
                    "Damage",
                    Some(ModType::Inc),
                    Value::Num(mv(&m, 620) * multiplier),
                    m.source.as_deref(),
                    m.flags,
                    m.keyword_flags,
                    modifiers,
                );
            }
        }
    }
    for (flag, improved, mflag, line) in [
        (
            "MinionAttackSpeedAppliesToPlayer",
            "ImprovedMinionAttackSpeedAppliesToPlayer",
            "Attack",
            630,
        ),
        (
            "MinionCastSpeedAppliesToPlayer",
            "ImprovedMinionCastSpeedAppliesToPlayer",
            "Cast",
            640,
        ),
    ] {
        if q.flag(None, &[flag]) {
            let multiplier = q.max(Some(&cfg), &[improved]).unwrap_or(100.0) / 100.0;
            for m in minion_mods(&q) {
                if &*m.name == "Speed" && m.ty == Some(ModType::Inc) && (m.flags == 0 || m.flags & mod_flag(mflag) != 0) {
                    let modifiers = converted_tags(&m, multiplier, true);
                    q.new_mod(
                        "Speed",
                        Some(ModType::Inc),
                        Value::Num(mv(&m, line) * multiplier),
                        m.source.as_deref(),
                        mod_flag(mflag),
                        m.keyword_flags,
                        modifiers,
                    );
                }
            }
        }
    }
    let empty = ModCfg::default();
    // :644-650 The Unblinking Eye
    if q.flag(None, &["EvasionAppliesToSpellDamage"]) {
        for e in q.tabulate(Some(ModType::Inc), Some(&empty), &["Evasion"]) {
            let m = &e.mod_;
            q.new_mod(
                "Damage",
                m.ty,
                m.value.clone(),
                m.source.as_deref(),
                mod_flag("Spell"),
                m.keyword_flags,
                tags_of(m),
            );
        }
    }
    // :651-665 spell damage -> attacks
    if q.flag(None, &["SpellDamageAppliesToAttacks"]) || q.flag(Some(&cfg), &["SpellDamageAppliesToAttacks"]) {
        let multiplier = q.max(Some(&cfg), &["ImprovedSpellDamageAppliesToAttacks"]).unwrap_or(100.0) / 100.0;
        for e in q.tabulate(Some(ModType::Inc), Some(&flags_cfg(mod_flag("Spell"), None)), &["Damage"]) {
            let m = &e.mod_;
            if m.flags & mod_flag("Spell") != 0 {
                let modifiers = converted_tags(m, multiplier, false);
                let flags = (m.flags & !mod_flag("Spell")) | mod_flag("Attack");
                q.new_mod(
                    "Damage",
                    Some(ModType::Inc),
                    Value::Num(mv(m, 658) * multiplier),
                    m.source.as_deref(),
                    flags,
                    m.keyword_flags,
                    modifiers,
                );
                if m.source.as_deref() == Some("Strength") {
                    q.replace_mod("PhysicalDamage", ModType::Inc, 0.0, "Strength", mod_flag("Melee"));
                    q.replace_mod(
                        "PhysicalDamage",
                        ModType::Inc,
                        0.0,
                        "Strength",
                        mod_flag("Attack") | mod_flag("Projectile"),
                    );
                }
            }
        }
    }
    // :666-678 cast speed -> attacks
    if q.flag(None, &["CastSpeedAppliesToAttacks"]) {
        let multiplier = q.max(Some(&cfg), &["ImprovedCastSpeedAppliesToAttacks"]).unwrap_or(100.0) / 100.0;
        for e in q.tabulate(Some(ModType::Inc), Some(&flags_cfg(mod_flag("Cast"), None)), &["Speed"]) {
            let m = &e.mod_;
            if m.flags & mod_flag("Cast") != 0 {
                let modifiers = converted_tags(m, multiplier, false);
                let flags = (m.flags & !mod_flag("Cast")) | mod_flag("Attack");
                q.new_mod(
                    "Speed",
                    Some(ModType::Inc),
                    Value::Num(mv(m, 675) * multiplier),
                    m.source.as_deref(),
                    flags,
                    m.keyword_flags,
                    modifiers,
                );
            }
        }
    }
    // :679-685 bow mastery
    if q.flag(None, &["ProjectileSpeedAppliesToBowDamage"]) {
        for e in q.tabulate(Some(ModType::Inc), Some(&empty), &["ProjectileSpeed"]) {
            let m = &e.mod_;
            q.new_mod(
                "Damage",
                m.ty,
                m.value.clone(),
                m.source.as_deref(),
                mod_flag("Bow") | mod_flag("Hit"),
                m.keyword_flags,
                tags_of(m),
            );
        }
    }
    // :686-712 Rigwald's Curse
    let claw_hit = mod_flag("Claw") | mod_flag("Hit");
    if q.flag(None, &["ClawDamageAppliesToUnarmed"]) {
        let c = flags_cfg(claw_hit, Some(crate::setup::keyword_flag("Hit")));
        for e in q.tabulate(Some(ModType::Inc), Some(&c), &["Damage"]) {
            let m = &e.mod_;
            if m.flags & mod_flag("Claw") != 0 {
                let flags = (m.flags & !mod_flag("Claw")) | mod_flag("Unarmed") | mod_flag("Melee");
                q.new_mod(
                    "Damage",
                    m.ty,
                    m.value.clone(),
                    m.source.as_deref(),
                    flags,
                    m.keyword_flags,
                    tags_of(m),
                );
            }
        }
    }
    if q.flag(None, &["ClawAttackSpeedAppliesToUnarmed"]) {
        let c = flags_cfg(claw_hit | mod_flag("Attack"), None);
        for e in q.tabulate(Some(ModType::Inc), Some(&c), &["Speed"]) {
            let m = &e.mod_;
            if m.flags & mod_flag("Claw") != 0 && m.flags & mod_flag("Attack") != 0 {
                let flags = (m.flags & !mod_flag("Claw")) | mod_flag("Unarmed");
                q.new_mod(
                    "Speed",
                    m.ty,
                    m.value.clone(),
                    m.source.as_deref(),
                    flags,
                    m.keyword_flags,
                    tags_of(m),
                );
            }
        }
    }
    if q.flag(None, &["ClawCritChanceAppliesToUnarmed"]) {
        for e in q.tabulate(Some(ModType::Inc), Some(&flags_cfg(claw_hit, None)), &["CritChance"]) {
            let m = &e.mod_;
            if m.flags & mod_flag("Claw") != 0 {
                let flags = (m.flags & !mod_flag("Claw")) | mod_flag("Unarmed");
                q.new_mod(
                    "CritChance",
                    m.ty,
                    m.value.clone(),
                    m.source.as_deref(),
                    flags,
                    m.keyword_flags,
                    tags_of(m),
                );
            }
        }
    }
    // :713-730 Law of the Wilds -- `env.minion.modDB` (minion actors are R7.10)
    for (flag, line) in [
        ("ClawCritChanceAppliesToMinions", "713-721"),
        ("ClawCritMultiplierAppliesToMinions", "722-730"),
    ] {
        if q.flag(None, &[flag]) {
            off.note(format!("{LUA}:{line} {flag}: env.minion.modDB (minion actors, R7.10)"));
        }
    }
    // :731-765 the first FLAG mod's source
    for (flag, name, ty, out_key) in [
        (
            "CritChanceIncreasedByUncappedLightningRes",
            "CritChance",
            ModType::Inc,
            "LightningResistTotal",
        ),
        ("CritChanceIncreasedByLightningRes", "CritChance", ModType::Inc, "LightningResist"),
        (
            "CritChanceIncreasedByOvercappedLightningRes",
            "CritChance",
            ModType::Inc,
            "LightningResistOverCap",
        ),
        (
            "CritChanceIncreasedBySpellSuppressChance",
            "CritChance",
            ModType::Inc,
            "SpellSuppressionChance",
        ),
        (
            "FirePenIncreasedByUncappedFireRes",
            "FirePenetration",
            ModType::Base,
            "FireResistOverCap",
        ),
    ] {
        if q.flag(None, &[flag]) {
            let first = ok(off.mod_db.borrow().tabulate(Some(ModType::Flag), None, &[flag]))
                .into_iter()
                .next();
            if let Some(e) = first {
                let v = off.out_val(out_key);
                q.new_mod(name, Some(ty), v, e.mod_.source.as_deref(), 0, 0, vec![]);
            }
        }
    }
    // :766-786 light radius
    for (flag, name, half) in [
        ("LightRadiusAppliesToAccuracy", "Accuracy", false),
        ("LightRadiusAppliesToAreaOfEffect", "AreaOfEffect", true),
        ("LightRadiusAppliesToDamage", "Damage", false),
    ] {
        if q.flag(None, &[flag]) {
            for e in q.tabulate(Some(ModType::Inc), Some(&empty), &["LightRadius"]) {
                let m = &e.mod_;
                let v = if half { (mv(m, 777) / 2.0).floor() } else { mv(m, 770) };
                q.new_mod(
                    name,
                    Some(ModType::Inc),
                    Value::Num(v),
                    m.source.as_deref(),
                    m.flags,
                    m.keyword_flags,
                    tags_of(m),
                );
            }
        }
    }
    // :787-795 Slavedriver's Hand
    if q.flag(None, &["CastSpeedAppliesToTrapThrowingSpeed"]) {
        for e in q.tabulate(Some(ModType::Inc), Some(&flags_cfg(mod_flag("Cast"), None)), &["Speed"]) {
            let m = &e.mod_;
            if m.flags == 0 || m.flags & mod_flag("Cast") != 0 {
                let flags = m.flags & !mod_flag("Cast") & !mod_flag("Attack");
                q.new_mod(
                    "TrapThrowingSpeed",
                    Some(ModType::Inc),
                    m.value.clone(),
                    m.source.as_deref(),
                    flags,
                    m.keyword_flags,
                    tags_of(m),
                );
            }
        }
    }
    // :796-802 Galvanic Arrow
    if off.sd_truthy("arrowSpeedAppliesToAreaOfEffect") {
        for e in q.tabulate(Some(ModType::Inc), Some(&flags_cfg(mod_flag("Bow"), None)), &["ProjectileSpeed"]) {
            let m = &e.mod_;
            q.new_mod(
                "AreaOfEffect",
                Some(ModType::Inc),
                m.value.clone(),
                m.source.as_deref(),
                m.flags,
                m.keyword_flags,
                tags_of(m),
            );
        }
    }
    // :803-806
    if q.flag(None, &["SequentialProjectiles"])
        && !q.flag(None, &["OneShotProj"])
        && !q.flag(None, &["NoAdditionalProjectiles"])
        && !q.flag(None, &["SingleProjectile"])
        && !q.flag(None, &["TriggeredBySnipe"])
    {
        let v = q.base(Some(&cfg), &["ProjectileCount"]);
        off.set_sd("dpsMultiplier", Value::Num(v));
    }

    // :808-904 repeats
    repeats(off, &q, &cfg);

    // :905-912 Runegraft of the Spellbound
    if q.flag(None, &["WeaponPhysAppliesToSpells"]) {
        let mult = q.base(Some(&cfg), &["WeaponPhysAppliesToSpellsPercent"]) / 100.0;
        for k in ["PhysicalMin", "PhysicalMax"] {
            let v = (wd(off, 1, k) * mult).floor();
            q.nm(k, ModType::Base, v, "Runegraft of the Spellbound", mod_flag("Spell"), 0, vec![]);
        }
    }
    // :913-931 Spellslinger
    if let Some(pct) = off.sd_num("gainPercentBaseWandDamageToSpells", 914) {
        let mult = pct / 100.0;
        let (w1, w2) = (wd_val(off, 1, "type").is_str("Wand"), wd_val(off, 2, "type").is_str("Wand"));
        for dt in DMG_TYPE_LIST {
            for mm in ["Min", "Max"] {
                let k = format!("{dt}{mm}");
                let v = if w1 && w2 {
                    Some(((wd(off, 1, &k) + wd(off, 2, &k)) / 2.0 * mult).floor())
                } else if w1 {
                    Some((wd(off, 1, &k) * mult).floor())
                } else if w2 {
                    Some((wd(off, 2, &k) * mult).floor())
                } else {
                    None
                };
                if let Some(v) = v {
                    q.nm(&k, ModType::Base, v, "Spellslinger", mod_flag("Spell"), 0, vec![]);
                }
            }
        }
    }
    // :932-942 Blade Blast of Dagger Detonation
    if let Some(pct) = off.sd_num("gainPercentBaseDaggerDamageToSpells", 935) {
        let is_dagger = |n: u8| wd_val(off, n, "AddedUsingDagger").truthy() || wd_val(off, n, "type").is_str("Dagger");
        let (d1, d2) = (is_dagger(1), is_dagger(2));
        let mult = pct / 100.0 * if d1 && d2 { 0.5 } else { 1.0 };
        for dt in DMG_TYPE_LIST {
            for mm in ["Min", "Max"] {
                let k = format!("{dt}{mm}");
                let base = (if d1 { wd(off, 1, &k) } else { 0.0 }) + (if d2 { wd(off, 2, &k) } else { 0.0 });
                q.nm(
                    &k,
                    ModType::Base,
                    (base * mult).floor(),
                    "Blade Blast of Dagger Detonation",
                    mod_flag("Spell"),
                    0,
                    vec![],
                );
            }
        }
    }

    // :944-978 seals
    if q.flag(None, &["HasSeals"]) && off.has_type("CanRapidFire") && !q.flag(None, &["NoRepeatBonuses"]) {
        let seal_cooldown = q.base(Some(&cfg), &["SealGainFrequency"]) / q.calc_mod(Some(&cfg), &["SealGainFrequency"]);
        let seal_max = q.base(Some(&cfg), &["SealCount"]);
        let o = off.out();
        o.set_num("SealCooldown", seal_cooldown);
        o.set_num("SealMax", seal_max);
        o.set_num("AverageBurstHits", seal_max);
        o.set_num("TimeMaxSeals", seal_cooldown * seal_max);
        if !off.sd_truthy("hitTimeOverride") {
            let cast_time = {
                let st = off.env.skills.as_ref().expect("skills on");
                st.skills.effects[off.sk().active_effect.borrow().granted_effect]
                    .cast_time
                    .unwrap_or_else(|| panic!("{LUA}:962: attempt to perform arithmetic on field 'castTime' (a nil value)"))
            };
            let action = off.out_num("ActionSpeedMod", 962);
            let rate = 1.0 / cast_time * 1.1 * q.calc_mod(Some(&cfg), &["Speed"]) * action;
            if q.flag(None, &["UseMaxUnleash"]) {
                for e in q.tabulate(Some(ModType::Inc), Some(&empty), &["MaxSealCrit"]) {
                    let m = &e.mod_;
                    q.new_mod(
                        "CritChance",
                        Some(ModType::Inc),
                        m.value.clone(),
                        m.source.as_deref(),
                        m.flags,
                        m.keyword_flags,
                        tags_of(m),
                    );
                }
                for e in q.tabulate(Some(ModType::More), Some(&empty), &["MaxSealDamage"]) {
                    let m = &e.mod_;
                    q.new_mod(
                        "Damage",
                        Some(ModType::More),
                        Value::Num(mv(m, 959) * seal_max),
                        m.source.as_deref(),
                        m.flags,
                        m.keyword_flags,
                        tags_of(m),
                    );
                }
                let dps = 1.0 + seal_max * q.calc_mod(Some(&cfg), &["SealRepeatPenalty"]);
                off.set_sd("dpsMultiplier", Value::Num(dps));
                off.set_sd("hitTimeOverride", Value::Num(max2(seal_cooldown * seal_max, rate)));
            } else {
                let dps = 1.0 + 1.0 / seal_cooldown / rate * q.calc_mod(Some(&cfg), &["SealRepeatPenalty"]);
                off.set_sd("dpsMultiplier", Value::Num(dps));
            }
        }
    }

    // :979-1036 random phys
    let phys_mode = config_str(off, "physMode").unwrap_or_else(|| "AVERAGE".to_owned());
    let mut processed: Vec<Rc<Mod>> = Vec::new();
    let cfgs: Vec<ModCfg> = {
        // `{ skillCfg, activeSkill.weapon1Cfg, activeSkill.weapon2Cfg }` under `ipairs`: stops at the first nil.
        let mut v = vec![cfg.clone()];
        if let Some(w1) = off.weapon_cfg(1) {
            v.push(w1);
            if let Some(w2) = off.weapon_cfg(2) {
                v.push(w2);
            }
        }
        v
    };
    for c in &cfgs {
        if q.base(
            Some(c),
            &[
                "PhysicalDamageGainAsRandom",
                "PhysicalDamageConvertToRandom",
                "PhysicalDamageGainAsColdOrLightning",
            ],
        ) > 0.0
        {
            off.set_flag("randomPhys", true);
            for (src, kind, div) in [
                ("PhysicalDamageGainAsRandom", "GainAs", 3.0),
                ("PhysicalDamageConvertToRandom", "ConvertTo", 3.0),
                ("PhysicalDamageGainAsColdOrLightning", "GainAs", 2.0),
            ] {
                for e in q.tabulate(Some(ModType::Base), Some(c), &[src]) {
                    let m = e.mod_.clone();
                    if processed.iter().any(|p| Rc::ptr_eq(p, &m)) {
                        continue;
                    }
                    processed.push(m.clone());
                    let eff = mv(&m, 988) / div;
                    let full = mv(&m, 994);
                    let targets: Vec<(&str, f64)> = if div == 3.0 {
                        match phys_mode.as_str() {
                            "AVERAGE" => vec![("Fire", eff), ("Cold", eff), ("Lightning", eff)],
                            "FIRE" => vec![("Fire", full)],
                            "COLD" => vec![("Cold", full)],
                            "LIGHTNING" => vec![("Lightning", full)],
                            _ => vec![],
                        }
                    } else {
                        match phys_mode.as_str() {
                            "AVERAGE" | "FIRE" => vec![("Cold", eff), ("Lightning", eff)],
                            "COLD" => vec![("Cold", full)],
                            "LIGHTNING" => vec![("Lightning", full)],
                            _ => vec![],
                        }
                    };
                    for (dst, v) in targets {
                        q.new_mod(
                            &format!("PhysicalDamage{kind}{dst}"),
                            Some(ModType::Base),
                            Value::Num(v),
                            m.source.as_deref(),
                            m.flags,
                            m.keyword_flags,
                            tags_of(&m),
                        );
                    }
                }
            }
        }
    }
    // :1038-1050 momentum
    if q.flag(None, &["SupportedByMomentum"]) {
        let max_stacks = q.base(Some(&cfg), &["MomentumStacksMax"]);
        let extra_stacks = q.base(Some(&cfg), &["MomentumStacksExtra"]);
        let db = off.mod_db.clone();
        let has = ok(db.borrow().has_mod(ModType::Base, None, &["Multiplier:MomentumStacks"]));
        let combat = || cond("Combat");
        if max_stacks > 0.0 {
            if !has {
                let v = min2((max_stacks + extra_stacks) / 2.0, max_stacks);
                db.borrow_mut().new_mod(pob_mods::create_mod(
                    "Multiplier:MomentumStacks",
                    Some(ModType::Base),
                    Value::Num(v),
                    Some("Config"),
                    0,
                    0,
                    vec![combat()],
                ));
            } else if ok(db.borrow().sum(ModType::Base, None, &["Multiplier:MomentumStacks"])) > max_stacks {
                db.borrow_mut().replace_mod(pob_mods::create_mod(
                    "Multiplier:MomentumStacks",
                    Some(ModType::Base),
                    Value::Num(max_stacks),
                    Some("Config"),
                    0,
                    0,
                    vec![combat()],
                ));
            }
        } else if has {
            db.borrow_mut().replace_mod(pob_mods::create_mod(
                "Multiplier:MomentumStacks",
                Some(ModType::Base),
                Value::Num(0.0),
                Some("Config"),
                0,
                0,
                vec![],
            ));
        }
    }

    // :1054
    run_skill_func(off, "preSkillTypeFunc");
    true
}

/// :808-904 -- `output.Repeats` and the repeat multipliers.
#[allow(clippy::too_many_lines)]
fn repeats(off: &mut Off<'_>, q: &super::Q, cfg: &ModCfg) {
    // :808-816 `repeatSkillTypesCheck(activeSkill.skillTypes)`
    let excluded = ["Instant", "Channel", "Triggered", "Retaliation", "NonRepeatable"]
        .iter()
        .any(|t| off.has_type(t));
    let check = !excluded && !q.flag(None, &["CannotRepeat"]) && (off.has_type("Attack") || off.has_type("Spell"));
    let repeats = 1.0 + if check { q.base(Some(cfg), &["RepeatCount"]) } else { 0.0 };
    off.out().set_num("Repeats", repeats);
    if repeats <= 1.0 {
        return;
    }
    off.out().set_num("RepeatCount", repeats);
    let mode = config_str(off, "repeatMode");
    // `env.configInput.repeatMode ~= "NONE"` (nil ~= "NONE" is true)
    if mode.as_deref() == Some("NONE") {
        return;
    }
    let average = mode.as_deref() == Some("AVERAGE");
    let add = |name: &str, ty: ModType, v: f64, m: &Mod| {
        q.new_mod(
            name,
            Some(ty),
            Value::Num(v),
            m.source.as_deref(),
            m.flags,
            m.keyword_flags,
            tags_of(m),
        );
    };
    // :822-829
    for e in q.tabulate(Some(ModType::Inc), Some(cfg), &["RepeatFinalAreaOfEffect"]) {
        let mut v = mv(&e.mod_, 824);
        if average {
            v /= repeats;
        }
        add("AreaOfEffect", ModType::Inc, v, &e.mod_);
    }
    // :830-837
    for e in q.tabulate(Some(ModType::Inc), Some(cfg), &["RepeatPerRepeatAreaOfEffect"]) {
        let mut v = mv(&e.mod_, 832) * (repeats - 1.0);
        if average {
            v /= 2.0;
        }
        add("AreaOfEffect", ModType::Inc, v, &e.mod_);
    }
    // :838-845
    for e in q.tabulate(Some(ModType::Base), Some(cfg), &["RepeatFinalDoubleDamageChance"]) {
        let mut v = mv(&e.mod_, 840);
        if average {
            v /= repeats;
        }
        add("DoubleDamageChance", ModType::Base, v, &e.mod_);
    }
    // :846-857
    let mut final_more_total = 1.0;
    let mut more_total = 0.0;
    for e in q.tabulate(Some(ModType::More), Some(cfg), &["RepeatFinalDamage"]) {
        let mut v = mv(&e.mod_, 850);
        final_more_total *= 1.0 + v / 100.0;
        more_total += v;
        if average && !q.flag(None, &["OnlyFinalRepeat"]) {
            v /= repeats;
        }
        add("Damage", ModType::More, v, &e.mod_);
    }
    // :858-872
    for e in q.tabulate(Some(ModType::More), Some(cfg), &["RepeatPerRepeatDamage"]) {
        let mvv = mv(&e.mod_, 860);
        let mut v = mvv * (repeats - 1.0);
        if average {
            if final_more_total != 1.0 {
                v = ((100.0 + mvv * (repeats - 2.0) / 2.0) * (repeats - 1.0) + (100.0 + mvv * (repeats - 1.0)) * final_more_total)
                    / (repeats + more_total / 100.0)
                    - 100.0;
            } else {
                v /= 2.0;
            }
        }
        add("Damage", ModType::More, v, &e.mod_);
    }
    // :874-894
    let mut last_mod: Option<Rc<Mod>> = None;
    let final_more_total = more_total;
    for (n, word) in [(2.0, "One"), (3.0, "Two"), (4.0, "Three")] {
        if n > repeats {
            break;
        } else if average {
            for e in q.tabulate(Some(ModType::More), Some(cfg), &[&format!("Repeat{word}Damage")]) {
                more_total += mv(&e.mod_, 881);
                last_mod = Some(e.mod_.clone());
            }
        } else if n == repeats {
            for e in q.tabulate(Some(ModType::More), Some(cfg), &[&format!("Repeat{word}Damage")]) {
                add("Damage", ModType::More, mv(&e.mod_, 886), &e.mod_);
            }
        }
    }
    if average && let Some(m) = &last_mod {
        let v = (more_total / repeats + 100.0) / (1.0 + final_more_total / repeats / 100.0) - 100.0;
        add("Damage", ModType::More, v, m);
    }
    // :895-899
    if q.flag(None, &["FinalRepeatSumsDamage"]) {
        for e in q.tabulate(Some(ModType::Flag), Some(cfg), &["FinalRepeatSumsDamage"]) {
            let v = (100.0 * repeats + final_more_total) / (1.0 + final_more_total / 100.0) - 100.0;
            add("Damage", ModType::More, v, &e.mod_);
        }
    }
    // :900-902
    if off.flag("trap") || off.flag("mine") {
        q.nm("DPS", ModType::More, (repeats - 1.0) * 100.0, "Repeat Count", 0, 0, vec![]);
    }
}
