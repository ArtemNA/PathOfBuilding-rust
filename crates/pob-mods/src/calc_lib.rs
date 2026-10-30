//! The leaf helpers of `Modules/CalcTools.lua` that EvalMod depends on (MOD-CORE §8; port of pob-ts `src/calc/calcLib.ts`):
//! gem type tests (`SocketedIn.keyword`) and the gem-name -> gameId lookup (`SkillName.includeTransfigured`).

use pob_lua::string::lower;

use crate::data::gems;
use crate::types::Gem;
use crate::value::Value;

/// CalcTools.lua:279-292 `calcLib.getGameIdFromGemName(gemName, dropVaal)`: nil for a non-string or unknown name.
pub fn get_game_id_from_gem_name(gem_name: &Value, drop_vaal: bool) -> Option<&'static str> {
    // CalcTools.lua:280: `if type(gemName) ~= "string" then return end`
    let name = gem_name.as_str()?;
    let data = gems();
    let gem_id = data.gem_for_base_name.get(&lower(name))?;
    let gem = data
        .gems
        .get(gem_id)
        .unwrap_or_else(|| panic!("Modules/CalcTools.lua:286: data.gems['{gem_id}'] is nil"));
    if drop_vaal && gem.vaal_gem {
        // CalcTools.lua:287 -- every one of the 101 Vaal gems has a base gem (MOD-CORE §7.13).
        let base = data
            .gem_vaal_gem_id_for_base_gem_id
            .get(gem_id)
            .and_then(|b| data.gems.get(b))
            .unwrap_or_else(|| panic!("Modules/CalcTools.lua:287: no base gem for '{gem_id}'"));
        return Some(&base.game_id);
    }
    Some(&gem.game_id)
}

/// CalcTools.lua:299-303 `calcLib.isGemIdSame`: both names resolve and to the same gameId.
pub fn is_gem_id_same(gem_name: &Value, type_name: &Value, drop_vaal: bool) -> bool {
    match (
        get_game_id_from_gem_name(gem_name, drop_vaal),
        get_game_id_from_gem_name(type_name, drop_vaal),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// CalcTools.lua:164-176 `calcLib.gemIsType(gem, type, includeTransfigured)` -- the `or` chain in order. The call site
/// (ModStore.lua:780) reduces it with `or false`, so a boolean is all anyone sees.
pub fn gem_is_type(gem: &Gem, ty: &str, include_transfigured: bool) -> bool {
    let tag = |t: &str| gem.tags.get(t).copied().unwrap_or(false);
    let skill_alias = matches!(ty, "active skill" | "grants_active_skill" | "skill");
    let lower_name = lower(&gem.name);
    ty == "all"
        || (ty == "elemental" && (tag("fire") || tag("cold") || tag("lightning")))
        || (ty == "aoe" && tag("area"))
        || (ty == "trap or mine" && (tag("trap") || tag("mine")))
        || (skill_alias && tag("grants_active_skill") && !tag("support"))
        || (ty == "non-vaal" && !tag("vaal"))
        || (ty == "non-exceptional" && !tag("exceptional"))
        || ty == lower_name
        // `gem.name:lower():gsub("^vaal ", "")` -- the first result only.
        || ty == lower_name.strip_prefix("vaal ").unwrap_or(&lower_name)
        || (include_transfigured && is_gem_id_same(&Value::Str(gem.name.clone()), &Value::str(ty), true))
        || (!skill_alias && tag(ty))
}
