//! The closure registry (R5-ARCH §1.3, §4): every function VALUE of the ModParser tables is a definition `"<src>:<from>"`
//! (`data/mod-parser.json` `fns`, 1 263) plus its upvalues (`up`). A definition is either registered here -- a Rust function
//! with the sha256 of the Lua body it ports (drift test: `drift()` = 0) -- or it is not, and calling it is the explicit
//! `ParseError::Unported(id)` (counted per id), never a panic.
//!
//! Signatures by call site (ModParser.lua):
//! * `specialModList` :6663 `return specialMod(tonumber(cap[1]), unpack(cap))` -- a tail call, so ALL returns become
//!   `parseMod`'s `(modList, extra)`: `Special`, args `[tonumber(cap1), cap1, .., capN]`.
//! * `modTagList` :6697/:6706 `modTag(tonumber(tagCap[1]) | tagCap[1], unpack(tagCap))` and `preFlagList` :6682
//!   `preFlag(unpack(preFlagCap))` -- one value kept: `Value`.
//! * `jewelFuncList` :6644 `patternVal.func(cap1, .., cap5)` -> a visitor `function(node, out, data)` -- declared only
//!   (`Jewel`); the jewel side is R5.4 (`helpers::{SimpleConv, Threshold}` are the two factories already ported).
//! * `Helper`: a named helper the Rust closures call directly (`helpers.rs`); registered for the drift contract only.
//!
//! `args` follow L11: `unpack(cap)` of the 5-slot capture table is exactly the captures the pattern has (captures are never
//! nil before the last one), so `args.len()` = 1 + #captures (max 5).

use std::collections::HashMap;
use std::sync::OnceLock;

use pob_data::mod_parser::{FnRef, ModParserData};
use pob_mods::{Str, Value};

use crate::ParseError;

/// `"<src>:<from>"`, e.g. `"Modules/ModParser.lua:1419"`.
pub type ClosureId = Str;

/// `(first return, second return)`; missing returns are nil.
pub type Ret2 = (Value, Value);

pub type SpecialFn = fn(&ModParserData, &FnRef, &[Value]) -> Result<Ret2, ParseError>;
pub type ValueFn = fn(&ModParserData, &FnRef, &[Value]) -> Result<Value, ParseError>;

#[derive(Clone, Copy)]
pub enum ClosureFn {
    Special(SpecialFn),
    Value(ValueFn),
    /// A radius-jewel visitor (or its factory) -- the jewel interface is R5.4.
    Jewel,
    Helper,
}

/// One ported definition: where its body is upstream and the sha256 of that body (as `fns` records it).
#[derive(Clone, Copy)]
pub struct Registered {
    pub id: &'static str,
    pub to: u32,
    pub sha: &'static str,
    pub f: ClosureFn,
}

impl std::fmt::Debug for Registered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Registered({}-{} {})", self.id, self.to, self.sha)
    }
}

const fn r(id: &'static str, to: u32, sha: &'static str, f: ClosureFn) -> Registered {
    Registered { id, to, sha, f }
}

