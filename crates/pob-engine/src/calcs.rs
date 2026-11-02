//! The Calcs tab model (R8.6, R8-ARCH §1 p. 6): the section data of `Modules/CalcSections.lua` (+ the `SkillSelect` section CalcsTab.lua:46-147
//! builds inline), the visibility of sections and rows (`CalcsTab:CheckFlag` :396, `CalcSectionControl:UpdateSize` :114) and the TEXT of every
//! drawn string of `CalcSectionControl:DrawContent` (:437, non-overlay) -- labels, `extra`, cell values through `formatCalcStr`
//! (Modules/CalcFormat.lua) -- for the player or the minion (`input.showMinion`). No graphics: positions, colours of rectangles and the
//! controls (drop-downs, check boxes) are the UI's.
//!
//! The data is the upstream file itself, parsed at build time ([`CALC_SECTIONS_LUA`]) by a reader of the Lua subset it is written in (table
//! constructors, strings, numbers, booleans, `colorCodes.X`, the file's `local` lists, `..`); every key is mapped to a model field and an unknown
//! key is an error, so nothing is dropped silently. Gate: `tests/calcs.rs` (data = LuaJIT's `LoadModule("Modules/CalcSections")`, texts = the
//! LuaJIT probe `tools/lua-corpus/export-calcs.ts`, which captures `DrawString` inside `DrawContent`).
//!
//! The subsection collapse state is `defaultCollapsed` unless the caller passes the build's `<Calcs><Section subsection=...>` states
//! (CalcsTab.lua:183-194; `pob-build` does not load them -- see [`render_sections`]).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use indexmap::IndexMap;
use pob_calc::{ActorState, Env, OutTable, OutValue};
use pob_lua::Repl;
use pob_mods::{ModCfg, ModType, StoreRef, Value};

use crate::display::{DisplaySettings, format_num_sep};

/// `src/Modules/CalcSections.lua` of the upstream checkout this port follows, vendored byte-for-byte so the crate builds
/// without that checkout (`tests/calcs.rs` checks it against upstream when present; data gate there too).
pub const CALC_SECTIONS_LUA: &str = include_str!("../upstream/CalcSections.lua");

// --- a Lua value of the data file ---------------------------------------------------------------------------------------------------

/// A value of the data: the file has no nil, no functions and only string keys besides the array part.
#[derive(Clone, Debug, PartialEq)]
pub enum LVal {
    Str(String),
    Num(f64),
    Bool(bool),
    Table(Rc<LTable>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LTable {
    pub array: Vec<LVal>,
    /// `key = value` fields (a repeated key keeps the last value, as a table constructor does).
    pub hash: IndexMap<String, LVal>,
}

/// `colorCodes` (Data/Global.lua:7-77).
pub fn color_code(name: &str) -> Option<&'static str> {
    Some(match name {
        "NORMAL" | "ARMOUR" | "PHYS" => "^xC8C8C8",
        "MAGIC" => "^x8888FF",
        "RARE" | "WARD" => "^xFFFF77",
        "UNIQUE" => "^xAF6025",
        "RELIC" => "^x60C060",
        "GEM" => "^x1AA29B",
        "PROPHECY" => "^xB54BFF",
        "CURRENCY" => "^xAA9E82",
        "CRAFTED" => "^xB8DAF1",
        "CUSTOM" => "^x5CF0BB",
        "SOURCE" | "ES" => "^x88FFFF",
        "UNSUPPORTED" => "^xF05050",
        "DISABLED" => "^x7F7F7F",
        "WARNING" | "RAGE" => "^xFF9922",
        "TIP" => "^x80A080",
        "FIRE" => "^xB97123",
        "COLD" => "^x3F6DB3",
        "LIGHTNING" => "^xADAA47",
        "CHAOS" => "^xD02090",
        "POSITIVE" | "EVASION" => "^x33FF77",
        "NEGATIVE" => "^xDD0022",
        "HIGHLIGHT" => "^xFF0000",
        "OFFENCE" => "^xE07030",
        "DEFENCE" => "^x8080E0",
        "SCION" => "^xFFF0F0",
        "MARAUDER" | "STRENGTH" | "LIFE" => "^xE05030",
        "RANGER" | "DEXTERITY" => "^x70FF70",
        "WITCH" | "INTELLIGENCE" | "MANA" => "^x7070FF",
        "DUELIST" => "^xE0E070",
        "TEMPLAR" => "^xC040FF",
        "SHADOW" => "^x30C0D0",
        "MAINHAND" => "^x50FF50",
        "MAINHANDBG" => "^x071907",
        "OFFHAND" => "^xB7B7FF",
        "OFFHANDBG" => "^x070719",
        "SHAPER" => "^x55BBFF",
        "ELDER" => "^xAA77CC",
        "FRACTURED" => "^xA29160",
        "MUTATED" => "^xCD2285",
        "ADJUDICATOR" => "^xE9F831",
        "BASILISK" => "^x00CB3A",
        "CRUSADER" => "^x2946FC",
        "EYRIE" => "^xAAB7B8",
        "CLEANSING" => "^xF24141",
        "TANGLE" => "^x038C8C",
        "CHILLBG" => "^x151e26",
        "FREEZEBG" => "^x0c262b",
        "SHOCKBG" => "^x191732",
        "SCORCHBG" => "^x270b00",
        "BRITTLEBG" => "^x00122b",
        "SAPBG" => "^x261500",
        "SCOURGE" => "^xFF6E25",
        "CRUCIBLE" => "^xFFA500",
        "SPLITPERSONALITY" => "^xFFD62A",
        "VESTIGIAL" => "^xCBA5F1",
        "INTANGIBILITY" => "^x9BF4BD",
        "MEMORY" => "^xBFE2FA",
        _ => return None,
    })
}

// --- the reader -----------------------------------------------------------------------------------------------------------------------

struct Reader<'s> {
    s: &'s [u8],
    i: usize,
    locals: HashMap<String, LVal>,
}

