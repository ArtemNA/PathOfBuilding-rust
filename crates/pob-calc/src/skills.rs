//! The skill half of `calcs.initEnv` (R6.3): CalcSetup.lua `socketedGems` (:1308-1356), the merge of item/tree-granted skills into
//! `build.skillsTab.socketGroupList` (:1448-1563), the main socket group (:1584-1592), supports into their buckets (:1593-1732, `applyGemMods`
//! :382, `applySocketMods` :419, `addBestSupport` :428), the active skills (:1733-1861, `createActiveSkill` in [`crate::active_skill`]),
//! the per-group lists and the main skill (:1862-1940) and `requirementsTableGems` (:1846-1854). `buildActiveSkillModList` (:1941-1945, R6.4)
//! is [`crate::skill_modlist`].
//!
//! State Lua keeps on shared tables lives in [`SkillsState`]: the skills tab (`build.skillsTab` -- the merge appends to it), the data writes
//! CalcSetup/ProcessSocketGroup make (`grantedEffect.fromItem = true`, `levels[l].cost = {}`), the active skill list and the main skill.
//!
//! Hash-order `pairs` (MINEFIELD-RUST R13/D13), all on tables whose order cannot be read off LuaJIT:
//!  * `pairs(socketGroupList)` (:1469, :1510) -- a sequence: in order.
//!  * `pairs(supportLists[slotName])` (:1806, :1825) -- keyed by group TABLES (address hash): Rust walks the groups in list order and
//!    counts every walk with more than one non-empty bucket in [`SkillsState::order_notes`] (only then can the order change the result).
//!  * `pairs(env.crossLinkedSupportGroups)` (:1813) -- string keys: data order (none of the goldens has `LinkedSupport`).

use std::collections::BTreeSet;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_build::{BuildModel, active_item_set};
use pob_data::gems::{GemEntry, gems};
use pob_data::skills::{GrantedEffect, Skills, skills};
use pob_items::Item;
use pob_lua::number::{max2, min2};
use pob_mods::{Gem, ModCfg, ModType, StoreRef, TabEntry, Value};

use crate::active_skill::{ActiveSkill, ActorView, EffectRef, FromItem, GemCfg, GroupCfg, SkillEffect, create_active_skill};
use crate::calc_tools::{GemLevelTarget, LuaError, WeaponView, validate_gem_level};
use crate::env::{GrantedSkill, ReqEntry, Slot};
use crate::setup::ok;
use crate::skills_tab::{
    ByRef, ExplodeSource, GemInstance, SkillsTab, SocketGroup, load_skills_tab, process_socket_group_ext, update_socket_groups,
};

/// What CalcSetup reads of a granted skill besides [`GrantedSkill`]: `nameSpec` (:829-830) and the fields `copyTable(skill)` carries
/// over from `item.grantedSkills` (`noSupports`, `triggered`, `triggerChance`). All nil for a tree node's skill.
#[derive(Clone, Debug, Default)]
pub struct GrantedSkillExtra {
    pub name_spec: Option<String>,
    pub no_supports: Value,
    pub triggered: Value,
    pub trigger_chance: Value,
}

/// `build.skillsTab` + the skill state of the env.
pub struct SkillsState {
    pub skills: Rc<Skills>,
    pub tab: SkillsTab,
    /// `env.player.activeSkillList`.
    pub active_skill_list: Vec<ActiveSkill>,
    /// `env.player.mainSkill` -- index into `active_skill_list`.
    pub main_skill: Option<usize>,
    /// `build.mainSocketGroup` after :1590.
    pub build_main_socket_group: f64,
    /// Effect ids CalcSetup.lua:1633-1634 set `grantedEffect.fromItem = true` on.
    pub from_item_marks: BTreeSet<String>,
    /// ProcessSocketGroup :1186-1190 `grantedEffect.levels[level].cost = {}` as `(effect id, level)`.
    pub cost_wiped: Vec<(String, f64)>,
    /// Hash-order walks whose order could change the result (see the module doc); empty on the goldens.
    pub order_notes: Vec<String>,
    /// `env.auxSkillList` (CalcActiveSkill.lua:871-874) -- indices into `active_skill_list` ([`DETACHED`] for a detached main skill).
    pub aux_skill_list: Vec<usize>,
    /// R7.9: `env.player.mainSkill` when it is a skill OUTSIDE `activeSkillList` -- the copy `calcs.copyActiveSkill` makes
    /// (CalcActiveSkill.lua:167-192) for a mirage (CalcMirages.lua:40/:51, :155/:232). Reached only through [`SkillsState::main_skill`] =
    /// `Some(`[`DETACHED`]`)` ([`SkillsState::skill`]); every walk over `activeSkillList` keeps not seeing it, as upstream.
    pub detached_main: Option<DetachedSkill>,
}

/// The index [`SkillsState::main_skill`] holds when `env.player.mainSkill` is [`SkillsState::detached_main`] (never a list index).
pub const DETACHED: usize = usize::MAX;

/// A skill outside `env.player.activeSkillList` (R7.9, `calcs.copyActiveSkill`): the skill, its own perform-time `skillModList` (upstream
/// `new("ModList"):ModList(newSkill.baseSkillModList)` at CalcActiveSkill.lua:184 -- `calcs.perform` does not rebuild it, :1295-1296 walks the
/// list only), and its `actor` (`skill.actor` of the copied skill -- another env's player: the skill list's parent is that actor's modDB).
#[derive(Clone)]
pub struct DetachedSkill {
    pub skill: ActiveSkill,
    /// `newSkill.skillModList` (a child of `skill.skill_mod_list`, the `baseSkillModList`).
    pub skill_mod_list: StoreRef,
    /// `newSkill.actor`.
    pub actor: Rc<std::cell::RefCell<crate::env::CalcActor>>,
    /// The cached run whose env owns `actor` (kept alive as upstream's GC keeps the env a skill's actor belongs to); `None` when the actor is
    /// the player of the env that made the copy.
    pub keep: Option<crate::session::RunRef>,
}

