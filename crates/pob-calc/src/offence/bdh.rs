//! R8.5: the file-level breakdown helpers of CalcOffence.lua (`calcRadiusBreakpoints` :197-224, `setMoltenStrikeTertiaryRadiusBreakdown`
//! :235-294, the `breakdown` side of `calcDamage` / `calcAilmentSourceDamage` :70-153) and the [`Off`] accessors of `actor.breakdown` /
//! `pass.breakdown`.

use pob_lua::number::round;

use super::helpers::{calc_area_mod, calc_molten_strike_tertiary_radius, calc_radius};
use super::{Off, PassOut};
use crate::breakdown::{BTable, BVal, Breakdown, row_table};
use crate::sf;

impl Off<'_> {
    /// `actor.breakdown` (nil unless `env.buildBreakdown`, CalcPerform.lua:1514-1521).
    pub fn bd(&mut self) -> Option<&mut Breakdown> {
        if !self.breakdown {
            return None;
        }
        self.st.player.breakdown.as_mut()
    }

    /// `breakdown[k] = t` on `actor.breakdown` (no-op without a breakdown).
    pub fn bd_put(&mut self, k: &str, t: BTable) {
        if let Some(b) = self.bd() {
            b.put(k, t);
        }
    }

    /// `breakdown[k] = v` (nil removes) on `actor.breakdown`.
    pub fn bd_set(&mut self, k: &str, v: Option<BVal>) {
        if let Some(b) = self.bd() {
            b.set(k, v);
        }
    }

    /// `pass.breakdown` (:2018/:2053/:2062): `breakdown.MainHand` / `breakdown.OffHand` for a hand pass, `breakdown` for the skill pass.
    pub fn pbd(&mut self, pi: usize) -> Option<&mut Breakdown> {
        let out = self.passes[pi].out;
        let g = self.bd()?;
        match out {
            PassOut::Global => Some(g),
            PassOut::Hand(h) => Some(g.module_mut(if h == 0 { "MainHand" } else { "OffHand" })),
        }
    }

    /// `breakdown[k] = t` on `pass.breakdown`.
    pub fn pbd_put(&mut self, pi: usize, k: &str, t: BTable) {
        if let Some(b) = self.pbd(pi) {
            b.put(k, t);
        }
    }
}

/// :197-224 `calcRadiusBreakpoints(baseRadius, incArea, moreArea, radiusMod)` -> `(inc, more, red, less)`; `None` = all nil (radius 0).
pub(crate) fn calc_radius_breakpoints(
    base_radius: f64,
    inc_area: f64,
    more_area: f64,
    radius_mod: Option<f64>,
) -> Option<(f64, f64, f64, f64)> {
    let radius = calc_radius(base_radius, calc_area_mod(inc_area, more_area), radius_mod);
    if radius <= 0.0 {
        return None;
    }
    let walk = |f: &dyn Fn(f64) -> f64, up: bool| {
        let mut bp = 0.0;
        loop {
            bp += 1.0;
            let new_radius = calc_radius(base_radius, f(bp), radius_mod);
            if (up && new_radius > radius) || (!up && new_radius < radius) {
                return bp;
            }
        }
    };
    let inc = walk(&|bp| calc_area_mod(inc_area + bp / 100.0, more_area), true);
    let red = walk(&|bp| calc_area_mod(inc_area - bp / 100.0, more_area), false);
    let more = walk(&|bp| calc_area_mod(inc_area, more_area * (1.0 + bp / 100.0)), true);
    let less = walk(&|bp| calc_area_mod(inc_area, more_area * (1.0 - bp / 100.0)), false);
    Some((inc, more, red, less))
}

