//! Modules/ItemTools.lua:36-94 and :96-355 -- `itemLib.applyValueScalar`, `itemLib.formatValue`, `isZeroValueLine` and
//! `itemLib.applyRange`, line for line. Every number -> string goes through `tostring` (%.14g, L1) and every
//! `string.format` through `pob_lua::format` (L2): PoB re-parses these strings, so one digit changes the mods.
//! Executable spec: pob-ts `src/items/applyRange.ts`.
//!
//! project_15's ">12 numbers" guard on `findScalableLine` is NOT reproduced (bug-compat, CALC-ARCH §5).

use pob_data::mod_scalability::{ModScalabilityEntry, mod_scalability};
use pob_mods::Value;

use crate::lua::{always_positive_round, arith, floor_symmetric, gsub, gsub_fn, matches, round, round_symmetric, tonumber, tostring};

const IT: &str = "Modules/ItemTools.lua";

/// ItemTools.lua:37-57 `itemLib.applyValueScalar`.
pub fn apply_value_scalar(
    line: &str,
    value_scalar: Option<f64>,
    base_value_scalar: Option<f64>,
    numbers: f64,
    precision: Option<f64>,
) -> String {
    // `type(valueScalar) == "number" and valueScalar or 1`
    let value_scalar = value_scalar.unwrap_or(1.0);
    if value_scalar == 1.0 && base_value_scalar.is_none_or(|b| b == 1.0) {
        return line.to_owned();
    }
    let power = 10f64.powf(precision.unwrap_or(0.0));
    let scale_value = |num: &str| -> String {
        let mut value = arith(num, IT);
        if let Some(b) = base_value_scalar
            && b != 1.0
        {
            value = round(value * b * power, None) / power;
        }
        // `precision and 0 or 0.001` -- a number is truthy, 0 included (L4)
        let nudge = if precision.is_some() { 0.0 } else { 0.001 };
        tostring((value * value_scalar * power + nudge).floor() / power)
    };
    let n = numbers as i64;
    if precision.is_some() {
        return gsub_fn(line, "(%d+%.?%d*)", Some(n), |c| Some(scale_value(&c[0])));
    }
    gsub_fn(line, "(%d+)([^%.])", Some(n), |c| Some(scale_value(&c[0]) + &c[1]))
}

/// ItemTools.lua:61-78 `itemLib.formatValue`. `value` is the captured number string (Lua coerces it in `value * precision`).
pub fn format_value(
    value: &str,
    base_value_scalar: Option<f64>,
    value_scalar: Option<f64>,
    precision: f64,
    display_precision: Option<f64>,
    if_required: bool,
) -> String {
    let mut v = round_symmetric(arith(value, "Modules/ItemTools.lua:62") * precision, None);
    if let Some(b) = base_value_scalar
        && b != 1.0
    {
        v = always_positive_round(v * b, None);
    }
    if let Some(s) = value_scalar
        && s != 1.0
    {
        v = floor_symmetric(v * s, None);
    }
    v /= precision;
    if let Some(dp) = display_precision {
        v = round_symmetric(v, Some(dp));
    }
    match display_precision {
        Some(dp) if !if_required => {
            let fmt = format!("%.{}f", tostring(dp));
            pob_lua::format(&fmt, &[pob_lua::Arg::Num(v)]).unwrap_or_else(|e| panic!("{IT}:72: {e}"))
        }
        // `tostring(value, displayPrecision)` -- tostring ignores its second argument.
        Some(_) => tostring(v),
        None => {
            // `math.log(precision, 10)` in LuaJIT: `lj_vm_log2(x) * (1 / lj_vm_log2(10))`; `m_floor(x + 0.001)`.
            let log10 = precision.log2() * (1.0 / 10f64.log2());
            let dec = pob_lua::number::min2(2.0, (log10 + 0.001).floor());
            tostring(round_symmetric(v, Some(dec)))
        }
    }
}

