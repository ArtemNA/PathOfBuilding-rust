//! What CalcPerform.lua:3797-3798 does before `calcs.offence` can run: `calcs.triggers(env, env.player)` (CalcTriggers.lua:1602-1627) and
//! `calcs.mirages(env)` (CalcMirages.lua:60-427). Both are R7.8/R7.9; R7.3 ports only their DISPATCH -- which branch upstream takes for
//! `env.player.mainSkill`:
//! * triggers: no `configTable` entry for any of the looked-up names -> upstream only does `mainSkill.skillData.triggered = nil` (:1624),
//!   which is ported; an entry exists -> its handler (rates, `SkillTriggerRate`, nested runs) is R7.8 ([`Gate::Unported`]);
//! * mirages: the five branches of :63-426 are told apart by flags/names; none taken -> `nil` (offence runs); one taken -> R7.9.
//!
//! An unported branch stops the run at :3797/:3798 (perform records it and does not call offence): offence on a state the trigger /
//! mirage code would have changed first is not upstream's offence.

use crate::env::Env;
use crate::session::{cache_skill_uuid, is_limited};

/// What the dispatch found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Nothing more of this module runs; offence follows.
    Pass,
    /// The branch upstream takes is not ported -- `"<File.lua>:<line> <what>"`.
    Unported(String),
}

/// `configTable` keys (CalcTriggers.lua:902-1584), with their line.
const CONFIG_TABLE: [(&str, u32); 82] = [
    ("law of the wilds", 903),
    ("the rippling thoughts", 910),
    ("the surging thoughts", 919),
    ("the hidden blade", 929),
    ("replica eternity shroud", 944),
    ("shroud of the lightless", 948),
    ("limbsplit", 952),
    ("the cauteriser", 955),
    ("duskblight", 958),
    ("lioneye's paws", 961),
    ("replica lioneye's paws", 967),
    ("moonbender's wing", 973),
    ("ngamahu's flame", 978),
    ("cameria's avarice", 981),
    ("starcaller", 984),
    ("uul-netol's embrace", 987),
    ("rigwald's crest", 990),
    ("jorrhast's blacksteel", 994),
    ("ashcaller", 998),
    ("arakaali's fang", 1002),
    ("sporeguard", 1005),
    ("mark of the elder", 1008),
    ("mark of the shaper", 1011),
    ("poet's pen", 1014),
    ("maloney's mechanism", 1023),
    ("asenath's chant", 1036),
    ("vixen's entrapment", 1045),
    ("flames of judgement", 1051),
    ("storm of judgement", 1057),
    ("trigger craft", 1063),
    ("kitava's thirst", 1086),
    ("foulborn kitava's thirst", 1100),
    ("mjolner", 1114),
    ("wing of the wyvern", 1122),
    ("cospri's malice", 1130),
    ("seven teachings", 1136),
    ("squirming terror", 1140),
    ("kinetic flux", 1152),
    ("cast on critical strike", 1156),
    ("cast on melee kill", 1160),
    ("nova", 1172),
    ("cast when damage taken", 1180),
    ("cast when stunned", 1195),
    ("cast on ward break", 1200),
    ("spellslinger", 1205),
    ("call to arms", 1218),
    ("automation", 1236),
    ("autoexertion", 1252),
    ("seize the flesh", 1274),
    ("fissure", 1277),
    ("mark on hit", 1280),
    ("hextouch", 1283),
    ("oskarm", 1289),
    ("tempest shield", 1295),
    ("shattershard", 1299),
    ("battlemage's cry", 1315),
    ("arcanist brand", 1326),
    ("cast on death", 1347),
    ("combust", 1352),
    ("prismatic burst", 1360),
    ("voidstorm", 1363),
    ("shockwave", 1366),
    ("void shockwave", 1369),
    ("falling crystal", 1372),
    ("call the pyre", 1375),
    ("manaforged arrows", 1383),
    ("doom blast", 1388),
    ("cast while channelling", 1398),
    ("focus", 1401),
    ("snipe", 1404),
    ("avenging flame", 1480),
    ("intuitive link", 1488),
    ("svalinn cast on block", 1503),
    ("festering resentment cast on block", 1510),
    ("hex on trap", 1517),
    ("supporttriggerfirespellonhit", 1526),
    ("ghostly artillery", 1535),
    ("replica gifts from above", 1544),
    ("BloodShrineUniqueTriggeredExplodingToad", 1551),
    ("bursting toad", 1556),
    ("TriggeredMoltenStrike", 1576),
    ("FieryImpactHeistMaceImplicit", 1579),
];

fn config_line(key: &str) -> Option<u32> {
    CONFIG_TABLE.iter().find(|(k, _)| *k == key).map(|(_, l)| *l)
}

/// `isTriggered(skill)` (CalcTriggers.lua:41-43).
fn is_triggered(env: &Env, i: usize) -> bool {
    let st = env.skills.as_ref().expect("skills on");
    let sk = &st.active_skill_list[i];
    let t = |k: &str| sk.skill_data.get(k).is_some_and(pob_mods::Value::truthy);
    let ty = |n: &str| sk.skill_types.contains(&super::skill_type_u32(n));
    let ge = &st.skills.effects[sk.active_effect.borrow().granted_effect];
    let src_triggered = sk
        .active_effect
        .borrow()
        .src_instance
        .is_some_and(|(g, k)| st.tab.socket_group_list()[g].gem_list[k].triggered);
    t("triggeredByUnique") || t("triggered") || ty("InbuiltTrigger") || ty("Triggered") || ge_triggered(ge) || src_triggered
}

