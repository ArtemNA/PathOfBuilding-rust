//! CalcTriggers.lua:136-221 `helmetFocusHandler`, :222-392 `CWCHandler`, :394-900 `defaultTriggerHandler`. Breakdown tables are R8; the
//! `if breakdown` branches whose evaluation is a Lua error on nil in MAIN (`s_format("%.2f", nil)`, arithmetic on nil) raise it here.

use pob_lua::number::{max2, min2};
use pob_mods::{ModType, Value};

use super::config::Config;
use super::{
    Found, SimSkill, add_trigger_inc_more_mods, cached_out, cached_sd, calc_multi_spell_rotation_impact, can_support, ceil_b, cfg_of,
    config_input_is, ensure_cached, find_trigger_skill, flag, from_item, has_type, is_triggered, line_err, main_idx, name_of, package,
    player_db, process_added_cast_time, q_of, sd, sd_num, sd_truthy, set_flag, set_sd, skill, skill_count, skill_mut, slot_match,
    triggered_by_cd, with_cached,
};
use crate::env::Env;
use crate::perform::{PerformState, Who};
use crate::session::cache_skill_uuid;
use crate::setup::misc;

fn tick_rate() -> f64 {
    misc("ServerTickRate")
}

/// `output[k] = v` for an optional number (nil removes the key).
fn set_out(st: &mut PerformState, k: &str, v: Option<f64>) {
    match v {
        Some(x) => st.player.output.set_num(k, x),
        None => st.player.output.remove(k),
    }
}

fn out_num(st: &PerformState, k: &str, line: u32) -> f64 {
    st.player
        .output
        .opt_num(k)
        .unwrap_or_else(|| line_err(line, &format!("attempt to perform arithmetic on field '{k}' (a nil value)")))
}

/// `actor.modDB:Flag(nil, "HaveTriggerBots") and actor.mainSkill.skillTypes[SkillType.Spell]`.
fn trigger_bots(env: &Env, who: Who, m: usize) -> bool {
    let db = crate::offence::Q {
        list: crate::perform::db_of(env, who),
        malformed: Vec::new(),
    };
    db.flag(None, &["HaveTriggerBots"]) && has_type(env, m, "Spell")
}

/// :136-220 `helmetFocusHandler(env)`.
pub(super) fn helmet_focus_handler(env: &mut Env, st: &mut PerformState) -> Result<(), String> {
    let m = main_idx(env);
    if flag(env, m, "minion") || flag(env, m, "disable") || skill(env, m).triggered_by.is_none() {
        return Ok(());
    }
    set_sd(env, m, "triggered", Value::Bool(true));
    let trigger_cd = triggered_by_cd(env, m, 145);
    let triggered_cd = sd_num(env, m, "cooldown", 146);
    let q = q_of(env, m);
    let cfg = cfg_of(env, m);
    let icdr_focus = q.calc_mod(Some(&cfg), &["FocusCooldownRecovery"]);
    let icdr_skill = q.calc_mod(Some(&cfg), &["CooldownRecovery"]);
    // :154-158 `env.data.skills["Focus"]`
    let (focus_duration, focus_base_cd) = {
        let s = env.skills.as_ref().expect("skills on");
        let f = s
            .skills
            .get("Focus")
            .unwrap_or_else(|| line_err(155, "attempt to index local 'skillFocus' (a nil value)"));
        let c = f
            .constant_stats
            .as_ref()
            .and_then(|c| c.first())
            .unwrap_or_else(|| line_err(155, "attempt to index field '?' (a nil value)"))
            .1;
        let cd = f
            .level(1.0)
            .and_then(|l| l.cooldown)
            .unwrap_or_else(|| line_err(156, "attempt to perform arithmetic on field 'cooldown' (a nil value)"));
        (c / 1000.0, cd)
    };
    let focus_cd = focus_base_cd / icdr_focus;
    let focus_total_cd = focus_duration + focus_cd;
    // :161-166
    let mod_action_cooldown = max2(triggered_cd.unwrap_or(0.0), trigger_cd.unwrap_or(0.0) / icdr_skill);
    let rate_cap_adjusted = (mod_action_cooldown * tick_rate()).ceil() / tick_rate();
    let mut trigger_rate = f64::INFINITY;
    if rate_cap_adjusted != 0.0 {
        trigger_rate = 1.0 / rate_cap_adjusted;
    }
    st.player.output.set_num("TriggerRateCap", trigger_rate);
    st.player.output.set_num("SkillTriggerRate", 1.0 / focus_total_cd);
    // :171-209 breakdown (R8): `triggerCD / icdrSkill` and `%.2f` of `triggerCD` raise on nil
    if env.build_breakdown && trigger_cd.is_none() {
        line_err(
            if triggered_cd.is_some() { 180 } else { 195 },
            "attempt to perform arithmetic on local 'triggerCD' (a nil value)",
        );
    }
    add_trigger_inc_more_mods(env, m, m);
    let sk = skill_mut(env, m);
    sk.info_message = Some("Assuming perfect focus Re-Use".into());
    sk.info_trigger = Some("Focus".into());
    let rate = out_num(st, "SkillTriggerRate", 217);
    set_sd(env, m, "triggerRate", Value::Num(rate));
    set_flag(env, m, "globalTrigger");
    Ok(())
}