/// ItemTools.lua:83-87.
fn antonym_func(num: &str, word: &str) -> String {
    let antonym = match word {
        "increased" => Some("reduced"),
        "reduced" => Some("increased"),
        "more" => Some("less"),
        "less" => Some("more"),
        _ => None,
    };
    match antonym {
        Some(a) => format!("{num} {a}"),
        None => format!("-{num} {word}"),
    }
}

/// ItemTools.lua:89-91 `itemLib.isZeroValueLine`.
pub fn is_zero_value_line(line: &str) -> bool {
    matches(line, "^%+?0%%? ")
        || (matches(line, " %+?0%%? ") && !matches(line, "0 to [1-9]") && !matches(line, "0%% to %d+%%"))
        || matches(line, " 0%-0 ")
        || matches(line, " 0 to 0 ")
}

/// `data.modScalability[line:gsub("+#", "#")]` -- "+" leads the pattern, so Lua reads it as a literal: the gsub is the
/// plain left-to-right replacement of the two bytes "+#" (a hot path, so done without the pattern engine).
fn scalability_of(line: &str) -> Option<&'static Vec<ModScalabilityEntry>> {
    mod_scalability().get(&line.replace("+#", "#"))
}

/// ItemTools.lua:120-131 `replaceNthInstance(input, "#", replacement, n)`: `gsub` of the literal byte "#" with a function
/// that returns `replacement` for the n-th match (the replacement is the function's return value, so no `%` processing).
fn replace_nth_hash(input: &str, replacement: &str, n: usize) -> String {
    match input.match_indices('#').nth(n.wrapping_sub(1)) {
        Some((at, _)) => format!("{}{replacement}{}", &input[..at], &input[at + 1..]),
        None => input.to_owned(),
    }
}

/// ItemTools.lua:120-179 `findScalableLine`: substitute the captured values back, most substitutions first, combinations
/// in increasing-index lexicographic order (the Lua recursion); the first line `data.modScalability` knows wins.
fn find_scalable_line(line: &str, values: &[String]) -> Option<(String, Vec<String>)> {
    fn check(line: &str, values: &[String], start: usize, num: usize, indices: &mut Vec<usize>) -> Option<(String, Vec<String>)> {
        if indices.len() == num {
            let mut modified = line.to_owned();
            for (substituted, &i) in indices.iter().enumerate() {
                modified = replace_nth_hash(&modified, &values[i - 1], i - substituted);
            }
            if scalability_of(&modified).is_some() {
                let remaining = values
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !indices.contains(&(i + 1)))
                    .map(|(_, v)| v.clone())
                    .collect();
                return Some((modified, remaining));
            }
            return None;
        }
        for j in start..=values.len() {
            indices.push(j);
            if let Some(found) = check(line, values, j + 1, num, indices) {
                return Some(found);
            }
            indices.pop();
        }
        None
    }
    for i in (1..=values.len()).rev() {
        if let Some(found) = check(line, values, 1, i, &mut Vec::new()) {
            return Some(found);
        }
    }
    scalability_of(line).map(|_| (line.to_owned(), values.to_vec()))
}

#[derive(Default)]
struct Formats {
    precision: Option<f64>,
    display_precision: Option<f64>,
    if_required: bool,
}

