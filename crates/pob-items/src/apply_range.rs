//! Modules/ItemTools.lua:36-94 and :96-355 -- `itemLib.applyValueScalar`, `itemLib.formatValue`, `isZeroValueLine` and
//! `itemLib.applyRange`, line for line. Every number -> string goes through `tostring` (%.14g, L1) and every
//! `string.format` through `pob_lua::format` (L2): PoB re-parses these strings, so one digit changes the mods.
//! Executable spec: pob-ts `src/items/applyRange.ts`.
//!
//! project_15's ">12 numbers" guard on `findScalableLine` is NOT reproduced (bug-compat, CALC-ARCH §5).

use std::sync::OnceLock;

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

/// The `data.modScalability` keys, byte-sorted: exact lookups and "does any key start with this" by binary search.
struct KeyIndex<'a> {
    sorted: Vec<&'a str>,
}

impl<'a> KeyIndex<'a> {
    fn new(keys: impl Iterator<Item = &'a str>) -> KeyIndex<'a> {
        let mut sorted: Vec<&str> = keys.collect();
        sorted.sort_unstable();
        KeyIndex { sorted }
    }

    fn contains(&self, key: &str) -> bool {
        self.sorted.binary_search(&key).is_ok()
    }

    /// Can a key still be reached from the normalised prefix `p`? A trailing `+` may yet vanish (`+#` -> `#`), so only the
    /// part before it is certain.
    fn may_extend(&self, p: &str) -> bool {
        let p = p.strip_suffix('+').unwrap_or(p);
        let at = self.sorted.partition_point(|k| *k < p);
        self.sorted.get(at).is_some_and(|k| k.starts_with(p))
    }
}

/// `+#` -> `#` as `line:gsub("+#", "#")` does, one `#` at a time: every `+#` pair ends at a `#` appended here. Returns
/// whether a `+` was dropped.
fn push_hash(norm: &mut String) -> bool {
    let dropped = norm.ends_with('+');
    if dropped {
        norm.pop();
    }
    norm.push('#');
    dropped
}

/// The walk of [`find_scalable_in`]: `segments` = the line split at `#`; `norm` = the normalised line so far.
struct Walk<'a> {
    keys: &'a KeyIndex<'a>,
    segments: &'a [&'a str],
    values: &'a [String],
    norm: String,
    chosen: Vec<usize>,
    best: Option<Vec<usize>>,
}

impl Walk<'_> {
    /// Both choices for the `#` number `at + 1` (0-based `at`), then the next one.
    fn go(&mut self, at: usize) {
        if at == self.values.len() {
            return self.leaf();
        }
        let keep = self.norm.len();
        self.norm.push_str(&self.values[at]);
        self.norm.push_str(self.segments[at + 1]);
        if self.keys.may_extend(&self.norm) {
            self.chosen.push(at + 1);
            self.go(at + 1);
            self.chosen.pop();
        }
        self.norm.truncate(keep);
        let dropped = push_hash(&mut self.norm);
        self.norm.push_str(self.segments[at + 1]);
        if self.keys.may_extend(&self.norm) {
            self.go(at + 1);
        }
        self.norm.truncate(keep - usize::from(dropped));
        if dropped {
            self.norm.push('+');
        }
    }

    /// Every value decided: the `#` left are literal text of the line. Keeps the upstream winner of the leaves seen.
    fn leaf(&mut self) {
        let mut full = self.norm.clone();
        for s in &self.segments[self.values.len() + 1..] {
            push_hash(&mut full);
            full.push_str(s);
        }
        if !self.keys.contains(&full) {
            return;
        }
        let better = self
            .best
            .as_ref()
            .is_none_or(|b| self.chosen.len() > b.len() || (self.chosen.len() == b.len() && self.chosen < *b));
        if better {
            self.best = Some(self.chosen.clone());
        }
    }
}