impl Reader<'_> {
    fn err<T>(&self, what: &str) -> Result<T, String> {
        let line = self.s[..self.i.min(self.s.len())].iter().filter(|&&b| b == b'\n').count() + 1;
        Err(format!("CalcSections.lua:{line}: {what}"))
    }

    fn skip(&mut self) {
        loop {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.s[self.i..].starts_with(b"--") {
                while self.i < self.s.len() && self.s[self.i] != b'\n' {
                    self.i += 1;
                }
            } else {
                return;
            }
        }
    }

    fn eat(&mut self, t: &[u8]) -> bool {
        self.skip();
        if self.s[self.i..].starts_with(t) {
            self.i += t.len();
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> Option<String> {
        self.skip();
        let st = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') {
            self.i += 1;
        }
        if self.i == st || self.s[st].is_ascii_digit() {
            self.i = st;
            return None;
        }
        Some(String::from_utf8_lossy(&self.s[st..self.i]).into_owned())
    }

    fn string(&mut self) -> Result<String, String> {
        let q = self.s[self.i];
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return self.err("unfinished string");
            };
            self.i += 1;
            match c {
                b'\\' => {
                    let e = self.s[self.i];
                    self.i += 1;
                    out.push(match e {
                        b'n' => b'\n',
                        b't' => b'\t',
                        b'\\' | b'"' | b'\'' => e,
                        _ => return self.err("escape not in the data subset"),
                    });
                }
                _ if c == q => break,
                b'\n' => return self.err("unfinished string"),
                _ => out.push(c),
            }
        }
        String::from_utf8(out).or_else(|_| self.err("non-UTF-8 string"))
    }

    fn primary(&mut self) -> Result<LVal, String> {
        self.skip();
        match self.s.get(self.i) {
            Some(b'"' | b'\'') => self.string().map(LVal::Str),
            Some(b'{') => self.table(),
            Some(c) if c.is_ascii_digit() || *c == b'-' => {
                let st = self.i;
                self.i += 1;
                while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.s[st..self.i]).expect("ascii");
                pob_lua::tonumber(t).map(LVal::Num).map_or_else(|| self.err("bad number"), Ok)
            }
            _ => {
                let Some(id) = self.ident() else {
                    return self.err("unexpected token");
                };
                match id.as_str() {
                    "true" => Ok(LVal::Bool(true)),
                    "false" => Ok(LVal::Bool(false)),
                    "colorCodes" => {
                        if !self.eat(b".") {
                            return self.err("colorCodes without a field");
                        }
                        let f = self.ident().map_or_else(|| self.err("colorCodes.?"), Ok)?;
                        color_code(&f)
                            .map(|c| LVal::Str(c.into()))
                            .map_or_else(|| self.err(&format!("colorCodes.{f} is nil")), Ok)
                    }
                    _ => self
                        .locals
                        .get(&id)
                        .cloned()
                        .map_or_else(|| self.err(&format!("unknown name {id}")), Ok),
                }
            }
        }
    }

    /// `a .. b .. c` (right associative; only strings and numbers appear).
    fn expr(&mut self) -> Result<LVal, String> {
        let a = self.primary()?;
        if self.eat(b"..") {
            let b = self.expr()?;
            let s = |v: LVal| match v {
                LVal::Str(s) => Ok(s),
                LVal::Num(n) => Ok(pob_lua::tostring_num(n)),
                _ => Err("attempt to concatenate a non-string".to_owned()),
            };
            return Ok(LVal::Str(s(a)? + &s(b)?));
        }
        Ok(a)
    }

    fn table(&mut self) -> Result<LVal, String> {
        self.i += 1; // `{`
        let mut t = LTable::default();
        loop {
            if self.eat(b"}") {
                return Ok(LVal::Table(Rc::new(t)));
            }
            let save = self.i;
            if let Some(k) = self.ident()
                && !matches!(k.as_str(), "true" | "false" | "colorCodes")
                && self.eat(b"=")
                && !self.s[self.i..].starts_with(b"=")
            {
                let v = self.expr()?;
                t.hash.insert(k, v);
            } else {
                self.i = save;
                let v = self.expr()?;
                t.array.push(v);
            }
            if !self.eat(b",") && !self.eat(b";") {
                if self.eat(b"}") {
                    return Ok(LVal::Table(Rc::new(t)));
                }
                return self.err("expected , or }");
            }
        }
    }
}

/// Reads the data file: its `local` lists, then `return { ... }`.
pub fn parse_calc_sections(src: &str) -> Result<Vec<LVal>, String> {
    let mut r = Reader {
        s: src.as_bytes(),
        i: 0,
        locals: HashMap::new(),
    };
    loop {
        r.skip();
        if r.eat(b"local") {
            let name = r.ident().map_or_else(|| r.err("local name"), Ok)?;
            if !r.eat(b"=") {
                return r.err("local without a value");
            }
            let v = r.expr()?;
            r.locals.insert(name, v);
        } else if r.eat(b"return") {
            return match r.expr()? {
                LVal::Table(t) => {
                    r.skip();
                    if r.i != r.s.len() {
                        return r.err("text after the return value");
                    }
                    Ok(t.array.clone())
                }
                _ => r.err("the module returns a non-table"),
            };
        } else {
            return r.err("statement not in the data subset");
        }
    }
}

// --- the model ------------------------------------------------------------------------------------------------------------------------

/// The visibility fields `CalcsTab:CheckFlag` reads (:396-432).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Flags {
    pub flag: Option<String>,
    pub flag_list: Option<Vec<String>>,
    pub player_flag: Option<String>,
    pub not_flag: Option<String>,
    pub not_flag_list: Option<Vec<String>>,
    pub have_output: Option<String>,
}

