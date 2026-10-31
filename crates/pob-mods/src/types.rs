//! The mod-core data model (MOD-CORE §1, §8): `Mod`, `Tag`, `modLib.createMod`, and the typed views of the actor, the
//! item and the query configuration -- only the fields `ModStore`/`EvalMod` actually read.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;

use crate::store::StoreRef;
use crate::value::{Str, Table, Value};

/// Every `type` that occurs in the data. `FlagTypo` is upstream's `"Flag"` (9 mods, L23): stored verbatim, it matches no
/// query of the core except an explicit `"Flag"` one. A mod with `type = nil` (L24) is `Mod::ty == None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModType {
    Base,
    Inc,
    More,
    Flag,
    Override,
    List,
    Max,
    Min,
    Chance,
    Dummy,
    FlagTypo,
}

impl ModType {
    pub fn as_str(self) -> &'static str {
        match self {
            ModType::Base => "BASE",
            ModType::Inc => "INC",
            ModType::More => "MORE",
            ModType::Flag => "FLAG",
            ModType::Override => "OVERRIDE",
            ModType::List => "LIST",
            ModType::Max => "MAX",
            ModType::Min => "MIN",
            ModType::Chance => "CHANCE",
            ModType::Dummy => "DUMMY",
            ModType::FlagTypo => "Flag",
        }
    }

    /// The data spelling back to the enum; `None` for anything else (not a type the data knows).
    pub fn parse(s: &str) -> Option<ModType> {
        Some(match s {
            "BASE" => ModType::Base,
            "INC" => ModType::Inc,
            "MORE" => ModType::More,
            "FLAG" => ModType::Flag,
            "OVERRIDE" => ModType::Override,
            "LIST" => ModType::List,
            "MAX" => ModType::Max,
            "MIN" => ModType::Min,
            "CHANCE" => ModType::Chance,
            "DUMMY" => ModType::Dummy,
            "Flag" => ModType::FlagTypo,
            _ => return None,
        })
    }
}

/// The tag types `EvalMod` has a branch for (ModStore.lua:369-957), plus everything else (`GlobalEffect`, `Global`,
/// `SlotNumber`, `InSlot`, `DisablesItem`, `Skill`, ... -- ignored by EvalMod, MOD-CORE §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagKind {
    Multiplier,
    MultiplierThreshold,
    PerStat,
    PercentStat,
    StatThreshold,
    DistanceRamp,
    MeleeProximity,
    Limit,
    Condition,
    ActorCondition,
    ItemCondition,
    SocketedIn,
    SkillName,
    SkillId,
    SkillPart,
    SkillType,
    BaseFlag,
    SlotName,
    ModFlagOr,
    KeywordFlagAnd,
    MonsterTag,
    Other,
}

impl TagKind {
    fn of(ty: &Value) -> TagKind {
        match ty.as_str() {
            Some("Multiplier") => TagKind::Multiplier,
            Some("MultiplierThreshold") => TagKind::MultiplierThreshold,
            Some("PerStat") => TagKind::PerStat,
            Some("PercentStat") => TagKind::PercentStat,
            Some("StatThreshold") => TagKind::StatThreshold,
            Some("DistanceRamp") => TagKind::DistanceRamp,
            Some("MeleeProximity") => TagKind::MeleeProximity,
            Some("Limit") => TagKind::Limit,
            Some("Condition") => TagKind::Condition,
            Some("ActorCondition") => TagKind::ActorCondition,
            Some("ItemCondition") => TagKind::ItemCondition,
            Some("SocketedIn") => TagKind::SocketedIn,
            Some("SkillName") => TagKind::SkillName,
            Some("SkillId") => TagKind::SkillId,
            Some("SkillPart") => TagKind::SkillPart,
            Some("SkillType") => TagKind::SkillType,
            Some("BaseFlag") => TagKind::BaseFlag,
            Some("SlotName") => TagKind::SlotName,
            Some("ModFlagOr") => TagKind::ModFlagOr,
            Some("KeywordFlagAnd") => TagKind::KeywordFlagAnd,
            Some("MonsterTag") => TagKind::MonsterTag,
            _ => TagKind::Other,
        }
    }
}

