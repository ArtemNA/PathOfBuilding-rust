//! CalcTriggers.lua:902-1584 -- `configTable`: 82 entries, each `function(env) return { ... } end` (or nil). An entry may write onto
//! `env.player.mainSkill` before it returns; a nil return falls through to the next candidate key with those writes kept (:1613-1617).

use pob_lua::number::{max2, min2};
use pob_mods::{ModType, Value};

use super::{
    Comparer, Found, SimSkill, SkillCond, TriggeredByExtra, cached_out, cached_out_truthy, cfg_flags, cfg_of, default_comparer,
    ensure_cached, find_trigger_skill, flag, ge_of, group_slot, has_type, is_triggered, line_err, main_idx, name_of, package, player_db,
    q_of, sd, sd_num, sd_truthy, set_flag, set_sd, skill, skill_count, skill_mut, slot_match, src_instance, with_cached,
};
use crate::env::Env;
use crate::perform::PerformState;
use crate::session::cache_skill_uuid;
use crate::setup::{mod_flag, tag};

/// `config.customHandler`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Custom {
    Cwc,
    Focus,
}

/// The table a `configTable` entry returns (nil fields = `None` / `false`). `stagesAreOverlaps` and `allowTriggered` are never set by an
/// entry (nil everywhere they are read); `useCastRate`, `assumingEveryHitKills` and `sourceName` only feed breakdown text (R8).
#[derive(Default)]
pub(crate) struct Config {
    pub trigger_name: Option<String>,
    pub trigger_skill_cond: Option<SkillCond>,
    pub triggered_skill_cond: Option<SkillCond>,
    pub triggered_skills: Option<Vec<SimSkill>>,
    pub trig_rate: Option<f64>,
    pub source: Option<usize>,
    pub comparer: Option<Comparer>,
    pub trigger_on_use: bool,
    pub source_weapon: bool,
    pub ignore_source_rate: bool,
    pub trigger_chance: Option<f64>,
    /// `overlaps` (Doom Blast: `#Tabulate(...) > 0 and m_max(...)` -- `false` reads as nil everywhere).
    pub overlaps: Option<f64>,
    pub custom_trigger_name: Option<String>,
    pub custom_handler: Option<Custom>,
    /// `actor.mainSkill.triggeredBy.<field>` written by the entry.
    pub tb: TriggeredByExtra,
}

fn types_any(env: &Env, i: usize, names: &[&str]) -> bool {
    names.iter().any(|n| has_type(env, i, n))
}

fn cond(f: impl Fn(&Env, usize) -> bool + 'static) -> Option<SkillCond> {
    Some(Box::new(f))
}

/// `(skill.skillTypes[SkillType.Melee] or skill.skillTypes[SkillType.Attack])`.
fn melee_or_attack() -> Option<SkillCond> {
    cond(|env, s| types_any(env, s, &["Melee", "Attack"]))
}

/// `(skill.skillTypes[SkillType.Damage] or skill.skillTypes[SkillType.Attack])`.
fn damage_or_attack() -> Option<SkillCond> {
    cond(|env, s| types_any(env, s, &["Damage", "Attack"]))
}

/// `band(skill.skillCfg.flags, mask) > 0`.
fn has_flags(env: &Env, i: usize, mask: u32) -> bool {
    cfg_flags(env, i) & mask > 0
}

/// `env.player.mainSkill.socketGroup.slot == skill.socketGroup.slot` (either nil socketGroup is the Lua error).
fn same_slot_as_main(env: &Env, i: usize, line: u32) -> bool {
    let m = main_idx(env);
    group_slot(env, m, line) == group_slot(env, i, line)
}

/// `env.player.modDB:Flag(nil, name)`.
fn player_flag(env: &Env, name: &str) -> bool {
    player_db(env).flag(None, &[name])
}

/// `env.player.mainSkill.skillFlags.globalTrigger = true` + `return {source = env.player.mainSkill}`.
fn global_self(env: &mut Env) -> Config {
    let m = main_idx(env);
    set_flag(env, m, "globalTrigger");
    Config {
        source: Some(m),
        ..Config::default()
    }
}

/// The "find the skill named X and make it the trigger's granted effect" `else` of Call to Arms / Automation / Autoexertion
/// (:1222-1228, :1240-1246, :1260-1270): `env.player.mainSkill.triggeredBy.grantedEffect = skill.activeEffect.grantedEffect`.
fn borrow_active_part(env: &mut Env, name: &str, line: u32) {
    let m = main_idx(env);
    for i in 0..skill_count(env) {
        if name_of(env, i) == name {
            let ge = skill(env, i).active_effect.borrow().granted_effect;
            let tb = skill(env, m)
                .triggered_by
                .clone()
                .unwrap_or_else(|| line_err(line, "attempt to index field 'triggeredBy' (a nil value)"));
            tb.borrow_mut().granted_effect = ge;
            break;
        }
    }
}

