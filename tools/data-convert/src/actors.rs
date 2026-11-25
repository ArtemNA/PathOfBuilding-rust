//! The actor-ish `data.*` modules (Modules/Data.lua 727, 874-912, 953-997, 1154-1176) -> minions, bosses, boss-skills,
//! pantheons, map-mods, cluster-jewels, foulborn-map, abyss-notable-names, node-id-list (.json). Readers: crates/pob-data.

use crate::ctx::{Ctx, R, key_text};
use crate::dump::V;
use crate::fields::Fields;
use crate::js::J;
use crate::shape::{S, Shaper, opt, req};

fn root<'a>(c: &Ctx<'a>, k: &str) -> R<&'a V> {
    c.d.path("data", &[k]).ok_or_else(|| format!("data.{k} missing"))
}

fn one(c: &mut Ctx, file: &str, key: &str, s: &S) -> R<J> {
    let v = root(c, key)?;
    let j = Shaper { file }.conv(c, s, v, &format!("data.{key}"))?;
    if let J::Obj(o) = &j {
        c.count(file, key, o.len());
    }
    Ok(j)
}

// ---- minions (+ spectres) -------------------------------------------------------------------------------------------

const MINION: S = S::Obj(
    "minion",
    &[
        req("name", S::Str),
        opt("monsterTags", S::Arr(&S::Str)),
        req("life", S::Num),
        opt("energyShield", S::Num),
        opt("armour", S::Num),
        opt("evasion", S::Num),
        req("fireResist", S::Num),
        req("coldResist", S::Num),
        req("lightningResist", S::Num),
        req("chaosResist", S::Num),
        req("damage", S::Num),
        req("damageSpread", S::Num),
        opt("damageFixup", S::Num),
        req("attackTime", S::Num),
        req("attackRange", S::Num),
        req("accuracy", S::Num),
        opt("limit", S::Str),
        opt("baseDamageIgnoresAttackSpeed", S::Bool),
        opt("hostile", S::Bool),
        opt("lifeScaling", S::Str),
        opt("weaponType1", S::Str),
        opt("weaponType2", S::Str),
        // 5606cc64f (Spectre Library, Build.lua:1690-2024 and MinionListControl.lua:79-101): absent before it.
        opt("baseMovementSpeed", S::Num),
        opt("monsterCategory", S::Str),
        opt("itemisedCorpse", S::Bool),
        opt("extraFlags", S::Obj("minion extraFlags", &[opt("recommendedSpectre", S::Bool)])),
        opt("spawnLocation", S::Arr(&S::Str)),
        req("skillList", S::Arr(&S::Str)),
        req("modList", S::Arr(&S::Mod)),
    ],
);

/// Data.lua merges Spectres.lua into data.minions; data.spectres holds the very same tables -> only their ids are written.
pub fn minions(c: &mut Ctx) -> R<J> {
    const FILE: &str = "minions.json";
    let m = one(c, FILE, "minions", &S::Rec(&MINION))?;
    let all = c.d.t(c.table(root(c, "minions")?, "data.minions")?);
    let mut ids = Vec::new();
    for (id, v) in c.str_entries(root(c, "spectres")?, "data.spectres")? {
        if all.get(id) != Some(v) {
            return Err(format!("data.spectres[{id}] is not data.minions[{id}]"));
        }
        ids.push(id.to_owned());
    }
    ids.sort();
    c.count(FILE, "spectres", ids.len());
    Ok(J::Obj(vec![("minions".into(), m), ("spectreIds".into(), J::Arr(ids.into_iter().map(J::Str).collect()))]))
}

// ---- world areas (5606cc64f: Data/WorldAreas.lua, Data.lua:1506-1507; read by MinionListControl.lua:148) ---------------

const WORLD_AREA: S = S::Obj(
    "world area",
    &[
        req("name", S::Str),
        req("baseName", S::Str),
        opt("description", S::Arr(&S::Str)),
        req("tags", S::Arr(&S::Str)),
        req("act", S::Num),
        req("level", S::Num),
        req("isMap", S::Bool),
        req("monsterVarieties", S::Arr(&S::Str)),
        opt("bossVarieties", S::Arr(&S::Str)),
    ],
);

/// `data.worldAreas`: id -> area; an upstream without it (before 5606cc64f) writes no file.
pub fn world_areas(c: &mut Ctx) -> R<J> {
    if c.d.path("data", &["worldAreas"]).is_none() {
        return Ok(J::Null);
    }
    one(c, "world-areas.json", "worldAreas", &S::Rec(&WORLD_AREA))
}

// ---- bosses, pantheons, foulborn map, abyss notable names: one record map each ----------------------------------------

pub fn bosses(c: &mut Ctx) -> R<J> {
    const BOSS: S = S::Obj("boss", &[req("armourMult", S::Num), req("evasionMult", S::Num), req("isUber", S::Bool)]);
    one(c, "bosses.json", "bosses", &S::Rec(&BOSS))
}