/// A tag: an open Lua table keyed by `type` (orchestrator decision 3). The branch kind is cached at construction; fields
/// are read with Lua semantics in `eval.rs`.
///
/// Tags are shared Lua tables, and EvalMod WRITES into them: `tag.div = GetMultiplier(self, tag.divVar, cfg)`
/// (ModStore.lua:401, :499; MOD-CORE §7.5 bug-compat). That write is observable by everyone holding the same tag object --
/// later reads of `tag.div`, Dump/`ser`, `formatTag` (and so `compareModParams`/`MergeMod`). `div` is the only field the
/// core ever writes, so it lives in a shared cell next to the immutable table: cloning a `Tag` shares the cell (the same
/// Lua object -- shallow copies such as `copyTable(mod, true)`, `AddList`), `deep_copy` (Lua `copyTable`) makes a new one.
///
/// The one field calc code writes into a tag is the second cell, `warcryPowerBonus` (CalcPerform.lua:2382
/// `warcryBuff[1].warcryPowerBonus = ...`, R6-ARCH §6): the warcry buff mod's first tag is the data's own table (`mergeLevelMod`
/// copies the mod with `copyTable(mod, true)`), so the write is seen by every mod sharing that tag object -- the same model as `div`.
#[derive(Clone, Debug)]
pub struct Tag {
    table: Rc<Table>,
    kind: TagKind,
    div: Rc<RefCell<Option<Value>>>,
    warcry_power_bonus: Rc<RefCell<Option<Value>>>,
}

impl Tag {
    pub fn new(table: Table) -> Tag {
        Tag::from_rc(Rc::new(table))
    }

    pub fn from_rc(table: Rc<Table>) -> Tag {
        let kind = TagKind::of(table.get("type"));
        Tag {
            table,
            kind,
            div: Rc::new(RefCell::new(None)),
            warcry_power_bonus: Rc::new(RefCell::new(None)),
        }
    }

    pub fn kind(&self) -> TagKind {
        self.kind
    }

    /// `tag.type`.
    pub fn ty(&self) -> &Value {
        self.table.get("type")
    }

    /// `tag.<key>` as Lua reads it now, including a `div` written by EvalMod.
    pub fn get(&self, key: &str) -> Value {
        if let Some(cell) = self.cell(key)
            && let Some(v) = &*cell.borrow()
        {
            return v.clone();
        }
        self.table.get(key).clone()
    }

    /// The shared cell behind a writable field (`div`, `warcryPowerBonus`).
    fn cell(&self, key: &str) -> Option<&Rc<RefCell<Option<Value>>>> {
        match key {
            "div" => Some(&self.div),
            "warcryPowerBonus" => Some(&self.warcry_power_bonus),
            _ => None,
        }
    }

    /// `tag.<key>` for any key but `div` (the only field the core writes), by reference.
    pub(crate) fn raw(&self, key: &str) -> &Value {
        debug_assert!(
            key != "div" && key != "warcryPowerBonus",
            "tag.{key} is mutable: read it with Tag::get"
        );
        self.table.get(key)
    }

    /// ModStore.lua:401 / :499 `tag.div = v` -- seen by every holder of this tag object.
    pub(crate) fn set_div(&self, v: Value) {
        *self.div.borrow_mut() = Some(v);
    }

    /// CalcPerform.lua:2382 `warcryBuff[1].warcryPowerBonus = v` -- seen by every holder of this tag object (the data's statMap
    /// mod included, as in LuaJIT, where the field outlives the perform pass).
    pub fn set_warcry_power_bonus(&self, v: Value) {
        *self.warcry_power_bonus.borrow_mut() = Some(v);
    }

    /// The tag as a Lua table right now (with a written `div` / `warcryPowerBonus`): what Dump/`ser` and `formatTag` see.
    pub fn table(&self) -> Rc<Table> {
        let (div, wpb) = (self.div.borrow(), self.warcry_power_bonus.borrow());
        if div.is_none() && wpb.is_none() {
            return self.table.clone();
        }
        let mut t = (*self.table).clone();
        for (k, v) in [("div", &*div), ("warcryPowerBonus", &*wpb)] {
            if let Some(v) = v {
                t.hash.insert(Rc::from(k), v.clone());
            }
        }
        Rc::new(t)
    }