impl SkillsState {
    /// `activeSkillList[i + 1]`, or the detached main skill for [`DETACHED`].
    pub fn skill(&self, i: usize) -> &ActiveSkill {
        if i == DETACHED {
            &self
                .detached_main
                .as_ref()
                .unwrap_or_else(|| panic!("env.player.mainSkill: detached main skill missing"))
                .skill
        } else {
            &self.active_skill_list[i]
        }
    }

    pub fn skill_mut(&mut self, i: usize) -> &mut ActiveSkill {
        if i == DETACHED {
            &mut self
                .detached_main
                .as_mut()
                .unwrap_or_else(|| panic!("env.player.mainSkill: detached main skill missing"))
                .skill
        } else {
            &mut self.active_skill_list[i]
        }
    }
}

impl FromItem for BTreeSet<String> {
    fn is_from_item(&self, ge: &GrantedEffect) -> bool {
        ge.from_item || self.contains(&ge.id)
    }
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// `gsub(" Swap", "")`.
fn no_swap(s: &str) -> String {
    s.replace(" Swap", "")
}

/// `data.gems[id]`.
fn gem(id: &str) -> &'static GemEntry {
    &gems().gems[id]
}

/// `cfg.skillGem` for a gem.
pub(crate) fn skill_gem_cfg(g: &GemEntry) -> Gem {
    Gem {
        name: Rc::from(g.name.as_str()),
        tags: g.tags.iter().map(|(k, v)| (Rc::from(k.as_str()), *v)).collect(),
    }
}

/// `group.slot and group.slot:gsub(" Swap", "") or (isTreeSkill and "Passive Tree") or nil` (:1609-1610).
fn group_slot_name(sg: &SocketGroup) -> Option<String> {
    let is_tree_skill = sg.slot.is_none() && sg.source.as_deref().is_some_and(|s| lower(s).contains("tree"));
    match &sg.slot {
        Some(s) => Some(no_swap(s)),
        None if is_tree_skill => Some("Passive Tree".into()),
        None => None,
    }
}

impl SkillsState {
    /// `SkillsTab:Load` + `UpdateSocketGroups` (Build.lua:691) -- the tab as the first `BuildOutput` finds it.
    pub fn load(build: &BuildModel, slots: &[Slot], items: &IndexMap<u64, Rc<Item>>) -> Result<SkillsState, LuaError> {
        Ok(SkillsState::from_tab(build, SkillsState::load_tab(build, slots, items)?))
    }

    /// The tab half of [`SkillsState::load`]: a function of the model, the ordered slots and the items only (R7-perf: a
    /// [`crate::init_env::BuildShared`] loads it once per build).
    pub fn load_tab(build: &BuildModel, slots: &[Slot], items: &IndexMap<u64, Rc<Item>>) -> Result<SkillsTab, LuaError> {
        let sk = skills();
        let mut tab = load_skills_tab(&build.skills, &sk)?;
        update_socket_groups(&mut tab, slots, items, &sk);
        Ok(tab)
    }

    /// The state of a fresh initEnv over a loaded tab (no active skills yet, no data writes).
    pub fn from_tab(build: &BuildModel, tab: SkillsTab) -> SkillsState {
        SkillsState {
            skills: skills(),
            tab,
            active_skill_list: Vec::new(),
            main_skill: None,
            build_main_socket_group: build.main_socket_group,
            from_item_marks: BTreeSet::new(),
            cost_wiped: Vec::new(),
            order_notes: Vec::new(),
            aux_skill_list: Vec::new(),
            detached_main: None,
        }
    }