/// One description of a cell (`CalcSectionEntry`): what the breakdown popup shows and what `{p:mod:...}` sums.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub label: Option<String>,
    pub breakdown: Option<String>,
    /// `modName` -- a string (`one = true`) or a list.
    pub mod_name: Vec<String>,
    pub mod_name_is_list: bool,
    pub mod_type: Option<String>,
    pub cfg: Option<String>,
    pub enemy: Option<bool>,
    pub actor: Option<String>,
    pub mod_source: Option<String>,
    pub skip_side_bar_index: Option<bool>,
    pub gear_only: Option<bool>,
    /// `flag` / `notFlag` / `notFlagList` of an entry (read by the breakdown popup, CalcBreakdownControl -- not by the cell text).
    pub flags: Flags,
}

/// One cell (`CalcSectionColumn`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    pub format: Option<String>,
    /// `modType` on the cell itself (data only: `formatCalcStr` reads the entries' `modType`).
    pub mod_type: Option<String>,
    pub flags: Flags,
    pub entries: Vec<Entry>,
    /// The `SkillSelect` cells hold a control instead of a format (`controlName`).
    pub control_name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub label: Option<String>,
    pub flags: Flags,
    pub color: Option<String>,
    pub bg_col: Option<String>,
    pub text_size: Option<f64>,
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubSection {
    pub label: String,
    pub default_collapsed: bool,
    /// `color` / `haveOutput` on a subsection (data only: `UpdateSize` / `DrawContent` read neither).
    pub color: Option<String>,
    pub have_output: Option<String>,
    /// `data.extra` -- the text beside the label.
    pub extra: Option<String>,
    pub flag: Option<String>,
    pub not_flag: Option<String>,
    pub col_width: Option<f64>,
    pub rows: Vec<Row>,
}

/// One `CalcSectionControl` (`CalcsTab:NewSection(width, id, group, colour, subSection, updateFunc)`).
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// Width in columns (`widthCols`).
    pub width_cols: f64,
    pub id: String,
    pub group: f64,
    pub colour: String,
    pub sub_sections: Vec<SubSection>,
    /// `updateFunc` (only `SkillSelect` has one; it refreshes controls).
    pub has_update_func: bool,
}

impl Section {
    /// `width * self.colWidth + 8 * (width - 1)` (CalcsTab.lua:370-371, `colWidth = 230`).
    pub fn width(&self) -> f64 {
        self.width_cols * 230.0 + 8.0 * (self.width_cols - 1.0)
    }
}

fn want_str(v: &LVal, what: &str) -> Result<String, String> {
    match v {
        LVal::Str(s) => Ok(s.clone()),
        other => Err(format!("{what}: a string expected, got {other:?}")),
    }
}

fn want_num(v: &LVal, what: &str) -> Result<f64, String> {
    match v {
        LVal::Num(n) => Ok(*n),
        other => Err(format!("{what}: a number expected, got {other:?}")),
    }
}

fn want_bool(v: &LVal, what: &str) -> Result<bool, String> {
    match v {
        LVal::Bool(b) => Ok(*b),
        other => Err(format!("{what}: a boolean expected, got {other:?}")),
    }
}

fn want_table<'a>(v: &'a LVal, what: &str) -> Result<&'a LTable, String> {
    match v {
        LVal::Table(t) => Ok(t),
        other => Err(format!("{what}: a table expected, got {other:?}")),
    }
}

fn str_list(v: &LVal, what: &str) -> Result<Vec<String>, String> {
    let t = want_table(v, what)?;
    t.array.iter().map(|x| want_str(x, what)).collect()
}

/// Takes the visibility keys; `false` for any other key.
fn flag_field(f: &mut Flags, k: &str, v: &LVal) -> Result<bool, String> {
    match k {
        "flag" => f.flag = Some(want_str(v, k)?),
        "flagList" => f.flag_list = Some(str_list(v, k)?),
        "playerFlag" => f.player_flag = Some(want_str(v, k)?),
        "notFlag" => f.not_flag = Some(want_str(v, k)?),
        "notFlagList" => f.not_flag_list = Some(str_list(v, k)?),
        "haveOutput" => f.have_output = Some(want_str(v, k)?),
        _ => return Ok(false),
    }
    Ok(true)
}

fn entry_of(t: &LTable) -> Result<Entry, String> {
    let mut e = Entry::default();
    if !t.array.is_empty() {
        return Err("an entry with an array part".into());
    }
    for (k, v) in &t.hash {
        if flag_field(&mut e.flags, k, v)? {
            continue;
        }
        match k.as_str() {
            "label" => e.label = Some(want_str(v, k)?),
            "breakdown" => e.breakdown = Some(want_str(v, k)?),
            "modName" => match v {
                LVal::Str(s) => e.mod_name = vec![s.clone()],
                _ => {
                    e.mod_name = str_list(v, k)?;
                    e.mod_name_is_list = true;
                }
            },
            "modType" => e.mod_type = Some(want_str(v, k)?),
            "cfg" => e.cfg = Some(want_str(v, k)?),
            "enemy" => e.enemy = Some(want_bool(v, k)?),
            "actor" => e.actor = Some(want_str(v, k)?),
            "modSource" => e.mod_source = Some(want_str(v, k)?),
            "skipSideBarIndex" => e.skip_side_bar_index = Some(want_bool(v, k)?),
            "gearOnly" => e.gear_only = Some(want_bool(v, k)?),
            _ => return Err(format!("entry key {k} not in the model")),
        }
    }
    Ok(e)
}

fn cell_of(t: &LTable) -> Result<Cell, String> {
    let mut c = Cell::default();
    for (k, v) in &t.hash {
        if flag_field(&mut c.flags, k, v)? {
            continue;
        }
        match k.as_str() {
            "format" => c.format = Some(want_str(v, k)?),
            "modType" => c.mod_type = Some(want_str(v, k)?),
            _ => return Err(format!("cell key {k} not in the model")),
        }
    }
    for x in &t.array {
        c.entries.push(entry_of(want_table(x, "entry")?)?);
    }
    Ok(c)
}

