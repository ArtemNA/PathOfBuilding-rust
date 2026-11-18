//! `data.gems` + the gem lookups (Modules/Data.lua setupGem and after) -> gems.json. Reader: crates/pob-data/src/gems.rs.

use crate::ctx::{Ctx, Obj, R};
use crate::dump::V;
use crate::fields::Fields;
use crate::js::J;

const FILE: &str = "gems.json";

pub fn convert(c: &mut Ctx) -> R<J> {
    let d = c.d;
    let data = d.root("data");
    let get = |k: &str| d.path("data", &[k]).ok_or_else(|| format!("{FILE}: data.{k} missing"));
    let skills = d.t(c.table(get("skills")?, "data.skills")?);
    let _ = data;

    let mut gems = Vec::new();
    let wgems = get("gems")?;
    for (id, g) in c.str_entries(wgems, "data.gems")? {
        let at = format!("data.gems[{id}]");
        let mut f = Fields::new(c, g, &at)?;
        let mut o = Obj::default();
        let own_id = f.string(c, "id")?;
        if own_id != id {
            return Err(format!("{at}.id = {own_id}, not its key"));
        }
        o.put("id", J::Str(own_id));
        o.put("gameId", J::Str(f.string(c, "gameId")?));
        o.put("variantId", J::Str(f.string(c, "variantId")?));
        o.put("name", J::Str(f.string(c, "name")?));
        o.opt("baseTypeName", f.opt_string(c, "baseTypeName")?.map(J::Str));
        let ge = f.string(c, "grantedEffectId")?;
        o.put("grantedEffectId", J::Str(ge.clone()));
        let sge = f.opt_string(c, "secondaryGrantedEffectId")?;
        o.opt("secondaryGrantedEffectId", sge.clone().map(J::Str));
        o.opt("secondaryEffectName", f.opt_string(c, "secondaryEffectName")?.map(J::Str));
        o.opt("vaalGem", f.opt_bool(c, "vaalGem")?.map(J::Bool));
        let tags = f.req("tags")?;
        let tags = c.str_entries(tags, &format!("{at}.tags"))?.into_iter().map(|(k, v)| match v {
            V::Bool(true) => Ok((k.to_owned(), J::Bool(true))),
            v => Err(format!("{at}.tags.{k}: expected true, got {}", c.show(v))),
        });
        o.put("tags", J::Obj(tags.collect::<R<_>>()?));
        o.put("tagString", J::Str(f.string(c, "tagString")?));
        for k in ["reqStr", "reqDex", "reqInt", "naturalMaxLevel"] {
            o.put(k, J::Num(f.num(c, k)?));
        }
        // Back-references Data.lua sets into data.skills: checked by identity, not written.
        let skill = |name: &str| skills.get(name).cloned();
        let check = |what: &str, v: Option<&V>, want: &str| -> R<()> {
            match (v, skill(want)) {
                (Some(a), Some(b)) if *a == b => Ok(()),
                _ => Err(format!("{at}.{what} is not data.skills[{want}]")),
            }
        };
        check("grantedEffect", f.take("grantedEffect"), &ge)?;
        if let Some(s) = &sge {
            check("secondaryGrantedEffect", f.take("secondaryGrantedEffect"), s)?;
        }
        let _ = f.take("grantedEffectList");
        f.finish(c, FILE, "gem");
        gems.push((id.to_owned(), o.done()));
    }
    c.count(FILE, "gems", gems.len());

    // gemForSkill: keyed by the skill TABLE -> re-keyed by that skill's id, sorted.
    let mut gfs = Vec::new();
    for (k, v) in &d.t(c.table(get("gemForSkill")?, "data.gemForSkill")?).entries {
        let t = c.table(k, "data.gemForSkill key")?;
        let sid = c.string(d.t(t).get("id").unwrap_or(&V::Nil), "data.gemForSkill key.id")?;
        gfs.push((sid, J::Str(c.string(v, "data.gemForSkill value")?)));
    }
    gfs.sort_by(|a, b| a.0.cmp(&b.0));

    let string_map = |c: &Ctx, k: &str| -> R<J> {
        Ok(J::Obj(
            c.str_entries(get(k)?, k)?.into_iter().map(|(k2, v)| Ok((k2.to_owned(), J::Str(c.string(v, k)?)))).collect::<R<_>>()?,
        ))
    };

    let mut by_game = Vec::new();
    for (game_id, variants) in c.str_entries(get("gemsByGameId")?, "data.gemsByGameId")? {
        let mut o = Vec::new();
        for (variant, g) in c.str_entries(variants, game_id)? {
            let gid = c.string(d.t(c.table(g, "gemsByGameId value")?).get("id").unwrap_or(&V::Nil), "gem.id")?;
            o.push((variant.to_owned(), J::Str(gid)));
        }
        by_game.push((game_id.to_owned(), J::Obj(o)));
    }

    let mut out = Obj::default();
    out.put("gems", J::Obj(gems));
    out.put("gemForSkill", J::Obj(gfs));
    out.put("gemForBaseName", string_map(c, "gemForBaseName")?);
    out.put("gemsByGameId", J::Obj(by_game));
    out.put("gemGrantedEffectIdForVaalGemId", string_map(c, "gemGrantedEffectIdForVaalGemId")?);
    out.put("gemVaalGemIdForBaseGemId", string_map(c, "gemVaalGemIdForBaseGemId")?);
    Ok(out.done())
}