/// The 9 named helpers (R5-ARCH §4) and the two radius-jewel visitors that are part of their bodies (:19 inside
/// getSimpleConv, :6457 inside getThreshold).
pub const HELPERS: [Registered; 11] = [
    r(
        "Modules/ModTools.lua:33",
        67,
        "c124b50cba30a13b0b1cb30bd8f470e8791f654e8a6362485dbda4199e72ae11",
        ClosureFn::Helper,
    ),
    r(
        "Modules/ModParser.lua:13",
        15,
        "a2bf19d5b84c50c606e67d4ad5c5e72fca1a491bfbb36d5a9efcfc18da0e4855",
        ClosureFn::Helper,
    ),
    r(
        "Modules/ModParser.lua:18",
        39,
        "160fdfa53239eaa0f181147368f87c0d57a8943f83fa9ded1f699c6451d13af3",
        ClosureFn::Helper,
    ),
    r(
        "Modules/ModParser.lua:19",
        38,
        "7eb893c0fe0803de8d1fbd4a498e6edafb54dc8f1a7181e1778181d8debcf645",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:2012",
        2014,
        "4d8c99ad0c14e471890242e289ff5c13fdb433840814b51028322710e631cca7",
        ClosureFn::Helper,
    ),
    r(
        "Modules/ModParser.lua:2018",
        2031,
        "93a041b1086e8f9c7aa47fc29fb8f4230ba0973a212025b8c2d0b092a323dc0e",
        ClosureFn::Helper,
    ),
    r(
        "Modules/ModParser.lua:2047",
        2054,
        "0e3a65919d593f121be96bf036e137a07ff496acd8f68c8f489c577b31b46d68",
        ClosureFn::Helper,
    ),
    r(
        "Modules/ModParser.lua:2055",
        2072,
        "5dd9ab49807140368fdcb38c59bec860300949422078746320d1e3bc55412777",
        ClosureFn::Helper,
    ),
    r(
        "Modules/ModParser.lua:2073",
        2106,
        "e4fdd01db275e22ce28edcea8ff00ec42ec0ea0d22f7bf6683342305ba52bf57",
        ClosureFn::Helper,
    ),
    r(
        "Modules/ModParser.lua:6455",
        6479,
        "9bf5eac1e794f8113dbe6a2cd2f7fd000496422dea1111de249c2f1b436042fa",
        ClosureFn::Helper,
    ),
    r(
        "Modules/ModParser.lua:6457",
        6478,
        "9e666b35d09cddfe4a399416fc23eea81ae4e104cbca716a1a6aed77037459cd",
        ClosureFn::Jewel,
    ),
];

/// Every registered definition, by id: the hand-ported helpers and the generated closures (`closures_gen.rs`, R5.3a);
/// the hand-ported class C joins in R5.3b.
pub fn registry() -> &'static HashMap<&'static str, Registered> {
    static R: OnceLock<HashMap<&'static str, Registered>> = OnceLock::new();
    R.get_or_init(|| {
        let mut m = HashMap::new();
        for e in HELPERS.iter().chain(&crate::closures_gen::GENERATED) {
            assert!(m.insert(e.id, *e).is_none(), "{} registered twice", e.id);
        }
        m
    })
}

/// The hand-ported entries in declaration order (the generated ones: `closures_gen::GENERATED`).
pub fn all_entries() -> Vec<Registered> {
    HELPERS.to_vec()
}

/// Every entry whose `to`/`sha` differ from `fns`, or whose id `fns` does not know (empty = no drift).
pub fn drift(entries: &[Registered], d: &ModParserData) -> Vec<String> {
    let mut out = Vec::new();
    for e in entries {
        match d.fns.get(e.id) {
            None => out.push(format!("{}: not in mod-parser.json fns", e.id)),
            Some(d) if d.sha != e.sha || d.to != e.to => {
                out.push(format!("{}-{} {} != fns {}-{} {}", e.id, e.to, e.sha, d.from, d.to, d.sha));
            }
            Some(_) => {}
        }
    }
    out
}

/// Look a closure up; an unregistered definition (or one whose kind does not fit the call site) is `Unported`.
pub(crate) fn lookup(f: &FnRef) -> Result<ClosureFn, ParseError> {
    match registry().get(&*f.id) {
        Some(r) => Ok(r.f),
        None => Err(ParseError::Unported(f.id.clone())),
    }
}

pub(crate) fn call_special(d: &ModParserData, f: &FnRef, args: &[Value]) -> Result<Ret2, ParseError> {
    match lookup(f)? {
        ClosureFn::Special(g) => g(d, f, args),
        _ => Err(ParseError::Unported(f.id.clone())),
    }
}

pub(crate) fn call_value(d: &ModParserData, f: &FnRef, args: &[Value]) -> Result<Value, ParseError> {
    match lookup(f)? {
        ClosureFn::Value(g) => g(d, f, args),
        _ => Err(ParseError::Unported(f.id.clone())),
    }
}