fn row_of(t: &LTable) -> Result<Row, String> {
    let mut r = Row::default();
    for (k, v) in &t.hash {
        if flag_field(&mut r.flags, k, v)? {
            continue;
        }
        match k.as_str() {
            "label" => r.label = Some(want_str(v, k)?),
            "color" => r.color = Some(want_str(v, k)?),
            "bgCol" => r.bg_col = Some(want_str(v, k)?),
            "textSize" => r.text_size = Some(want_num(v, k)?),
            _ => return Err(format!("row key {k} not in the model")),
        }
    }
    for x in &t.array {
        r.cells.push(cell_of(want_table(x, "cell")?)?);
    }
    Ok(r)
}

fn sub_section_of(t: &LTable) -> Result<SubSection, String> {
    let mut s = SubSection::default();
    let mut data = None;
    for (k, v) in &t.hash {
        match k.as_str() {
            "label" => s.label = want_str(v, k)?,
            "defaultCollapsed" => s.default_collapsed = want_bool(v, k)?,
            "color" => s.color = Some(want_str(v, k)?),
            "haveOutput" => s.have_output = Some(want_str(v, k)?),
            "data" => data = Some(want_table(v, k)?),
            _ => return Err(format!("subsection key {k} not in the model")),
        }
    }
    let data = data.ok_or("subsection without data")?;
    for (k, v) in &data.hash {
        match k.as_str() {
            "extra" => s.extra = Some(want_str(v, k)?),
            "flag" => s.flag = Some(want_str(v, k)?),
            "notFlag" => s.not_flag = Some(want_str(v, k)?),
            "colWidth" => s.col_width = Some(want_num(v, k)?),
            _ => return Err(format!("subsection data key {k} not in the model")),
        }
    }
    for x in &data.array {
        s.rows.push(row_of(want_table(x, "row")?)?);
    }
    Ok(s)
}

/// One `{ width, id, group, colour, subSection, updateFunc }` entry of the data.
pub fn section_of(v: &LVal) -> Result<Section, String> {
    let t = want_table(v, "section")?;
    if !t.hash.is_empty() || !(5..=6).contains(&t.array.len()) {
        return Err(format!("section shape {} / {}", t.array.len(), t.hash.len()));
    }
    let subs = want_table(&t.array[4], "subSection")?;
    Ok(Section {
        width_cols: want_num(&t.array[0], "width")?,
        id: want_str(&t.array[1], "id")?,
        group: want_num(&t.array[2], "group")?,
        colour: want_str(&t.array[3], "colour")?,
        sub_sections: subs
            .array
            .iter()
            .map(|x| sub_section_of(want_table(x, "subsection")?))
            .collect::<Result<_, _>>()?,
        has_update_func: t.array.len() == 6,
    })
}

/// CalcsTab.lua:46-147, the `SkillSelect` section (controls; the last three rows are text).
fn skill_select() -> Section {
    let ctl = |label: &str, name: &str, flags: Flags| Row {
        label: Some(label.into()),
        flags,
        cells: vec![Cell {
            control_name: Some(name.into()),
            ..Cell::default()
        }],
        ..Row::default()
    };
    let flag = |f: &str| Flags {
        flag: Some(f.into()),
        ..Flags::default()
    };
    let player_flag = |f: &str| Flags {
        player_flag: Some(f.into()),
        ..Flags::default()
    };
    let text = |label: &str, f: &str, format: &str, breakdown: Option<&str>| Row {
        label: Some(label.into()),
        flags: flag(f),
        text_size: Some(12.0),
        cells: vec![Cell {
            format: Some(format.into()),
            entries: breakdown
                .map(|b| {
                    vec![Entry {
                        breakdown: Some(b.into()),
                        ..Entry::default()
                    }]
                })
                .unwrap_or_default(),
            ..Cell::default()
        }],
        ..Row::default()
    };
    Section {
        width_cols: 3.0,
        id: "SkillSelect".into(),
        group: 1.0,
        colour: color_code("NORMAL").expect("NORMAL").into(),
        sub_sections: vec![SubSection {
            label: "View Skill Details".into(),
            default_collapsed: false,
            rows: vec![
                ctl("Socket Group", "mainSocketGroup", Flags::default()),
                ctl("Active Skill", "mainSkill", Flags::default()),
                ctl("Skill Part", "mainSkillPart", player_flag("multiPart")),
                ctl("Skill Stages", "mainSkillStageCount", player_flag("multiStage")),
                ctl("Active Mines", "mainSkillMineCount", player_flag("mine")),
                ctl("Show Minion Stats", "showMinion", flag("haveMinion")),
                ctl("Minion", "mainSkillMinion", flag("minion")),
                ctl("Spectre Library", "mainSkillMinionLibrary", flag("spectre")),
                ctl("Minion Skill", "mainSkillMinionSkill", flag("haveMinion")),
                ctl("Calculation Mode", "mode", Flags::default()),
                text("Aura and Buff Skills", "buffs", "{output:BuffList}", Some("SkillBuffs")),
                text("Combat Buffs", "combat", "{output:CombatList}", None),
                text("Curses and Debuffs", "effective", "{output:CurseList}", Some("SkillDebuffs")),
            ],
            ..SubSection::default()
        }],
        has_update_func: true,
    }
}

/// `calcsTab.sectionList`: `SkillSelect`, then every entry of `Modules/CalcSections.lua` (CalcsTab.lua:46-156).
pub fn sections() -> &'static [Section] {
    static S: OnceLock<Vec<Section>> = OnceLock::new();
    S.get_or_init(|| {
        let data = parse_calc_sections(CALC_SECTIONS_LUA).unwrap_or_else(|e| panic!("{e}"));
        let mut v = vec![skill_select()];
        v.extend(
            data.iter()
                .map(|x| section_of(x).unwrap_or_else(|e| panic!("Modules/CalcSections.lua: {e}"))),
        );
        v
    })
}

// --- the actor a section is drawn for -----------------------------------------------------------------------------------------------

/// `actor.mainSkill[name.."Cfg"]` as a ModStore cfg (`None` = nil).
pub type CfgFn<'a> = dyn Fn(&str) -> Option<ModCfg> + 'a;