    /// :1312-1320 `socketedGems` of one slot: the enabled gems with `gemData` of the slot's own (not granted) enabled groups.
    pub fn socketed_gems(&self, slot_name: &str) -> Vec<&'static GemEntry> {
        let mut out = Vec::new();
        for sg in self.tab.socket_group_list() {
            if sg.source.is_none() && sg.enabled && sg.slot.as_deref() == Some(slot_name) {
                for gi in &sg.gem_list {
                    if let Some(g) = &gi.gem_data
                        && gi.enabled
                    {
                        out.push(gem(g));
                    }
                }
            }
        }
        out
    }

    /// :1448-1563 -- process extra skills granted by items or tree nodes, the explode group, drop the stale granted groups.
    pub fn merge_granted(
        &mut self,
        granted: &[GrantedSkill],
        extra: &[GrantedSkillExtra],
        explode_sources: &[ExplodeSource],
    ) -> Result<(), LuaError> {
        let sk = self.skills.clone();
        let list = self.tab.socket_group_list_mut();
        let mut marked: Vec<bool> = vec![false; list.len()];
        for (gs, ex) in granted.iter().zip(extra) {
            let skill_id = gs.skill_id.as_str().map(str::to_owned);
            let level = gs.level.as_num();
            // :1453-1461 getNormalizedSkillLevel
            let normalized = || -> Result<f64, LuaError> {
                let ge = skill_id
                    .as_deref()
                    .and_then(|id| sk.get(id))
                    .ok_or_else(|| "Modules/CalcTools.lua:43: attempt to index field 'grantedEffect' (a nil value)".to_owned())?;
                validate_gem_level(
                    &GemLevelTarget {
                        granted_effect: ge,
                        natural_max_level: None,
                    },
                    level,
                )
            };
            // :1468-1478 `pairs(build.skillsTab.socketGroupList)` -- a sequence, in order
            let mut found = None;
            for (idx, sg) in list.iter().enumerate() {
                if sg.source.as_deref() == Some(gs.source.as_str())
                    && sg.slot == gs.slot_name
                    && let Some(g1) = sg.gem_list.first()
                    && g1.skill_id == skill_id
                    && (g1.level == level || g1.level == Some(normalized()?))
                {
                    found = Some(idx);
                    break;
                }
            }
            let idx = match found {
                Some(i) => i,
                None => {
                    // :1480-1483 create a new group for this skill
                    list.push(SocketGroup {
                        label: Some(String::new()),
                        enabled: true,
                        source: Some(gs.source.clone()),
                        slot: gs.slot_name.clone(),
                        ..SocketGroup::new_empty()
                    });
                    marked.push(false);
                    list.len() - 1
                }
            };
            marked[idx] = true;
            let group = &mut list[idx];
            // :1486-1503 update the group
            group.source_item = gs.source_item.clone().map(ByRef);
            group.source_node = gs.source_node;
            let mut gi = match group.gem_list.first() {
                Some(g) => g.clone(),
                None => GemInstance {
                    skill_id: skill_id.clone(),
                    name_spec: ex.name_spec.clone(),
                    quality: Some(0.0),
                    enabled: true,
                    ..GemInstance::default()
                },
            };
            gi.from_item = Some(gs.source_item.is_some());
            gi.gem_id = None;
            gi.level = level;
            gi.enable_global1 = Some(true);
            gi.no_supports = ex.no_supports.truthy().then_some(true).or(match ex.no_supports {
                Value::Bool(false) => Some(false),
                _ => None,
            });
            group.no_supports = gi.no_supports;
            gi.triggered = ex.triggered.truthy();
            gi.trigger_chance = ex.trigger_chance.as_num();
            group.gem_list = vec![gi];
            process_socket_group_ext(group, &sk, &mut self.cost_wiped)?;
        }

        if !explode_sources.is_empty() {
            // :1509-1520
            let idx = match list.iter().position(|sg| sg.source.as_deref() == Some("Explode")) {
                Some(i) => i,
                None => {
                    list.push(SocketGroup {
                        label: Some("On Kill Monster Explosion".into()),
                        enabled: true,
                        source: Some("Explode".into()),
                        no_supports: Some(true),
                        ..SocketGroup::new_empty()
                    });
                    marked.push(false);
                    list.len() - 1
                }
            };
            let group = &mut list[idx];
            group.explode_sources = Some(explode_sources.to_vec());
            // :1523-1528 `gemsBySource[explodeSource.modSource or explodeSource.id] = gem`
            let mut by_source: IndexMap<String, GemInstance> = IndexMap::new();
            for g in &group.gem_list {
                if let Some(src) = &g.explode_source {
                    by_source.insert(src.key(), g.clone());
                }
            }
            group.gem_list = explode_sources
                .iter()
                .map(|src| {
                    by_source.get(&src.key()).cloned().unwrap_or_else(|| GemInstance {
                        skill_id: Some("EnemyExplode".into()),
                        quality: Some(0.0),
                        enabled: true,
                        level: Some(1.0),
                        triggered: true,
                        explode_source: Some(src.clone()),
                        ..GemInstance::default()
                    })
                })
                .collect();
            marked[idx] = true;
            process_socket_group_ext(group, &sk, &mut self.cost_wiped)?;
        }

        // :1550-1561 remove any socket groups that no longer have a matching item
        let mut i = 0;
        list.retain(|sg| {
            let keep = sg.source.is_none() || marked[i];
            i += 1;
            keep
        });
        Ok(())
    }
}

/// What the skill phase reads of the env (borrowed apart so the state can be mutated alongside).
pub struct SetupInputs<'a> {
    pub build: &'a BuildModel,
    pub mod_db: &'a StoreRef,
    pub ordered_slots: &'a [Slot],
    /// `env.player.itemList`.
    pub item_list: &'a IndexMap<String, Rc<Item>>,
    pub weapon1: WeaponView<'a>,
    pub weapon2: WeaponView<'a>,
    pub main_socket_group: f64,
    /// `env.modDB.conditions["AffectedByEnergyBlade"]` (:1918).
    pub affected_by_energy_blade: bool,
}

/// What the skill phase hands back to the env.
#[derive(Default)]
pub struct SetupOutputs {
    pub requirements_table_gems: Vec<ReqEntry>,
    pub unimplemented: Vec<String>,
}

/// Where `addBestSupport` writes: `supportLists[slotName][group]` or `supportLists[group]` (:1618-1625).
#[derive(Clone, Debug)]
enum Target {
    Slot(String, usize),
    Group(usize),
}

#[derive(Default)]
struct SupportLists {
    by_slot: IndexMap<String, IndexMap<usize, Vec<EffectRef>>>,
    by_group: IndexMap<usize, Vec<EffectRef>>,
}

impl SupportLists {
    fn get_mut(&mut self, t: &Target) -> &mut Vec<EffectRef> {
        match t {
            Target::Slot(s, g) => self.by_slot.entry(s.clone()).or_default().entry(*g).or_default(),
            Target::Group(g) => self.by_group.entry(*g).or_default(),
        }
    }
}

/// CalcSetup.lua:428-451 `addBestSupport(supportEffect, appliedSupportList, "MAIN")`.
fn add_best_support(sk: &Skills, se: &EffectRef, list: &mut Vec<EffectRef>) -> Result<(), LuaError> {
    let mut add = true;
    let (ge_idx, level, quality) = {
        let e = se.borrow();
        (e.granted_effect, e.level, e.quality)
    };
    let ge = &sk.effects[ge_idx];
    let cmp_err = || "Modules/CalcSetup.lua:434: attempt to compare nil with number".to_owned();
    for slot in list.iter_mut() {
        let other = slot.clone();
        let (o_idx, o_level, o_quality) = {
            let e = other.borrow();
            (e.granted_effect, e.level, e.quality)
        };
        let oge = &sk.effects[o_idx];
        if ge_idx == o_idx {
            add = false;
            let (l, ol) = (level.ok_or_else(cmp_err)?, o_level.ok_or_else(cmp_err)?);
            let better = l > ol || (l == ol && quality.ok_or_else(cmp_err)? > o_quality.ok_or_else(cmp_err)?);
            if better {
                other.borrow_mut().superseded = Some(true);
                *slot = se.clone();
            } else {
                se.borrow_mut().superseded = Some(true);
            }
            break;
        } else if ge.plus_version_of.as_deref() == Some(oge.id.as_str()) {
            add = false;
            other.borrow_mut().superseded = Some(true);
            *slot = se.clone();
        } else if oge.plus_version_of.as_deref() == Some(ge.id.as_str()) {
            add = false;
            se.borrow_mut().superseded = Some(true);
        }
    }
    if add {
        list.push(se.clone());
    }
    Ok(())
}