/// ItemTools.lua:191-256 `findScalableLine` (`checkSubstitutionCombinations`): upstream tries the subsets of `values` to
/// substitute back -- most substitutions first, equal sizes in increasing-index lexicographic order, the empty one last --
/// and the first line `keys` knows wins. Same result here without trying all 2^n subsets (a `Unique ID: <hash>` line reaches
/// this with n = 10..23):
///
/// * every subset is a leaf of this binary walk: left to right over the first n `#`, each substituted or kept. Upstream's
///   `replaceNthInstance(.., i - substituted)` addresses exactly the `#` number i of `line`, whether that `#` came from a
///   value or was already in the text, as a value holds no `#` (`(%-?%d+%.?%d*)`, which also gives n <= the number of `#`);
/// * a branch is cut only when no key starts with its normalised prefix. Normalising is left to right and every later piece
///   only appends, except that a trailing `+` can vanish before a `#`, which `may_extend` allows for. So every leaf below a
///   cut branch has a line no key equals: it could not have won;
/// * of the leaves that remain, the known ones are compared by upstream's own order (`Walk::leaf`), not by walk order.
///
/// No limit on n: `project_15`'s guard could change the result (see the top of this file).
fn find_scalable_in(keys: &KeyIndex<'_>, line: &str, values: &[String]) -> Option<(String, Vec<String>)> {
    let segments: Vec<&str> = line.split('#').collect();
    debug_assert!(segments.len() > values.len(), "every value left a `#` in the line");
    if !keys.may_extend(segments[0]) {
        return None;
    }
    let mut w = Walk {
        keys,
        segments: &segments,
        values,
        norm: segments[0].to_owned(),
        chosen: Vec::with_capacity(values.len()),
        best: None,
    };
    w.go(0);
    let chosen = w.best?;
    let mut modified = line.to_owned();
    for (substituted, &i) in chosen.iter().enumerate() {
        modified = replace_nth_hash(&modified, &values[i - 1], i - substituted);
    }
    let remaining = values
        .iter()
        .enumerate()
        .filter(|(i, _)| !chosen.contains(&(i + 1)))
        .map(|(_, v)| v.clone())
        .collect();
    Some((modified, remaining))
}

