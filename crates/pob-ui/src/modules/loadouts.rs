//! The loadouts of `Modules/Build.lua` (2.69.0, 8596b2001 "Upgrade Loadout Management UI"): `SyncLoadouts` (:684-818), `NewLoadout`
//! (:820-832), `CopyLoadout` (:834-848), `CustomLoadout` (:850-888), `DeleteLoadout` (:890-920), `RenameLoadout` (:922-938),
//! `GetLoadoutByName` (:940-995), `SetActiveLoadout` (:997-1021), `ReorderLoadout` (:1023-1065), the `buildLoadouts` selFunc (:310-331)
//! and `OpenBuildSetManagePopup` (:2808-2820). `loadoutsList` holds the specs as their 1-based index in `specList` at the time of the
//! `SyncLoadouts` that made it (the Lua holds the objects; every place that reads it runs before `specList` changes again).
use super::build::BuildMode;
use super::main::{HelpSection, main};
use crate::lua;
use pob_engine::engine::Engine;
use pob_gfx::Gfx;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// What `GetLoadoutByName` returns (Build.lua:989-994).
#[derive(Clone, Debug, PartialEq)]
pub struct Loadout {
    pub spec_id: Option<usize>,
    pub item_set_id: Option<f64>,
    pub skill_set_id: Option<f64>,
    pub config_set_id: Option<f64>,
}

/// `self.treeListSpecialLinks` & co. of the last `SyncLoadouts`: `linkId -> setId`.
#[derive(Clone, Debug, Default)]
pub struct SpecialLinks {
    tree: HashMap<String, usize>,
    item: HashMap<String, f64>,
    skill: HashMap<String, f64>,
    config: HashMap<String, f64>,
}

/// The sets as the loadout code reads them: `(id, title or "Default")` in the order lists, the specs' titles and `GetSpecList` labels.
struct SetsView {
    /// `(spec.title or "Default", GetSpecList label)`.
    specs: Vec<(String, String)>,
    /// `treeTab.activeSpec`.
    active_spec: usize,
    items: Vec<(f64, String)>,
    active_item: f64,
    skills: Vec<(f64, String)>,
    active_skill: f64,
    configs: Vec<(f64, String)>,
    active_config: f64,
}

fn or_default(t: Option<String>) -> String {
    t.unwrap_or_else(|| "Default".to_string())
}

fn sets_view(e: &Engine) -> SetsView {
    let m = e.model();
    let labels = e.spec_list_labels();
    SetsView {
        specs: m
            .tree
            .specs
            .iter()
            .zip(labels)
            .map(|(s, l)| (or_default(s.title.clone()), l))
            .collect(),
        active_spec: m.tree.active_spec as usize,
        items: m.item_sets.iter().map(|s| (s.id, or_default(s.title.clone()))).collect(),
        active_item: m.active_item_set,
        skills: e.skill_sets().into_iter().map(|(id, t)| (id, or_default(t))).collect(),
        active_skill: e.skills_tab().active_skill_set_id,
        configs: e
            .config_set_order()
            .into_iter()
            .map(|id| (id, or_default(e.config_set_title(id))))
            .collect(),
        active_config: e.config_active_set(),
    }
}

/// `string.match(title, "%{([%w,]+)%}")` and the set name without it (Build.lua:704-711).
fn link_of(title: &str) -> Option<(String, String)> {
    let link = lua::lmatch(title, "%{([%w,]+)%}")?;
    let name = lua::gsub(&lua::gsub(&lua::gsub(title, &format!("%{{{link}%}}"), ""), "^%s*", ""), "%s*$", "");
    Some((link.clone(), if name.is_empty() { "Default".into() } else { name }))
}

/// `string.gmatch(linkIdentifier, "[^%,]+")`.
fn link_ids(link: &str) -> impl Iterator<Item = &str> {
    link.split(',').filter(|x| !x.is_empty())
}

/// What one `SyncLoadouts` makes.
struct Synced {
    list: Vec<String>,
    loadouts: Vec<usize>,
    links: SpecialLinks,
    /// `SetSel(i)` + `activeLoadout = i - 1` (:803-804); `None`: `activeLoadout = 1`, `SetSel(1)` (:812-813).
    matched: Option<usize>,
}