/// :222-392 `CWCHandler(env)`.
#[allow(clippy::too_many_lines)]
pub(super) fn cwc_handler(env: &mut Env, st: &mut PerformState) -> Result<(), String> {
    let m = main_idx(env);
    if flag(env, m, "minion") || flag(env, m, "disable") {
        return Ok(());
    }
    let trigger_name = "Cast While Channeling";
    let mut triggered: Vec<SimSkill> = Vec::new();
    let mut source: Option<usize> = None;
    // `env.player.mainSkill.triggeredBy.gemData` (a nil triggeredBy is the Lua error)
    let tb_gem_ge = {
        let s = env.skills.as_ref().expect("skills on");
        let tb = s.active_skill_list[m]
            .triggered_by
            .as_ref()
            .unwrap_or_else(|| line_err(232, "attempt to index field 'triggeredBy' (a nil value)"));
        tb.borrow().gem_data.clone().map(|g| {
            let id = &pob_data::gems::gems().gems[&g].granted_effect_id;
            s.skills
                .get(id)
                .unwrap_or_else(|| line_err(232, "attempt to index field 'grantedEffect' (a nil value)"))
                .index
        })
    };
    for i in 0..skill_count(env) {
        let sm = slot_match(env, i);
        if source.is_none() {
            let can = tb_gem_ge.is_some_and(|g| can_support(env, g, i));
            if sd_truthy(env, i, "triggerTime") && can && i != m && sm && !is_triggered(env, i) {
                source = Some(i);
            }
        }
        if sd_truthy(env, i, "triggeredWhileChannelling") && sm {
            triggered.push(package(env, i));
        }
    }
    let Some(source) = source.filter(|_| !triggered.is_empty()) else {
        set_sd(env, m, "triggered", Value::Nil);
        let sk = skill_mut(env, m);
        sk.info_message2 = Some("DPS reported assuming Self-Cast".into());
        sk.info_message = Some(format!("No {trigger_name} Triggering Skill Found"));
        sk.info_trigger = Some(String::new());
        return Ok(());
    };
    // :258
    let adds = process_added_cast_time(env, m).map(|(a, _)| a);
    set_out(st, "addsCastTime", adds);
    let q = q_of(env, m);
    let cfg = cfg_of(env, m);
    let icdr = q.calc_mod(Some(&cfg), &["CooldownRecovery"]);
    let trigger_interval = sd_num(env, source, "triggerTime", 261)
        .unwrap_or_else(|| line_err(262, "attempt to perform arithmetic on local 'triggerInterval' (a nil value)"));
    let trigger_rate_of_trigger = 1.0 / trigger_interval;
    let triggered_cd = sd_num(env, m, "cooldown", 263);
    let cooldown_override = q.override_num(Some(&cfg), &["CooldownRecovery"], "Modules/CalcTriggers.lua:264");
    if cooldown_override.is_some() {
        set_flag(env, m, "hasOverride");
    }
    // :270-272
    let triggered_total_cooldown = cooldown_override.unwrap_or_else(|| max2(triggered_cd.unwrap_or(0.0), adds.unwrap_or(0.0)) / icdr);
    let triggered_cd_adjusted = (triggered_total_cooldown * tick_rate()).ceil() / tick_rate();
    let eff_cd_triggered_skill = (triggered_cd_adjusted * trigger_rate_of_trigger).ceil() / trigger_rate_of_trigger;
    // :274-280
    st.player
        .output
        .set_num("TriggerRateCap", min2(1.0 / eff_cd_triggered_skill, trigger_rate_of_trigger));
    let main_uuid = cache_skill_uuid(env, m);
    let (mut str_, _) = calc_multi_spell_rotation_impact(&main_uuid, &mut triggered, trigger_rate_of_trigger, Some(0.0), None);
    if trigger_bots(env, Who::Player, m) {
        str_ *= 2.0;
    }
    st.player.output.set_num("SkillTriggerRate", str_);
    // :282-377 breakdown (R8): `s_format("%.2f", triggeredCD)` / `triggeredCD / icdr` raise when only the override is set
    if env.build_breakdown && triggered_cd.is_none() && cooldown_override.is_some() {
        line_err(287, "bad argument #2 to 'format' (number expected, got nil)");
    }
    add_trigger_inc_more_mods(env, m, m);
    st.player.output.set_num("ChannelTimeToTrigger", trigger_interval);
    set_sd(env, m, "triggered", Value::Bool(true));
    set_flag(env, m, "globalTrigger");
    set_sd(env, m, "triggerRate", Value::Num(str_));
    let src_uuid = cache_skill_uuid(env, source);
    set_sd(env, m, "triggerSourceUUID", Value::str(&src_uuid));
    let src_name = name_of(env, source);
    skill_mut(env, m).info_message = Some(format!("{trigger_name}'s Trigger: {src_name}"));
    // :390 `env.player.infoTrigger = ...` -- a field of the player actor no module reads
    Ok(())
}