/// [`find_scalable_in`] over `data.modScalability`.
fn find_scalable_line(line: &str, values: &[String]) -> Option<(String, Vec<String>)> {
    static KEYS: OnceLock<KeyIndex<'static>> = OnceLock::new();
    let keys = KEYS.get_or_init(|| KeyIndex::new(mod_scalability().keys().map(String::as_str)));
    find_scalable_in(keys, line, values)
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
pub fn apply_range(
    line: &str,
    range: f64,
    value_scalar: Option<f64>,
    base_value_scalar: Option<f64>,
    mode: pob_parser::ParseMode,
) -> String {
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

    // MINEFIELD D18: in `CacheOnly` a ModCache miss here leaves `precision` unset and changes the number; `Full` parses
    // the line like the desktop.
    let mut precision: Option<f64> = None;
    let parsed = pob_parser::parse_mod_line(&test_line, false, mode);
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::lua::gsub_fn;

    type Found = Option<(String, Vec<String>)>;

    /// The reference: ItemTools.lua:191-256 as written -- every subset, most substitutions first, in the Lua recursion's order;
    /// the first line `keys` knows wins (2^n lookups).
    fn upstream_walk(keys: &KeyIndex<'_>, line: &str, values: &[String]) -> Found {
        fn check(keys: &KeyIndex<'_>, line: &str, values: &[String], start: usize, num: usize, indices: &mut Vec<usize>) -> Found {
            if indices.len() == num {
                let mut modified = line.to_owned();
                for (substituted, &i) in indices.iter().enumerate() {
                    modified = replace_nth_hash(&modified, &values[i - 1], i - substituted);
                }
                if keys.contains(&modified.replace("+#", "#")) {
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
                if let Some(found) = check(keys, line, values, j + 1, num, indices) {
                    return Some(found);
                }
                indices.pop();
            }
            None
        }
        for i in (1..=values.len()).rev() {
            if let Some(found) = check(keys, line, values, 1, i, &mut Vec::new()) {
                return Some(found);
            }
        }
        keys.contains(&line.replace("+#", "#")).then(|| (line.to_owned(), values.to_vec()))
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    /// Both walks agree; the answer, for the cases that state it.
    fn both(keys: &[&str], line: &str, values: &[&str]) -> Found {
        let index = KeyIndex::new(keys.iter().copied());
        let values = strings(values);
        let got = find_scalable_in(&index, line, &values);
        assert_eq!(got, upstream_walk(&index, line, &values), "{line:?} {values:?} over {keys:?}");
        got
    }

    fn hit(line: &str, remaining: &[&str]) -> Found {
        Some((line.to_owned(), strings(remaining)))
    }

    #[test]
    fn literal_hash_in_the_text_is_counted_like_upstream() {
        // "#5 x" strips to "## x": value 1 goes to the FIRST `#`, the literal one (replaceNthInstance counts them all).
        assert_eq!(both(&["5# x"], "## x", &["5"]), hit("5# x", &[]));
        assert_eq!(both(&["#5 x"], "## x", &["5"]), None);
        assert_eq!(both(&["## x"], "## x", &["5"]), hit("## x", &["5"]));
        // Literal `#` after the values are never decided.
        assert_eq!(both(&["5 of #"], "# of #", &["5"]), hit("5 of #", &[]));
        assert_eq!(both(&["5 of 7"], "# of #", &["5"]), None);
        assert_eq!(both(&["# # #"], "# # #", &[]), hit("# # #", &[]));
        // A literal `+#` after the values normalises too.
        assert_eq!(both(&["5 #"], "# +#", &["5"]), hit("5 +#", &[]));
    }

    #[test]
    fn plus_before_hash_at_the_value_text_boundary() {
        assert_eq!(both(&["# to x"], "+# to x", &["5"]), hit("+# to x", &["5"]));
        assert_eq!(both(&["+5 to x"], "+# to x", &["5"]), hit("+5 to x", &[]));
        assert_eq!(both(&["+# to x"], "+# to x", &["5"]), None);
        // Only one `+` goes: gsub does not rescan.
        assert_eq!(both(&["+# y"], "++# y", &["5"]), hit("++# y", &["5"]));
        assert_eq!(both(&["# y"], "++# y", &["5"]), None);
        // The prefix ending in `+` must survive the cut, both ways of going on.
        assert_eq!(both(&["a # b # c"], "a +# b +# c", &["1", "2"]), hit("a +# b +# c", &["1", "2"]));
        assert_eq!(both(&["a +1 b # c"], "a +# b +# c", &["1", "2"]), hit("a +1 b +# c", &["2"]));
        assert_eq!(both(&["a # b +2 c"], "a +# b +# c", &["1", "2"]), hit("a +# b +2 c", &["1"]));
        // The `+` restored after the `#` branch serves the next value.
        assert_eq!(both(&["# #", "+1 #"], "+# +#", &["1", "2"]), hit("+1 +#", &["2"]));
    }

    #[test]
    fn values_holding_a_plus() {
        assert_eq!(both(&["5#"], "##", &["5+"]), hit("5+#", &[]));
        assert_eq!(both(&["5+6"], "##", &["5+", "6"]), hit("5+6", &[]));
        assert_eq!(both(&["5##"], "###", &["5+"]), hit("5+##", &[]));
        assert_eq!(both(&["+6 x"], "# x", &["+6"]), hit("+6 x", &[]));
    }

    #[test]
    fn the_winner_is_upstreams_at_every_depth() {
        let line = "# # #";
        let v = ["1", "2", "3"];
        assert_eq!(both(&["1 2 3", "1 2 #", "# # #"], line, &v), hit("1 2 3", &[]));
        // Two substitutions beat one found earlier in the walk, whatever the order of the keys.
        assert_eq!(both(&["1 # #", "# 2 3"], line, &v), hit("# 2 3", &["1"]));
        assert_eq!(both(&["1 # #", "# # 3", "# 2 3"], line, &v), hit("# 2 3", &["1"]));
        // Equal sizes: lexicographically smallest indices ...
        assert_eq!(both(&["1 2 #", "1 # 3", "# 2 3"], line, &v), hit("1 2 #", &["3"]));
        assert_eq!(both(&["1 # 3", "# 2 3"], line, &v), hit("1 # 3", &["2"]));
        assert_eq!(both(&["# # 3", "# 2 #"], line, &v), hit("# 2 #", &["1", "3"]));
        // ... and the empty subset only when nothing else is known.
        assert_eq!(both(&["# # #", "# # 3"], line, &v), hit("# # 3", &["1", "2"]));
        assert_eq!(both(&["# # #"], line, &v), hit("# # #", &["1", "2", "3"]));
    }

    #[test]
    fn empty_values_and_no_match() {
        assert_eq!(both(&["abc"], "abc", &[]), hit("abc", &[]));
        assert_eq!(both(&["abd"], "abc", &[]), None);
        assert_eq!(both(&[], "abc", &[]), None);
        assert_eq!(both(&[], "a#c", &["1"]), None);
        assert_eq!(both(&[""], "", &[]), hit("", &[]));
        assert_eq!(both(&["ac"], "a#c", &[""]), hit("ac", &[]));
        assert_eq!(both(&["a#c"], "a#c", &[""]), hit("a#c", &[""]));
        // Keys reaching the prefix but not the end.
        assert_eq!(both(&["a 1 b"], "a # b # c", &["1", "2"]), None);
        assert_eq!(both(&["a 1 b 2 c d"], "a # b # c", &["1", "2"]), None);
    }

    /// A small fixed generator: the cases are the same on every run.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) as usize) % n
        }

        fn pick<'a>(&mut self, of: &[&'a str]) -> &'a str {
            of[self.below(of.len())]
        }
    }

    #[test]
    fn random_lines_over_a_dense_alphabet() {
        const TEXT: [&str; 7] = ["", "+", "++", " ", "a", "a+", " +"];
        const VALUE: [&str; 7] = ["1", "-2", "3.5", "", "+", "5+", "+6"];
        let mut r = Rng(0x5eed);
        let (mut known, mut unknown) = (0, 0);
        for _ in 0..40_000 {
            let n = r.below(7);
            let literal = r.below(3);
            let values: Vec<String> = (0..n).map(|_| r.pick(&VALUE).to_owned()).collect();
            let segments: Vec<&str> = (0..=n + literal).map(|_| r.pick(&TEXT)).collect();
            let line = segments.join("#");
            // Keys: what some subsets give (so the walk really branches), plus near misses.
            let mut keys = Vec::new();
            for _ in 0..r.below(6) {
                let mut key = segments[0].to_owned();
                for (i, s) in segments[1..].iter().enumerate() {
                    if i < n && r.below(2) == 0 {
                        key.push_str(&values[i]);
                    } else {
                        key.push('#');
                    }
                    key.push_str(s);
                }
                let mut key = key.replace("+#", "#");
                if r.below(5) == 0 {
                    key.push('x');
                }
                keys.push(key);
            }
            let index = KeyIndex::new(keys.iter().map(String::as_str));
            let got = find_scalable_in(&index, &line, &values);
            assert_eq!(got, upstream_walk(&index, &line, &values), "{line:?} {values:?} over {keys:?}");
            if got.is_some() {
                known += 1;
            } else {
                unknown += 1;
            }
        }
        // The generator exercises both outcomes.
        assert!(known > 10_000 && unknown > 1_000, "known {known}, unknown {unknown}");
    }

    /// ItemTools.lua:171-182 for range = 1: what `findScalableLine` is called with.
    fn strip(line: &str) -> (String, Vec<String>) {
        let mut values = Vec::new();
        let s = gsub_fn(line, "([%+-]?)%((%-?%d+%.?%d*)%-(%-?%d+%.?%d*)%)", None, |c| Some(c[2].clone()));
        let s = gsub_fn(&s, "(%-?%d+%.?%d*)", None, |c| {
            values.push(c[0].clone());
            Some("#".to_owned())
        });
        (s, values)
    }

    fn real_keys() -> KeyIndex<'static> {
        KeyIndex::new(mod_scalability().keys().map(String::as_str))
    }

    #[test]
    fn every_real_key_as_a_line() {
        let keys = real_keys();
        let mut scalable = 0;
        for key in &keys.sorted {
            for (number, plus) in [("7", ""), ("12.5", "+"), ("-3", "")] {
                let line = key.replace('#', &format!("{plus}{number}"));
                let (s, values) = strip(&line);
                if values.len() > 12 {
                    continue;
                }
                let got = find_scalable_in(&keys, &s, &values);
                assert_eq!(got, upstream_walk(&keys, &s, &values), "{line:?}");
                scalable += usize::from(got.is_some());
            }
        }
        // 15 202 keys, 3 forms: most lines come back to their key.
        assert!(scalable > 30_000, "{scalable}");
    }

    /// A `Unique ID:` header line (a hash with `n` runs of digits), as an item with a repeated header sends it through.
    fn unique_id(n: usize) -> String {
        let hash: String = (0..n).map(|i| format!("{}a", i % 10)).collect();
        format!("Unique ID: {hash}")
    }

    #[test]
    fn unique_id_lines() {
        let keys = real_keys();
        for n in 1..=13 {
            let (s, values) = strip(&unique_id(n));
            assert_eq!(values.len(), n);
            assert_eq!(find_scalable_in(&keys, &s, &values), None);
            assert_eq!(upstream_walk(&keys, &s, &values), None);
        }
        // Far past what the subset walk can do: no limit on n, and the line stays as it was.
        let line = unique_id(64);
        assert_eq!(apply_range(&line, 1.0, Some(1.0), None, pob_parser::ParseMode::Full), line);
    }

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn xml_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                xml_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "xml") {
                out.push(p);
            }
        }
    }

    /// The (de-duplicated) lines an item of these builds can send here: each raw line and each one joined to the next, as
    /// `(n, stripped, values)` sorted by n.
    fn corpus(files: &[PathBuf]) -> Vec<(usize, String, Vec<String>)> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for f in files {
            let Ok(xml) = pob_build::read_build_file(f) else { continue };
            let Ok(model) = pob_build::load_build_xml(&xml) else { continue };
            for im in &model.items {
                let lines: Vec<String> = im.raw.lines().map(|l| gsub(l, "%b{}", "", None)).collect();
                let joined = lines.windows(2).map(|w| format!("{} {}", w[0], w[1]));
                for line in lines.iter().cloned().chain(joined) {
                    let (s, values) = strip(&line);
                    if seen.insert((s.clone(), values.clone())) {
                        out.push((values.len(), s, values));
                    }
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        out
    }

    /// The personal builds named in `builds/local-builds.json`, when they are here. n <= 13 keeps this inside the gate.
    #[test]
    fn local_builds_lines() {
        let root = repo_root();
        let Ok(text) = std::fs::read_to_string(root.join("builds/local-builds.json")) else {
            println!("apply_range: builds/local-builds.json absent -- skipped (local only)");
            return;
        };
        let lists: serde_json::Value = serde_json::from_str(&text).expect("local-builds.json");
        let names: Vec<&str> = lists["ui_root"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let mut files = Vec::new();
        xml_files(&root.join("builds/pc"), &mut files);
        files.retain(|f| f.file_stem().and_then(|s| s.to_str()).is_some_and(|s| names.contains(&s)));
        if files.is_empty() {
            println!("apply_range: no build of local-builds.json here -- skipped (local only)");
            return;
        }
        let keys = real_keys();
        let mut compared = 0;
        for (_, s, values) in corpus(&files).iter().filter(|c| c.0 <= 13) {
            assert_eq!(
                find_scalable_in(&keys, s, values),
                upstream_walk(&keys, s, values),
                "a line of a local build"
            );
            compared += 1;
        }
        println!("apply_range: {} local builds, {compared} lines with n <= 13: same", files.len());
    }

    /// Coverage run outside the gate, over all of `builds/pc`: `cargo test -p pob-items --release --lib corpus_coverage -- --ignored
    /// --nocapture`, with `POB_WALK_MIN_N`, `POB_WALK_MAX_N`, `POB_WALK_PER_N` (lines at most per n, evenly spaced), `POB_WALK_THREADS`.
    #[test]
    #[ignore = "minutes of CPU; needs builds/pc"]
    fn corpus_coverage() {
        let env = |k: &str, d: usize| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        let (min_n, max_n) = (env("POB_WALK_MIN_N", 0), env("POB_WALK_MAX_N", 17));
        let (per_n, threads) = (env("POB_WALK_PER_N", usize::MAX), env("POB_WALK_THREADS", 4));
        let mut files = Vec::new();
        xml_files(&repo_root().join("builds/pc"), &mut files);
        let all = corpus(&files);
        let keys = real_keys();
        for n in min_n..=max_n {
            let of_n: Vec<_> = all.iter().filter(|c| c.0 == n).collect();
            if of_n.is_empty() {
                continue;
            }
            let step = (of_n.len() / per_n.max(1)).max(1);
            let sample: Vec<_> = of_n.iter().step_by(step).take(per_n).collect();
            let t = std::time::Instant::now();
            std::thread::scope(|scope| {
                for chunk in sample.chunks(sample.len().div_ceil(threads.max(1))) {
                    let keys = &keys;
                    scope.spawn(move || {
                        for c in chunk {
                            assert_eq!(find_scalable_in(keys, &c.1, &c.2), upstream_walk(keys, &c.1, &c.2), "n = {}", c.0);
                        }
                    });
                }
            });
            println!(
                "n = {n}: {} of {} distinct lines same, {:.1} s",
                sample.len(),
                of_n.len(),
                t.elapsed().as_secs_f64()
            );
        }
    }
}