/// Build.lua:684-818 over the sets.
fn sync(v: &SetsView) -> Synced {
    let mut links = SpecialLinks::default();
    let mut filtered = vec!["^7^7Loadouts:".to_string()];
    let mut loadouts = Vec::new();
    let one_skill = v.skills.len() == 1;
    let one_item = v.items.len() == 1;
    let one_config = v.configs.len() == 1;
    let mut filtered_specs: Vec<usize> = Vec::new();
    let mut sorted_tree_links: Vec<(String, String)> = Vec::new(); // (setName, linkId)
    for (i, (title, _)) in v.specs.iter().enumerate() {
        match link_of(title) {
            Some((link, name)) => {
                for id in link_ids(&link) {
                    links.tree.insert(id.to_string(), i + 1);
                    sorted_tree_links.push((name.clone(), id.to_string()));
                }
            }
            None => filtered_specs.push(i + 1),
        }
    }
    // :731-755 `identifyLinks`
    let identify = |sets: &[(f64, String)], special: &mut HashMap<String, f64>| {
        let mut names = HashSet::new();
        for (id, title) in sets {
            match link_of(title) {
                Some((link, _)) => {
                    for l in link_ids(&link) {
                        special.insert(l.to_string(), *id);
                    }
                }
                None => {
                    names.insert(title.clone());
                }
            }
        }
        names
    };
    let item_list = identify(&v.items, &mut links.item);
    let skill_list = identify(&v.skills, &mut links.skill);
    let config_list = identify(&v.configs, &mut links.config);
    // :761-770 only exact match loadouts
    for &i in &filtered_specs {
        let tree = &v.specs[i - 1].1;
        if (one_item || item_list.contains(tree)) && (one_skill || skill_list.contains(tree)) && (one_config || config_list.contains(tree))
        {
            filtered.push(tree.clone());
            loadouts.push(i);
        }
    }
    // :772-778
    for (name, id) in &sorted_tree_links {
        if (one_item || links.item.contains_key(id))
            && (one_skill || links.skill.contains_key(id))
            && (one_config || links.config.contains_key(id))
        {
            filtered.push(format!("{name} {{{id}}}"));
        }
    }
    for s in ["^7^7-----", "^7^7Manage", "^7^7Sync", "^7^7Help >>"] {
        filtered.push(s.to_string());
    }
    // :789-810 the loadout of the active tree
    let mut matched = None;
    let tree_name = v
        .specs
        .get(v.active_spec.wrapping_sub(1))
        .unwrap_or_else(|| panic!("Modules/Build.lua:790: attempt to index a nil value (field '?')"))
        .0
        .clone();
    if let Some(i) = filtered.iter().position(|l| *l == tree_name) {
        let link_match = lua::lmatch(&tree_name, "%{(%w+)%}").unwrap_or_else(|| tree_name.clone());
        let title_of = |sets: &[(f64, String)], active: f64, line: u32| {
            sets.iter()
                .find(|(id, _)| *id == active)
                .map(|(_, t)| t.clone())
                .unwrap_or_else(|| panic!("Modules/Build.lua:{line}: attempt to index a nil value (field '?')"))
        };
        let skill_match = one_skill || title_of(&v.skills, v.active_skill, 795).contains(&link_match);
        let item_match = one_item || title_of(&v.items, v.active_item, 797).contains(&link_match);
        let config_match = one_config || title_of(&v.configs, v.active_config, 799).contains(&link_match);
        if skill_match && item_match && config_match {
            matched = Some(i + 1);
        }
    }
    Synced {
        list: filtered,
        loadouts,
        links,
        matched,
    }
}

impl BuildMode {
    fn sets_view(&self) -> SetsView {
        let eng = self.engine.borrow();
        sets_view(eng.as_ref().expect("engine"))
    }