    /// Lua `copyTable(tag)`: a new table object carrying the current fields (a written `div` / `warcryPowerBonus` included).
    pub fn deep_copy(&self) -> Tag {
        Tag {
            table: self.table(),
            kind: self.kind,
            div: Rc::new(RefCell::new(None)),
            warcry_power_bonus: Rc::new(RefCell::new(None)),
        }
    }
}

/// A mod (ModTools.lua:51-66). `tags` is the Lua array part `[1..k]` -- what `ipairs(mod)` sees (L9). `replaced`/`converted`
/// are the ModDB markers (ModDB.lua:55, :99); they are cells because one mod object can sit in several stores (`AddList`).
#[derive(Clone, Debug)]
pub struct Mod {
    pub name: Str,
    pub ty: Option<ModType>,
    pub value: Value,
    pub flags: u32,
    pub keyword_flags: u32,
    pub source: Option<Str>,
    pub tags: Vec<Tag>,
    pub replaced: Cell<bool>,
    pub converted: Cell<bool>,
}

impl Mod {
    /// Lua `copyTable(mod)` (ModStore.lua:67, :92): a new mod with new tag tables. `Clone` is the shallow
    /// `copyTable(mod, true)` (ModList.lua:77) -- the tags stay the same objects.
    pub fn deep_copy(&self) -> Mod {
        Mod {
            tags: self.tags.iter().map(Tag::deep_copy).collect(),
            ..self.clone()
        }
    }

    /// `mod.<key>` as Lua sees the record (used where a mod travels as a table value: `value.mod`, `ser`).
    pub fn field(&self, key: &str) -> Value {
        match key {
            "name" => Value::Str(self.name.clone()),
            "type" => self.ty.map_or(Value::Nil, |t| Value::str(t.as_str())),
            "value" => self.value.clone(),
            "flags" => Value::Num(f64::from(self.flags)),
            "keywordFlags" => Value::Num(f64::from(self.keyword_flags)),
            "source" => Value::from(self.source.as_ref()),
            "replaced" if self.replaced.get() => Value::Bool(true),
            "converted" if self.converted.get() => Value::Bool(true),
            _ => Value::Nil,
        }
    }

    pub(crate) fn set_field(&mut self, key: &str, v: Value) {
        match key {
            "value" => self.value = v,
            _ => unimplemented!("Mod::set_field({key}) -- the core only writes `value`"),
        }
    }

    /// `mod[1]` is truthy: the mod has tags and goes through EvalMod.
    pub fn has_tags(&self) -> bool {
        !self.tags.is_empty()
    }

    /// The identity of ReplaceModInternal/ConvertModInternal (ModDB.lua:52, ModList.lua:39): type/flags/keywordFlags/source.
    pub(crate) fn same_identity_but_name(&self, o: &Mod) -> bool {
        self.ty == o.ty && self.flags == o.flags && self.keyword_flags == o.keyword_flags && self.source == o.source
    }
}

/// ModTools.lua:33-67 `modLib.createMod`, with the explicit API of MOD-CORE §1: upstream decodes source/flags/keywordFlags
/// out of the vararg by type (tags start at slot 4 whenever slots 2-3 are numbers); the call sites that port a Lua vararg do
/// that decoding themselves.
pub fn create_mod(
    name: &str,
    ty: Option<ModType>,
    value: Value,
    source: Option<&str>,
    flags: u32,
    keyword_flags: u32,
    tags: Vec<Tag>,
) -> Mod {
    Mod {
        name: Rc::from(name),
        ty,
        value,
        flags,
        keyword_flags,
        source: source.map(Rc::from),
        tags,
        replaced: Cell::new(false),
        converted: Cell::new(false),
    }
}

/// The item EvalMod's `ItemCondition` talks to (§8). In R2 the only implementation is the corpus harness stub (the same
/// one as pob-ts `harness.ts buildItem`); the real `Item` class is R3.
pub trait ItemLike {
    fn name(&self) -> Option<&str>;
    fn item_type(&self) -> Option<&str>;
    fn rarity(&self) -> Option<&str>;
    fn corrupted(&self) -> Option<bool>;
    fn shaper(&self) -> Option<bool>;
    fn elder(&self) -> Option<bool>;
    /// `item:FindModifierSubstring(substring, itemSlotName)` (Item.lua:289).
    fn find_modifier_substring(&self, substring: &str, slot_name: &str) -> bool;
}