pub fn pantheons(c: &mut Ctx) -> R<J> {
    const MOD: S = S::Obj("pantheon mod", &[req("line", S::Str), req("value", S::Arr(&S::Num))]);
    const SOUL: S = S::Obj("pantheon soul", &[req("name", S::Str), req("mods", S::Arr(&MOD))]);
    const GOD: S = S::Obj("pantheon", &[req("isMajorGod", S::Bool), req("souls", S::Arr(&SOUL))]);
    one(c, "pantheons.json", "pantheons", &S::Rec(&GOD))
}

pub fn foulborn_map(c: &mut Ctx) -> R<J> {
    one(c, "foulborn-map.json", "foulbornMap", &S::Rec(&S::Rec(&S::Str)))
}

pub fn abyss_notable_names(c: &mut Ctx) -> R<J> {
    one(c, "abyss-notable-names.json", "abyssNotableNames", &S::Rec(&S::Str))
}

// ---- boss skills -------------------------------------------------------------------------------------------------------

fn num_or_empty(c: &mut Ctx, v: &V, at: &str) -> R<J> {
    // BossSkills.lua "Maven Fireball".DamagePenetrations.FirePen = "" -- kept verbatim.
    match v {
        V::Num(n) if n.is_finite() => Ok(J::Num(*n)),
        V::Str(s) if s.is_empty() => Ok(J::str("")),
        v => Err(format!("{at}: expected a number or \"\", got {}", c.show(v))),
    }
}

fn flag_or_num(c: &mut Ctx, v: &V, at: &str) -> R<J> {
    match v {
        V::Num(n) if n.is_finite() => Ok(J::Num(*n)),
        V::Str(s) if &**s == "flag" => Ok(J::str("flag")),
        v => Err(format!("{at}: expected a number or \"flag\", got {}", c.show(v))),
    }
}

const VAL_LABEL: S = S::Obj("val/label row", &[req("val", S::Str), req("label", S::Str)]);

pub fn boss_skills(c: &mut Ctx) -> R<J> {
    const FILE: &str = "boss-skills.json";
    const STATS: S = S::Rec(&S::Custom(flag_or_num));
    const SKILL: S = S::Obj(
        "boss skill",
        &[
            req("DamageType", S::Str),
            req("DamageMultipliers", S::Rec(&S::Tuple(&[S::Num, S::Num]))),
            opt("DamagePenetrations", S::Rec(&S::Custom(num_or_empty))),
            opt("UberDamagePenetrations", S::Rec(&S::Num)),
            opt("UberDamageMultiplier", S::Num),
            opt("UberSpeed", S::Num),
            req("speed", S::Num),
            opt("critChance", S::Num),
            opt("earlierUber", S::Bool),
            opt("additionalStats", S::Obj("boss skill additionalStats", &[opt("base", STATS), opt("uber", STATS)])),
            req("tooltip", S::Str),
        ],
    );
    let skills = one(c, FILE, "bossSkills", &S::Rec(&SKILL))?;
    let list = Shaper { file: FILE }.conv(c, &S::Arr(&VAL_LABEL), root(c, "bossSkillsList")?, "data.bossSkillsList")?;
    Ok(J::Obj(vec![("skills".into(), skills), ("list".into(), list)]))
}

// ---- map mods ----------------------------------------------------------------------------------------------------------

fn num_tree(c: &mut Ctx, v: &V, at: &str) -> R<J> {
    match v {
        V::Num(n) if n.is_finite() => Ok(J::Num(*n)),
        V::Table(_) => Ok(J::Arr(c.seq(v, at)?.into_iter().map(|x| num_tree(c, x, at)).collect::<R<_>>()?)),
        v => Err(format!("{at}: expected a number or a list, got {}", c.show(v))),
    }
}

/// An AffixData entry: `apply` (a closure) becomes `hasApply`; an empty table is `{hasApply: false}`.
fn affix(c: &mut Ctx, v: &V, at: &str) -> R<J> {
    const FILE: &str = "map-mods.json";
    let sh = Shaper { file: FILE };
    let mut f = Fields::new(c, v, at)?;
    let has_apply = match f.take("apply") {
        None => false,
        Some(V::Func(_)) => true,
        Some(x) => return Err(format!("{at}.apply: expected a function, got {}", c.show(x))),
    };
    let mut o = vec![("hasApply".to_owned(), J::Bool(has_apply))];
    o.extend(sh.fields(
        c,
        &mut f,
        &[
            opt("type", S::Str),
            opt("label", S::Str),
            opt("tooltipLines", S::Arr(&S::Str)),
            opt("tooltip", S::Str),
            opt("values", S::Arr(&S::Custom(num_tree))),
        ],
        at,
    )?);
    f.finish(c, FILE, "map affix");
    Ok(J::Obj(o))
}

pub fn map_mods(c: &mut Ctx) -> R<J> {
    const MAP_MODS: S = S::Obj(
        "mapMods",
        &[req("AffixData", S::Rec(&S::Custom(affix))), req("Prefix", S::Arr(&VAL_LABEL)), req("Suffix", S::Arr(&VAL_LABEL))],
    );
    let j = one(c, "map-mods.json", "mapMods", &MAP_MODS)?;
    Ok(j)
}

