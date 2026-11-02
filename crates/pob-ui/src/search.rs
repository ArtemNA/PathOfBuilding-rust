//! `Classes/SearchHost.lua`: type-to-search over a list (case-insensitive words, in order unless `ignoreOrder`).

use crate::lua;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// `{ from, to }` (1-based inclusive byte range).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub from: usize,
    pub to: usize,
}

/// `searchInfo = { ranges, matches }`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchInfo {
    pub ranges: Vec<Range>,
    pub matches: bool,
}

/// `splitWords(s)` (SearchHost.lua:19-25).
fn split_words(s: &str) -> Vec<&str> {
    s.split([' ', '\t', '\n', '\x0b', '\x0c', '\r']).filter(|w| !w.is_empty()).collect()
}

/// `wordsToCaselessPatterns(words)` (SearchHost.lua:27-40): `%W` → `%%%1`, `%a` → `[aA]`.
fn words_to_caseless_patterns(words: &[&str]) -> Vec<String> {
    words
        .iter()
        .map(|w| {
            let mut p = String::new();
            // A multi-byte char gets one `%` instead of one per byte: `%` + the first byte escapes it, the
            // continuation bytes are literal either way — the same pattern semantics.
            for c in w.chars() {
                if c.is_ascii_alphabetic() {
                    p.push('[');
                    p.push(c.to_ascii_lowercase());
                    p.push(c.to_ascii_uppercase());
                    p.push(']');
                } else if c.is_ascii_digit() {
                    p.push(c);
                } else {
                    p.push('%');
                    p.push(c);
                }
            }
            p
        })
        .collect()
}

/// `matchWords(searchWords, entry, valueAccessor, ignoreOrder)` (SearchHost.lua:42-79).
fn match_words(search_words: &[String], value: &str, ignore_order: bool) -> SearchInfo {
    let mut info = SearchInfo {
        ranges: Vec::new(),
        matches: true,
    };
    let mut last_match_end = 0;
    for word in search_words {
        if let Some((from, to)) = lua::find(value, word, Some(last_match_end as i64 + 1), false) {
            info.ranges.push(Range { from, to });
            if !ignore_order {
                last_match_end = to;
            }
        } else {
            info.matches = false;
        }
    }
    if ignore_order {
        // `table.sort` by `from` (unstable in LuaJIT; ranges with equal `from` merge to the same result)
        info.ranges.sort_by_key(|r| r.from);
        let mut i = 0;
        while i < info.ranges.len() {
            if i + 1 < info.ranges.len() && info.ranges[i + 1].from <= info.ranges[i].to {
                info.ranges[i].to = info.ranges[i].to.max(info.ranges[i + 1].to);
                info.ranges.remove(i + 1);
            } else {
                i += 1;
            }
        }
    }
    info
}

/// Fields of the `SearchHost` mixin. `list` = `searchListAccessor()` mapped through `valueAccessor`.
pub struct SearchHost {
    pub search_list: RefCell<Option<Rc<dyn Fn() -> Vec<String>>>>,
    pub search_term: RefCell<String>,
    /// `searchInfos[i]` for list index `i + 1`.
    pub search_infos: RefCell<Vec<SearchInfo>>,
    pub ignore_order: Cell<bool>,
    pub match_count: Cell<usize>,
}

impl SearchHost {
    /// `SearchHost:SearchHost(listAccessor, valueAccessor, ignoreOrder)` (SearchHost.lua:10-17).
    pub fn new(values: Option<Rc<dyn Fn() -> Vec<String>>>, ignore_order: bool) -> Self {
        Self {
            search_list: RefCell::new(values),
            search_term: RefCell::new(String::new()),
            search_infos: RefCell::new(Vec::new()),
            ignore_order: Cell::new(ignore_order),
            match_count: Cell::new(0),
        }
    }

    /// SearchHost.lua:94-96
    pub fn is_search_active(&self) -> bool {
        !self.search_term.borrow().is_empty()
    }

    /// SearchHost.lua:98-110 (returns `self`: always truthy).
    pub fn on_search_char(&self, ch: &str) {
        if lua::is_match(ch, "%s") {
            let t = self.search_term.borrow().clone();
            if t.is_empty() || lua::is_match(&lua::sub_from(&t, -1), "%s") {
                return;
            }
        }
        if !lua::is_match(ch, "%c") {
            self.search_term.borrow_mut().push_str(ch);
            self.update_search();
        }
    }

    /// SearchHost.lua:112-121: `true` = handled (`return self`).
    pub fn on_search_key_down(&self, key: &str) -> bool {
        if self.is_search_active() && key == "ESCAPE" {
            self.reset_search();
            true
        } else if self.is_search_active() && key == "BACK" {
            let t = lua::sub(&self.search_term.borrow(), 1, -2);
            *self.search_term.borrow_mut() = t;
            self.update_search();
            true
        } else {
            false
        }
    }

    /// SearchHost.lua:123-131
    pub fn update_match_count(&self) {
        let n = self.search_infos.borrow().iter().filter(|i| i.matches).count();
        self.match_count.set(n);
    }

    pub fn get_match_count(&self) -> usize {
        self.match_count.get()
    }

    /// SearchHost.lua:137-142 (`matchTerm`, :81-92).
    pub fn update_search(&self) {
        let acc = self.search_list.borrow().clone();
        if let Some(acc) = acc {
            let term = self.search_term.borrow().clone();
            let infos = if term.is_empty() {
                Vec::new()
            } else {
                let words = split_words(&term);
                let patterns = words_to_caseless_patterns(&words);
                acc().iter().map(|v| match_words(&patterns, v, self.ignore_order.get())).collect()
            };
            *self.search_infos.borrow_mut() = infos;
            self.update_match_count();
        }
    }

    /// SearchHost.lua:144-148
    pub fn reset_search(&self) {
        self.search_term.borrow_mut().clear();
        self.match_count.set(0);
        self.search_infos.borrow_mut().clear();
    }

    /// SearchHost.lua:150-153
    pub fn get_search_term_pretty(&self) -> String {
        let color = if self.is_search_active() && self.match_count.get() > 0 {
            "^xFFFFFF"
        } else {
            "^xFF0000"
        };
        format!("{color}{}", self.search_term.borrow())
    }
}