/// :235-294 `setMoltenStrikeTertiaryRadiusBreakdown(breakdown, deadzoneRadius, baseRadius, label, incArea, moreArea, incSpd, moreSpd)`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn molten_strike_tertiary_breakdown(
    bd: &mut Breakdown,
    deadzone_radius: f64,
    base_radius: f64,
    label: Option<&str>,
    inc_area: f64,
    more_area: f64,
    inc_spd: f64,
    more_spd: f64,
) {
    let calc = |a: [f64; 4]| {
        let area_mod = round(
            round((inc_area + a[0] / 100.0) * more_area * (1.0 + a[1] / 100.0), Some(10.0)),
            Some(2.0),
        );
        let speed_mod = round(
            round((inc_spd + a[2] / 100.0) * more_spd * (1.0 + a[3] / 100.0), Some(10.0)),
            Some(2.0),
        );
        let dist = calc_molten_strike_tertiary_radius(base_radius, deadzone_radius, area_mod, speed_mod);
        (dist, area_mod, speed_mod)
    };
    let (current_dist, current_area_mod, current_speed_mod) = calc([0.0; 4]);
    let t = bd.table_or("AreaOfEffectRadiusTertiary", BTable::new);
    if let Some(l) = label {
        t.push(l);
    }
    t.push(" = (sqrt(R*R*a - r*r*(a-1)) - r) * s + r");
    t.push(sf!(" = %d", current_dist));
    if current_dist > 0.0 {
        let find = |sign: f64, idx: usize| {
            let mut args = [0.0; 4];
            loop {
                args[idx] += sign;
                let (new_dist, _, _) = calc(args);
                if new_dist != current_dist || new_dist == 0.0 {
                    return args[idx] * sign;
                }
            }
        };
        t.push(sf!(
            "^8Next AoE breakpoint: %d%% increased or %d%% more",
            find(1.0, 0),
            find(1.0, 1)
        ));
        t.push(sf!(
            "^8Next Proj. Speed breakpoint: %d%% increased or %d%% more",
            find(1.0, 2),
            find(1.0, 3)
        ));
        t.push(sf!(
            "^8Previous AoE breakpoint: %d%% increased or %d%% more",
            find(-1.0, 0),
            find(-1.0, 1)
        ));
        t.push(sf!(
            "^8Previous Proj. Speed breakpoint: %d%% increased or %d%% more",
            find(-1.0, 2),
            find(-1.0, 3)
        ));
    }
    let inputs = row_table(
        Some("Inputs"),
        &[("Variable", "name"), ("Value", "value"), ("Description", "description")],
    );
    for (k, v) in inputs.fields {
        t.fields.insert(k, v);
    }
    let rows = t.field_mut("rowList");
    for (name, value, desc) in [
        ("r", sf!("%d", deadzone_radius), "fixed deadzone radius"),
        ("R", sf!("%d", base_radius), "base outer radius"),
        ("a", sf!("%.2f", current_area_mod), "net AoE multiplier (scales area)"),
        (
            "s",
            sf!("%.2f", current_speed_mod),
            "net projectile speed multiplier (scales range)",
        ),
    ] {
        rows.push(crate::breakdown::row(&[
            ("name", name.to_owned()),
            ("value", value),
            ("description", desc.to_owned()),
        ]));
    }
    t.fields.insert("radius".into(), BVal::Num(current_dist));
}

impl Off<'_> {
    /// `pass.source[k]` as a Lua value.
    pub fn source_val(&self, pi: usize, k: &str) -> pob_mods::Value {
        match &self.passes[pi].source {
            super::Source::SkillData => self.sd(k),
            super::Source::Weapon(d) => d.get(k),
        }
    }
}

/// Takes `pass.breakdown[k]` (a table) out of the breakdown so it can be filled while `off` is borrowed; the caller puts it back
/// ([`Off::pbd_put`]). `None` without a breakdown (or when the entry is not a table).
pub(crate) fn take_pbd_table(off: &mut Off<'_>, pi: usize, k: &str) -> Option<BTable> {
    let bd = off.pbd(pi)?;
    match bd.get(k) {
        Some(BVal::Table(_)) => match bd.get_mut(k).map(|v| std::mem::replace(v, BVal::Bool(false))) {
            Some(BVal::Table(t)) => Some(t),
            _ => None,
        },
        _ => None,
    }
}