/// `actor` of `DrawContent` / `CheckFlag`: `calcsEnv.player` or `calcsEnv.minion`.
pub struct CalcsActor<'a> {
    pub state: &'a ActorState,
    pub env: &'a Env,
    /// `actor.mainSkill.skillFlags`.
    pub skill_flags: &'a IndexMap<String, bool>,
    /// `actor.modDB`.
    pub mod_db: StoreRef,
    /// `actor.enemy.modDB` (`None` = `actor.enemy` nil).
    pub enemy_db: Option<StoreRef>,
    /// `actor.mainSkill.skillModList`.
    pub skill_mod_list: Option<StoreRef>,
    /// `actor.mainSkill[name.."Cfg"]` (`None` = nil; the cell then uses `{ }`, CalcFormat.lua:46).
    pub cfg: Box<CfgFn<'a>>,
}

/// Tables `output[ns]` may be.
enum NsTable<'a> {
    Hand(&'a IndexMap<pob_mods::Str, OutValue>),
    Out(&'a OutTable),
}

/// `actor.output[k]` as the format code reads it.
#[derive(Clone, Debug)]
enum OV {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    Table,
}

impl OV {
    fn of(v: Option<&OutValue>) -> OV {
        match v {
            None => OV::Nil,
            Some(OutValue::Num(n)) => OV::Num(*n),
            Some(OutValue::Bool(b)) => OV::Bool(*b),
            Some(OutValue::Str(s)) => OV::Str(s.to_string()),
        }
    }
    fn truthy(&self) -> bool {
        !matches!(self, OV::Nil | OV::Bool(false))
    }
    fn type_name(&self) -> &'static str {
        match self {
            OV::Nil => "nil",
            OV::Bool(_) => "boolean",
            OV::Num(_) => "number",
            OV::Str(_) => "string",
            OV::Table => "table",
        }
    }
}

impl CalcsActor<'_> {
    fn ns_table(&self, ns: &str) -> Option<NsTable<'_>> {
        if let Some(t) = self.state.output.table(ns) {
            return Some(NsTable::Out(t));
        }
        let hand = match ns {
            "MainHand" => 0,
            "OffHand" => 1,
            _ => return None,
        };
        self.state.hand_output(hand).map(NsTable::Hand)
    }

    /// `actor.output[k]`.
    fn out(&self, k: &str) -> OV {
        match self.state.output.get(k) {
            Some(v) => OV::of(Some(v)),
            None if self.ns_table(k).is_some() => OV::Table,
            None => OV::Nil,
        }
    }

    /// `actor.output[ns] and actor.output[ns][var]` -- `Err` = the Lua error of indexing a non-table.
    fn out_ns(&self, ns: &str, var: &str) -> Result<OV, String> {
        match self.ns_table(ns) {
            Some(NsTable::Hand(m)) => Ok(OV::of(m.get(var))),
            Some(NsTable::Out(OutTable::Minion(m))) => {
                let st = m.0.borrow();
                Ok(match st.output.get(var) {
                    Some(v) => OV::of(Some(v)),
                    None if st.output.table(var).is_some() || matches!(var, "MainHand" | "OffHand") && st.hand_output(0).is_some() => {
                        OV::Table
                    }
                    None => OV::Nil,
                })
            }
            // a list / requirement table indexed by a name: nil (no such string key)
            Some(NsTable::Out(OutTable::SkillDps(_) | OutTable::Names(_))) => Ok(OV::Nil),
            Some(NsTable::Out(OutTable::ReqSource(_))) => Err(format!("calcs: output.{ns}.{var} of a requirement entry is not modelled")),
            None => match self.out(ns) {
                v if !v.truthy() => Ok(v),
                v => Err(format!("attempt to index field '{ns}' (a {} value)", v.type_name())),
            },
        }
    }
}

/// The input of `CheckFlag`: the shown actor and the player (`playerFlag`).
pub struct CheckCtx<'a, 'b> {
    pub actor: &'b CalcsActor<'a>,
    pub player_flags: &'a IndexMap<String, bool>,
}

fn set(f: &IndexMap<String, bool>, k: &str) -> bool {
    f.get(k).copied().unwrap_or(false)
}