/// `configTable[key](env)` -- `Ok(None)` for a key not in the table or an entry returning nil.
#[allow(clippy::too_many_lines)]
pub(crate) fn config_table(env: &mut Env, st: &mut PerformState, key: &str) -> Result<Option<Config>, String> {
    let m = main_idx(env);
    let main_name = name_of(env, m);
    let c = match key {
        // :903-909
        "law of the wilds" => Config {
            trigger_skill_cond: cond(|env, s| {
                !has_type(env, s, "SummonsTotem") && types_any(env, s, &["Melee", "Attack"]) && has_flags(env, s, mod_flag("Claw"))
            }),
            ..Config::default()
        },
        // :910-918
        "the rippling thoughts" => {
            if main_name != "Storm Cascade" {
                return Ok(None);
            }
            Config {
                trigger_skill_cond: melee_or_attack(),
                ..Config::default()
            }
        }
        // :919-928
        "the surging thoughts" => {
            if main_name != "Storm Cascade" {
                return Ok(None);
            }
            Config {
                trigger_on_use: true,
                trigger_skill_cond: melee_or_attack(),
                ..Config::default()
            }
        }
        // :929-943
        "the hidden blade" => {
            set_flag(env, m, "globalTrigger");
            set_sd(env, m, "triggerRateCapOverride", Value::Num(2.0));
            if player_flag(env, "Condition:Phasing") {
                return Ok(Some(Config {
                    source: Some(m),
                    ..Config::default()
                }));
            }
            set_flag(env, m, "disable");
            skill_mut(env, m).disable_reason = Some("This skill is requires you to be phasing".to_owned());
            return Ok(None);
        }
        // :944-951
        "replica eternity shroud" | "shroud of the lightless" => global_self(env),
        // :952-960
        "limbsplit" | "the cauteriser" => Config {
            trigger_name: Some("Gore Shockwave".into()),
            trigger_skill_cond: melee_or_attack(),
            ..Config::default()
        },
        "duskblight" => Config {
            trigger_name: Some("Stalking Pustule".into()),
            trigger_skill_cond: damage_or_attack(),
            ..Config::default()
        },
        // :961-972
        "lioneye's paws" | "replica lioneye's paws" => {
            set_sd(env, m, "cooldown", Value::Num(1.0));
            Config {
                trigger_on_use: true,
                trigger_skill_cond: cond(|env, s| has_type(env, s, "Attack") && has_flags(env, s, mod_flag("Bow"))),
                ..Config::default()
            }
        }
        // :973-977
        "moonbender's wing" => {
            set_sd(env, m, "cooldown", Value::Num(1.0));
            Config {
                trigger_name: Some("Lightning Warp".into()),
                trigger_skill_cond: melee_or_attack(),
                ..Config::default()
            }
        }
        "ngamahu's flame" => Config {
            trigger_name: Some("Molten Burst".into()),
            trigger_skill_cond: melee_or_attack(),
            ..Config::default()
        },
        "cameria's avarice" => Config {
            trigger_name: Some("Icicle Burst".into()),
            trigger_skill_cond: damage_or_attack(),
            ..Config::default()
        },
        "starcaller" => Config {
            trigger_name: Some("Starfall".into()),
            trigger_skill_cond: melee_or_attack(),
            ..Config::default()
        },
        "uul-netol's embrace" => Config {
            trigger_name: Some("Bone Nova".into()),
            trigger_skill_cond: damage_or_attack(),
            ..Config::default()
        },
        // :990-1001
        "rigwald's crest" | "jorrhast's blacksteel" | "ashcaller" => {
            set_sd(env, m, "sourceRateIsFinal", Value::Bool(true));
            Config {
                trigger_skill_cond: damage_or_attack(),
                ..Config::default()
            }
        }
        // :1002-1013
        "arakaali's fang" | "sporeguard" | "mark of the elder" | "mark of the shaper" => Config {
            trigger_skill_cond: damage_or_attack(),
            ..Config::default()
        },
        // :1014-1022
        "poet's pen" => Config {
            trigger_on_use: true,
            trigger_skill_cond: cond(|env, s| types_any(env, s, &["Damage", "Attack"]) && has_flags(env, s, mod_flag("Wand"))),
            triggered_skill_cond: cond(|env, s| {
                sd_truthy(env, s, "triggeredByUnique") && same_slot_as_main(env, s, 1020) && has_type(env, s, "Spell")
            }),
            ..Config::default()
        },
        // :1023-1035
        "maloney's mechanism" => {
            let slot = skill(env, m)
                .skill_cfg
                .as_ref()
                .and_then(|c| c.slot_name.clone())
                .unwrap_or_else(|| line_err(1024, "attempt to index field '?' (a nil value)"));
            let src = env
                .player
                .borrow()
                .item_list
                .get(&slot)
                .unwrap_or_else(|| line_err(1024, "attempt to index field '?' (a nil value)"))
                .mod_source
                .clone()
                .unwrap_or_else(|| line_err(1024, "attempt to index field 'modSource' (a nil value)"));
            let name = pob_lua::find(src.as_bytes(), b".*:.*:(.*),.*", None, false)
                .ok()
                .flatten()
                .and_then(|(_, _, caps)| {
                    caps.get(0)
                        .and_then(|c| c.bytes(src.as_bytes()).map(|b| String::from_utf8_lossy(b).into_owned()))
                })
                .unwrap_or_else(|| line_err(1025, "attempt to index local 'uniqueTriggerName' (a nil value)"));
            let is_replica = matches!(pob_lua::find(name.as_bytes(), b"Replica.", None, false), Ok(Some(_)));
            Config {
                trigger_on_use: true,
                trigger_name: Some(name),
                trigger_skill_cond: cond(move |env, s| {
                    let attack = has_type(env, s, "Attack") && has_flags(env, s, mod_flag("Bow")) && !is_replica;
                    let spell = has_type(env, s, "Spell") && is_replica;
                    attack || spell
                }),
                triggered_skill_cond: cond(|env, s| {
                    sd_truthy(env, s, "triggeredByUnique") && same_slot_as_main(env, s, 1033) && has_type(env, s, "RangedAttack")
                }),
                ..Config::default()
            }
        }
        // :1036-1044
        "asenath's chant" => Config {
            trigger_on_use: true,
            trigger_skill_cond: cond(|env, s| types_any(env, s, &["Damage", "Attack"]) && has_flags(env, s, mod_flag("Bow"))),
            triggered_skill_cond: cond(|env, s| {
                sd_truthy(env, s, "triggeredByUnique") && same_slot_as_main(env, s, 1042) && has_type(env, s, "Spell")
            }),
            ..Config::default()
        },
        // :1045-1050
        "vixen's entrapment" => Config {
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Hex")),
            ..Config::default()
        },
        // :1051-1062
        "flames of judgement" | "storm of judgement" => {
            set_sd(env, m, "sourceRateIsFinal", Value::Bool(true));
            Config {
                trigger_name: Some(main_name.clone()),
                trigger_skill_cond: cond(|env, s| name_of(env, s) == "Queen's Demand"),
                triggered_skill_cond: cond(|env, s| sd_truthy(env, s, "triggeredByUnique") && same_slot_as_main(env, s, 1055)),
                ..Config::default()
            }
        }
        // :1063-1085
        "trigger craft" => {
            if !sd_truthy(env, m, "triggeredByCraft") {
                return Ok(None);
            }
            let mut found = Found::default();
            let mut triggered = Vec::new();
            for s in 0..skill_count(env) {
                if types_any(env, s, &["Damage", "Attack", "Spell"])
                    && !flag(env, s, "aura")
                    && s != m
                    && !sd_truthy(env, s, "triggeredByCraft")
                    && !super::from_item(env, ge_of(env, s))
                    && !is_triggered(env, s)
                {
                    found = find_trigger_skill(env, s, &found, None)?;
                    let sk = skill(env, s);
                    let ge = ge_of(env, s);
                    if ["totem", "golem", "banner", "ballista"].iter().any(|f| sk.flag(f))
                        && let Some(ct) = ge.cast_time
                    {
                        let level = sk.active_effect.borrow().level;
                        let lvl = level
                            .and_then(|l| ge.level(l))
                            .unwrap_or_else(|| line_err(1072, "attempt to index field '?' (a nil value)"));
                        found.rate = Some(1.0 / (ct + lvl.cooldown.unwrap_or(0.0)));
                    }
                }
                if sd_truthy(env, s, "triggeredByCraft") && same_slot_as_main(env, s, 1080) {
                    triggered.push(package(env, s));
                }
            }
            Config {
                trig_rate: found.rate,
                source: found.source,
                triggered_skills: Some(triggered),
                ..Config::default()
            }
        }
        // :1086-1113
        "kitava's thirst" | "foulborn kitava's thirst" => {
            let foulborn = key.starts_with("foulborn");
            let (req_name, chance_name, cost_life) = if foulborn {
                ("FoulbornKitavaRequiredLifeCost", "FoulbornKitavaTriggerChance", true)
            } else {
                ("KitavaRequiredManaCost", "KitavaTriggerChance", false)
            };
            let required = player_db(env).base(None, &[req_name]);
            let chance = player_db(env).base(None, &[chance_name]);
            Config {
                trigger_chance: Some(chance),
                trigger_name: Some(if foulborn { "Foulborn Kitava's Thirst" } else { "Kitava's Thirst" }.into()),
                comparer: Some(Box::new(move |c, source, rate| {
                    let cost = if cost_life { c.life_cost } else { c.mana_cost };
                    default_comparer(c, source, rate) && cost.unwrap_or(0.0) >= required
                })),
                trigger_skill_cond: cond(|_, _| true),
                ..Config::default()
            }
        }
        // :1114-1121
        "mjolner" => Config {
            trigger_skill_cond: cond(|env, s| {
                types_any(env, s, &["Damage", "Attack"])
                    && has_flags(env, s, mod_flag("Mace") | mod_flag("Weapon1H"))
                    && !slot_match(env, s)
            }),
            triggered_skill_cond: cond(|env, s| sd_truthy(env, s, "triggeredByMjolner") && slot_match(env, s)),
            ..Config::default()
        },
        // :1122-1129
        "wing of the wyvern" => Config {
            trigger_skill_cond: cond(|env, s| {
                types_any(env, s, &["Damage", "Attack"]) && has_flags(env, s, mod_flag("Bow")) && !slot_match(env, s)
            }),
            triggered_skill_cond: cond(|env, s| sd_truthy(env, s, "triggeredByUnique") && slot_match(env, s)),
            ..Config::default()
        },
        // :1130-1135
        "cospri's malice" => Config {
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Melee") && has_flags(env, s, mod_flag("Sword") | mod_flag("Weapon1H"))),
            triggered_skill_cond: cond(|env, s| sd_truthy(env, s, "triggeredByCospris") && same_slot_as_main(env, s, 1134)),
            ..Config::default()
        },
        // :1136-1139
        "seven teachings" => {
            let mask = mod_flag("Unarmed") | mod_flag("Melee");
            Config {
                trigger_skill_cond: cond(move |env, s| has_type(env, s, "Melee") && cfg_flags(env, s) & mask == mask),
                triggered_skill_cond: cond(|env, s| sd_truthy(env, s, "triggeredBySevenTeachings") && slot_match(env, s)),
                ..Config::default()
            }
        }
        // :1140-1151
        "squirming terror" => {
            if player_flag(env, "Condition:KilledRecently") {
                Config {
                    trigger_skill_cond: cond(|env, s| has_type(env, s, "Attack") && has_type(env, s, "Melee")),
                    triggered_skill_cond: cond(|env, s| sd_truthy(env, s, "triggeredBySquirmingTerror") && slot_match(env, s)),
                    ..Config::default()
                }
            } else {
                let sk = skill_mut(env, m);
                sk.info_message2 = Some("DPS reported assuming Self-Cast".into());
                sk.info_message = Some("Squirming Terror requires recent kills".into());
                return Ok(None);
            }
        }
        // :1152-1155
        "kinetic flux" => Config {
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Attack") && slot_match(env, s)),
            triggered_skill_cond: cond(|env, s| sd_truthy(env, s, "triggeredByKineticFlux") && slot_match(env, s)),
            ..Config::default()
        },
        // :1156-1159
        "cast on critical strike" => Config {
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Attack") && slot_match(env, s)),
            triggered_skill_cond: cond(|env, s| sd_truthy(env, s, "triggeredByCoc") && slot_match(env, s)),
            ..Config::default()
        },
        // :1160-1171
        "cast on melee kill" => {
            if player_flag(env, "Condition:KilledRecently") {
                Config {
                    trigger_skill_cond: cond(|env, s| has_type(env, s, "Attack") && has_type(env, s, "Melee") && slot_match(env, s)),
                    triggered_skill_cond: cond(|env, s| sd_truthy(env, s, "triggeredByMeleeKill") && slot_match(env, s)),
                    ..Config::default()
                }
            } else {
                let sk = skill_mut(env, m);
                sk.info_message2 = Some("DPS reported assuming Self-Cast".into());
                sk.info_message = Some("Cast on Melee Kill requires recent kills".into());
                return Ok(None);
            }
        }
        // :1172-1179 `if env.minion and env.minion.mainSkill` -- the minion actor is R7.10
        "nova" => {
            if skill(env, m).minion.is_some() {
                return Err("CalcTriggers.lua:1173-1178 configTable[\"nova\"]: config.actor = env.minion (minion actors are R7.10)".into());
            }
            return Ok(None);
        }
        // :1180-1194
        "cast when damage taken" => {
            let Some(base) = sd_num(env, m, "triggeredByDamageTaken", 1183) else {
                return Ok(None);
            };
            let threshold_mod = q_of(env, m).calc_mod(None, &["CWDTThreshold"]);
            st.player.output.set_num("CWDTThreshold", base * threshold_mod);
            global_self(env)
        }
        // :1195-1204
        "cast when stunned" | "cast on ward break" => {
            let k = if key == "cast when stunned" {
                "chanceToTriggerOnStun"
            } else {
                "chanceToTriggerOnWardBreak"
            };
            set_flag(env, m, "globalTrigger");
            Config {
                trigger_chance: sd_num(env, m, k, 1197),
                source: Some(m),
                ..Config::default()
            }
        }
        // :1205-1217
        "spellslinger" => {
            if main_name == "Spellslinger" {
                set_flag(env, m, "skipEffectiveRate");
            }
            set_sd(env, m, "sourceRateIsFinal", Value::Bool(true));
            Config {
                trigger_name: Some("Spellslinger".into()),
                trigger_on_use: true,
                trigger_skill_cond: cond(|env, s| {
                    let wand_proj_attack =
                        has_type(env, s, "Attack") && has_type(env, s, "Projectile") && has_flags(env, s, mod_flag("Wand"));
                    wand_proj_attack && !sd_truthy(env, s, "triggeredBySpellSlinger")
                }),
                ..Config::default()
            }
        }
        // :1218-1235, :1236-1251, :1252-1273
        "call to arms" | "automation" | "autoexertion" => {
            let (active, line) = match key {
                "call to arms" => ("Call to Arms", 1224),
                "automation" => ("Automation", 1242),
                _ => ("Autoexertion", 1262),
            };
            if main_name == active {
                set_flag(env, m, "globalTrigger");
                set_flag(env, m, "skipEffectiveRate");
            } else {
                borrow_active_part(env, active, line);
            }
            if key != "automation" {
                set_sd(env, m, "sourceRateIsFinal", Value::Bool(true));
                set_sd(env, m, "ignoresTickRate", Value::Bool(true));
            }
            Config {
                trigger_on_use: true,
                source: Some(m),
                ..Config::default()
            }
        }
        // :1274-1276
        "seize the flesh" => Config {
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Warcry") && slot_match(env, s)),
            ..Config::default()
        },
        // :1277-1279
        "fissure" => Config {
            trigger_on_use: true,
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Slam") && slot_match(env, s)),
            ..Config::default()
        },
        // :1280-1282
        "mark on hit" => Config {
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Attack")),
            ..Config::default()
        },
        // :1283-1288
        "hextouch" => {
            set_sd(env, m, "sourceRateIsFinal", Value::Bool(true));
            Config {
                trigger_skill_cond: cond(|env, s| has_type(env, s, "Attack") && slot_match(env, s)),
                ..Config::default()
            }
        }
        // :1289-1294
        "oskarm" => {
            set_sd(env, m, "sourceRateIsFinal", Value::Bool(true));
            Config {
                trigger_skill_cond: cond(|env, s| has_type(env, s, "Attack")),
                ..Config::default()
            }
        }
        // :1295-1298
        "tempest shield" => global_self(env),
        // :1299-1314
        "shattershard" => {
            set_flag(env, m, "globalTrigger");
            let uuid = cache_skill_uuid(env, m);
            ensure_cached(env, m, &uuid, std::slice::from_ref(&uuid), true, 1303)?;
            let d = with_cached(env, Some(&uuid), 1305, |c| cached_out(c, "Duration"))?
                .unwrap_or_else(|| line_err(1305, "attempt to index field '?' (a nil value)"))
                .unwrap_or_else(|| line_err(1305, "attempt to perform arithmetic on field 'Duration' (a nil value)"));
            set_sd(env, m, "triggerRateCapOverride", Value::Num(1.0 / d));
            Config {
                source: Some(m),
                ..Config::default()
            }
        }
        // :1315-1325
        "battlemage's cry" => {
            if main_name == "Battlemage's Cry" {
                return Ok(None);
            }
            Config {
                trigger_skill_cond: cond(|env, s| has_type(env, s, "Melee")),
                comparer: Some(Box::new(|c, source, rate| {
                    default_comparer(c, source, rate) && cached_out_truthy(c, "BattlemageUpTimeRatio")
                })),
                triggered_skill_cond: cond(|env, s| sd_truthy(env, s, "triggeredByBattleMageCry") && slot_match(env, s)),
                ..Config::default()
            }
        }
        // :1326-1346
        "arcanist brand" => {
            if main_name == "Arcanist Brand" {
                return Ok(None);
            }
            set_sd(env, m, "sourceRateIsFinal", Value::Bool(true));
            let brand = (0..skill_count(env)).find(|&s| name_of(env, s) == "Arcanist Brand");
            if skill(env, m).triggered_by.is_none() {
                line_err(1330, "attempt to index field 'triggeredBy' (a nil value)");
            }
            let b = brand.unwrap_or_else(|| line_err(1335, "attempt to index field 'mainSkill' (a nil value)"));
            let bq = q_of(env, b);
            let bcfg = cfg_of(env, b);
            let inc = (100.0 + bq.inc(Some(&bcfg), &["Speed", "BrandActivationFrequency"])) / 100.0;
            let more = bq.more(Some(&bcfg), &["BrandActivationFrequency"]);
            let count = sd(env, b, "attachedBrandCount");
            let repeat = sd_num(env, b, "repeatFrequency", 1341)
                .unwrap_or_else(|| line_err(1341, "attempt to perform arithmetic on field 'repeatFrequency' (a nil value)"));
            let n = count
                .as_num()
                .unwrap_or_else(|| line_err(1341, "attempt to perform arithmetic on a nil value"));
            Config {
                trig_rate: Some(repeat * inc * more * n),
                source: Some(b),
                triggered_skill_cond: cond(|env, s| sd_truthy(env, s, "triggeredByBrand") && slot_match(env, s)),
                tb: TriggeredByExtra {
                    main_skill: Some(b),
                    activation_freq_inc: Some(inc),
                    activation_freq_more: Some(more),
                    attached_brand_count: count,
                    ignores_tick_rate: true,
                },
                ..Config::default()
            }
        }
        // :1347-1351 (returns nil)
        "cast on death" => {
            set_flag(env, m, "globalTrigger");
            set_sd(env, m, "triggered", Value::Bool(true));
            skill_mut(env, m).info_message = Some(format!("{main_name} Triggered on Death"));
            return Ok(None);
        }
        // :1352-1359
        "combust" => Config {
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Melee")),
            comparer: Some(Box::new(|c, source, rate| {
                default_comparer(c, source, rate) && cached_out_truthy(c, "InfernalUpTimeRatio")
            })),
            ..Config::default()
        },
        // :1360-1362
        "prismatic burst" => Config {
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Attack") && slot_match(env, s)),
            ..Config::default()
        },
        // :1363-1365
        "voidstorm" => Config {
            trigger_on_use: true,
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Attack") && has_type(env, s, "Rain") && slot_match(env, s)),
            ..Config::default()
        },
        // :1366-1374
        "shockwave" | "void shockwave" | "falling crystal" => Config {
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Melee") && slot_match(env, s)),
            ..Config::default()
        },
        // :1375-1382
        "call the pyre" => {
            let ignited = crate::setup::ok(env.enemy_db.borrow().flag(None, &["Condition:Ignited"])) == Some(true);
            if ignited {
                Config {
                    trigger_chance: Some(50.0),
                    trigger_skill_cond: cond(|env, s| has_type(env, s, "Melee") && slot_match(env, s)),
                    ..Config::default()
                }
            } else {
                skill_mut(env, m).info_message = Some("Call the Pyre requires Ignited enemies".into());
                return Ok(None);
            }
        }
        // :1383-1387
        "manaforged arrows" => Config {
            trigger_on_use: true,
            trigger_name: Some("Manaforged Arrows".into()),
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Attack") && has_flags(env, s, mod_flag("Bow"))),
            ..Config::default()
        },
        // :1388-1397
        "doom blast" => {
            if super::config_input_is(env, "doomBlastSource", "replacement") {
                player_db(env).new_mod(
                    "UsesCurseOverlaps",
                    Some(ModType::Flag),
                    Value::Bool(true),
                    Some("Config"),
                    0,
                    0,
                    vec![],
                );
            }
            set_sd(env, m, "ignoresTickRate", Value::Bool(true));
            let db = player_db(env);
            let overlaps = if db.tabulate(Some(ModType::Base), None, &["Multiplier:CurseOverlaps"]).is_empty() {
                None
            } else {
                Some(max2(db.base(None, &["Multiplier:CurseOverlaps"]), 1.0))
            };
            Config {
                overlaps,
                custom_trigger_name: Some("Doom Blast triggering Hex: ".into()),
                trigger_skill_cond: cond(|env, s| has_type(env, s, "Hex") && slot_match(env, s)),
                ..Config::default()
            }
        }
        // :1398-1403
        "cast while channelling" => Config {
            custom_handler: Some(Custom::Cwc),
            ..Config::default()
        },
        "focus" => Config {
            custom_handler: Some(Custom::Focus),
            ..Config::default()
        },
        // :1404-1479
        "snipe" => match snipe(env, st)? {
            Some(c) => c,
            None => return Ok(None),
        },
        // :1480-1487
        "avenging flame" => Config {
            trigger_skill_cond: cond(|env, s| flag(env, s, "totem") && slot_match(env, s)),
            comparer: Some(Box::new(|c, source, current| {
                match cached_out(c, "TotemLife") {
                    Some(life) => !source || life > current.unwrap_or(0.0),
                    // a non-number TotemLife never happens; nil fails both branches
                    None => false,
                }
            })),
            ignore_source_rate: true,
            ..Config::default()
        },
        // :1488-1502
        "intuitive link" => {
            if main_name == "Intuitive Link" {
                return Ok(None);
            }
            let link = (0..skill_count(env)).find(|&s| name_of(env, s) == "Intuitive Link");
            if skill(env, m).triggered_by.is_none() {
                line_err(1491, "attempt to index field 'triggeredBy' (a nil value)");
            }
            let rate = player_db(env).base(None, &["IntuitiveLinkSourceRate"]);
            Config {
                triggered_skill_cond: cond(move |env, s| has_type(env, s, "Spell") && slot_match(env, s) && Some(s) != link),
                trig_rate: Some(rate),
                source: link,
                tb: TriggeredByExtra {
                    main_skill: link,
                    ..TriggeredByExtra::default()
                },
                ..Config::default()
            }
        }
        // :1503-1516
        "svalinn cast on block" | "festering resentment cast on block" => {
            set_flag(env, m, "globalTrigger");
            Config {
                source: Some(m),
                triggered_skill_cond: cond(|env, s| slot_match(env, s) && super::triggered_by_can_support(env, s)),
                ..Config::default()
            }
        }
        // :1517-1525
        "hex on trap" => Config {
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Trapped")),
            triggered_skill_cond: cond(|env, s| sd_truthy(env, s, "triggeredByTrapTrigger") && slot_match(env, s)),
            ..Config::default()
        },
        // :1526-1534
        "supporttriggerfirespellonhit" => Config {
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Melee")),
            triggered_skill_cond: cond(|env, s| sd_truthy(env, s, "triggeredBySettlersEnchantTrigger") && slot_match(env, s)),
            ..Config::default()
        },
        // :1535-1543
        "ghostly artillery" => Config {
            source_weapon: true,
            trigger_on_use: true,
            trigger_skill_cond: cond(|env, s| has_type(env, s, "Attack")),
            ..Config::default()
        },
        // :1544-1550
        "replica gifts from above" => Config {
            trigger_skill_cond: cond(|env, s| !has_type(env, s, "SummonsTotem") && has_type(env, s, "Attack")),
            ..Config::default()
        },
        // :1551-1555
        "BloodShrineUniqueTriggeredExplodingToad" => {
            let base = src_instance(env, m, |g| g.trigger_chance)
                .unwrap_or_else(|| line_err(1552, "attempt to index field 'srcInstance' (a nil value)"))
                .unwrap_or_else(|| line_err(1552, "attempt to perform arithmetic on field 'triggerChance' (a nil value)"));
            let chance = base + player_db(env).base(None, &["BloodShrineExplodingToadTriggerChance"]);
            Config {
                trigger_chance: Some(min2(chance, 100.0)),
                trigger_skill_cond: damage_or_attack(),
                ..Config::default()
            }
        }
        // :1556-1575
        "bursting toad" => {
            let mut interval = f64::INFINITY;
            for s in 0..skill_count(env) {
                if let Some(v) = sd_num(env, s, "hextoadTriggerInterval", 1561) {
                    interval = min2(interval, v);
                }
            }
            if interval < f64::INFINITY {
                set_flag(env, m, "globalTrigger");
                set_sd(env, m, "triggerRateCapOverride", Value::Num(1.0 / interval));
                Config {
                    source: Some(m),
                    ..Config::default()
                }
            } else {
                return Ok(None);
            }
        }
        // :1576-1578
        "TriggeredMoltenStrike" => Config {
            trigger_skill_cond: melee_or_attack(),
            ..Config::default()
        },
        // :1579-1581
        "FieryImpactHeistMaceImplicit" => Config {
            source_weapon: true,
            trigger_skill_cond: melee_or_attack(),
            ..Config::default()
        },
        _ => return Ok(None),
    };
    Ok(Some(c))
}