/// CalcSetup.lua:382-417 `applyGemMods(effect, modList)`.
fn apply_gem_mods(effect: &EffectRef, gem_data: &GemEntry, mod_list: &[TabEntry]) {
    let g = skill_gem_cfg(gem_data);
    let is_type = |kw: &Value| match kw.as_str() {
        Some(k) => pob_mods::calc_lib::gem_is_type(&g, k, true),
        // `type == nil`: only `gem.tags[nil]` is reached -- nil
        None => false,
    };
    for e in mod_list {
        let value = &e.value;
        let kw_list = value.get("keywordList");
        let matched = if kw_list.truthy() {
            kw_list.ipairs().iter().all(&is_type)
        } else {
            is_type(&value.get("keyword"))
        };
        if !matched {
            continue;
        }
        let key = value.get("key");
        let v = value
            .get("value")
            .as_num()
            .unwrap_or_else(|| panic!("Modules/CalcSetup.lua:411: attempt to perform arithmetic on field 'value'"));
        let mut eff = effect.borrow_mut();
        if key.is_str("quality") {
            let socketed = e.mod_.tags.iter().any(|t| t.get("type").is_str("SocketedIn"));
            if socketed {
                eff.item_quality = Some(eff.item_quality.unwrap_or(0.0) + v);
            } else {
                eff.global_quality = Some(eff.global_quality.unwrap_or(0.0) + v);
            }
        }
        let k = key
            .as_str()
            .unwrap_or_else(|| panic!("Modules/CalcSetup.lua:411: GemProperty key is not a string"));
        let slot = match k {
            "level" => &mut eff.level,
            "quality" => &mut eff.quality,
            "req" => &mut eff.req,
            "globalQuality" => &mut eff.global_quality,
            "itemQuality" => &mut eff.item_quality,
            "supportQuality" => &mut eff.support_quality,
            "socketQuality" => &mut eff.socket_quality,
            other => {
                let x = eff.other.entry(other.to_owned()).or_insert(0.0);
                *x += v;
                eff.gem_property_info.get_or_insert_with(Vec::new).push(e.clone());
                continue;
            }
        };
        *slot = Some(slot.unwrap_or(0.0) + v);
        eff.gem_property_info.get_or_insert_with(Vec::new).push(e.clone());
    }
}

fn add_mod_with_source(mod_db: &StoreRef, value: &Value, source: &str, line: &str) {
    let Value::Mod(m) = value else {
        panic!("Modules/CalcSetup.lua:{line}: modLib.setSource on a non-mod value {value:?}")
    };
    // `modLib.setSource(value.value, ...)` writes the source onto the shared mod table; the Rust mod is copied first.
    let mut m = (**m).clone();
    pob_mods::format::set_source(&mut m, source);
    mod_db.borrow_mut().add_mod(Rc::new(m));
}

/// CalcSetup.lua:419-426 `applySocketMods(env, gem, groupCfg, socketNum, modSource)`.
fn apply_socket_mods(mod_db: &StoreRef, gem_data: &GemEntry, group_cfg: &GroupCfg, socket_num: f64, mod_source: Option<&str>) {
    let cfg = ModCfg {
        skill_gem: Some(skill_gem_cfg(gem_data)),
        socket_num: Some(socket_num),
        ..group_cfg.mod_cfg()
    };
    let values = ok(mod_db.borrow().list(Some(&cfg), &["SocketProperty"]));
    let src = mod_source.or(group_cfg.slot_name.as_deref()).unwrap_or("");
    for v in values {
        add_mod_with_source(mod_db, &v.get("value"), src, "422");
    }
}