/// `CalcsTab:CheckFlag(obj)` (:396-432) -- `Err` = a Lua error (indexing a non-table output value).
pub fn check_flag(ctx: &CheckCtx<'_, '_>, f: &Flags) -> Result<bool, String> {
    let sf = ctx.actor.skill_flags;
    if f.flag.as_deref().is_some_and(|x| !set(sf, x)) {
        return Ok(false);
    }
    if f.flag_list.as_ref().is_some_and(|l| l.iter().any(|x| !set(sf, x))) {
        return Ok(false);
    }
    if f.player_flag.as_deref().is_some_and(|x| !set(ctx.player_flags, x)) {
        return Ok(false);
    }
    if f.not_flag.as_deref().is_some_and(|x| set(sf, x)) {
        return Ok(false);
    }
    if f.not_flag_list.as_ref().is_some_and(|l| l.iter().any(|x| set(sf, x))) {
        return Ok(false);
    }
    if let Some(h) = &f.have_output {
        let zero = |v: &OV| !v.truthy() || matches!(v, OV::Num(n) if *n == 0.0);
        let v = match ns_var(h) {
            Some((ns, var)) => {
                // `not actor.output[ns] or not actor.output[ns][var] or ... == 0`
                if !ctx.actor.out(&ns).truthy() {
                    return Ok(false);
                }
                ctx.actor.out_ns(&ns, &var)?
            }
            None => ctx.actor.out(h),
        };
        if zero(&v) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `c:match("^(%a+)%.(%a+)$")`.
fn ns_var(c: &str) -> Option<(String, String)> {
    let (a, b) = c.split_once('.')?;
    let alpha = |s: &str| !s.is_empty() && s.bytes().all(|x| x.is_ascii_alphabetic());
    (alpha(a) && alpha(b)).then(|| (a.to_owned(), b.to_owned()))
}

// --- formatCalcStr ---------------------------------------------------------------------------------------------------------------------

/// Common.lua:709-715 `round(val, dec)`.
fn round(val: f64, dec: f64) -> f64 {
    let p = 10f64.powf(dec);
    (val * p + 0.5).floor() / p
}

/// Lua arithmetic on an output value (`round(val, p)`): numbers and numeric strings; the rest is the Lua error.
fn arith(v: &OV) -> Result<f64, String> {
    match v {
        OV::Num(n) => Ok(*n),
        OV::Str(s) => pob_lua::tonumber(s).ok_or_else(|| "attempt to perform arithmetic on a string value".to_owned()),
        other => Err(format!("attempt to perform arithmetic on a {} value", other.type_name())),
    }
}

/// CalcFormat.lua:12 `formatCalcVal(val, p)`.
fn format_calc_val(v: f64, p: f64, st: &DisplaySettings) -> String {
    format_num_sep(&pob_lua::tostring_num(round(v, p)), st)
}

/// `string.gsub(str, pat, f)` where `f` may raise.
/// A `gsub` replacement function: the captures -> the replacement (`None` keeps the match) or a Lua error.
type ReplFn<'f> = dyn FnMut(&[&str]) -> Result<Option<String>, String> + 'f;

fn gsub_fn(s: &str, pat: &str, f: &mut ReplFn<'_>) -> Result<String, String> {
    let err: RefCell<Option<String>> = RefCell::new(None);
    let mut cb = |subject: &[u8], caps: &pob_lua::Captures| -> Option<Vec<u8>> {
        if err.borrow().is_some() {
            return None;
        }
        let parts: Vec<String> = (0..caps.len())
            .map(|i| String::from_utf8_lossy(caps.get(i).and_then(|c| c.bytes(subject)).unwrap_or_default()).into_owned())
            .collect();
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        match f(&refs) {
            Ok(v) => v.map(String::into_bytes),
            Err(e) => {
                *err.borrow_mut() = Some(e);
                None
            }
        }
    };
    let (out, _) = pob_lua::gsub(s.as_bytes(), pat.as_bytes(), Repl::Fn(&mut cb), None).map_err(|e| format!("{e:?}"))?;
    if let Some(e) = err.into_inner() {
        return Err(e);
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// The `gsub` replacement rules for a value (`lstrlib.c add_value`): a string or a number is used, nil/false keep the match, the rest raise.
fn repl_value(v: OV) -> Result<Option<String>, String> {
    match v {
        OV::Str(s) => Ok(Some(s)),
        OV::Num(n) => Ok(Some(pob_lua::tostring_num(n))),
        OV::Nil | OV::Bool(false) => Ok(None),
        other => Err(format!("invalid replacement value (a {})", other.type_name())),
    }
}

/// `modStore:Combine(modType, modCfg, ...)` (ModStore.lua:145).
pub(crate) fn combine(store: &StoreRef, ty: &str, cfg: &ModCfg, names: &[&str]) -> Result<Value, String> {
    let s = store.borrow();
    let r = match ty {
        "MORE" => s.more(Some(cfg), names).map(Value::Num),
        "FLAG" => s.flag(Some(cfg), names).map(|b| b.map_or(Value::Nil, Value::Bool)),
        "OVERRIDE" => s.override_(Some(cfg), names).map(Option::unwrap_or_default),
        "MAX" => s.max(Some(cfg), names).map(|m| m.map_or(Value::Nil, Value::Num)),
        "LIST" => return Err("calcs: {p:mod} over a LIST (a table total) is not modelled".into()),
        other => {
            let t = match other {
                "BASE" => ModType::Base,
                "INC" => ModType::Inc,
                _ => return Err(format!("calcs: Sum of mod type {other}")),
            };
            s.sum(t, Some(cfg), names).map(Value::Num)
        }
    };
    r.map_err(|e| e.to_string())
}

/// CalcFormat.lua:16 `formatCalcStr(str, actor, colData)`.
pub fn format_calc_str(s: &str, actor: &CalcsActor<'_>, cell: Option<&Cell>, st: &DisplaySettings) -> Result<String, String> {
    // :18-25 `{output:Key}`
    let s = gsub_fn(s, "{output:([%a%.:]+)}", &mut |c| {
        let v = match ns_var(c[0]) {
            Some((ns, var)) => {
                if actor.out(&ns).truthy() {
                    actor.out_ns(&ns, &var)?
                } else {
                    actor.out(&ns)
                }
            }
            None => actor.out(c[0]),
        };
        // `x or ""`
        repl_value(if v.truthy() { v } else { OV::Str(String::new()) })
    })?;
    // :26-33 `{p:output:Key}`
    let s = gsub_fn(&s, "{(%d+):output:([%a%.:]+)}", &mut |c| {
        let p = pob_lua::tonumber(c[0]).expect("digits");
        let v = match ns_var(c[1]) {
            Some((ns, var)) => {
                let t = actor.out(&ns);
                if t.truthy() { actor.out_ns(&ns, &var)? } else { t }
            }
            None => actor.out(c[1]),
        };
        let v = if v.truthy() { v } else { OV::Num(0.0) };
        Ok(Some(format_calc_val(arith(&v)?, p, st)))
    })?;
    // :34-71 `{p:mod:1,2,...}`
    gsub_fn(&s, "{(%d+):mod:([%d,]+)}", &mut |c| {
        let p = pob_lua::tonumber(c[0]).expect("digits");
        let nums: Vec<usize> = c[1]
            .split(',')
            .filter(|x| !x.is_empty())
            .map(|x| x.parse().expect("digits"))
            .collect();
        let Some(cell) = cell else {
            // `colData` nil (the subsection `extra`): `colData[numList[1]]` indexes nil
            return Err("attempt to index local 'colData' (a nil value)".into());
        };
        let entry = |k: usize| k.checked_sub(1).and_then(|i| cell.entries.get(i));
        let Some(first_ty) = entry(nums[0]).and_then(|e| e.mod_type.clone()) else {
            return Ok(Some("?".into()));
        };
        let more = first_ty == "MORE";
        let mut total = if more { 1.0 } else { 0.0 };
        for &k in &nums {
            let Some(e) = entry(k) else { break };
            let mut cfg = match e.cfg.as_deref() {
                Some(name) if actor.skill_mod_list.is_some() => (actor.cfg)(name).unwrap_or_default(),
                _ => ModCfg::default(),
            };
            if let Some(src) = &e.mod_source {
                cfg.source = Some(pob_mods::Str::from(src.as_str()));
            }
            if let Some(a) = &e.actor {
                cfg.actor = Some(pob_mods::Str::from(a.as_str()));
            }
            // `(enemy and actor.enemy and actor.enemy.modDB) or (cfg and actor.mainSkill and actor.mainSkill.skillModList) or actor.modDB`
            let store = match (&e.enemy, &actor.enemy_db, &e.cfg, &actor.skill_mod_list) {
                (Some(true), Some(db), _, _) => db.clone(),
                (_, _, Some(_), Some(l)) => l.clone(),
                _ => actor.mod_db.clone(),
            };
            let ty = e.mod_type.as_deref().unwrap_or("nil");
            let names: Vec<&str> = e.mod_name.iter().map(String::as_str).collect();
            let v = match combine(&store, ty, &cfg, &names)? {
                Value::Num(n) => n,
                other => {
                    return Err(format!(
                        "attempt to perform arithmetic on local 'modVal' (a {} value)",
                        other.type_name()
                    ));
                }
            };
            if more {
                total *= v;
            } else {
                total += v;
            }
        }
        if more {
            total = (total - 1.0) * 100.0;
        }
        Ok(Some(format_calc_val(total, p, st)))
    })
}

// --- UpdateSize + DrawContent ---------------------------------------------------------------------------------------------------------

/// Which `DrawString` of `CalcSectionControl:DrawContent` drew a text -- the line of the call (the probe records it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawKind {
    /// :451 the label of a disabled section (`"^8"..label`).
    DisabledLabel = 451,
    /// :458 `"^7"..label..":"`.
    Label = 458,
    /// :461 `"^7"..formatCalcStr(extra)`.
    Extra = 461,
    /// :501 the row label.
    RowLabel = 501,
    /// :538 a cell.
    Cell = 538,
}

/// One section as `UpdateSize` + `DrawContent` leave it.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionRender {
    pub id: String,
    pub enabled: bool,
    pub height: f64,
    pub draws: Vec<(DrawKind, String)>,
    /// A Lua error that stopped the section (the probe's `pcall`).
    pub error: Option<String>,
}

/// `section:UpdateSize()` then `section:DrawContent(...)` for every section, `actor` shown (`collapsed`: the `<Section subsection>` states by
/// `(section id, subsection id)`, where the id is the label without non-alphanumerics; absent = `defaultCollapsed`).
pub fn render_sections(
    actor: &CalcsActor<'_>,
    player_flags: &IndexMap<String, bool>,
    collapsed: &HashMap<(String, String), bool>,
    st: &DisplaySettings,
) -> Vec<SectionRender> {
    let ctx = CheckCtx { actor, player_flags };
    sections()
        .iter()
        .map(|sec| {
            let mut r = SectionRender {
                id: sec.id.clone(),
                enabled: false,
                height: 0.0,
                draws: Vec::new(),
                error: None,
            };
            if let Err(e) = render_one(sec, &ctx, collapsed, st, &mut r) {
                r.error = Some(e);
            }
            r
        })
        .collect()
}

fn render_one(
    sec: &Section,
    ctx: &CheckCtx<'_, '_>,
    collapsed: &HashMap<(String, String), bool>,
    st: &DisplaySettings,
    r: &mut SectionRender,
) -> Result<(), String> {
    let sub_collapsed: Vec<bool> = sec
        .sub_sections
        .iter()
        .map(|s| {
            let id: String = s.label.chars().filter(char::is_ascii_alphanumeric).collect();
            collapsed.get(&(sec.id.clone(), id)).copied().unwrap_or(s.default_collapsed)
        })
        .collect();
    // :115-160 UpdateSize -- `self.flag` / `self.notFlag` are `subSection[1].data.flag` / `.notFlag`
    let first = &sec.sub_sections[0];
    let sec_flags = Flags {
        flag: first.flag.clone(),
        not_flag: first.not_flag.clone(),
        ..Flags::default()
    };
    let mut row_enabled: Vec<Vec<bool>> = sec.sub_sections.iter().map(|s| vec![false; s.rows.len()]).collect();
    let mut enabled = check_flag(ctx, &sec_flags)?;
    if !enabled {
        r.height = 22.0;
    } else {
        let mut height = 2.0;
        enabled = false;
        for (i, sub) in sec.sub_sections.iter().enumerate() {
            let mut temp = 0.0;
            for (j, row) in sub.rows.iter().enumerate() {
                let e = check_flag(ctx, &row.flags)?;
                if e {
                    enabled = true;
                    height += 18.0;
                    temp += 18.0;
                }
                row_enabled[i][j] = e && !sub_collapsed[i];
            }
            if enabled && !sub_collapsed[i] {
                height += 22.0;
            } else {
                height = height - temp + 20.0;
            }
        }
        if !(enabled && !sub_collapsed[0]) {
            height = 22.0;
        }
        r.height = height;
    }
    r.enabled = enabled;
    // :437-549 DrawContent (non-overlay; the search box is empty)
    let mut primary = true;
    for (i, sub) in sec.sub_sections.iter().enumerate() {
        if !enabled {
            r.draws.push((DrawKind::DisabledLabel, format!("^8{}", sub.label)));
        } else {
            r.draws.push((DrawKind::Label, format!("^7{}:", sub.label)));
            if let Some(extra) = &sub.extra {
                r.draws
                    .push((DrawKind::Extra, format!("^7{}", format_calc_str(extra, ctx.actor, None, st)?)));
            }
        }
        if sub_collapsed[i] || !enabled {
            if primary {
                return Ok(());
            }
            primary = false;
            continue;
        }
        primary = false;
        for (j, row) in sub.rows.iter().enumerate() {
            if !row_enabled[i][j] {
                continue;
            }
            if let Some(label) = &row.label {
                let colour = row.color.as_deref().unwrap_or("^7");
                r.draws.push((DrawKind::RowLabel, format!("{colour}{label}^7:")));
            }
            for cell in &row.cells {
                if let Some(fmt) = &cell.format
                    && check_flag(ctx, &cell.flags)?
                {
                    r.draws
                        .push((DrawKind::Cell, format!("^7{}", format_calc_str(fmt, ctx.actor, Some(cell), st)?)));
                }
            }
        }
    }
    Ok(())
}

// --- the actors of a CALCS run --------------------------------------------------------------------------------------------------------

/// `actor.mainSkill[name.."Cfg"]` for a skill: `skillCfg` / `weapon1Cfg` / `weapon2Cfg` (CalcActiveSkill.lua:464-489, with the `skillCond`
/// writes of offence) and the cfgs offence leaves on the skill (CalcOffence.lua:4414/:4687/:4992 `bleedCfg`..., :5701 `decayCfg`, :5803/:5812
/// `dotCfg` / `dot<Type>Cfg`; [`pob_calc::active_skill::ActiveSkill::offence_cfg`]). Any other name is nil (`{ }`) upstream too.
fn skill_cfg_lookup<'a>(
    skills: &'a pob_calc::skills::SkillsState,
    sk: &'a pob_calc::active_skill::ActiveSkill,
) -> impl Fn(&str) -> Option<ModCfg> + 'a {
    move |name: &str| {
        let base = sk.skill_cfg.as_ref();
        let of = |c: Option<&pob_calc::active_skill::SkillCfg>, parent: Option<&IndexMap<String, bool>>| {
            c.map(|c| c.mod_cfg(&skills.skills, &sk.skill_types, parent))
        };
        match name {
            "skill" => of(base, None),
            "weapon1" => of(sk.weapon1_cfg.as_ref(), base.map(|b| &b.skill_cond)),
            "weapon2" => of(sk.weapon2_cfg.as_ref(), base.map(|b| &b.skill_cond)),
            other => sk.offence_cfg(&skills.skills, &format!("{other}Cfg")),
        }
    }
}