/// :1404-1479 `configTable["snipe"]`.
fn snipe(env: &mut Env, st: &mut PerformState) -> Result<Option<Config>, String> {
    let m = main_idx(env);
    let db = player_db(env);
    let stages = min2(
        db.base(None, &["Multiplier:SnipeStage"]),
        db.base(None, &["Multiplier:SnipeStagesMax"]),
    );
    let mq = q_of(env, m);
    let mcfg = cfg_of(env, m);
    let hit_multi = mq.base(Some(&mcfg), &["snipeHitMulti"]);
    let ailment_multi = mq.base(Some(&mcfg), &["snipeAilmentMulti"]);
    // :1410-1414
    let main_slot = || group_slot(env, m, 1411);
    let mut triggered = Vec::new();
    for s in 0..skill_count(env) {
        if sd_truthy(env, s, "triggeredBySnipe") && skill(env, s).socket_group.is_some() && group_slot(env, s, 1411) == main_slot() {
            triggered.push(s);
        }
    }
    let snipe_stages_tag = || tag(&[("type", Value::str("Multiplier")), ("var", Value::str("SnipeStages"))]);
    if name_of(env, m) == "Snipe" {
        let mut stages = stages;
        if crate::session::is_limited(env, m) {
            stages = 0.0;
        } else {
            set_sd(env, m, "hitTimeMultiplier", Value::Num(max2(1.0, stages) - 0.5));
        }
        if triggered.is_empty() {
            // `if snipeStages then` -- always a number
            mq.nm("Multiplier:SnipeStages", ModType::Base, stages, "Snipe", 0, 0, vec![]);
            mq.nm(
                "Damage",
                ModType::More,
                hit_multi,
                "Snipe",
                mod_flag("Hit"),
                0,
                vec![snipe_stages_tag()],
            );
            mq.nm(
                "Damage",
                ModType::More,
                ailment_multi,
                "Snipe",
                mod_flag("Ailment"),
                0,
                vec![snipe_stages_tag()],
            );
        } else {
            for n in ["DealNoLightning", "DealNoCold", "DealNoFire", "DealNoChaos", "DealNoPhysical"] {
                let t = tag(&[
                    ("type", Value::str("SkillName")),
                    ("skillName", Value::str("Snipe")),
                    ("includeTransfigured", Value::Bool(true)),
                ]);
                // `NewMod(name, "FLAG", true, { tag })`: `createMod` takes a non-string 4th argument as the first tag (no source)
                mq.new_mod(n, Some(ModType::Flag), Value::Bool(true), None, 0, 0, vec![t]);
            }
        }
        return Ok(None);
    }
    // :1440-1446
    let index = triggered.iter().position(|&s| s == m).map(|p| p as f64 + 1.0);
    if let Some(idx) = index
        && idx <= stages
    {
        mq.nm("Multiplier:SnipeStages", ModType::Base, stages, "Snipe", 0, 0, vec![]);
        mq.nm(
            "Damage",
            ModType::More,
            ailment_multi,
            "Snipe",
            mod_flag("Ailment"),
            0,
            vec![snipe_stages_tag()],
        );
        mq.nm(
            "Damage",
            ModType::More,
            hit_multi,
            "Snipe",
            mod_flag("Hit"),
            0,
            vec![snipe_stages_tag()],
        );
        let mut source = None;
        let mut rate: Option<f64> = None;
        for s in 0..skill_count(env) {
            if name_of(env, s) == "Snipe" && skill(env, s).socket_group.is_some() && group_slot(env, s, 1455) == group_slot(env, m, 1455) {
                set_sd(env, s, "hitTimeMultiplier", Value::Num(stages - 0.5));
                let uuid = cache_skill_uuid(env, s);
                ensure_cached(env, s, &uuid, &[], true, 1459)?;
                let (cached_speed, hit_time) =
                    with_cached(env, Some(&uuid), 1461, |c| (cached_out(c, "HitSpeed"), cached_out(c, "HitTime")))?
                        .unwrap_or_else(|| line_err(1461, "attempt to index field '?' (a nil value)"));
                let sk = skill(env, s);
                let used_by_mirage = sk
                    .skill_cfg
                    .as_ref()
                    .is_some_and(|c| c.skill_cond.get("usedByMirage").copied().unwrap_or(false));
                let speed_ok = match cached_speed {
                    Some(v) => source.is_none() || v > rate.unwrap_or(0.0),
                    None => false,
                };
                if !sk.flag("disable") && !used_by_mirage && !has_type(env, s, "OtherThingUsesSkill") && speed_ok {
                    rate = cached_speed;
                    match hit_time {
                        Some(t) => st.player.output.set_num("ChannelTimeToTrigger", t),
                        None => st.player.output.remove("ChannelTimeToTrigger"),
                    }
                    source = Some(s);
                }
            }
        }
        return Ok(Some(Config {
            trig_rate: rate,
            source,
            ..Config::default()
        }));
    }
    set_sd(env, m, "triggered", Value::Nil);
    let sk = skill_mut(env, m);
    sk.info_message2 = Some("DPS reported assuming Self-Cast".into());
    sk.info_message = Some("Not enough Snipe stages to trigger this skill".into());
    sk.info_trigger = Some(String::new());
    Ok(None)
}
