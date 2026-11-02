//! `Data/Global.lua:7-90`: the global `colorCodes` table (mutable: `updateColorCode`), `defaultColorCodes` and
//! `hexToRGB`.

use indexmap::IndexMap;
use std::cell::RefCell;

const COLOR_CODES: &[(&str, &str)] = &[
    ("NORMAL", "^xC8C8C8"),
    ("MAGIC", "^x8888FF"),
    ("RARE", "^xFFFF77"),
    ("UNIQUE", "^xAF6025"),
    ("RELIC", "^x60C060"),
    ("GEM", "^x1AA29B"),
    ("PROPHECY", "^xB54BFF"),
    ("CURRENCY", "^xAA9E82"),
    ("CRAFTED", "^xB8DAF1"),
    ("CUSTOM", "^x5CF0BB"),
    ("SOURCE", "^x88FFFF"),
    ("UNSUPPORTED", "^xF05050"),
    ("DISABLED", "^x7F7F7F"),
    ("WARNING", "^xFF9922"),
    ("TIP", "^x80A080"),
    ("FIRE", "^xB97123"),
    ("COLD", "^x3F6DB3"),
    ("LIGHTNING", "^xADAA47"),
    ("CHAOS", "^xD02090"),
    ("POSITIVE", "^x33FF77"),
    ("NEGATIVE", "^xDD0022"),
    ("HIGHLIGHT", "^xFF0000"),
    ("OFFENCE", "^xE07030"),
    ("DEFENCE", "^x8080E0"),
    ("SCION", "^xFFF0F0"),
    ("MARAUDER", "^xE05030"),
    ("RANGER", "^x70FF70"),
    ("WITCH", "^x7070FF"),
    ("DUELIST", "^xE0E070"),
    ("TEMPLAR", "^xC040FF"),
    ("SHADOW", "^x30C0D0"),
    ("MAINHAND", "^x50FF50"),
    ("MAINHANDBG", "^x071907"),
    ("OFFHAND", "^xB7B7FF"),
    ("OFFHANDBG", "^x070719"),
    ("SHAPER", "^x55BBFF"),
    ("ELDER", "^xAA77CC"),
    ("FRACTURED", "^xA29160"),
    ("MUTATED", "^xCD2285"),
    ("ADJUDICATOR", "^xE9F831"),
    ("BASILISK", "^x00CB3A"),
    ("CRUSADER", "^x2946FC"),
    ("EYRIE", "^xAAB7B8"),
    ("CLEANSING", "^xF24141"),
    ("TANGLE", "^x038C8C"),
    ("CHILLBG", "^x151e26"),
    ("FREEZEBG", "^x0c262b"),
    ("SHOCKBG", "^x191732"),
    ("SCORCHBG", "^x270b00"),
    ("BRITTLEBG", "^x00122b"),
    ("SAPBG", "^x261500"),
    ("SCOURGE", "^xFF6E25"),
    ("CRUCIBLE", "^xFFA500"),
    ("SPLITPERSONALITY", "^xFFD62A"),
    ("VESTIGIAL", "^xCBA5F1"),
    ("INTANGIBILITY", "^x9BF4BD"),
    ("MEMORY", "^xBFE2FA"),
];

/// Global.lua:65-77: aliases (copies of the value at load time).
const ALIASES: &[(&str, &str)] = &[
    ("STRENGTH", "MARAUDER"),
    ("DEXTERITY", "RANGER"),
    ("INTELLIGENCE", "WITCH"),
    ("LIFE", "MARAUDER"),
    ("MANA", "WITCH"),
    ("ES", "SOURCE"),
    ("WARD", "RARE"),
    ("ARMOUR", "NORMAL"),
    ("EVASION", "POSITIVE"),
    ("RAGE", "WARNING"),
    ("PHYS", "NORMAL"),
];

fn defaults() -> IndexMap<String, String> {
    let mut m: IndexMap<String, String> = COLOR_CODES.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    for (k, src) in ALIASES {
        let v = m[*src].clone();
        m.insert(k.to_string(), v);
    }
    m
}

thread_local! {
    static CODES: RefCell<IndexMap<String, String>> = RefCell::new(defaults());
    static DEFAULTS: IndexMap<String, String> = defaults();
    /// `rgbColor` (set by `updateColorCode("HIGHLIGHT", …)`).
    static RGB_COLOR: RefCell<Option<[f64; 3]>> = const { RefCell::new(None) };
}

/// `colorCodes[code]`.
pub fn color_code(code: &str) -> Option<String> {
    CODES.with(|c| c.borrow().get(code).cloned())
}

/// `defaultColorCodes[code]`.
pub fn default_color_code(code: &str) -> Option<String> {
    DEFAULTS.with(|c| c.get(code).cloned())
}

/// `updateColorCode(code, color)` (Global.lua:80-87): `color:gsub("^0", "^")`.
pub fn update_color_code(code: &str, color: &str) {
    CODES.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(v) = c.get_mut(code) {
            *v = crate::lua::gsub(color, "^0", "^");
        }
    });
    if code == "HIGHLIGHT" && color_code(code).is_some() {
        RGB_COLOR.with(|r| *r.borrow_mut() = hex_to_rgb(color));
    }
}

/// `hexToRGB(hex)` (Global.lua:89-101).
pub fn hex_to_rgb(hex: &str) -> Option<[f64; 3]> {
    let hex = crate::lua::gsub(hex, "%^x", "");
    let hex = crate::lua::gsub(&hex, "0x", "");
    let hex = crate::lua::gsub(&hex, "#", "");
    if hex.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|v| f64::from(v) / 255.0);
    Some([p(0)?, p(2)?, p(4)?])
}

/// Restores the defaults (tests start from a fresh `Data/Global`).
pub fn reset() {
    CODES.with(|c| *c.borrow_mut() = defaults());
}