/// ItemTools.lua:185-285 -- the GGG stat-format ids; the last matching format in the list wins (each sets its fields).
fn formats_of(s: &ModScalabilityEntry) -> Formats {
    let mut f = Formats::default();
    let Some(formats) = &s.formats else { return f };
    for format in formats {
        let (p, dp, ifr): (f64, Option<f64>, bool) = match format.as_str() {
            "divide_by_two_0dp" => (2.0, Some(0.0), true),
            "divide_by_three" => (3.0, None, false),
            "divide_by_four" => (4.0, None, false),
            "divide_by_five" => (5.0, None, false),
            "divide_by_six" => (6.0, None, false),
            "divide_by_ten_0dp" => (10.0, Some(0.0), false),
            "divide_by_ten_1dp" => (10.0, Some(1.0), false),
            "divide_by_ten_1dp_if_required" => (10.0, Some(1.0), true),
            "divide_by_twelve" => (12.0, None, false),
            "divide_by_fifteen_0dp" => (15.0, Some(0.0), false),
            "divide_by_twenty" => (20.0, None, false),
            "divide_by_twenty_then_double_0dp" => (10.0, Some(0.0), false),
            "divide_by_one_hundred" | "divide_by_one_hundred_and_negate" => (100.0, None, false),
            "divide_by_one_hundred_0dp" => (100.0, Some(0.0), false),
            "divide_by_one_hundred_1dp" => (100.0, Some(1.0), false),
            "divide_by_one_hundred_2dp" => (100.0, Some(2.0), false),
            "divide_by_one_hundred_2dp_if_required" => (100.0, Some(2.0), true),
            "divide_by_one_thousand" => (1000.0, None, false),
            "divide_by_ten_thousand_1dp" => (10000.0, Some(1.0), false),
            "per_minute_to_per_second" => (60.0, None, false),
            "per_minute_to_per_second_0dp" => (60.0, Some(0.0), false),
            "per_minute_to_per_second_1dp" => (60.0, Some(1.0), false),
            "per_minute_to_per_second_2dp" => (60.0, Some(2.0), false),
            "per_minute_to_per_second_2dp_if_required" => (60.0, Some(2.0), true),
            "milliseconds_to_seconds" | "milliseconds_to_seconds_halved" => (1000.0, None, false),
            "milliseconds_to_seconds_0dp" => (1000.0, Some(0.0), false),
            "milliseconds_to_seconds_1dp" => (1000.0, Some(1.0), false),
            "milliseconds_to_seconds_2dp" => (1000.0, Some(2.0), false),
            "locations_to_metres" => (10.0, Some(1.0), false),
            "milliseconds_to_seconds_2dp_if_required" => (1000.0, Some(2.0), true),
            "deciseconds_to_seconds" => (10.0, None, false),
            _ => continue,
        };
        // Each upstream branch only ASSIGNS the fields it names; the others keep earlier values.
        f.precision = Some(p);
        if let Some(dp) = dp {
            f.display_precision = Some(dp);
        }
        if ifr {
            f.if_required = true;
        }
    }
    f
}