// ---- cluster jewels ------------------------------------------------------------------------------------------------------

pub fn cluster_jewels(c: &mut Ctx) -> R<J> {
    const FILE: &str = "cluster-jewels.json";
    const SKILL: S = S::Obj(
        "cluster jewel skill",
        &[
            req("name", S::Str),
            req("icon", S::Str),
            // Can differ from its key (legacy "old_do_not_use_*" tags): kept verbatim.
            req("tag", S::Str),
            req("stats", S::Arr(&S::Str)),
            req("enchant", S::Arr(&S::Str)),
            opt("masteryIcon", S::Str),
        ],
    );
    const SIZE: S = S::Obj(
        "cluster jewel size",
        &[
            req("size", S::Str),
            req("sizeIndex", S::Num),
            req("minNodes", S::Num),
            req("maxNodes", S::Num),
            req("smallIndicies", S::Arr(&S::Num)),
            req("notableIndicies", S::Arr(&S::Num)),
            req("socketIndicies", S::Arr(&S::Num)),
            req("totalIndicies", S::Num),
            req("skills", S::Rec(&SKILL)),
        ],
    );
    const INFO: S = S::Obj("cluster notable info", &[req("jewelTypes", S::Arr(&S::Str)), req("size", S::Rec(&S::True))]);
    let sh = Shaper { file: FILE };
    let w = root(c, "clusterJewels")?;
    let mut f = Fields::new(c, w, "data.clusterJewels")?;
    let mut o = sh.fields(
        c,
        &mut f,
        &[
            req("jewels", S::Rec(&SIZE)),
            req("keystones", S::Arr(&S::Str)),
            req("notableSortOrder", S::Rec(&S::Num)),
            req("orbitOffsets", S::Rec(&S::Rec(&S::Num))),
        ],
        "data.clusterJewels",
    )?;
    let info = sh.conv(c, &S::Rec(&INFO), root(c, "clusterJewelInfoForNotable")?, "data.clusterJewelInfoForNotable")?;
    if let J::Obj(x) = &info {
        c.count(FILE, "infoForNotable", x.len());
    }
    o.push(("infoForNotable".into(), info));
    // ClusterJewels.lua (07fa089bf on): Putrid Seed (Blightcore) node -> enchant index.
    o.extend(sh.fields(c, &mut f, &[opt("blightcoreNotableIndex", S::Rec(&S::Num))], "data.clusterJewels")?);
    f.finish(c, FILE, "clusterJewels");
    Ok(J::Obj(o))
}

// ---- node id list -----------------------------------------------------------------------------------------------------------

/// NodeIndexMapping.lua: node id -> {index, size}, plus `size`, `sizeNotable` and `localIdToGlobalId` in the same table.
pub fn node_id_list(c: &mut Ctx) -> R<J> {
    const FILE: &str = "node-id-list.json";
    const NODE: S = S::Obj("node", &[req("index", S::Num), req("size", S::Num)]);
    let sh = Shaper { file: FILE };
    let w = root(c, "nodeIDList")?;
    let t = c.d.t(c.table(w, "data.nodeIDList")?);
    let (mut size, mut size_notable, mut local) = (None, None, None);
    let mut nodes = Vec::new();
    for (k, v) in &t.entries {
        match k {
            V::Num(_) => {
                let k = key_text(k, "data.nodeIDList")?;
                let at = format!("data.nodeIDList[{k}]");
                nodes.push((k, sh.conv(c, &NODE, v, &at)?));
            }
            V::Str(s) if &**s == "size" => size = Some(sh.conv(c, &S::Num, v, "data.nodeIDList.size")?),
            V::Str(s) if &**s == "sizeNotable" => size_notable = Some(sh.conv(c, &S::Num, v, "data.nodeIDList.sizeNotable")?),
            V::Str(s) if &**s == "localIdToGlobalId" => {
                let at = "data.nodeIDList.localIdToGlobalId";
                for (i, e) in c.seq(v, at)?.into_iter().enumerate() {
                    // Cross-check: each map's own `size` is its entry count.
                    let et = c.d.t(c.table(e, at)?);
                    let n = et.entries.iter().filter(|(k, _)| k.as_str() != Some("size")).count();
                    if et.get("size").and_then(V::as_num) != Some(n as f64) {
                        return Err(format!("{at}[{}].size is not its {n} entries", i + 1));
                    }
                }
                local = Some(sh.conv(c, &S::Arr(&S::Rec(&S::Num)), v, at)?);
            }
            k => c.drift(FILE, "nodeIDList", format!("unknown key '{}'", key_text(k, "data.nodeIDList")?)),
        }
    }
    c.count(FILE, "nodes", nodes.len());
    Ok(J::Obj(vec![
        ("nodes".into(), J::Obj(nodes)),
        ("size".into(), size.ok_or("data.nodeIDList.size missing")?),
        ("sizeNotable".into(), size_notable.ok_or("data.nodeIDList.sizeNotable missing")?),
        ("localIdToGlobalId".into(), local.ok_or("data.nodeIDList.localIdToGlobalId missing")?),
    ]))
}