/// CalcSetup.lua:1584-1940: the main socket group, supports, active skills, the per-group lists and the main skill.
#[allow(clippy::too_many_lines)]
pub fn setup_skills(st: &mut SkillsState, inp: &SetupInputs<'_>) -> Result<SetupOutputs, LuaError> {
    let sk = st.skills.clone();
    let mut out = SetupOutputs::default();
    let mod_db = inp.mod_db;
    let main_group = inp.main_socket_group;
    let is_main = |index: usize| (index + 1) as f64 == main_group;

    // :1594-1598 cross-linked supports. `mod.mod.sourceSlot` is written onto the item's mod table (Item.lua:2242) and is not carried by
    // `pob_mods::Mod` -- recorded, never guessed (no golden has LinkedSupport).
    let linked = ok(mod_db.borrow().tabulate(Some(ModType::List), None, &["LinkedSupport"]));
    let cross_linked: IndexMap<String, Vec<String>> = IndexMap::new();
    if !linked.is_empty() {
        out.unimplemented.push(format!(
            "CalcSetup.lua:1595 {} LinkedSupport mods (mod.sourceSlot is not carried by pob_mods::Mod)",
            linked.len()
        ));
    }

    let use_second = active_item_set(inp.build).use_second_weapon_set;
    let want_set = if use_second { 2.0 } else { 1.0 };
    let n_groups = st.tab.socket_group_list().len();
    let mut group_cfg: Vec<Option<GroupCfg>> = vec![None; n_groups];
    let mut support_lists = SupportLists::default();
    let mut processed_sockets: BTreeSet<(usize, usize)> = BTreeSet::new();
    let imbued_by_slot = st.tab.imbued_support_by_slot.clone();

    let property_mod_list = |slot_name: &Option<String>| -> Rc<Vec<TabEntry>> {
        let cfg = ModCfg {
            slot_name: slot_name.as_deref().map(Rc::from),
            ..ModCfg::default()
        };
        Rc::new(ok(mod_db.borrow().tabulate(Some(ModType::List), Some(&cfg), &["GemProperty"])))
    };
    let new_group_cfg = |slot_name: Option<String>| GroupCfg {
        property_mod_list: property_mod_list(&slot_name),
        slot_name,
        ..GroupCfg::default()
    };
    let item_in = |slot_name: &Option<String>| slot_name.as_ref().and_then(|s| inp.item_list.get(s));
    // `socketedIn` (:1696, :1773)
    let socketed_in = |slot_name: &Option<String>, gem_index: usize| -> Option<String> {
        item_in(slot_name).and_then(|it| it.sockets.get(gem_index).map(|s| s.color.clone()))
    };
    let new_gem_cfg = |cfg: &GroupCfg, socket_color: Option<String>, gem_index: usize| GemCfg {
        group: cfg.clone(),
        socket_color,
        socket_num: (gem_index + 1) as f64,
    };

    // --- :1603-1732 process support gems adding them to applicable support lists ------------------------------------------
    for index in 0..n_groups {
        let list = st.tab.socket_group_list_mut();
        let slot_enabled = {
            let sg = &list[index];
            let slot = sg.slot.as_ref().and_then(|s| inp.ordered_slots.iter().find(|x| &x.slot_name == s));
            slot.is_none_or(|s| s.weapon_set.is_none_or(|w| w == want_set))
        };
        list[index].slot_enabled = Some(slot_enabled);
        if !(is_main(index) || (list[index].enabled && slot_enabled)) {
            continue;
        }
        let slot_name = group_slot_name(&list[index]);
        if group_cfg[index].is_none() {
            group_cfg[index] = Some(new_group_cfg(slot_name.clone()));
        }
        let target = match &group_cfg[index].as_ref().expect("set").slot_name {
            Some(s) => Target::Slot(s.clone(), index),
            None => Target::Group(index),
        };
        support_lists.get_mut(&target);
        let targets = [target];

        // :1627-1650 addExtraSupports(value, grantedEffect, level)
        let add_extra_supports = |st: &mut SkillsState,
                                  sl: &mut SupportLists,
                                  value: Option<&Value>,
                                  ge_idx: Option<usize>,
                                  level: Option<f64>|
         -> Result<(), LuaError> {
            let mut ge_idx = ge_idx.or_else(|| value.and_then(|v| v.get("skillId").as_str().and_then(|id| sk.index.get(id).copied())));
            if let (Some(v), Some(g)) = (value, ge_idx)
                && !sk.effects[g].support
            {
                let id = match v.get("skillId") {
                    Value::Str(s) => s.to_string(),
                    Value::Num(x) => pob_lua::format::tostring_num(x),
                    other => {
                        return Err(format!(
                            "Modules/CalcSetup.lua:1632: attempt to concatenate a {} value",
                            other.type_name()
                        ));
                    }
                };
                ge_idx = sk.index.get(&format!("Support{id}")).copied();
            }
            if let (Some(_), Some(g)) = (value, ge_idx) {
                // `grantedEffect.fromItem = true` -- a write onto the shared data table
                st.from_item_marks.insert(sk.effects[g].id.clone());
            }
            let Some(g) = ge_idx else { return Ok(()) };
            let ge = &sk.effects[g];
            let data = gems();
            let gem_data = data
                .gem_for_base_name
                .get(&lower(&ge.name))
                .or_else(|| data.gem_for_base_name.get(&lower(&format!("{} Support", ge.name))))
                .cloned();
            let level = level.or_else(|| value.and_then(|v| v.get("level").as_num()));
            for t in &targets {
                let e = EffectRef::new(SkillEffect {
                    granted_effect: g,
                    gem_data: gem_data.clone(),
                    level,
                    applies_to_granted_skills: value.and_then(|v| match v.get("appliesToGrantedSkills") {
                        Value::Nil => None,
                        x => Some(x.truthy()),
                    }),
                    quality: Some(0.0),
                    enabled: Some(true),
                    ..SkillEffect::default()
                });
                add_best_support(&sk, &e, sl.get_mut(t))?;
            }
            Ok(())
        };

        // :1653-1658 extra supports from the item this group is socketed in
        let cfg = group_cfg[index].as_ref().expect("set").mod_cfg();
        let has_source = st.tab.socket_group_list()[index].source.is_some();
        for value in ok(mod_db.borrow().list(Some(&cfg), &["ExtraSupport"])) {
            if !has_source || value.get("appliesToGrantedSkills").truthy() {
                add_extra_supports(st, &mut support_lists, Some(&value), None, None)?;
            }
        }
        // :1660-1667 the slot's imbued support
        let group_imbued = st.tab.socket_group_list()[index].imbued_support.is_some();
        if let Some(imbued) = slot_name.as_ref().and_then(|s| imbued_by_slot.get(s))
            && group_imbued
        {
            let g = sk.index.get(imbued).copied();
            add_extra_supports(st, &mut support_lists, None, g, Some(1.0))?;
            // `env.data.gems[env.data.gemForSkill[imbuedSupport]]` -- keyed by the effect table (D4: by id here)
            if let Some(gd) = gems().gem_for_skill.get(imbued).map(|id| gem(id))
                && let Some(sec) = gd.secondary_granted_effect_id.as_deref().and_then(|id| sk.index.get(id).copied())
                && sk.effects[sec].support
            {
                add_extra_supports(st, &mut support_lists, None, Some(sec), Some(1.0))?;
            }
        }

        // :1669-1730 the group's gems
        let n_gems = st.tab.socket_group_list()[index].gem_list.len();
        for gem_index in 0..n_gems {
            let gi_snapshot = {
                let gi = &mut st.tab.socket_group_list_mut()[index].gem_list[gem_index];
                gi.display_effect = None;
                gi.support_effect = None;
                gi.clone()
            };
            if !gi_snapshot.enabled {
                continue;
            }
            let effects: Vec<Option<usize>> = match &gi_snapshot.gem_data {
                Some(g) => {
                    let gd = gem(g);
                    vec![
                        sk.index.get(&gd.granted_effect_id).copied(),
                        gd.secondary_granted_effect_id.as_deref().and_then(|id| sk.index.get(id).copied()),
                    ]
                }
                None => vec![gi_snapshot.granted_effect.as_deref().and_then(|id| sk.index.get(id).copied())],
            };
            for ge_idx in effects {
                // :1677-1679 processGrantedEffect
                let Some(ge_idx) = ge_idx else { continue };
                let ge = &sk.effects[ge_idx];
                if !ge.support {
                    continue;
                }
                let gi = &st.tab.socket_group_list()[index].gem_list[gem_index];
                let socket_quality = if gi.matches_socket == Some(true) {
                    pob_data_misc_matching_socket_quality_bonus()
                } else {
                    0.0
                };
                let quality = gi.quality.ok_or_else(|| {
                    "Modules/CalcSetup.lua:1680: attempt to perform arithmetic on field 'quality' (a nil value)".to_owned()
                })? + socket_quality;
                let se = EffectRef::new(SkillEffect {
                    granted_effect: ge_idx,
                    level: gi.level,
                    quality: Some(quality),
                    global_quality: Some(0.0),
                    item_quality: Some(0.0),
                    support_quality: Some(0.0),
                    socket_quality: Some(socket_quality),
                    src_instance: Some((index, gem_index)),
                    gem_data: gi.gem_data.clone(),
                    superseded: Some(false),
                    is_supporting: Some(Vec::new()),
                    ..SkillEffect::default()
                });
                {
                    let gi = &mut st.tab.socket_group_list_mut()[index].gem_list[gem_index];
                    gi.display_effect = Some(se.clone());
                    gi.support_effect = Some(se.clone());
                }
                if let Some(gd_id) = gi_snapshot.gem_data.as_deref() {
                    let gd = gem(gd_id);
                    let gcfg = group_cfg[index].as_ref().expect("set");
                    let socketed = socketed_in(&gcfg.slot_name, gem_index);
                    let gem_cfg = new_gem_cfg(gcfg, socketed.clone(), gem_index);
                    let mods = if socketed.is_some() {
                        Rc::new(ok(mod_db.borrow().tabulate(
                            Some(ModType::List),
                            Some(&gem_cfg.mod_cfg()),
                            &["GemProperty"],
                        )))
                    } else {
                        gcfg.property_mod_list.clone()
                    };
                    se.borrow_mut().gem_cfg = Some(gem_cfg);
                    apply_gem_mods(&se, gd, &mods);
                    st.tab.socket_group_list_mut()[index].gem_list[gem_index].req_override = se.borrow().req;
                    if processed_sockets.insert((index, gem_index)) {
                        let gcfg = group_cfg[index].as_mut().expect("set");
                        let item_name = item_in(&gcfg.slot_name).map(|i| i.name.clone());
                        apply_socket_mods(mod_db, gd, gcfg, (gem_index + 1) as f64, item_name.as_deref());
                        bump_gem_counts(gcfg, gd);
                    }
                }
                // :1714 validate support gem level in case there is no active skill
                {
                    let mut e = se.borrow_mut();
                    let nml = e.gem_data.as_deref().map(|g| gem(g).natural_max_level);
                    e.level = Some(validate_gem_level(
                        &GemLevelTarget {
                            granted_effect: ge,
                            natural_max_level: nml,
                        },
                        e.level,
                    )?);
                }
                for t in &targets {
                    add_best_support(&sk, &se, support_lists.get_mut(t))?;
                }
            }
        }
    }

    // --- :1734-1861 process active skills adding the applicable supports --------------------------------------------------
    let mut socket_group_skill_list: Vec<Option<Vec<usize>>> = vec![None; n_groups];
    let mut requirements = Vec::new();
    let actor = ActorView {
        weapon1: inp.weapon1,
        weapon2: inp.weapon2,
        is_player: true,
    };
    for index in 0..n_groups {
        let (enabled, slot_enabled) = {
            let sg = &st.tab.socket_group_list()[index];
            (sg.enabled, sg.slot_enabled == Some(true))
        };
        if !(is_main(index) || (enabled && slot_enabled)) {
            continue;
        }
        let slot_name = group_slot_name(&st.tab.socket_group_list()[index]);
        if group_cfg[index].is_none() {
            group_cfg[index] = Some(new_group_cfg(slot_name.clone()));
        }
        socket_group_skill_list[index].get_or_insert_with(Vec::new);
        let mut slot_has_active_skill = false;

        let n_gems = st.tab.socket_group_list()[index].gem_list.len();
        for gem_index in 0..n_gems {
            let gi_snapshot = st.tab.socket_group_list()[index].gem_list[gem_index].clone();
            if !(gi_snapshot.enabled && (gi_snapshot.gem_data.is_some() || gi_snapshot.granted_effect.is_some())) {
                continue;
            }
            // `gemData.grantedEffectList` = { grantedEffect, secondaryGrantedEffect } -- `ipairs` stops at a nil secondary
            let effect_list: Vec<usize> = match &gi_snapshot.gem_data {
                Some(g) => {
                    let gd = gem(g);
                    let mut v = vec![*sk.index.get(&gd.granted_effect_id).expect("gem's granted effect")];
                    if let Some(sec) = gd.secondary_granted_effect_id.as_deref().and_then(|id| sk.index.get(id).copied()) {
                        v.push(sec);
                    }
                    v
                }
                None => vec![
                    *sk.index
                        .get(gi_snapshot.granted_effect.as_deref().expect("checked"))
                        .expect("granted effect"),
                ],
            };
            for (k, &ge_idx) in effect_list.iter().enumerate() {
                let ge = &sk.effects[ge_idx];
                let enable_global = match k {
                    0 => gi_snapshot.enable_global1 == Some(true),
                    _ => gi_snapshot.enable_global2 == Some(true),
                };
                if ge.support || (ge.has_global_effect.get() && !enable_global) {
                    continue;
                }
                slot_has_active_skill = true;
                let socket_quality = if gi_snapshot.matches_socket == Some(true) {
                    pob_data_misc_matching_socket_quality_bonus()
                } else {
                    0.0
                };
                let quality = gi_snapshot.quality.ok_or_else(|| {
                    "Modules/CalcSetup.lua:1757: attempt to perform arithmetic on field 'quality' (a nil value)".to_owned()
                })? + socket_quality;
                let ae = EffectRef::new(SkillEffect {
                    granted_effect: ge_idx,
                    level: gi_snapshot.level,
                    quality: Some(quality),
                    global_quality: Some(0.0),
                    item_quality: Some(0.0),
                    support_quality: Some(0.0),
                    socket_quality: Some(socket_quality),
                    src_instance: Some((index, gem_index)),
                    gem_data: gi_snapshot.gem_data.clone(),
                    ..SkillEffect::default()
                });
                if let Some(gd_id) = gi_snapshot.gem_data.as_deref() {
                    let gd = gem(gd_id);
                    let gcfg = group_cfg[index].as_ref().expect("set");
                    let socketed = socketed_in(&gcfg.slot_name, gem_index);
                    let gem_cfg = new_gem_cfg(gcfg, socketed.clone(), gem_index);
                    let mods = if socketed.is_some() {
                        Rc::new(ok(mod_db.borrow().tabulate(
                            Some(ModType::List),
                            Some(&gem_cfg.mod_cfg()),
                            &["GemProperty"],
                        )))
                    } else {
                        gcfg.property_mod_list.clone()
                    };
                    ae.borrow_mut().gem_cfg = Some(gem_cfg);
                    apply_gem_mods(&ae, gd, &mods);
                    st.tab.socket_group_list_mut()[index].gem_list[gem_index].req_override = ae.borrow().req;
                    if processed_sockets.insert((index, gem_index)) {
                        let gcfg = group_cfg[index].as_mut().expect("set");
                        let item_name = item_in(&gcfg.slot_name).map(|i| i.name.clone());
                        apply_socket_mods(mod_db, gd, gcfg, (gem_index + 1) as f64, item_name.as_deref());
                        bump_gem_counts(gcfg, gd);
                    }
                }
                st.tab.socket_group_list_mut()[index].gem_list[gem_index].display_effect = Some(ae.clone());

                let mut applied: Vec<EffectRef> = Vec::new();
                let no_supports = st.tab.socket_group_list()[index].no_supports == Some(true);
                if !no_supports {
                    // :1790 `copyTable(supportLists[group] or supportLists[slotName][group], true)`
                    applied = match support_lists.by_group.get(&index) {
                        Some(l) => l.clone(),
                        None => slot_name
                            .as_ref()
                            .and_then(|s| support_lists.by_slot.get(s))
                            .and_then(|m| m.get(&index))
                            .cloned()
                            .ok_or_else(|| "Modules/CalcSetup.lua:1790: attempt to index a nil value".to_owned())?,
                    };
                    let (own_slot, own_source, n_own) = {
                        let sg = &st.tab.socket_group_list()[index];
                        (sg.slot.clone(), sg.source.clone(), sg.gem_list.len())
                    };
                    let mut display: Vec<(usize, usize)> = (0..n_own).map(|i| (index, i)).collect();
                    if own_source.is_some()
                        && let Some(slot_lists) = slot_name.as_ref().and_then(|s| support_lists.by_slot.get(s))
                    {
                        // :1797-1805 socketed supports from other groups (display only)
                        for (oi, other) in st.tab.socket_group_list().iter().enumerate() {
                            if other.slot.is_some() && other.slot == own_slot && !(other.source.is_some() && other.source == own_source) {
                                for (gj, g) in other.gem_list.iter().enumerate() {
                                    if let Some(gd) = &g.gem_data
                                        && sk.get(&gem(gd).granted_effect_id).is_some_and(|e| e.support)
                                    {
                                        display.push((oi, gj));
                                    }
                                }
                            }
                        }
                        // :1806-1810 `pairs(supportLists[slotName])` -- keyed by group tables (address order in LuaJIT)
                        let non_empty = slot_lists.values().filter(|l| !l.is_empty()).count();
                        if non_empty > 1 {
                            st.order_notes.push(format!(
                                "CalcSetup.lua:1806 pairs(supportLists[\"{}\"]) over {non_empty} non-empty groups (group {})",
                                slot_name.as_deref().unwrap_or(""),
                                index + 1
                            ));
                        }
                        for group_list in slot_lists.values() {
                            for se in group_list {
                                add_best_support(&sk, se, &mut applied)?;
                            }
                        }
                    }
                    // :1813-1832 supports from cross-linked groups (empty: see above)
                    for (cl_slot, cl_targets) in &cross_linked {
                        for t in cl_targets {
                            if Some(t) == slot_name.as_ref()
                                && let Some(slot_lists) = support_lists.by_slot.get(cl_slot)
                            {
                                for group_list in slot_lists.values() {
                                    for se in group_list {
                                        add_best_support(&sk, se, &mut applied)?;
                                    }
                                }
                            }
                        }
                    }
                    st.tab.socket_group_list_mut()[index].display_gem_list = Some(display);
                }
                let src_from_item = gi_snapshot.from_item == Some(true);
                let mut skill = create_active_skill(&sk, ae, applied, &actor, Some(index), src_from_item, &st.from_item_marks)?;
                if gi_snapshot.gem_data.is_some() {
                    skill.slot_name = group_cfg[index].as_ref().expect("set").slot_name.clone();
                }
                let n = st.active_skill_list.len();
                socket_group_skill_list[index].get_or_insert_with(Vec::new).push(n);
                st.active_skill_list.push(skill);
            }
            if gi_snapshot.gem_data.is_some() {
                // :1846-1854
                let gi = &st.tab.socket_group_list()[index].gem_list[gem_index];
                requirements.push(ReqEntry {
                    source: "Gem",
                    source_item: None,
                    source_slot: None,
                    source_gem: Some((index, gem_index)),
                    str_: gi.req_override.or(gi.req_str),
                    dex: gi.req_override.or(gi.req_dex),
                    int: gi.req_override.or(gi.req_int),
                });
            }
        }
        let sg = &mut st.tab.socket_group_list_mut()[index];
        if !slot_has_active_skill && sg.display_gem_list.is_some() {
            sg.display_gem_list = None;
        }
    }

    // --- :1863-1927 process calculated active skill lists ------------------------------------------------------------------
    for index in 0..n_groups {
        let skill_list = socket_group_skill_list[index].get_or_insert_with(Vec::new).clone();
        let (enabled, slot_enabled) = {
            let sg = &st.tab.socket_group_list()[index];
            (sg.enabled, sg.slot_enabled == Some(true))
        };
        if is_main(index) || (enabled && slot_enabled) {
            if group_cfg[index].is_none() {
                group_cfg[index] = Some(new_group_cfg(group_slot_name(&st.tab.socket_group_list()[index])));
            }
            let gcfg = group_cfg[index].as_ref().expect("set");
            for value in ok(mod_db.borrow().list(Some(&gcfg.mod_cfg()), &["GroupProperty"])) {
                add_mod_with_source(mod_db, &value.get("value"), gcfg.slot_name.as_deref().unwrap_or(""), "1874");
            }
            if is_main(index) && !skill_list.is_empty() {
                let sg = &mut st.tab.socket_group_list_mut()[index];
                let active_skill_index = min2(skill_list.len() as f64, sg.main_active_skill.unwrap_or(1.0));
                sg.main_active_skill = Some(active_skill_index);
                // `socketGroupSkillList[activeSkillIndex]` -- a fractional index is nil
                st.main_skill =
                    (active_skill_index.fract() == 0.0 && active_skill_index >= 1.0).then(|| skill_list[active_skill_index as usize - 1]);
            }
        }
        // :1891-1906 display label
        let sg = &st.tab.socket_group_list()[index];
        let label = match &sg.label {
            Some(l) if l.bytes().any(|b| !matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')) => Some(l.clone()),
            _ => {
                let mut label: Option<String> = None;
                for gi in &sg.gem_list {
                    let ge = match &gi.gem_data {
                        Some(g) => sk.get(&gem(g).granted_effect_id),
                        None => gi.granted_effect.as_deref().and_then(|id| sk.get(id)),
                    };
                    if let Some(ge) = ge
                        && !ge.support
                        && gi.enabled
                    {
                        label = Some(match label {
                            Some(l) => format!("{l}, {}", ge.name),
                            None => ge.name.clone(),
                        });
                    }
                }
                Some(label.unwrap_or_else(|| "<No active skills>".into()))
            }
        };
        // :1917-1926 energy blade: `return calcs.initEnv(build, mode, override, specEnv)` with AffectedByEnergyBlade
        if !inp.affected_by_energy_blade && sg.enabled && sg.slot_enabled == Some(true) {
            for gi in &sg.gem_list {
                let ge = match &gi.gem_data {
                    Some(g) => sk.get(&gem(g).granted_effect_id),
                    None => gi.granted_effect.as_deref().and_then(|id| sk.get(id)),
                };
                if let Some(ge) = ge
                    && !ge.support
                    && gi.enabled
                    && ge.name == "Energy Blade"
                {
                    out.unimplemented
                        .push("CalcSetup.lua:1923 Energy Blade: initEnv re-run with override.conditions AffectedByEnergyBlade".into());
                }
            }
        }
        let sg = &mut st.tab.socket_group_list_mut()[index];
        sg.display_label = label;
        sg.display_skill_list = Some(skill_list);
    }

    // :1929-1938 a default main skill if none is specified
    if st.main_skill.is_none() {
        let melee = *sk
            .index
            .get("Melee")
            .ok_or_else(|| "Modules/CalcActiveSkill.lua:96: attempt to index local 'activeGrantedEffect' (a nil value)".to_owned())?;
        let e = EffectRef::new(SkillEffect {
            granted_effect: melee,
            level: Some(1.0),
            quality: Some(0.0),
            enabled: Some(true),
            ..SkillEffect::default()
        });
        let skill = create_active_skill(&sk, e, Vec::new(), &actor, None, false, &st.from_item_marks)?;
        st.main_skill = Some(st.active_skill_list.len());
        st.active_skill_list.push(skill);
    }
    out.requirements_table_gems = requirements;
    Ok(out)
}

/// :1706-1708 `groupCfg.<colour>Gems = (groupCfg.<colour>Gems or 0) + (gemData.tags.<colour> and 1 or 0)`.
fn bump_gem_counts(cfg: &mut GroupCfg, gd: &GemEntry) {
    let has = |t: &str| gd.tags.get(t).copied().unwrap_or(false);
    cfg.intelligence_gems = Some(cfg.intelligence_gems.unwrap_or(0.0) + if has("intelligence") { 1.0 } else { 0.0 });
    cfg.dexterity_gems = Some(cfg.dexterity_gems.unwrap_or(0.0) + if has("dexterity") { 1.0 } else { 0.0 });
    cfg.strength_gems = Some(cfg.strength_gems.unwrap_or(0.0) + if has("strength") { 1.0 } else { 0.0 });
}

/// `data.misc.MatchingSocketQualityBonus` (Data.lua:303).
fn pob_data_misc_matching_socket_quality_bonus() -> f64 {
    crate::setup::misc("MatchingSocketQualityBonus")
}

/// The main socket group (:1587-1590): `build.mainSocketGroup = m_min(m_max(#socketGroupList, 1), build.mainSocketGroup or 1)`.
pub fn main_socket_group(st: &mut SkillsState) -> f64 {
    let n = st.tab.socket_group_list().len() as f64;
    st.build_main_socket_group = min2(max2(n, 1.0), st.build_main_socket_group);
    st.build_main_socket_group
}