/// `calcsEnv.player` of a CALCS run.
pub fn player_actor<'a>(env: &'a Env, state: &'a ActorState) -> Result<CalcsActor<'a>, String> {
    let st = env.skills.as_ref().ok_or("calcs: env.player.mainSkill needs the skill phase")?;
    let main = st.main_skill.ok_or("calcs: env.player.mainSkill is nil")?;
    let sk = st.skill(main);
    Ok(CalcsActor {
        state,
        env,
        skill_flags: &sk.skill_flags,
        mod_db: env.player.borrow().mod_db.clone(),
        enemy_db: Some(env.enemy.borrow().mod_db.clone()),
        skill_mod_list: Some(env.skill_mod_list(main)),
        cfg: Box::new(skill_cfg_lookup(st, sk)),
    })
}

/// `calcsEnv.minion` of a CALCS run (`state` = its perform state, `output.Minion`); `None` when the env has no minion.
pub fn minion_actor<'a>(env: &'a Env, state: &'a ActorState) -> Result<Option<CalcsActor<'a>>, String> {
    let Some(actor) = env.minion.as_ref() else { return Ok(None) };
    let st = env.skills.as_ref().ok_or("calcs: env.player.mainSkill needs the skill phase")?;
    let main = st.skill(st.main_skill.ok_or("calcs: env.player.mainSkill is nil")?);
    let m = main
        .minion
        .as_deref()
        .ok_or("calcs: env.minion without env.player.mainSkill.minion")?;
    let ms = m.main();
    let a = actor.borrow();
    Ok(Some(CalcsActor {
        state,
        env,
        skill_flags: &ms.skill_flags,
        mod_db: a.mod_db.clone(),
        enemy_db: a.enemy.upgrade().map(|e| e.borrow().mod_db.clone()),
        skill_mod_list: ms.skill_mod_list.clone(),
        cfg: Box::new(skill_cfg_lookup(st, ms)),
    }))
}