/// The cached run's `HitChance` / `CritChance` path of :338-387 for a source that hits with both weapons: `output.MainHand/OffHand` of
/// a cached run are kept by offence only while it runs (R7.4) -- a boundary.
fn both_hands_boundary(line: u32) -> String {
    format!(
        "CalcTriggers.lua:{line} defaultTriggerHandler: GlobalCache...Env.player.output.MainHand/OffHand of a doubleHitsWhenDualWielding source (the hand tables are not kept on a cached run)"
    )
}

/// :394-900 `defaultTriggerHandler(env, config)`; `who` = `config.actor`, `m` = `actor.mainSkill` (R7.10b: `st.player` is that actor's perform
/// state, see [`crate::offence::Off`]). `env.player.mainSkill` is [`main_idx`].
#[allow(clippy::too_many_lines)]
pub(super) fn default_trigger_handler(env: &mut Env, st: &mut PerformState, mut config: Config, who: Who, m: usize) -> Result<(), String> {
    let pm = main_idx(env);
    let mut source = config.source;
    let mut triggered_skills = config.triggered_skills.take().unwrap_or_default();
    let mut trig_rate = config.trig_rate;
    let mut uuid: Option<String> = None;
    // :404-405
    let source_weapon_flag = if config.source_weapon {
        let slot = super::group_slot(env, m, 405);
        slot.map(|s| {
            if s.as_bytes().starts_with(b"Weapon 2") {
                "weapon2Attack"
            } else {
                "weapon1Attack"
            }
        })
    } else {
        None
    };
    // :408-418
    if config.triggered_skill_cond.is_some() || config.trigger_skill_cond.is_some() {
        for i in 0..skill_count(env) {
            if let Some(c) = &config.trigger_skill_cond
                && c(env, i)
                && source_weapon_flag.is_none_or(|f| flag(env, i, f))
                && (!is_triggered(env, i) || flag(env, m, "globalTrigger"))
                && i != m
            {
                let f = find_trigger_skill(
                    env,
                    i,
                    &Found {
                        source,
                        rate: trig_rate,
                        uuid: uuid.clone(),
                    },
                    config.comparer.as_ref(),
                )?;
                source = f.source;
                trig_rate = f.rate;
                uuid = f.uuid;
            }
            if let Some(c) = &config.triggered_skill_cond
                && c(env, i)
            {
                triggered_skills.push(package(env, i));
            }
        }
    }
    // :419
    if triggered_skills.is_empty() && config.triggered_skill_cond.is_some() {
        return Ok(());
    }
    // :420-425
    if source.is_none() && !(flag(env, m, "globalTrigger") && config.triggered_skill_cond.is_some()) {
        set_sd(env, m, "triggered", Value::Nil);
        let tn = config.trigger_name.clone().unwrap_or_default();
        let sk = skill_mut(env, m);
        sk.info_message2 = Some("DPS reported assuming Self-Cast".into());
        sk.info_message = Some(format!("No {tn} Triggering Skill Found"));
        sk.info_trigger = Some(String::new());
        return Ok(());
    }
    set_sd(env, m, "triggered", Value::Bool(true));
    // :428-445 breakdown only (R8)

    // :447-55 dual wield triggers
    let dual_wield = {
        let p = env.player.borrow();
        p.weapon_data1.get("type").truthy() && p.weapon_data2.get("type").truthy()
    };
    let source_weapon_trigger = config.source_weapon && source.is_some_and(|s| flag(env, s, "bothWeaponAttack"));
    let item_support_trigger = super::triggered_by_ge(env, m).is_some_and(|g| {
        let ge = &env.skills.as_ref().expect("skills on").skills.effects[g];
        ge.support && from_item(env, ge)
    });
    if let (Some(r), Some(s)) = (trig_rate, source)
        && dual_wield
        && !sd_truthy(env, s, "doubleHitsWhenDualWielding")
        && (has_type(env, s, "Melee") || has_type(env, s, "Attack"))
        && (source_weapon_trigger || item_support_trigger)
    {
        trig_rate = Some(r / 2.0);
    }
    // :457 `ignoresTickRate = ignoresTickRate or (storedUses and storedUses > 1)`
    if !sd_truthy(env, m, "ignoresTickRate") {
        let v = match sd(env, m, "storedUses") {
            Value::Num(n) => Value::Bool(n > 1.0),
            v if !v.truthy() => v,
            v => line_err(457, &format!("attempt to compare number with {}", v.type_name())),
        };
        set_sd(env, m, "ignoresTickRate", v);
    }
    let req_rate =
        |r: Option<f64>, line: u32| r.unwrap_or_else(|| line_err(line, "attempt to perform arithmetic on local 'trigRate' (a nil value)"));
    let req_source = |s: Option<usize>, line: u32| s.unwrap_or_else(|| line_err(line, "attempt to index local 'source' (a nil value)"));
    // :459-468 source unleash
    if let Some(s) = source
        && let Some(mult) = with_cached(env, uuid.as_deref(), 460, |c| cached_sd(c, "dpsMultiplier"))?
        && q_of(env, s).flag(None, &["HasSeals"])
        && has_type(env, s, "CanRapidFire")
    {
        let mult = if mult.truthy() {
            mult.as_num()
                .unwrap_or_else(|| line_err(462, "attempt to perform arithmetic on a non-number"))
        } else {
            1.0
        };
        trig_rate = Some(req_rate(trig_rate, 462) * mult);
        set_flag(env, m, "HasSeals");
        set_sd(env, m, "ignoresTickRate", Value::Bool(true));
    }
    // :470-477 multiple hits per use
    if let Some(s) = source
        && let Some(pc) = with_cached(env, uuid.as_deref(), 471, |c| cached_out(c, "ProjectileCount"))?
    {
        let part_all = skill(env, s)
            .skill_part_name
            .as_deref()
            .is_some_and(|n| matches!(pob_lua::find(n.as_bytes(), b"(.*)All(.*)Projectiles(.*)", None, false), Ok(Some(_))));
        if part_all && flag(env, s, "projectile") {
            trig_rate = Some(req_rate(trig_rate, 473) * pc.unwrap_or(1.0));
        }
    }
    // :479-487 Kitava's Thirst: repeats do not spend mana
    if sd_truthy(env, m, "triggeredByManaSpent") {
        let s = req_source(source, 482);
        let repeats = 1.0 + q_of(env, s).base(None, &["RepeatCount"]);
        trig_rate = Some(req_rate(trig_rate, 483) / repeats);
    }
    let tick_time = misc("ServerTickTime");
    // :489-106 Battlemage's Cry uptime / :108-125 Infernal Cry uptime
    for (cond_bm, prefix, exerts) in [
        (true, "BattleMageCry", "BattleCryExertsCount"),
        (false, "InfernalCry", "InfernalExertsCount"),
    ] {
        let applies = if cond_bm {
            sd_truthy(env, m, "triggeredByBattleMageCry")
        } else {
            name_of(env, m) == "Combust"
        };
        if !applies {
            continue;
        }
        let Some(s) = source else { continue };
        if !has_type(env, s, "Melee") {
            continue;
        }
        let vals = with_cached(env, uuid.as_deref(), 491, |c| {
            (
                cached_out(c, exerts),
                cached_out(c, &format!("{prefix}Duration")),
                cached_out(c, &format!("{prefix}CastTime")),
                cached_out(c, &format!("{prefix}Cooldown")),
            )
        })?;
        let Some((count, dur, cast, cd)) = vals else { continue };
        let need = |v: Option<f64>| v.unwrap_or_else(|| line_err(492, "attempt to perform arithmetic on a nil value"));
        let duration = ceil_b(need(dur), tick_time);
        let cooldown = ceil_b(need(cd), tick_time);
        let hits = max2(min2(req_rate(trig_rate, 497) * duration, need(count)), 0.0);
        trig_rate = Some(hits / (need(cast) + cooldown));
    }
    // :127-162 Manaforged Arrows
    if sd_truthy(env, m, "triggeredByManaforged") && req_rate(trig_rate, 528) > 0.0 {
        let tuuid = cache_skill_uuid(env, m);
        if !super::session(env, 530)?
            .cache()
            .contains(crate::session::CalcMode::of(env), &tuuid)
        {
            ensure_cached(env, m, &tuuid, std::slice::from_ref(&tuuid), false, 531)?;
        }
        let triggered_mana = with_cached(env, Some(&tuuid), 533, |c| cached_out(c, "ManaCostRaw"))?
            .unwrap_or_else(|| line_err(533, "attempt to index field '?' (a nil value)"))
            .unwrap_or(0.0);
        if triggered_mana > 0.0 {
            let threshold = triggered_mana
                * sd_num(env, m, "ManaForgedArrowsPercentThreshold", 535).unwrap_or_else(|| {
                    line_err(
                        535,
                        "attempt to perform arithmetic on field 'ManaForgedArrowsPercentThreshold' (a nil value)",
                    )
                });
            let source_mana = with_cached(env, uuid.as_deref(), 536, |c| cached_out(c, "ManaCostRaw"))?
                .unwrap_or_else(|| line_err(536, "attempt to index field '?' (a nil value)"))
                .unwrap_or(0.0);
            if source_mana > 0.0 {
                trig_rate = Some(req_rate(trig_rate, 554) / (threshold / source_mana).ceil());
            } else {
                trig_rate = Some(0.0);
            }
        }
    }
    // :164-187
    let q = q_of(env, m);
    let cfg = cfg_of(env, m);
    let icdr = q.calc_mod(Some(&cfg), &["CooldownRecovery"]);
    let added = q.base(Some(&cfg), &["CooldownRecovery"]);
    let added_cooldown = (added != 0.0).then_some(added);
    let cooldown_override = q.override_num(Some(&cfg), &["CooldownRecovery"], "Modules/CalcTriggers.lua:567");
    // :567 `actor.mainSkill.triggeredBy and env.player.mainSkill.triggeredBy.grantedEffect.levels[...].cooldown`
    let mut trigger_cd = if skill(env, m).triggered_by.is_some() {
        if skill(env, pm).triggered_by.is_none() {
            line_err(567, "attempt to index field 'triggeredBy' (a nil value)");
        }
        triggered_by_cd(env, pm, 568)
    } else {
        None
    };
    if trigger_cd.is_none() {
        let s = req_source(source, 569);
        trigger_cd = triggered_by_cd(env, s, 569);
    }
    let triggered_cd = sd_num(env, m, "cooldown", 570);
    let by_brand = sd_truthy(env, m, "triggeredByBrand");
    if by_brand {
        let tb = &config.tb;
        let b = tb
            .main_skill
            .unwrap_or_else(|| line_err(573, "attempt to index field 'mainSkill' (a nil value)"));
        let rf = sd_num(env, b, "repeatFrequency", 573)
            .unwrap_or_else(|| line_err(573, "attempt to perform arithmetic on field 'repeatFrequency' (a nil value)"));
        let more = tb
            .activation_freq_more
            .unwrap_or_else(|| line_err(573, "attempt to perform arithmetic on field 'activationFreqMore' (a nil value)"));
        let inc = tb
            .activation_freq_inc
            .unwrap_or_else(|| line_err(573, "attempt to perform arithmetic on field 'activationFreqInc' (a nil value)"));
        trigger_cd = Some(rf / more / inc * icdr);
    }
    // :578 `processAddedCastTime(env.player.mainSkill, breakdown)`
    let pac = process_added_cast_time(env, pm);
    let adds = pac.map(|(a, _)| a);
    set_out(st, "addsCastTime", adds);
    let triggered_cd_adjusted = (triggered_cd.unwrap_or(0.0) + added_cooldown.unwrap_or(0.0)) / icdr;
    let trigger_cd_adjusted = (trigger_cd.unwrap_or(0.0) + adds.unwrap_or(0.0)) / icdr;
    let triggered_cd_tick_rounded = if sd_truthy(env, m, "ignoresTickRate") {
        triggered_cd_adjusted
    } else {
        (triggered_cd_adjusted * tick_rate()).ceil() / tick_rate()
    };
    let trigger_cd_tick_rounded = if skill(env, m).triggered_by.is_some() && config.tb.ignores_tick_rate {
        trigger_cd_adjusted
    } else {
        (trigger_cd_adjusted * tick_rate()).ceil() / tick_rate()
    };
    let action_cooldown_tick_rounded = match cooldown_override {
        Some(co) => (co * tick_rate()).ceil() / tick_rate(),
        None => max2(trigger_cd_tick_rounded, triggered_cd_tick_rounded),
    };
    // :189-192
    let mut rate_cap = f64::INFINITY;
    if source == Some(m)
        && let Some(o) = sd_num(env, m, "triggerRateCapOverride", 589)
    {
        rate_cap = o;
    }
    if action_cooldown_tick_rounded != 0.0 {
        rate_cap = 1.0 / action_cooldown_tick_rounded;
    }
    st.player.output.set_num("TriggerRateCap", rate_cap);
    // :193-220 Doom Blast sources
    let trigger_name = config.trigger_name.clone().unwrap_or_default();
    if trigger_name == "Doom Blast" && config_input_is(env, "doomBlastSource", "expiration") {
        let d = with_cached(env, uuid.as_deref(), 594, |c| cached_out(c, "Duration"))?
            .unwrap_or_else(|| line_err(594, "attempt to index field '?' (a nil value)"))
            .unwrap_or_else(|| line_err(594, "attempt to perform arithmetic on field 'Duration' (a nil value)"));
        let expiration_rate = 1.0 / d;
        if expiration_rate > req_rate(trig_rate, 598) {
            player_db(env).new_mod(
                "UsesCurseOverlaps",
                Some(ModType::Flag),
                Value::Bool(true),
                Some("Config"),
                0,
                0,
                vec![],
            );
        } else {
            trig_rate = Some(expiration_rate);
        }
    } else if trigger_name == "Doom Blast" && config_input_is(env, "doomBlastSource", "hexblast") {
        let mut hex = Found::default();
        for i in 0..skill_count(env) {
            if name_of(env, i) == "Hexblast" && !is_triggered(env, i) && i != m {
                hex = find_trigger_skill(env, i, &hex, None)?;
                uuid.clone_from(&hex.uuid);
            }
        }
        if hex.source.is_some() {
            let r = req_rate(trig_rate, 618);
            let hr = hex
                .rate
                .unwrap_or_else(|| line_err(618, "attempt to perform arithmetic on local 'rate' (a nil value)"));
            trig_rate = Some(1.0 / (1.0 / r + 1.0 / hr));
        }
    }
    // :222-304 breakdown only (R8)

    // :306-325
    let mut global_trigger = flag(env, m, "globalTrigger");
    if name_of(env, pm) == "Doom Blast" && config_input_is(env, "doomBlastSource", "vixen") {
        let vixen_gloves = env.player.borrow().item_list.get("Gloves").is_some_and(|it| {
            it.title
                .as_deref()
                .is_some_and(|t| matches!(pob_lua::find(t.as_bytes(), b"Vixen's Entrapment", None, false), Ok(Some(_))))
        });
        if !vixen_gloves {
            st.player.output.set_bool("VixenModeNoVixenGlovesWarn", true);
        }
        player_db(env).new_mod(
            "UsesCurseOverlaps",
            Some(ModType::Flag),
            Value::Bool(true),
            Some("Config"),
            0,
            0,
            vec![],
        );
        let vixens_cd = env
            .skills
            .as_ref()
            .expect("skills on")
            .skills
            .get("SupportUniqueCastCurseOnCurse")
            .map(|v| {
                v.level(1.0)
                    .and_then(|l| l.cooldown)
                    .unwrap_or_else(|| line_err(313, "attempt to perform arithmetic on field 'cooldown' (a nil value)"))
                    / icdr
            });
        let r = req_rate(trig_rate, 314);
        let main_uuid = cache_skill_uuid(env, pm);
        let mut rot = vec![SimSkill {
            uuid: main_uuid.clone(),
            icdr: Some(icdr),
            ..SimSkill::default()
        }];
        let (esr, _) = calc_multi_spell_rotation_impact(&main_uuid, &mut rot, r, vixens_cd, None);
        st.player.output.set_num("EffectiveSourceRate", esr);
        let vcd = vixens_cd.unwrap_or_else(|| line_err(315, "attempt to compare nil with number"));
        st.player.output.set_bool("VixensTooMuchCastSpeedWarn", vcd > 1.0 / r);
    } else if let Some(r) = trig_rate
        && !global_trigger
        && !config.ignore_source_rate
    {
        st.player.output.set_num("EffectiveSourceRate", r);
    } else {
        st.player.output.set_num("EffectiveSourceRate", rate_cap);
        set_flag(env, m, "globalTrigger");
        global_trigger = true;
    }
    // :327-329 breakdown only (R8)

    // :333-481
    let esr = out_num(st, "EffectiveSourceRate", 333);
    let skill_trigger_rate;
    if esr != 0.0 && !flag(env, pm, "skipEffectiveRate") {
        let mut trigger_chance = 100.0;
        // :338-387 accuracy and crit chance
        if let Some(s) = source
            && (has_type(env, s, "Melee") || has_type(env, s, "Attack"))
            && !config.trigger_on_use
            && let Some((hit, crit, on_crit)) = with_cached(env, uuid.as_deref(), 338, |c| {
                (c.hit_chance, c.crit_chance, cached_sd(c, "chanceToTriggerOnCrit"))
            })?
        {
            let both = dual_wield && sd_truthy(env, s, "doubleHitsWhenDualWielding");
            let source_hit = hit.unwrap_or(0.0);
            if source_hit != 100.0 {
                if both {
                    return Err(both_hands_boundary(344));
                }
                trigger_chance = trigger_chance * source_hit / 100.0;
            }
            if sd_truthy(env, m, "triggerOnCrit") {
                // :362-363 `onCritChance = skillData.chanceToTriggerOnCrit or cached...`; `config.triggerChance = config.triggerChance or
                // skillData.chanceToTriggerOnCrit or onCritChance`
                let own = sd_num(env, m, "chanceToTriggerOnCrit", 362);
                let on_crit = own.or_else(|| on_crit.as_num().filter(|_| on_crit.truthy()));
                if config.trigger_chance.is_none() {
                    config.trigger_chance = own.or(on_crit);
                }
                let source_crit = crit.unwrap_or(0.0);
                if source_crit != 100.0 {
                    if both {
                        return Err(both_hands_boundary(369));
                    }
                    trigger_chance = trigger_chance * source_crit / 100.0;
                }
            }
        }
        // :389-395
        if let Some(tc) = config.trigger_chance
            && tc != 100.0
        {
            trigger_chance = trigger_chance * tc / 100.0;
        }
        // :397-473
        let single_ignoring_tick = sd_truthy(env, m, "ignoresTickRate")
            && (config.triggered_skill_cond.is_none() || {
                // `#triggeredSkills == 1 and triggeredSkills[1] == packageSkillDataForSimulation(actor.mainSkill, env)`: a new table is
                // never equal; the call still runs (it writes `skillFlags.addsCastTime`)
                if triggered_skills.len() == 1 {
                    let _ = package(env, m);
                }
                false
            });
        if single_ignoring_tick {
            let overlaps = config.overlaps;
            skill_trigger_rate = min2(rate_cap, esr * overlaps.unwrap_or(1.0));
        } else if global_trigger && config.triggered_skill_cond.is_none() {
            skill_trigger_rate = esr;
        } else {
            let mut rot = if config.triggered_skill_cond.is_some() {
                std::mem::take(&mut triggered_skills)
            } else {
                vec![package(env, m)]
            };
            let tcd = if by_brand {
                Some(0.0)
            } else {
                Some(trigger_cd.or(triggered_cd).unwrap_or(0.0))
            };
            let main_uuid = cache_skill_uuid(env, m);
            let (mut r, _) = calc_multi_spell_rotation_impact(&main_uuid, &mut rot, esr, tcd, Some(trigger_chance));
            if trigger_bots(env, who, m) {
                r *= 2.0;
            }
            // `hits_per_cast` -- `config.stagesAreOverlaps` is never set: 1
            skill_trigger_rate = 1.0 * r;
        }
    } else {
        skill_trigger_rate = 0.0;
    }
    st.player.output.set_num("SkillTriggerRate", skill_trigger_rate);
    set_sd(env, m, "triggerRate", Value::Num(skill_trigger_rate));
    // :484-498
    st.player.output.set_num("Speed", skill_trigger_rate);
    add_trigger_inc_more_mods(env, m, source.unwrap_or(m));
    // :576 `actor ~= env.minion and actor.mainSkill.activeEffect.grantedEffect.name or "Triggered skill"`
    let triggered_name = if who == Who::Minion {
        "Triggered skill".to_owned()
    } else {
        name_of(env, m)
    };
    let info = match source {
        Some(s) if s != m => {
            let su = cache_skill_uuid(env, s);
            set_sd(env, m, "triggerSourceUUID", Value::str(&su));
            let sname = name_of(env, s);
            match &config.custom_trigger_name {
                Some(c) => format!("{c}{sname}"),
                None => {
                    let head = if trigger_name != sname {
                        trigger_name.clone()
                    } else {
                        triggered_name
                    };
                    // :888 `actor == env.minion and "'s attack Trigger: " or "'s Trigger: "`
                    let mid = if who == Who::Minion {
                        "'s attack Trigger: "
                    } else {
                        "'s Trigger: "
                    };
                    format!("{head}{mid}{sname}")
                }
            }
        }
        _ => match super::triggered_by_ge(env, m) {
            Some(g) => env.skills.as_ref().expect("skills on").skills.effects[g].name.clone(),
            None => format!("{trigger_name} Trigger"),
        },
    };
    let info = if by_brand {
        let n = config.tb.attached_brand_count.clone();
        let k = n
            .as_num()
            .unwrap_or_else(|| line_err(495, "bad argument #2 to 'format' (number expected, got nil)"));
        let tail = pob_lua::format(
            "%d attached Brand%s",
            &[pob_lua::Arg::Num(k), pob_lua::Arg::Str(if k == 1.0 { "" } else { "s" })],
        )
        .unwrap_or_else(|e| line_err(495, &e));
        format!("{info}:{tail}")
    } else {
        info
    };
    let sk = skill_mut(env, m);
    sk.info_message = Some(info);
    sk.info_trigger = Some(trigger_name);
    Ok(())
}