    /// `buildMode:SyncLoadouts()` (Build.lua:684-818).
    pub(crate) fn sync_loadouts_impl(&self, g: &mut Gfx) {
        if self.is_compare_entry.get() {
            // CompareEntry.lua:419-421
            return;
        }
        let out = sync(&self.sets_view());
        *self.loadouts_list.borrow_mut() = Rc::new(RefCell::new(out.loadouts));
        *self.special_links.borrow_mut() = Rc::new(out.links);
        let c = self.c();
        *c.build_loadouts.list.borrow_mut() = out.list.iter().map(|s| crate::controls::DropItem::str(s)).collect();
        match out.matched {
            Some(i) => {
                c.build_loadouts.set_sel(g, i as i64, false);
                self.active_loadout.set(Some((i - 1) as f64));
            }
            None => {
                self.active_loadout.set(Some(1.0));
                c.build_loadouts.set_sel(g, 1, false);
            }
        }
    }

    /// `self.loadoutsList` (a copy of the table's contents).
    pub fn loadouts_list_values(&self) -> Vec<usize> {
        self.loadouts_list.borrow().borrow().clone()
    }

    /// The `buildLoadouts` selFunc (Build.lua:310-331).
    pub(crate) fn loadouts_sel_func(&self, g: &mut Gfx, value: &str) {
        let c = self.c();
        match value {
            "^7^7Loadouts:" | "^7^7-----" => {
                c.build_loadouts.set_sel(g, 1, false);
                return;
            }
            "^7^7Sync" => {
                self.sync_loadouts_impl(g);
                c.build_loadouts.set_sel(g, 1, false);
                return;
            }
            "^7^7Help >>" => {
                main().open_about_popup(g, Some(HelpSection::Index(7)));
                c.build_loadouts.set_sel(g, 1, false);
                return;
            }
            "^7^7Manage" => self.open_build_set_manage_popup(g),
            _ => {}
        }
        let loadout = self.get_loadout_by_name(value);
        self.set_active_loadout(g, loadout);
        c.build_loadouts.sel_by_label(value);
    }

    /// `buildMode:GetLoadoutByName(loadoutName)` (Build.lua:940-995) with the special links of the last `SyncLoadouts`.
    pub fn get_loadout_by_name(&self, name: &str) -> Option<Loadout> {
        let v = self.sets_view();
        let links = self.special_links.borrow().clone();
        let link = lua::lmatch(name, "%{(%w+)%}");
        let find_set = |sets: &[(f64, String)], special: &HashMap<String, f64>| -> Option<f64> {
            for (id, title) in sets {
                if name == title {
                    return Some(*id);
                }
                if let Some(l) = &link {
                    return Some(
                        *special
                            .get(l)
                            .unwrap_or_else(|| panic!("Modules/Build.lua:950: attempt to index a nil value (field '?')")),
                    );
                }
            }
            None
        };
        let mut spec_id = None;
        for (i, (_, label)) in v.specs.iter().enumerate() {
            if name == label {
                spec_id = Some(i + 1);
                break;
            }
            if let Some(l) = &link {
                spec_id = Some(
                    *links
                        .tree
                        .get(l)
                        .unwrap_or_else(|| panic!("Modules/Build.lua:966: attempt to index a nil value (field '?')")),
                );
                break;
            }
        }
        let item_id = if v.items.len() == 1 {
            Some(v.items[0].0)
        } else {
            find_set(&v.items, &links.item)
        };
        let skill_id = if v.skills.len() == 1 {
            Some(v.skills[0].0)
        } else {
            find_set(&v.skills, &links.skill)
        };
        let config_id = if v.configs.len() == 1 {
            Some(v.configs[0].0)
        } else {
            find_set(&v.configs, &links.config)
        };
        if spec_id.is_none() && item_id.is_none() && skill_id.is_none() && config_id.is_none() {
            return None;
        }
        Some(Loadout {
            spec_id,
            item_set_id: item_id,
            skill_set_id: skill_id,
            config_set_id: config_id,
        })
    }

