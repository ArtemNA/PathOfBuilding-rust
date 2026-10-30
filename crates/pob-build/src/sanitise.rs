//! Common.lua:306-329 `sanitiseText`, byte for byte (L16: Lua strings are bytes; the gsubs run on the UTF-8 bytes in the
//! upstream order, so e.g. a multi-byte character outside the handled ones becomes one `?` PER BYTE).
//!
//! pob-ts works on code points instead (one `?` per character, and no CP1252 single-byte rules) -- a known, goldens-inert
//! difference: the 6 goldens hold no non-ASCII outside the handled characters in the fields that go through here.

use pob_lua::{Repl, find, gsub};

/// The gsub chain after the `[\128-\255<]` guard (Common.lua:311-327), in order.
const STEPS: &[(&[u8], &[u8])] = &[
    (b"%b<>", b""),
    (b"\xe2\x80\x90", b"-"), // U+2010 HYPHEN
    (b"\xe2\x80\x91", b"-"), // U+2011 NON-BREAKING HYPHEN
    (b"\xe2\x80\x92", b"-"), // U+2012 FIGURE DASH
    (b"\xe2\x80\x93", b"-"), // U+2013 EN DASH
    (b"\xe2\x80\x94", b"-"), // U+2014 EM DASH
    (b"\xe2\x80\x95", b"-"), // U+2015 HORIZONTAL BAR
    (b"\xe2\x88\x92", b"-"), // U+2212 MINUS SIGN
    (b"\xc3\xa4", b"a"),     // U+00E4 a with diaeresis
    (b"\xc3\xb6", b"o"),     // U+00F6 o with diaeresis
    (b"\x96", b"-"),         // Windows-1252 EN DASH
    (b"\x97", b"-"),         // Windows-1252 EM DASH
    (b"\xe4", b"a"),         // Windows-1252 a with diaeresis
    (b"\xf6", b"o"),         // Windows-1252 o with diaeresis
    (b"[\x80-\xff]", b"?"),  // unsupported
];

/// `sanitiseText(text)`: only when a byte 128-255 or `<` occurs; otherwise the text itself.
pub fn sanitise_text(text: &str) -> String {
    let guard = find(text.as_bytes(), b"[\x80-\xff<]", None, false).expect("sanitiseText guard pattern");
    if guard.is_none() {
        return text.to_owned();
    }
    let mut s = text.as_bytes().to_vec();
    for (pat, repl) in STEPS {
        s = gsub(&s, pat, Repl::Str(repl), None).expect("sanitiseText pattern").0;
    }
    // Every byte >= 0x80 is gone after the last step, so this is ASCII.
    String::from_utf8(s).expect("sanitiseText output is ASCII")
}

#[cfg(test)]
mod tests {
    use super::sanitise_text;

    #[test]
    fn upstream_cases() {
        assert_eq!(sanitise_text("Plain"), "Plain");
        assert_eq!(sanitise_text("a<b>c"), "ac");
        assert_eq!(sanitise_text("a<b"), "a<b"); // unbalanced: %b<> does not match
        assert_eq!(sanitise_text("Mj\u{f6}lner \u{2013} x"), "Mjolner - x");
        assert_eq!(sanitise_text("Ma\u{f1}ana"), "Ma??ana"); // U+00F1 = C3 B1: one '?' per byte (L16)
        assert_eq!(sanitise_text("\u{2016}"), "??-"); // E2 80 96: the CP1252 rule eats the last byte first
    }
}