/// One `actor.activeSkillList` entry as ModStore.lua:322-327 reads it.
#[derive(Clone, Debug, Default)]
pub struct ActiveSkill {
    /// `activeSkill.skillTypes` -- a Lua set keyed by SkillType numbers.
    pub skill_types: Vec<f64>,
    /// `activeSkill.skillFlags.disable`.
    pub disable: Option<bool>,
    /// `activeSkill.buffList` -- only the buff names are read.
    pub buff_list: Option<Vec<Str>>,
    pub skill_data: IndexMap<Str, f64>,
}

/// `cfg.skillGem` as `calcLib.gemIsType` reads it.
#[derive(Clone, Debug, Default)]
pub struct Gem {
    pub name: Str,
    pub tags: IndexMap<Str, bool>,
}

/// `cfg.skillGrantedEffect`: `id` (SkillId tag) and `baseFlags` (BaseFlag tag).
#[derive(Clone, Debug, Default)]
pub struct GrantedEffect {
    pub id: Str,
    pub base_flags: Option<IndexMap<Str, bool>>,
}

/// ModCfg (§8): every field is optional, exactly like the Lua table.
#[derive(Clone, Default)]
pub struct ModCfg {
    pub flags: Option<f64>,
    pub keyword_flags: Option<f64>,
    pub source: Option<Str>,
    pub skill_name: Option<Str>,
    pub summon_skill_name: Option<Str>,
    pub skill_dist: Option<f64>,
    pub skill_cond: Option<IndexMap<Str, bool>>,
    pub actor: Option<Str>,
    pub item: Option<Rc<dyn ItemLike>>,
    pub slot_name: Option<Str>,
    pub skill_gem: Option<Gem>,
    pub socket_color: Option<Str>,
    pub socket_num: Option<f64>,
    pub strength_gems: Option<f64>,
    pub dexterity_gems: Option<f64>,
    pub intelligence_gems: Option<f64>,
    pub skill_granted_effect: Option<GrantedEffect>,
    pub base_flags: Option<IndexMap<Str, bool>>,
    pub skill_part: Option<f64>,
    /// `cfg.skillTypes` -- a Lua set keyed by SkillType numbers.
    pub skill_types: Option<Vec<f64>>,
    pub skill_stats: Option<IndexMap<Str, f64>>,
}

pub type ActorRef = Rc<RefCell<Actor>>;

/// `store.actor` (ModStore.lua:42 `self.actor = parent and parent.actor or { }`), held WEAKLY (R3-ARCH §2.2): the actor graph
/// of a calc (`env.player.modDB` -> store -> `store.actor` -> player) is a cycle in Lua; in Rust the owner of the actors
/// (`Env`) holds them strongly, `actor.mod_db` holds the store strongly and the store points back through this `Weak`, so
/// dropping the owner frees everything.
///
/// The `{ }` a parentless store makes for itself has no other owner, so the store keeps that one alive (`own`); a child
/// store shares its parent's link (the same table, ModStore.lua:42). `ModStore::set_actor` (Lua `modDB.actor = actor`)
/// switches the link to an actor owned elsewhere and drops `own`. An actor reached through `own` must not point back at
/// the store (`actor.mod_db`): that would rebuild the cycle -- such actors belong to their owner, linked via `set_actor`.
#[derive(Clone)]
pub struct ActorLink {
    /// Only keeps the parentless store's own `{ }` alive; never read.
    #[allow(dead_code)]
    own: Option<ActorRef>,
    weak: Weak<RefCell<Actor>>,
}

impl ActorLink {
    /// The fresh `{ }` of a parentless store.
    pub fn fresh() -> ActorLink {
        let own = Rc::new(RefCell::new(Actor::default()));
        ActorLink {
            weak: Rc::downgrade(&own),
            own: Some(own),
        }
    }

    /// A link to an actor owned elsewhere (`modDB.actor = actor`).
    pub fn to(actor: &ActorRef) -> ActorLink {
        ActorLink {
            own: None,
            weak: Rc::downgrade(actor),
        }
    }

    /// The actor table. Panics when its owner is gone: a store queried after its `Env` was dropped is a bug of the caller
    /// (Lua cannot get there -- the table lives as long as anything references it).
    pub fn get(&self) -> ActorRef {
        self.weak
            .upgrade()
            .expect("ModStore.actor: the actor was dropped while its store is still in use (R3-ARCH §2.2)")
    }
}