/// `CalcsTab:BuildOutput()` (CalcsTab.lua:441-474) on a fresh session: `buildOutput(MAIN)` -> `mainEnv`, `buildOutput(CALCS)` -> `calcsEnv`,
/// then `getMiscCalculator` (its base run shares the build's tables with the two above, as upstream).
pub struct CalcsTabOutput {
    pub session: pob_calc::CalcSession,
    pub main: pob_calc::BuildOutput,
    pub calcs: pob_calc::BuildOutput,
}

pub fn build_output(model: pob_calc::pob_build::BuildModel) -> Result<CalcsTabOutput, String> {
    let session = pob_calc::CalcSession::new(Rc::new(model), pob_calc::CalculateOptions::default());
    let main = session.build_output(pob_calc::CalcMode::Main).map_err(|e| e.to_string())?;
    let calcs = session.build_output(pob_calc::CalcMode::Calcs).map_err(|e| e.to_string())?;
    session.get_misc_calculator().map_err(|e| e.to_string())?;
    Ok(CalcsTabOutput { session, main, calcs })
}

/// The sections of a CALCS run for the player (`input.showMinion` false) and, when the env has one, the minion (`showMinion` true).
pub fn render_run(run: &pob_calc::CalcRun, st: &DisplaySettings) -> Result<(Vec<SectionRender>, Option<Vec<SectionRender>>), String> {
    let none = HashMap::new();
    let player = player_actor(&run.env, &run.state.player)?;
    let p = render_sections(&player, player.skill_flags, &none, st);
    let m = match &run.state.minion {
        Some(ms) if run.env.minion.is_some() => {
            let ms = ms.borrow();
            let minion = minion_actor(&run.env, &ms)?.expect("env.minion");
            Some(render_sections(&minion, player.skill_flags, &none, st))
        }
        _ => None,
    };
    Ok((p, m))
}
