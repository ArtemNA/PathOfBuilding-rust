//! Byte-level string helpers with LuaJIT semantics (L16: C locale, ASCII only).

/// `string.lower`: ASCII letters only; every other byte untouched.
pub fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// `string.upper`: ASCII letters only.
pub fn upper(s: &str) -> String {
    s.to_ascii_uppercase()
}

/// `s:match("[^:]+")` (MOD-CORE §2.4): the first run of non-colon bytes; ":X" -> "X", "::" and "" -> None.
pub fn first_non_colon(s: &str) -> Option<&str> {
    s.split(':').find(|part| !part.is_empty())
}
