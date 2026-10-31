//! Modules/CalcTools.lua:16-39 -- the three aggregation helpers every calc module uses (pob-ts `src/calc/calcLib.ts`). The
//! gem helpers of CalcTools (`gemIsType`, `getGameIdFromGemName`, `isGemIdSame`) live in `pob_mods::calc_lib`.

use pob_mods::{LuaResult, ModCfg, ModStore, ModType};

/// CalcTools.lua:16-18 `calcLib.mod` -- `(1 + Sum("INC")/100) * More()`.
pub fn mod_(store: &ModStore, cfg: Option<&ModCfg>, names: &[&str]) -> LuaResult<f64> {
    Ok((1.0 + store.sum(ModType::Inc, cfg, names)? / 100.0) * store.more(cfg, names)?)
}

/// CalcTools.lua:25-29 `calcLib.mods` -- the same two halves, returned separately.
pub fn mods(store: &ModStore, cfg: Option<&ModCfg>, names: &[&str]) -> LuaResult<(f64, f64)> {
    let inc = 1.0 + store.sum(ModType::Inc, cfg, names)? / 100.0;
    let more = store.more(cfg, names)?;
    Ok((inc, more))
}

/// CalcTools.lua:32-39 `calcLib.val(modStore, name, cfg)` -- name BEFORE cfg; a zero base short-circuits to 0.
pub fn val(store: &ModStore, name: &str, cfg: Option<&ModCfg>) -> LuaResult<f64> {
    let base = store.sum(ModType::Base, cfg, &[name])?;
    if base != 0.0 {
        Ok(base * mod_(store, cfg, &[name])?)
    } else {
        Ok(0.0)
    }
}