/// `store.actor` (§8): what EvalMod/GetStat read. `actors` is `actor[name]` for any other key (`parent`, `enemy`,
/// `player`, ...), which `getActor` (ModStore.lua:48) indexes; each such actor carries its own `modDB`.
#[derive(Default)]
pub struct Actor {
    pub mod_db: Option<StoreRef>,
    pub output: Option<IndexMap<Str, f64>>,
    pub active_skill_list: Option<Vec<ActiveSkill>>,
    pub weapon_data1: Option<Rc<Table>>,
    pub weapon_data2: Option<Rc<Table>>,
    pub item_list: Option<IndexMap<Str, Rc<dyn ItemLike>>>,
    /// `actor.minionData.monsterTags`; `Some(None)` = minionData without monsterTags.
    pub minion_data: Option<Option<Vec<Str>>>,
    /// ModStore.lua:352 reads `self.actor["ManaEfficiency"]` off the actor itself.
    pub mana_efficiency: Option<f64>,
    pub actors: IndexMap<Str, ActorRef>,
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::Actor;
    use crate::store::ModStore;

    /// R3-ARCH §2.2: `actor.mod_db` (strong) -> store -> `store.actor` (weak) is no cycle -- dropping the owner frees both.
    #[test]
    fn actor_graph_frees_on_drop() {
        let db = ModStore::new_db(None);
        let player = Rc::new(RefCell::new(Actor {
            mod_db: Some(db.clone()),
            ..Actor::default()
        }));
        db.borrow_mut().set_actor(&player);
        let child = ModStore::new_list(Some(db.clone()));
        assert!(
            Rc::ptr_eq(&child.borrow().actor(), &player),
            "a child shares its parent's actor table"
        );
        let (weak_player, weak_db) = (Rc::downgrade(&player), Rc::downgrade(&db));
        drop((db, child));
        assert!(weak_db.upgrade().is_some(), "the actor still owns its modDB");
        drop(player);
        assert!(
            weak_player.upgrade().is_none() && weak_db.upgrade().is_none(),
            "no cycle left behind"
        );
    }

    /// CalcPerform.lua:2382 writes `warcryPowerBonus` into the shared tag object: every clone (`copyTable(mod, true)`, `AddList`)
    /// sees it -- `get`, `formatTag` -- while a `copyTable` taken before the write does not, and one taken after carries the value.
    #[test]
    fn warcry_power_bonus_is_written_into_the_shared_tag() {
        use crate::matching::format_tag;
        use crate::value::{Str, Table, Value};
        let t = super::Tag::new(Table {
            hash: [
                (Str::from("type"), Value::str("GlobalEffect")),
                (Str::from("effectType"), Value::str("Warcry")),
                (Str::from("div"), Value::Num(5.0)),
            ]
            .into_iter()
            .collect(),
            ..Table::default()
        });
        let shared = t.clone();
        let before = t.deep_copy();
        t.set_warcry_power_bonus(Value::Num(4.0));
        assert_eq!(shared.get("warcryPowerBonus").as_num(), Some(4.0));
        assert_eq!(shared.get("div").as_num(), Some(5.0));
        assert_eq!(format_tag(&shared), "type=GlobalEffect/div=5/effectType=Warcry/warcryPowerBonus=4");
        assert!(
            before.get("warcryPowerBonus").is_nil(),
            "a copyTable taken before the write has its own table"
        );
        let after = t.deep_copy();
        t.set_warcry_power_bonus(Value::Num(6.0));
        assert_eq!(
            after.get("warcryPowerBonus").as_num(),
            Some(4.0),
            "a copyTable keeps the value it copied"
        );
        assert_eq!(shared.get("warcryPowerBonus").as_num(), Some(6.0));
    }

    /// A parentless store keeps its own `{ }` (ModStore.lua:42), and its children see the same table.
    #[test]
    fn fresh_actor_is_owned_and_shared() {
        let db = ModStore::new_db(None);
        let child = ModStore::new_db(Some(db.clone()));
        db.borrow().actor().borrow_mut().mana_efficiency = Some(5.0);
        assert_eq!(child.borrow().actor().borrow().mana_efficiency, Some(5.0));
    }
}
