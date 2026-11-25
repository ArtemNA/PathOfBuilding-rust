//! `itemsTab.itemSets`, `skillsTab.skillSets` and `configTab.configSets` as LuaJIT tables: 2.69.0 takes the id of a new or copied
//! set as `#self.xSets + 1` (ItemsTab.lua:1686, SkillsTab.lua:1469/:1496, ConfigTab.lua:1285/:1312, Build.lua:822-824), and with a
//! hole (a deleted set) `#t` is a border the table's layout chooses. Each table here gets the assignments the Lua one gets:
//! - `Load` (ItemsTab.lua:1285, SkillsTab.lua:526, ConfigTab.lua:843): `{}`, then `t[id] = set` in the order of the file (the model's
//!   sets are in that order after a load; a repeated id updates in place);
//! - a new / copied set: `t[id] = set`; a deleted one: `t[id] = nil`;
//! - the items' undo (ItemsTab.lua:5226, :5240): `self.itemSets = copyTableSafe(...)` of the snapshot -- a new table filled in the
//!   `pairs` order of the old one; the skills' (SkillsTab.lua:1414-1426, :1437-1440): `wipeTable(self.skillSets)` then the snapshot's
//!   keys in its `pairs` order; the config's undo does not touch `configSets` (ConfigTab.lua:1244-1270).
use pob_build::BuildModel;
use pob_tree::lj_table::{LjTable, NIL_STORE_CREATES_KEY};

#[derive(Clone, Debug)]
pub struct SetTables {
    pub items: LjTable,
    pub skills: LjTable,
    pub config: LjTable,
}

fn table_of(ids: impl IntoIterator<Item = f64>) -> LjTable {
    let mut t = LjTable::new();
    for id in ids {
        t.set(id);
    }
    t
}

impl SetTables {
    /// The tables `Load` leaves (see the module doc).
    pub fn from_model(m: &BuildModel) -> SetTables {
        SetTables {
            items: table_of(m.item_sets.iter().map(|s| s.id)),
            skills: table_of(m.skills.sets.iter().map(|s| s.id)),
            config: table_of(m.config.sets.iter().map(|s| s.id)),
        }
    }
}

/// `#t + 1`: the id `NewLoadout`, `CopyXSet` and `CreateSkillSet` / `CreateConfigSet` without an id take.
pub fn next_id(t: &LjTable) -> f64 {
    (t.len() + 1) as f64
}

/// `t[id] = nil`.
pub fn remove(t: &mut LjTable, id: f64) {
    t.remove(id, NIL_STORE_CREATES_KEY);
}

/// A new table filled in the `pairs` order of `t` (`copyTable`, `copyTableSafe`, the loop of SkillsTab:CreateUndoState).
pub fn copy_table(t: &LjTable) -> LjTable {
    table_of(t.pairs_keys())
}

/// `wipeTable(t)` (Common.lua:550-558: `t[k] = nil` for `pairs(t)`), then `t[k] = v` for `pairs(from)` (SkillsTab.lua:1437-1440).
pub fn wipe_and_fill(t: &mut LjTable, from: &LjTable) {
    for k in t.pairs_keys() {
        t.remove(k, NIL_STORE_CREATES_KEY);
    }
    for k in from.pairs_keys() {
        t.set(k);
    }
}