/// `grantedEffect.triggered` -- not a field of the converted data (no granted effect sets it in Data/Skills).
fn ge_triggered(_ge: &pob_data::skills::GrantedEffect) -> bool {
    false
}

/// `getUniqueItemTriggerName(skill)` (CalcTriggers.lua:1585-1600).
fn unique_item_trigger_name(env: &Env, i: usize) -> Option<String> {
    let st = env.skills.as_ref().expect("skills on");
    let sk = &st.active_skill_list[i];
    if let Some(v) = sk.skill_data.get("triggerSource").filter(|v| v.truthy()) {
        return Some(v.as_str().map_or_else(|| format!("{v:?}"), str::to_owned));
    }
    // :1588-1594 `skill.supportList` -- `gemInstance.grantedEffect.fromItem and not support`
    for se in &sk.support_list {
        let ge = &st.skills.effects[se.borrow().granted_effect];
        if (ge.from_item || st.from_item_marks.contains(&ge.id)) && !ge.support {
            return Some(ge.name.clone());
        }
    }
    // :1596-1599 `socketGroup.source:find(".*:.*:(.*),.*")`
    let src = sk.socket_group.and_then(|g| st.tab.socket_group_list()[g].source.clone())?;
    let (_, _, caps) = pob_lua::find(src.as_bytes(), b".*:.*:(.*),.*", None, false).ok().flatten()?;
    let cap = caps.get(0)?.bytes(src.as_bytes())?;
    Some(String::from_utf8_lossy(cap).into_owned())
}

/// `calcs.triggers(env, env.player)` (CalcTriggers.lua:1602-1627): the dispatch (see the module doc).
pub(crate) fn triggers_gate(env: &mut Env) -> Gate {
    let Some(i) = env.skills.as_ref().and_then(|s| s.main_skill) else {
        return Gate::Pass;
    };
    let disabled = env.skills.as_ref().expect("skills on").active_skill_list[i].flag("disable");
    if disabled || is_limited(env, i) {
        return Gate::Pass;
    }
    let (skill_name, skill_id, trigger_name) = {
        let st = env.skills.as_ref().expect("skills on");
        let sk = &st.active_skill_list[i];
        let ge = &st.skills.effects[sk.active_effect.borrow().granted_effect];
        let trig = sk
            .triggered_by
            .as_ref()
            .map(|t| st.skills.effects[t.borrow().granted_effect].name.clone());
        (ge.name.clone(), ge.id.clone(), trig)
    };
    let unique_name = if is_triggered(env, i) {
        unique_item_trigger_name(env, i)
    } else {
        None
    };
    let lower = |s: &str| s.to_ascii_lowercase();
    let trigger_lower = trigger_name.as_deref().map(lower);
    // `triggerNameLower:gsub("^awakened ", "")`
    let awakened = trigger_lower
        .as_deref()
        .map(|t| t.strip_prefix("awakened ").unwrap_or(t).to_owned());
    let candidates = [
        Some(skill_id),
        Some(lower(&skill_name)),
        trigger_lower,
        awakened,
        unique_name.as_deref().map(lower),
    ];
    for key in candidates.into_iter().flatten() {
        if let Some(line) = config_line(&key) {
            return Gate::Unported(format!(
                "CalcTriggers.lua:{line} configTable[\"{key}\"] trigger handler for {skill_name} (R7.8)"
            ));
        }
    }
    // :1624
    env.skills.as_mut().expect("skills on").active_skill_list[i]
        .skill_data
        .shift_remove("triggered");
    Gate::Pass
}

/// `calcs.mirages(env)` (CalcMirages.lua:60-427): `Pass` when it returns nil without doing anything (no branch, or the General's Cry
/// recursion guard :352-355).
pub(crate) fn mirages_gate(env: &Env) -> Gate {
    let Some(i) = env.skills.as_ref().and_then(|s| s.main_skill) else {
        return Gate::Pass;
    };
    let st = env.skills.as_ref().expect("skills on");
    let sk = &st.active_skill_list[i];
    // :63-65
    let used_by_mirage = sk
        .skill_cfg
        .as_ref()
        .is_some_and(|c| c.skill_cond.get("usedByMirage").copied().unwrap_or(false));
    if used_by_mirage || sk.flag("disable") {
        return Gate::Pass;
    }
    let t = |k: &str| sk.skill_data.get(k).is_some_and(pob_mods::Value::truthy);
    let name = st.skills.effects[sk.active_effect.borrow().granted_effect].name.as_str();
    let branch = if t("triggeredByMirageArcher") {
        Some("67-119 Mirage Archer")
    } else if name == "Reflection" {
        Some("120-164 Reflection (The Saviour)")
    } else if name == "Tawhoa's Chosen" {
        Some("165-283 Tawhoa's Chosen")
    } else if t("triggeredBySacredWisps") {
        Some("284-345 Sacred Wisps")
    } else if t("triggeredByGeneralsCry") {
        // :350-355 the recursion guard returns nil before touching anything
        if env.limited_skills.as_ref().is_some_and(|l| l.contains(&cache_skill_uuid(env, i))) {
            return Gate::Pass;
        }
        Some("346-426 General's Cry")
    } else {
        None
    };
    match branch {
        Some(b) => Gate::Unported(format!("CalcMirages.lua:{b} for {name} (R7.9)")),
        None => Gate::Pass,
    }
}