/// ItemTools.lua:96-355 `itemLib.applyRange(line, range, valueScalar, baseValueScalar)` for a numeric `range` (the table
/// form is the ItemsTab affix UI's, not reachable from a parsed item).
pub fn apply_range(line: &str, range: f64, value_scalar: Option<f64>, base_value_scalar: Option<f64>) -> String {
    let mut values: Vec<String> = Vec::new();
    let stripped = gsub_fn(line, "([%+-]?)%((%-?%d+%.?%d*)%-(%-?%d+%.?%d*)%)", None, |c| {
        let (sign, min, max) = (&c[0], &c[1], &c[2]);
        // `min + valueRange * (tonumber(max) - min)`
        let mn = arith(min, IT);
        let mut value = mn + range * (arith(max, IT) - mn);
        if sign == "-" {
            value *= -1.0;
        }
        Some(if sign == "+" && value > 0.0 {
            format!("{sign}{}", tostring(value))
        } else {
            tostring(value)
        })
    });
    let stripped = gsub_fn(&stripped, "%-(%d+%.?%d*%%) (%a+)", None, |c| Some(antonym_func(&c[0], &c[1])));
    let stripped = gsub_fn(&stripped, "(%-?%d+%.?%d*)", None, |c| {
        values.push(c[0].clone());
        Some("#".to_owned())
    });

    if let Some((mut scalable_line, mut scalable_values)) = find_scalable_line(&stripped, &values) {
        let entries = scalability_of(&scalable_line).expect("found above");
        for (i, scalability) in entries.iter().enumerate() {
            let f = formats_of(scalability);
            let Some(value) = scalable_values.get(i) else {
                panic!("{IT}:62: attempt to perform arithmetic on local 'value' (a nil value)");
            };
            let scaled =
                if scalability.is_scalable && (base_value_scalar.is_some_and(|b| b != 1.0) || value_scalar.is_some_and(|s| s != 1.0)) {
                    format_value(
                        value,
                        base_value_scalar,
                        value_scalar,
                        f.precision.unwrap_or(1.0),
                        f.display_precision,
                        f.if_required,
                    )
                } else {
                    format_value(
                        value,
                        Some(1.0),
                        Some(1.0),
                        f.precision.unwrap_or(1.0),
                        f.display_precision,
                        f.if_required,
                    )
                };
            scalable_values[i] = scaled;
        }
        for replacement in &scalable_values {
            scalable_line = gsub(&scalable_line, "#", replacement, Some(1));
        }
        return scalable_line;
    }

    // ItemTools.lua:301-355 -- no scalability data: the pre-3.26 implementation.
    let mut precision_same = true;
    let test_line = if crate::lua::find_plain(line, "-").is_none() {
        line.to_owned()
    } else {
        let t = gsub_fn(line, "(%+?)%((%-?%d+%.?%d*)%-(%-?%d+%.?%d*)%)", None, |c| {
            let min = arith(&c[1], IT);
            let max_precision = min + range * (arith(&c[2], IT) - min);
            let min_precision = (max_precision + 0.5).floor();
            if min_precision != max_precision {
                precision_same = false;
            }
            Some(format!(
                "{}{}",
                if min_precision < 0.0 { "" } else { &c[0] },
                tostring(min_precision)
            ))
        });
        gsub_fn(&t, "%-(%d+%%) (%a+)", None, |c| Some(antonym_func(&c[0], &c[1])))
    };
    if precision_same && value_scalar.is_none_or(|s| s == 1.0) && base_value_scalar.is_none_or(|b| b == 1.0) {
        return test_line;
    }

    // MINEFIELD D18: a ModCache miss here leaves `precision` unset and changes the number.
    let mut precision: Option<f64> = None;
    let parsed = pob_parser::parse_mod_line(&test_line, false);
    if let (Some(mods), None) = (&parsed.mod_list, &parsed.extra) {
        // `pairs(modList)` over an array: 1..n
        for m in mods {
            // `subMod = mod.value.mod` when the value is a table with a `mod`
            let (sub_value, sub_name, sub_type) = match m.value.get("mod") {
                Value::Mod(inner) if m.value.is_table() => (inner.value.clone(), inner.name.clone(), inner.ty),
                _ => (m.value.clone(), m.name.clone(), m.ty),
            };
            if let (Value::Num(_), Some(ty)) = (&sub_value, sub_type)
                && let Some(p) = pob_data::rules::rules()
                    .high_precision_mods
                    .get(&*sub_name)
                    .and_then(|by_type| by_type.get(ty.as_str()))
            {
                precision = Some(*p);
            }
        }
    }
    if precision.is_none() && matches(line, "(%d+%.%d*)") {
        precision = Some(pob_data::rules::rules().default_high_precision);
    }
    let mut numbers = 0.0;
    let power = 10f64.powf(precision.unwrap_or(0.0));
    let scaled = gsub_fn(line, "(%+?)%((%-?%d+%.?%d*)%-(%-?%d+%.?%d*)%)", None, |c| {
        numbers += 1.0;
        let (min, max) = (tonumber(&c[1]).expect("number"), tonumber(&c[2]).expect("number"));
        let num_val = ((min + range * (max - min)) * power + 0.5).floor() / power;
        Some(format!("{}{}", if num_val < 0.0 { "" } else { &c[0] }, tostring(num_val)))
    });
    let scaled = gsub_fn(&scaled, "%-(%d+%%) (%a+)", None, |c| Some(antonym_func(&c[0], &c[1])));
    if numbers == 0.0 && matches(&scaled, "(%d+%.?%d*)%%? ") {
        numbers = 1.0;
    }
    apply_value_scalar(&scaled, value_scalar, base_value_scalar, numbers, precision)
}