    /// `buildMode:SetActiveLoadout(loadout)` (Build.lua:997-1021).
    pub fn set_active_loadout(&self, g: &mut Gfx, loadout: Option<Loadout>) {
        let Some(l) = loadout else { return };
        let Some(spec_id) = l.spec_id else { return };
        let (active_spec, active_item, active_skill, active_config) = {
            let eng = self.engine.borrow();
            let e = eng.as_ref().expect("engine");
            (
                e.model().tree.active_spec as usize,
                e.model().active_item_set,
                self.stale_active_skill_set.take().unwrap_or(e.skills_tab().active_skill_set_id),
                e.model().config.active_config_set,
            )
        };
        // :1008 `newSpecId ~= activeSpec` compares numbers: when `specList[activeSpec]` changed under `build.spec` (the active loadout deleted
        // while it was first, or moved) and `newSpecId == activeSpec`, `SetActiveSpec` is skipped and the old object stays `build.spec` (the
        // calcs and the tree) while `specList` saves the new one; the engine keeps it ([`Engine::spec_list_orphaned`]) until the next
        // `SetActiveSpec` (user 2026-11-25: as the original)
        // :1011-1019. `self.itemsTab.itemSets[nil]` is nil: the first set
        if l.item_set_id != Some(active_item) {
            let tab = self.items_tab.borrow().clone().expect("itemsTab");
            tab.set_active_item_set_with(g, l.item_set_id.unwrap_or(f64::NAN), true);
        }
        if l.skill_set_id != Some(active_skill) {
            let tab = self.skills_tab.borrow().clone().expect("skillsTab");
            tab.set_active_skill_set_with(g, l.skill_set_id, true);
        }
        if l.config_set_id != Some(active_config) {
            let tab = self.config_tab.borrow().clone().expect("configTab");
            tab.set_active_config_set_with(g, l.config_set_id, true);
        }
        // :1008-1010 `SetActiveSpec(newSpecId, true)` comes first in the Lua, and nothing runs the calcs before the next frame; the port's
        // `SetActiveSpec` runs them at once (tree_tab.rs `activate_spec`), so it goes last here: a run of the new tree over the old sets would
        // change those sets (CalcSetup drops the groups whose source went with the tree, Build.lua's next frame never sees them)
        if spec_id != active_spec {
            // the tree tab is made on first view, and a build opened on another tab has none yet
            self.tree_tab_made(g).set_active_spec_with(g, spec_id, true);
        }
        self.sync_loadouts_impl(g);
    }

    /// `buildMode:NewLoadout(loadoutName)` (Build.lua:820-832).
    pub fn new_loadout(&self, g: &mut Gfx, name: &str) {
        let items = self.items_tab.borrow().clone().expect("itemsTab");
        let skills = self.skills_tab.borrow().clone().expect("skillsTab");
        let config = self.config_tab.borrow().clone().expect("configTab");
        items.new_item_set(Some((items.item_sets_len() + 1) as f64), name);
        skills.new_skill_set(Some((skills.skill_sets_len() + 1) as f64), name);
        config.new_config_set(Some((config.config_sets_len() + 1) as f64), name);
        {
            let mut eng = self.engine.borrow_mut();
            let e = eng.as_mut().expect("engine");
            let mut spec = e.fresh_spec_model();
            spec.title = Some(name.to_string());
            e.push_spec(spec);
        }
        self.sync_loadouts_impl(g);
        let l = self.get_loadout_by_name(name);
        self.set_active_loadout(g, l);
        self.mod_flag.set(true);
    }

    /// `buildMode:CopyLoadout(copyLoadoutName, loadoutName)` (Build.lua:834-848).
    pub fn copy_loadout(&self, g: &mut Gfx, copy_name: &str, name: &str) {
        let Some(l) = self.get_loadout_by_name(copy_name) else { return };
        let spec_id = l
            .spec_id
            .unwrap_or_else(|| panic!("Classes/TreeTab.lua:658: attempt to index a nil value (field '?')"));
        self.copy_tree(spec_id, name);
        let items = self.items_tab.borrow().clone().expect("itemsTab");
        let skills = self.skills_tab.borrow().clone().expect("skillsTab");
        let config = self.config_tab.borrow().clone().expect("configTab");
        items.copy_item_set(l.item_set_id.unwrap_or(1.0), name);
        skills.copy_skill_set(l.skill_set_id.unwrap_or(1.0), name);
        config.copy_config_set(l.config_set_id.unwrap_or(1.0), name);
        let copy = self.get_loadout_by_name(name);
        self.set_active_loadout(g, copy);
        self.mod_flag.set(true);
    }

