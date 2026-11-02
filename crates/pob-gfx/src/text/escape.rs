//! Colour escapes in text: `^N` (one ASCII digit) and `^xRRGGBB` / `^XRRGGBB` (six hex digits, any case).
//!
//! Detection is a single left-to-right pass (SG oracle corpus `strip.json`: `"^^1"` -> `"^"`, `"^10"` -> `"0"`,
//! `"^x1234567"` -> `"7"`, `"^x12345"`/`"^xGGGGGG"`/`"^a"`/`"^ x"` stay). This differs from the headless stub
//! `_SimpleGraphic.def.lua:296` (two sequential `gsub`s) on inputs such as `"^x^1FF00AA"`, which the stub reduces to `""`.

/// Length in bytes of the colour escape starting at `bytes[at]` (2 or 8), or 0 if there is none.
pub fn escape_len(bytes: &[u8], at: usize) -> usize {
    if bytes.get(at) != Some(&b'^') {
        return 0;
    }
    match bytes.get(at + 1) {
        Some(c) if c.is_ascii_digit() => 2,
        // Upper-case `^X` is an escape too (text-extra corpus: "^XFF8000A" -> "A"), unlike the stub's `%^x`.
        Some(b'x' | b'X') if bytes.len() >= at + 8 && bytes[at + 2..at + 8].iter().all(u8::is_ascii_hexdigit) => 8,
        _ => 0,
    }
}

/// The host sees Lua strings as C strings: everything from the first NUL on is ignored by all text functions
/// (text-extra corpus: `StripEscapes("a\0b")` = `"a"`, `DrawStringWidth(…, "a\0b")` = width of `"a"`).
pub fn until_nul(text: &str) -> &str {
    text.split('\0').next().unwrap_or("")
}

/// `StripEscapes(text)`: the text without colour escapes.
pub fn strip_escapes(text: &str) -> String {
    let b = until_nul(text).as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let e = escape_len(b, i);
        if e > 0 {
            i += e;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    // Escapes are pure ASCII and never split a multi-byte sequence, so the rest stays valid UTF-8.
    String::from_utf8(out).expect("escape removal keeps UTF-8 valid")
}