    /// `self.treeTab:CopyTree(specId, name)` (TreeTab.lua:657-667).
    fn copy_tree(&self, spec_id: usize, name: &str) {
        let mut eng = self.engine.borrow_mut();
        eng.as_mut()
            .expect("engine")
            .copy_tree(spec_id, Some(name))
            .unwrap_or_else(|e| panic!("{e}"));
    }

    /// `buildMode:CustomLoadout(specId, itemSetId, skillSetId, configSetId, name)` (Build.lua:850-888): `-1` makes a new set.
    pub fn custom_loadout(&self, g: &mut Gfx, spec_id: i64, item_set_id: f64, skill_set_id: f64, config_set_id: f64, name: &str) {
        if spec_id == -1 {
            let mut eng = self.engine.borrow_mut();
            let e = eng.as_mut().expect("engine");
            let mut spec = e.fresh_spec_model();
            spec.title = Some(name.to_string());
            e.push_spec(spec);
        } else {
            self.copy_tree(usize::try_from(spec_id).expect("a spec index"), name);
        }
        let items = self.items_tab.borrow().clone().expect("itemsTab");
        let skills = self.skills_tab.borrow().clone().expect("skillsTab");
        let config = self.config_tab.borrow().clone().expect("configTab");
        if item_set_id == -1.0 {
            items.new_item_set(Some((items.item_sets_len() + 1) as f64), name);
        } else {
            items.copy_item_set(item_set_id, name);
        }
        let skill_id = if skill_set_id == -1.0 {
            skills.new_skill_set(Some((skills.skill_sets_len() + 1) as f64), name)
        } else {
            skills.copy_skill_set(skill_set_id, name)
        };
        // :872 `newSkillSet.title = name` (already)
        let _ = skill_id;
        if config_set_id == -1.0 {
            config.new_config_set(Some((config.config_sets_len() + 1) as f64), name);
        } else {
            config.copy_config_set(config_set_id, name);
        }
        let l = self.get_loadout_by_name(name);
        self.set_active_loadout(g, l);
        self.mod_flag.set(true);
    }

    /// `buildMode:DeleteLoadout(loadoutName, nextLoadoutName)` (Build.lua:890-920).
    pub fn delete_loadout(&self, g: &mut Gfx, name: &str, next_name: &str) {
        let l = self
            .get_loadout_by_name(name)
            .unwrap_or_else(|| panic!("Modules/Build.lua:901: attempt to index local 'loadout' (a nil value)"));
        if let Some(spec_id) = l.spec_id {
            let mut eng = self.engine.borrow_mut();
            eng.as_mut().expect("engine").remove_spec_raw(spec_id);
        }
        let items = self.items_tab.borrow().clone().expect("itemsTab");
        let skills = self.skills_tab.borrow().clone().expect("skillsTab");
        let config = self.config_tab.borrow().clone().expect("configTab");
        let index_of = |order: &[(f64, String)], id: f64| order.iter().position(|(x, _)| *x == id).map(|i| i + 1);
        let v = self.sets_view();
        if let Some(id) = l.item_set_id
            && v.items.len() > 1
        {
            let index = index_of(&v.items, id).expect("reverseLookup");
            items.delete_item_set(id, index);
        }
        if let Some(id) = l.skill_set_id
            && v.skills.len() > 1
        {
            let index = index_of(&v.skills, id).expect("reverseLookup");
            // `activeSkillSetId` stays the deleted id until `SetActiveLoadout` (the engine moves it on at once)
            if id == v.active_skill {
                self.stale_active_skill_set.set(Some(id));
            }
            skills.delete_skill_set(id, index);
        }
        if let Some(id) = l.config_set_id
            && v.configs.len() > 1
        {
            let index = index_of(&v.configs, id).expect("reverseLookup");
            config.delete_config_set(id, index);
        }
        self.mod_flag.set(true);
        let next = self.get_loadout_by_name(next_name);
        self.set_active_loadout(g, next);
        self.stale_active_skill_set.set(None);
    }

    /// `buildMode:RenameLoadout(oldName, newName)` (Build.lua:922-938).
    pub fn rename_loadout(&self, name: &str, new_name: &str) {
        let l = self
            .get_loadout_by_name(name)
            .unwrap_or_else(|| panic!("Modules/Build.lua:924: attempt to index local 'loadout' (a nil value)"));
        if let Some(spec_id) = l.spec_id {
            let mut eng = self.engine.borrow_mut();
            eng.as_mut().expect("engine").rename_spec(spec_id, new_name);
        }
        if let Some(id) = l.item_set_id {
            self.items_tab.borrow().clone().expect("itemsTab").rename_item_set(id, new_name);
        }
        if let Some(id) = l.skill_set_id {
            self.skills_tab.borrow().clone().expect("skillsTab").rename_skill_set(id, new_name);
        }
        if let Some(id) = l.config_set_id {
            self.config_tab.borrow().clone().expect("configTab").rename_config_set(id, new_name);
        }
        self.mod_flag.set(true);
    }

    /// `buildMode:ReorderLoadout(oldIndex, newIndex)` (Build.lua:1023-1065).
    pub fn reorder_loadout(&self, g: &mut Gfx, old_index: usize, new_index: usize) {
        if old_index == new_index {
            return;
        }
        let n = self.engine.borrow().as_ref().expect("engine").spec_count();
        let loadouts = self.loadouts_list_values();
        if old_index == 0 || old_index > n || new_index > n || new_index > loadouts.len() {
            return;
        }
        assert!(
            new_index > 0,
            "Modules/Build.lua:1036: bad argument #2 to 'insert' (position out of bounds)"
        );
        let active_title = {
            let v = self.sets_view();
            v.specs
                .get(v.active_spec.wrapping_sub(1))
                .unwrap_or_else(|| panic!("Modules/Build.lua:1031: attempt to index a nil value (field '?')"))
                .0
                .clone()
        };
        let mut order: Vec<usize> = (1..=n).collect();
        if n == loadouts.len() {
            // :1034-1041
            let moved = order.remove(old_index - 1);
            order.insert(new_index - 1, moved);
        } else {
            // :1043-1060 the specs of `loadoutsList` take their places in its order
            let in_list: HashSet<usize> = loadouts.iter().copied().collect();
            let mut idx = 0;
            for (i, o) in order.iter_mut().enumerate() {
                if in_list.contains(&(i + 1)) {
                    *o = loadouts[idx];
                    idx += 1;
                }
            }
        }
        self.engine.borrow_mut().as_mut().expect("engine").permute_specs(&order);
        self.mod_flag.set(true);
        let l = self.get_loadout_by_name(&active_title);
        self.set_active_loadout(g, l);
    }

    /// `buildMode:OpenBuildSetManagePopup()` (Build.lua:2808-2820).
    pub fn open_build_set_manage_popup(&self, g: &mut Gfx) {
        let list = super::build_set_list::new_build_set_list(self.me.clone());
        let me = self.me.clone();
        let done = crate::controls::ButtonControl::new(None, crate::control::rect(0.0, 260.0, 90.0, 20.0), "Done".to_string(), move |g| {
            main().close_popup();
            let b = me.upgrade().expect("buildMode");
            let c = b.c();
            match b.active_loadout.get() {
                Some(a) if a > 0.0 => c.build_loadouts.set_sel(g, (a + 1.0) as i64, false),
                _ => c.build_loadouts.set_sel(g, 1, false),
            }
        });
        let mut controls: indexmap::IndexMap<String, crate::control::ControlRef> = indexmap::IndexMap::new();
        controls.insert("1".into(), list);
        controls.insert("2".into(), done);
        main().open_popup(g, 400.0, 290.0, "Manage Loadouts", controls, None, None, None, None);
    }
}
