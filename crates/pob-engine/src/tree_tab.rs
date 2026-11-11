//! What `Classes/TreeTab.lua` (+ `PassiveSpecListControl`, the class drop-downs of `Modules/Build.lua`, the class switch of
//! `PassiveTreeView.lua:408-505`) edits and reads (U3b). Every mutator edits the model the way the Lua edits the live `PassiveSpec` objects,
//! so "the mutator == a fresh load of what `SaveDB` writes" (`tests/tree_tab.rs`). `specList` = `model.tree.specs`, `activeSpec` =
//! `model.tree.active_spec` (1-based). Undo lists live with the tab (`pob-ui`), per spec.

use std::rc::Rc;

use pob_build::{NumMap, SpecModel, SpecOverrideModel};
use pob_calc::ParseMode;
use pob_calc::init_env::{cluster_jewel_of, post_load_spec, spec_jewels_of};
use pob_tree::cluster::ClusterJewel;
use pob_tree::{NodeType, PassiveSpec};

use crate::engine::Engine;

pub use pob_calc::power::{NodePower, PowerMax, PowerProgress, PowerResult, PowerStat, ReportDist, ReportEntry, power_stat_list};

/// `PassiveSpec:CreateUndoState()` (PassiveSpec.lua:2359-2381). `secondaryAscendClassId = self.secondaryAscendClassId` reads a field nothing sets
/// (the spec's is `curSecondaryAscendClassId`): an undo always passes nil -- kept (no field here).
#[derive(Clone, Debug)]
pub struct SpecUndoState {
    pub class_id: f64,
    pub ascend_class_id: f64,
    pub hash_list: Vec<f64>,
    pub overrides: Vec<SpecOverrideModel>,
    pub mastery_effects: NumMap<f64>,
    pub tree_version: String,
}

/// What the spec lists read of a `PassiveSpec` (TreeTab.lua:59-103, PassiveSpecListControl.lua:76-84).
#[derive(Clone, Debug)]
pub struct SpecSummary {
    pub title: Option<String>,
    pub tree_version: String,
    pub class_id: Option<usize>,
    pub class_name: String,
    pub ascend_class_name: String,
    pub used: u32,
    pub sockets: u32,
}

/// One effect of `OpenMasteryPopup` (TreeTab.lua:1026-1031).
#[derive(Clone, Debug, PartialEq)]
pub struct MasteryChoice {
    pub label: String,
    pub id: f64,
}

/// One entry of `buildMods` of `ModifyNodePopup` (TreeTab.lua:851-883).
#[derive(Clone, Debug, PartialEq)]
pub struct TattooChoice {
    pub label: String,
    pub descriptions: Vec<String>,
    pub key: String,
}

pub(crate) fn cluster_lookup(items: &indexmap::IndexMap<u64, Rc<pob_items::item::Item>>, id: f64) -> Option<Rc<ClusterJewel>> {
    if id < 0.0 || id.fract() != 0.0 {
        return None;
    }
    items.get(&(id as u64)).and_then(|i| cluster_jewel_of(i)).map(Rc::new)
}

fn overrides_of(spec: &PassiveSpec) -> Vec<SpecOverrideModel> {
    spec.hash_overrides
        .iter()
        .map(|(id, t)| SpecOverrideModel {
            node_id: f64::from(*id),
            icon: Some(t.icon.clone()),
            active_effect_image: Some(t.active_effect_image.clone()),
            dn: Some(t.dn.clone()),
            attrib: indexmap::IndexMap::new(),
        })
        .collect()
}

/// `common.base64.decode` (runtime/lua/base64.lua:27): what is outside the alphabet is stripped, not an error.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

impl Engine {
    /// A cluster jewel of the build's items (`spec:GetJewel`).
    fn edit_spec<R>(
        &mut self,
        f: impl FnOnce(&mut PassiveSpec, &dyn Fn(f64) -> Option<Rc<ClusterJewel>>) -> Result<R, pob_tree::tree::TreeError>,
    ) -> Result<R, String> {
        let mut spec = (*self.result().env.spec).clone();
        let items = self.result().env.items.clone();
        let lookup = move |id: f64| cluster_lookup(&items, id);
        let r = f(&mut spec, &lookup).map_err(|e| e.0)?;
        self.write_spec_to_model(spec);
        Ok(r)
    }

    /// `#self.specList`.
    pub fn spec_count(&self) -> usize {
        self.model().tree.specs.len()
    }

    /// `self.activeSpec` (1-based).
    pub fn active_spec(&self) -> usize {
        pob_lua::number::min2(self.model().tree.active_spec, self.spec_count() as f64) as usize
    }

    /// `self.specList[i]` (1-based) with its `PostLoad`: what `calcFunc({ spec = spec })`, the compare overlay and the lists read.
    pub fn load_spec(&self, i: usize) -> Result<PassiveSpec, String> {
        if let Some(Some(o)) = self.spec_objects.get(i - 1) {
            return Ok((**o).clone());
        }
        let sm = &self.model().tree.specs[i - 1];
        let items = &self.result().env.items;
        let jewels = spec_jewels_of(&sm.jewels, items).map_err(|e| e.0)?;
        let mut s = PassiveSpec::load_mode_jewels(sm, ParseMode::Full, jewels).map_err(|e| e.0)?;
        post_load_spec(&mut s, items).map_err(|e| e.0)?;
        Ok(s)
    }

    /// The summaries of `specList`, in order.
    pub fn spec_summaries(&self) -> Result<Vec<SpecSummary>, String> {
        let active = self.active_spec();
        (1..=self.spec_count())
            .map(|i| {
                let sm = &self.model().tree.specs[i - 1];
                let loaded;
                let spec: &PassiveSpec = if i == active {
                    &self.result().env.spec
                } else {
                    loaded = self.load_spec(i)?;
                    &loaded
                };
                let c = spec.count_alloc_nodes();
                Ok(SpecSummary {
                    title: sm.title.clone(),
                    tree_version: spec.tree_version.clone(),
                    class_id: spec.cur_class_id,
                    class_name: spec.cur_class_name.clone(),
                    ascend_class_name: spec.cur_ascend_class_name.clone(),
                    used: c.used,
                    sockets: c.sockets,
                })
            })
            .collect()
    }

    /// TreeTab.lua:491-497 `GetSpecList` (without the "Manage trees..." row).
    pub fn spec_list_labels(&self) -> Vec<String> {
        let t = pob_data::tree::tree_versions().expect("GameVersions");
        self.model()
            .tree
            .specs
            .iter()
            .map(|s| {
                let v = if s.tree_version == t.latest_tree_version {
                    String::new()
                } else {
                    let d = t
                        .tree_versions
                        .iter()
                        .find(|v| v.ver == s.tree_version)
                        .map_or("?", |v| v.display.as_str());
                    format!("[{d}] ")
                };
                format!("{v}{}", s.title.as_deref().unwrap_or("Default"))
            })
            .collect()
    }

    /// TreeTab.lua:548-584 `SetActiveSpec(specId)`: `prevSpec.jewels` / `curSpec.jewels` are per `<Spec>` in the model already.
    pub fn set_active_spec(&mut self, spec_id: usize) -> Result<(), String> {
        self.keep_active_object();
        self.activate_spec(spec_id)
    }

    fn sync_spec_objects(&mut self) {
        let n = self.spec_count();
        self.spec_objects.resize(n, None);
    }

    /// The object of the active spec stays in `specList` when `build.spec` moves on.
    fn keep_active_object(&mut self) {
        self.sync_spec_objects();
        let a = self.active_spec();
        self.spec_objects[a - 1] = Some(self.result().env.spec.clone());
    }

    /// `build.spec = specList[specId]`: the kept object, else a load of the model; its run now (the Lua reads `build.spec` in this very frame); the `buildFlag` frame runs
    /// again ([`Engine::refresh`]), since a loadout changes the item, skill and config sets after the spec.
    fn activate_spec(&mut self, spec_id: usize) -> Result<(), String> {
        self.sync_spec_objects();
        self.model_mut().tree.active_spec = spec_id as f64;
        let n = self.spec_count() as f64;
        let m = self.model_mut();
        m.tree.active_spec = pob_lua::number::min2(m.tree.active_spec, n);
        let a = self.active_spec();
        let kept = self.spec_objects[a - 1].clone();
        self.set_live_spec(kept);
        self.recalc()?;
        Ok(())
    }

    /// `self.specList[i].title = title`.
    pub fn rename_spec(&mut self, i: usize, title: &str) {
        self.model_mut().tree.specs[i - 1].title = Some(title.to_string());
    }

    /// `t_insert(self.specList, newSpec)` of a decoded link: the object as the decode left it (`PassiveSpec::apply_decoded`), kept for `SetActiveSpec`. The run reads the model.
    pub fn push_decoded_spec(&mut self, spec: SpecModel) -> Result<(), String> {
        let tree = pob_tree::tree::load_passive_tree_mode(&spec.tree_version, ParseMode::Full).map_err(|e| e.0)?;
        let mut object = PassiveSpec::new(tree).map_err(|e| e.0)?;
        object
            .apply_decoded(
                spec.class_id,
                spec.ascend_class_id,
                spec.secondary_ascend_class_id,
                &spec.nodes,
                &spec.mastery_effects,
            )
            .map_err(|e| e.0)?;
        self.model_mut().tree.specs.push(spec);
        self.sync_spec_objects();
        *self.spec_objects.last_mut().expect("just pushed") = Some(Rc::new(object));
        Ok(())
    }

    /// `t_insert(self.list, spec)`: `spec` at the end.
    pub fn push_spec(&mut self, spec: SpecModel) {
        self.model_mut().tree.specs.push(spec);
        self.sync_spec_objects();
    }

    /// PassiveSpecListControl.lua:17-21 `Copy`: `specList[i]` as a new spec (not in the list yet).
    pub fn copy_spec_model(&self, i: usize) -> Result<SpecModel, String> {
        let spec = if i == self.active_spec() {
            (*self.result().env.spec).clone()
        } else {
            self.load_spec(i)?
        };
        let sm = &self.model().tree.specs[i - 1];
        let mut out = sm.clone();
        // `CreateUndoState`: `hashList` in `pairs(allocNodes)` order, what `ImportFromNodeList` of the copy walks (the paths over equal lengths follow it)
        out.nodes = spec.alloc_table_pairs().into_iter().map(f64::from).collect();
        out.class_id = spec.cur_class_id.map_or(0.0, |c| c as f64);
        out.ascend_class_id = spec.cur_ascend_class_id.map_or(0.0, |c| c as f64);
        // `CreateUndoState`'s nil secondary (see `SpecUndoState`)
        out.secondary_ascend_class_id = None;
        out.mastery_effects = NumMap::default();
        for (m, e) in &spec.mastery_selections {
            out.mastery_effects.set(f64::from(*m), *e);
        }
        out.overrides = overrides_of(&spec);
        out.cluster_hash_format_version = 2.0;
        out.url = None;
        Ok(out)
    }

    /// PassiveSpecListControl.lua:40-43 `New`: a new spec of the latest tree with the class of the active one.
    pub fn new_spec_model(&self) -> SpecModel {
        let spec = &self.result().env.spec;
        SpecModel {
            title: None,
            tree_version: pob_data::tree::tree_versions().expect("GameVersions").latest_tree_version.clone(),
            cluster_hash_format_version: 2.0,
            class_id: spec.cur_class_id.map_or(0.0, |c| c as f64),
            ascend_class_id: spec.cur_ascend_class_id.map_or(0.0, |c| c as f64),
            secondary_ascend_class_id: spec.cur_secondary_ascend_class_id,
            nodes: Vec::new(),
            mastery_effects: NumMap::default(),
            jewels: NumMap::default(),
            url: None,
            overrides: Vec::new(),
        }
    }

    /// `t_remove(self.list, index)`; the active index follows `build.spec` (PassiveSpecListControl.lua:102-109). `true`: the active spec went.
    pub fn delete_spec(&mut self, index: usize) -> Result<bool, String> {
        let active = self.active_spec();
        self.sync_spec_objects();
        self.spec_objects.remove(index - 1);
        self.model_mut().tree.specs.remove(index - 1);
        if index == active {
            self.activate_spec(usize::max(1, index - 1))?;
            Ok(true)
        } else {
            if index < active {
                self.model_mut().tree.active_spec = (active - 1) as f64;
            }
            Ok(false)
        }
    }

    /// `OnOrderChange` (PassiveSpecListControl.lua:86-91): the row moved; `activeSpec` follows `build.spec`.
    pub fn move_spec(&mut self, old_index: usize, new_index: usize) {
        let active = self.active_spec();
        self.sync_spec_objects();
        let o = self.spec_objects.remove(old_index - 1);
        self.spec_objects.insert(new_index - 1, o);
        let m = self.model_mut();
        let s = m.tree.specs.remove(old_index - 1);
        m.tree.specs.insert(new_index - 1, s);
        let a = if active == old_index {
            new_index
        } else if old_index < active && new_index >= active {
            active - 1
        } else if old_index > active && new_index <= active {
            active + 1
        } else {
            active
        };
        m.tree.active_spec = a as f64;
    }

    /// TreeTab.lua:593-617 `ConvertToVersion(version, remove, success, ignoreTreeSubType)`: returns the version used.
    pub fn convert_to_version(&mut self, version: &str, remove: bool, ignore_tree_sub_type: bool) -> Result<String, String> {
        let active = self.active_spec();
        let mut version = version.to_string();
        let t = pob_data::tree::tree_versions().map_err(|e| e.0)?;
        // :594 `treeVersion:match("(_%l+_?%l*)")`
        let cur = self.result().env.spec.tree_version.clone();
        let sub = pob_lua::match_(cur.as_bytes(), b"(_%l+_?%l*)", None)
            .map_err(|e| e.0)?
            .and_then(|c| {
                c.get(0)
                    .and_then(|x| x.bytes(cur.as_bytes()))
                    .map(|b| String::from_utf8_lossy(b).into_owned())
            });
        if !ignore_tree_sub_type
            && let Some(sub) = sub
            && !version.contains(&sub)
        {
            let v = format!("{version}{sub}");
            if t.tree_versions.iter().any(|x| x.ver == v) {
                version = v;
            }
        }
        let mut new_spec = self.copy_spec_model(active)?;
        new_spec.tree_version.clone_from(&version);
        self.keep_active_object();
        self.spec_objects.insert(active, None);
        let m = self.model_mut();
        m.tree.specs.insert(active, new_spec);
        if remove {
            m.tree.specs.remove(active - 1);
            self.spec_objects.remove(active - 1);
            self.activate_spec(active)?;
        } else {
            self.activate_spec(active + 1)?;
        }
        Ok(version)
    }

    /// TreeTab.lua:619-632 `ConvertAllToVersion(version)`.
    pub fn convert_all_to_version(&mut self, version: &str) -> Result<(), String> {
        let cur = self.active_spec();
        let list: Vec<String> = self.model().tree.specs.iter().map(|s| s.tree_version.clone()).collect();
        for (i, v) in list.iter().enumerate() {
            if v != version {
                self.set_active_spec(i + 1)?;
                self.convert_to_version(version, true, false)?;
            }
        }
        self.set_active_spec(cur)
    }

    /// TreeTab.lua:137-139: `ResetNodes`, `BuildAllDependsAndPaths`.
    pub fn reset_tree(&mut self) -> Result<(), String> {
        self.edit_spec(|s, _| s.reset_tree())
    }

    /// `CreateUndoState` of the active spec.
    pub fn create_undo_state(&self) -> SpecUndoState {
        let spec = &self.result().env.spec;
        let mut me = NumMap::default();
        for (m, e) in &spec.mastery_selections {
            me.set(f64::from(*m), *e);
        }
        SpecUndoState {
            class_id: spec.cur_class_id.map_or(0.0, |c| c as f64),
            ascend_class_id: spec.cur_ascend_class_id.map_or(0.0, |c| c as f64),
            hash_list: spec.alloc_table_pairs().into_iter().map(f64::from).collect(),
            overrides: overrides_of(spec),
            mastery_effects: me,
            tree_version: spec.tree_version.clone(),
        }
    }

    /// `RestoreUndoState(state)` (PassiveSpec.lua:2383-2386) of the active spec.
    pub fn restore_undo_state(&mut self, st: &SpecUndoState) -> Result<(), String> {
        // `ImportFromNodeList` on the same object: its `allocNodes` table keeps its history (the paths over equal lengths follow it)
        let overrides: Vec<pob_tree::tattoo::OverrideInput> = st
            .overrides
            .iter()
            .map(|o| pob_tree::tattoo::OverrideInput {
                node_id: Some(o.node_id),
                dn: o.dn.clone(),
                icon: o.icon.clone(),
                active_effect_image: o.active_effect_image.clone(),
            })
            .collect();
        self.edit_spec(|s, _| {
            s.hash_overrides.clear();
            s.load_overrides(&overrides)?;
            s.import_from_node_list(st.class_id, st.ascend_class_id, None, &st.hash_list, &st.mastery_effects)
        })?;
        let sm = crate::engine::active_spec_mut(self.model_mut());
        sm.overrides.clone_from(&st.overrides);
        sm.tree_version.clone_from(&st.tree_version);
        self.recalc()?;
        Ok(())
    }

    /// `spec:SelectClass(classId)`.
    pub fn select_class(&mut self, class_id: f64) -> Result<(), String> {
        self.edit_spec(|s, _| s.select_class(class_id))
    }

    /// `spec:SelectAscendClass(id)`.
    pub fn select_ascend_class(&mut self, id: f64) -> Result<(), String> {
        self.edit_spec(|s, _| s.select_ascend_class(id))
    }

    /// `spec:SelectSecondaryAscendClass(id)`.
    pub fn select_secondary_ascend_class(&mut self, id: f64) -> Result<(), String> {
        self.edit_spec(|s, _| s.select_secondary_ascend_class(Some(id)))
    }

    /// `spec:IsClassConnected(classId)` (the walk marks `visited` on the nodes; a copy here).
    pub fn is_class_connected(&self, class_id: usize) -> bool {
        (*self.result().env.spec).clone().is_class_connected(class_id)
    }

    /// `spec:ConnectToClass(classId)`.
    pub fn connect_to_class(&mut self, class_id: usize) -> Result<bool, String> {
        self.edit_spec(|s, l| s.connect_to_class(class_id, l))
    }

    /// `(targetBaseClassId, targetAscendClassId)` of an ascendancy name over every class (PassiveTreeView.lua:449-459), `None`: no such ascendancy.
    pub fn find_ascendancy(&self, name: &str) -> Option<(usize, usize, String)> {
        let spec = &self.result().env.spec;
        for (ci, c) in spec.tree.classes.iter().enumerate() {
            for (ai, a) in c.classes.iter().enumerate() {
                if a.id.as_deref() == Some(name) {
                    return Some((ci, ai, c.name.clone()));
                }
            }
        }
        None
    }

    /// The `tree.alternate_ascendancies` key of a bloodline id (PassiveTreeView.lua:413-420).
    pub fn find_bloodline(&self, name: &str) -> Option<f64> {
        let spec = &self.result().env.spec;
        spec.tree
            .alternate_ascendancies
            .as_ref()?
            .iter()
            .find(|(_, a)| a.id.as_deref() == Some(name))
            .map(|(k, _)| *k as f64)
    }

    /// TreeTab.lua:1026-1031 the choices of `OpenMasteryPopup` for mastery `node_id`.
    pub fn mastery_choices(&self, node_id: u32) -> Vec<MasteryChoice> {
        let spec = &self.result().env.spec;
        let Some(n) = spec.node(node_id) else { return Vec::new() };
        let mut out = Vec::new();
        for &effect in &n.tree_node().mastery_effect_ids {
            // `isValueInTable(masterySelections, effect.effect)`: the node holding that effect (`pairs`: at most one node holds it)
            let assigned = spec.mastery_selections.iter().find(|(_, v)| **v == effect).map(|(k, _)| *k);
            if assigned.is_none() || assigned == Some(node_id) {
                // `effect.stats` is the table `tree.masteryEffects[effect].sd` that ProcessStats split in place (PassiveTree.lua:562)
                let label = spec
                    .tree
                    .mastery_effects
                    .get(&(effect as u64))
                    .map_or_else(String::new, |m| m.sd.join(" / "));
                out.push(MasteryChoice { label, id: effect });
            }
        }
        out
    }

    /// TreeTab.lua:1000-1017 `SaveMasteryPopup`: the selection, the allocation when the node has none (`alt_path`: the trace path), the stats.
    pub fn save_mastery(&mut self, node_id: u32, effect: f64, alt_path: Option<&[u32]>) -> Result<(), String> {
        self.edit_spec(|s, l| {
            s.mastery_selections.insert(node_id, effect);
            let i = s.node_idx(node_id).expect("mastery node");
            if s.nodes[i].alloc {
                // :1005-1008 the node's own stats: the pass over the selections does the same
                s.build_all_depends_and_paths()
            } else {
                let alt: Option<Vec<usize>> = alt_path.map(|p| p.iter().filter_map(|n| s.node_idx(*n)).collect());
                s.alloc_node_full(i, alt.as_deref(), l)
            }
        })
    }

    /// `buildMods(selectedNode)` of `ModifyNodePopup` (TreeTab.lua:851-883); `show_legacy`: `self.showLegacyTattoo`.
    pub fn tattoo_choices(&self, node_id: u32, show_legacy: bool) -> Result<Vec<TattooChoice>, String> {
        let spec = &self.result().env.spec;
        let tree = spec.tree.clone();
        let tn = spec.node(node_id).ok_or("tree node")?.tree_node();
        let node_name = tn.dn.as_str();
        let num_linked = tn.linked_id.len();
        let node_value = tn.sd.first().map_or("", String::as_str);
        let raw = pob_data::tattoo::tattoo_passives().map_err(|e| e.0)?;
        let tattoo = tree.tattoo().map_err(|e| e.0)?;
        let lua_find = |s: &str, p: &str| pob_lua::match_(s.as_bytes(), p.as_bytes(), None).ok().flatten().is_some();
        let mut out = Vec::new();
        for (key, node) in raw.nodes.iter() {
            let target = node.target_type.strip_prefix("Small ").unwrap_or(&node.target_type);
            let matches = lua_find(node_name, target)
                || (!node.target_value.is_empty() && lua_find(node_value, &node.target_value))
                || (node.target_type == "Small Attribute" && matches!(node_name, "Intelligence" | "Strength" | "Dexterity"))
                || (node.target_type == "Keystone" && tn.ty.as_str() == node.target_type);
            let legacy_ok = !node.legacy.unwrap_or(false) || show_legacy;
            if matches && (node.minimum_connected as usize) <= num_linked && legacy_ok {
                let t = &tattoo.nodes[key];
                // :861-871 `combine` needs `#node.stats == #node.sd - 1`; `node.stats` has string keys only (`#` = 0): a one-line `sd`, whose concat is itself
                let mut descriptions = t.sd.clone();
                if let Some(r) = t.reminder_text.as_ref().and_then(|r| r.first()) {
                    descriptions.push(r.clone());
                }
                out.push(TattooChoice {
                    label: format!("{}                                                {}", t.dn, t.sd.join(",")),
                    descriptions,
                    key: key.clone(),
                });
            }
        }
        // `table.sort(modGroups, a.label < b.label)` (byte order; labels of a node are distinct or the same tattoo)
        out.sort_by(|a, b| a.label.as_bytes().cmp(b.label.as_bytes()));
        Ok(out)
    }

    /// TreeTab.lua:884-893 `addModifier`.
    pub fn add_tattoo(&mut self, node_id: u32, key: &str) -> Result<(), String> {
        let tree = self.result().env.spec.tree.clone();
        let t = tree.tattoo().map_err(|e| e.0)?.nodes.get(key).ok_or("tattoo")?.clone();
        self.edit_spec(|s, _| {
            let i = s.node_idx(node_id).expect("node");
            s.set_tattoo(i, &t)
        })
    }

    /// TreeTab.lua:350-356 `RemoveTattooFromNode`.
    pub fn remove_tattoo(&mut self, node_id: u32) -> Result<(), String> {
        self.edit_spec(|s, _| {
            let i = s.node_idx(node_id).expect("node");
            s.remove_tattoo(i)
        })
    }

    /// TreeTab.lua:143-148 `Remove All Tattoos`.
    pub fn remove_all_tattoos(&mut self) -> Result<(), String> {
        let ids: Vec<u32> = self
            .result()
            .env
            .spec
            .hash_overrides
            .iter()
            .filter(|(_, t)| t.is_tattoo)
            .map(|(i, _)| *i)
            .collect();
        for id in ids {
            self.remove_tattoo(id)?;
        }
        Ok(())
    }

    /// `getTattooCount` (TreeTab.lua:954-986): the count and the `(mult, combined)` lines of the tooltip.
    pub fn tattoo_count(&self) -> (usize, Vec<(usize, String)>) {
        let spec = &self.result().env.spec;
        let mut count = 0;
        let mut lines: indexmap::IndexMap<String, usize> = indexmap::IndexMap::new();
        for t in spec.hash_overrides.values() {
            if t.is_tattoo && !t.dn.contains("Runegraft") {
                let mut combined = String::new();
                for l in &t.sd {
                    if !(l.contains("Limited") || l.contains("Requires")) {
                        combined.push(' ');
                        combined.push_str(l);
                    }
                }
                *lines.entry(combined).or_insert(0) += 1;
                count += 1;
            }
        }
        (count, lines.into_iter().map(|(l, m)| (m, l)).collect())
    }

    /// `EncodeURL(prefix)`.
    pub fn encode_tree_url(&self, prefix: &str) -> Result<String, String> {
        let spec = &self.result().env.spec;
        let mut ids = spec.alloc_table_pairs();
        ids.sort_unstable();
        crate::save::encode_url(spec, &ids, prefix)
    }

    /// `DecodeURL(url)` (PassiveSpec.lua:498-541) on a new `PassiveSpec(build, version)`: the new spec's model or the error text. Upstream's decode does
    /// not rebuild the paths (the spec's next rebuild does); the model's load does -- the same allocation.
    pub fn decode_tree_url(url: &str, version: &str, title: &str) -> Result<Result<SpecModel, String>, String> {
        let tree = pob_tree::tree::load_passive_tree_mode(version, ParseMode::Full).map_err(|e| e.0)?;
        let blank = PassiveSpec::new(tree.clone()).map_err(|e| e.0)?;
        // `url:gsub("^.+/",""):gsub("-","+"):gsub("_","/")`
        let tail = url.rfind('/').map_or(url, |p| &url[p + 1..]);
        let tail = tail.replace('-', "+").replace('_', "/");
        let Some(b) = base64_decode(&tail).filter(|b| b.len() >= 6) else {
            return Ok(Err("Invalid tree link (unrecognised format)".into()));
        };
        let ver = u32::from(b[0]) * 16_777_216 + u32::from(b[1]) * 65536 + u32::from(b[2]) * 256 + u32::from(b[3]);
        if ver > 6 {
            return Ok(Err(format!("Invalid tree link (unknown version number '{ver}')")));
        }
        let class_id = usize::from(b[4]);
        let ascendancy_ids = if ver >= 4 { b[5] } else { 0 };
        if class_id >= tree.classes.len() {
            return Ok(Err(format!("Invalid tree link (bad class ID '{class_id}')")));
        }
        let mut nodes = Vec::new();
        let mut alloc = |nodes_b: &[u8], cluster: bool| {
            for p in nodes_b.as_chunks::<2>().0 {
                let id = u32::from(p[0]) * 256 + u32::from(p[1]) + if cluster { 65536 } else { 0 };
                if blank.node_idx(id).is_some() {
                    nodes.push(f64::from(id));
                }
            }
        };
        // 1-based positions of the Lua
        let nodes_start = if ver >= 4 { 8 } else { 7 };
        let nodes_end = if ver >= 5 {
            7 + usize::from(*b.get(6).unwrap_or(&0)) * 2
        } else {
            b.len()
        };
        let sub = |a: usize, e: usize| -> &[u8] {
            let e = e.min(b.len());
            if a > e { &[] } else { &b[a - 1..e] }
        };
        alloc(sub(nodes_start, nodes_end), false);
        let mut masteries = NumMap::default();
        if ver >= 5 {
            let cluster_start = nodes_end + 1;
            let cluster_end = cluster_start + usize::from(*b.get(cluster_start - 1).unwrap_or(&0)) * 2;
            alloc(sub(cluster_start + 1, cluster_end), true);
            if ver >= 6 {
                let mastery_start = cluster_end + 1;
                let mastery_end = mastery_start + usize::from(*b.get(mastery_start - 1).unwrap_or(&0)) * 4;
                for p in sub(mastery_start + 1, mastery_end).as_chunks::<4>().0 {
                    let effect = u32::from(p[0]) * 256 + u32::from(p[1]);
                    let id = u32::from(p[2]) * 256 + u32::from(p[3]);
                    if tree.mastery_effects.contains_key(&u64::from(effect)) {
                        masteries.set(f64::from(id), f64::from(effect));
                    } else {
                        // :387-389 no effect on this tree: not allocated
                        nodes.retain(|n| *n != f64::from(id));
                    }
                }
            }
        }
        Ok(Ok(SpecModel {
            title: Some(title.to_string()),
            tree_version: version.to_string(),
            cluster_hash_format_version: 2.0,
            class_id: class_id as f64,
            ascend_class_id: f64::from(ascendancy_ids & 3),
            secondary_ascend_class_id: Some(f64::from((ascendancy_ids & 12) >> 2)),
            nodes,
            mastery_effects: masteries,
            jewels: NumMap::default(),
            url: None,
            overrides: Vec::new(),
        }))
    }

    /// `url:gsub("^.+/",""):gsub("-","+"):gsub("_","/")` + the check of `DecodePoePlannerURL` (PassiveSpec.lua:404-407).
    fn poe_planner_bytes(url: &str) -> Option<Vec<u8>> {
        let tail = match url.rfind('/') {
            Some(p) if p > 0 => &url[p + 1..],
            _ => url,
        };
        base64_decode(&tail.replace('-', "+").replace('_', "/")).filter(|b| b.len() >= 15)
    }

    /// `DecodePoePlannerURL(url, true)` (PassiveSpec.lua:395-451): the tree version or the error text; `versions`: `poePlannerVersions` (GameVersions.lua:229).
    pub fn poe_planner_version(url: &str, versions: &dyn Fn(u32) -> Option<String>) -> Result<String, String> {
        let Some(b) = Self::poe_planner_bytes(url) else {
            return Err("Invalid tree link (unrecognised format).".into());
        };
        // byteToInt(b, 7): little endian, 1-based
        versions(u32::from(b[6]) + u32::from(b[7]) * 256).ok_or_else(|| "Invalid tree version found in link.".to_string())
    }

    /// `DecodePoePlannerURL(url, false)` (PassiveSpec.lua:453-495) on a new `PassiveSpec(build, version)`. The link was proven by [`Self::poe_planner_version`]. What
    /// reads past the end, or a class the tree does not have, is a Lua error upstream: `Err` here.
    pub fn decode_poe_planner_url(url: &str, version: &str, title: &str) -> Result<SpecModel, String> {
        let tree = pob_tree::tree::load_passive_tree_mode(version, ParseMode::Full).map_err(|e| e.0)?;
        let blank = PassiveSpec::new(tree.clone()).map_err(|e| e.0)?;
        let b = Self::poe_planner_bytes(url).ok_or("Invalid tree link (unrecognised format).")?;
        let arith = || "attempt to perform arithmetic on a nil value".to_string();
        // `byteToInt(bytes, start)`, 1-based
        let int_at = |start: usize| -> Result<usize, String> {
            let lo = *b.get(start - 1).ok_or_else(arith)?;
            let hi = *b.get(start).ok_or_else(arith)?;
            Ok(usize::from(lo) + usize::from(hi) * 256)
        };
        // `b:sub(a, e)`, 1-based and clamped
        let sub = |a: usize, e: usize| -> &[u8] {
            let e = e.min(b.len());
            if a > e { &[] } else { &b[a - 1..e] }
        };
        let class_id = usize::from(b[8]);
        let ascend_class_id = f64::from(b[9]);
        if class_id >= tree.classes.len() {
            return Err("attempt to index local 'class' (a nil value)".to_string());
        }
        let mut nodes: Vec<f64> = Vec::new();
        let mut alloc = |list: &[u8], cluster: bool| {
            for p in list.as_chunks::<2>().0 {
                let id = u32::from(p[0]) + u32::from(p[1]) * 256 + if cluster { 65536 } else { 0 };
                if blank.node_idx(id).is_some() && !nodes.contains(&f64::from(id)) {
                    nodes.push(f64::from(id));
                }
            }
        };
        let idx = 12;
        let nodes_end = idx + 2 + int_at(idx)? * 2;
        alloc(sub(idx + 2, nodes_end - 1), false);
        let idx = nodes_end;
        let cluster_end = idx + 2 + int_at(idx)? * 2;
        alloc(sub(idx + 2, cluster_end - 1), true);
        let idx = cluster_end;
        let ascendancy_end = idx + 2 + int_at(idx)? * 2;
        alloc(sub(idx + 2, ascendancy_end - 1), false);
        let idx = ascendancy_end;
        let mastery_end = idx + 2 + int_at(idx)? * 4;
        // AllocateMasteryEffects(masteryEffects, "little") (:351-391): poeplanner swaps effectId and id
        let mut masteries = NumMap::default();
        for p in sub(idx + 2, mastery_end - 1).as_chunks::<4>().0 {
            let id = u32::from(p[0]) + u32::from(p[1]) * 256;
            let effect = u32::from(p[2]) + u32::from(p[3]) * 256;
            if blank.node_idx(id).is_none() {
                return Err("attempt to index a nil value".to_string());
            }
            if tree.mastery_effects.contains_key(&u64::from(effect)) {
                if !nodes.contains(&f64::from(id)) {
                    nodes.push(f64::from(id));
                }
                masteries.set(f64::from(id), f64::from(effect));
            } else {
                // :387-389 no effect on this tree: not allocated
                nodes.retain(|n| *n != f64::from(id));
            }
        }
        Ok(SpecModel {
            title: Some(title.to_string()),
            tree_version: version.to_string(),
            cluster_hash_format_version: 2.0,
            class_id: class_id as f64,
            ascend_class_id,
            secondary_ascend_class_id: None,
            nodes,
            mastery_effects: masteries,
            jewels: NumMap::default(),
            url: None,
            overrides: Vec::new(),
        })
    }

    /// The node-type predicate of `PassiveTreeView.lua:530-534`: right click opens `ModifyNodePopup`.
    pub fn node_can_tattoo(&self, node_id: u32) -> bool {
        let spec = &self.result().env.spec;
        let Some(n) = spec.node(node_id) else { return false };
        // `expansionSkill`: every node BuildSubgraph makes (PassiveSpec.lua:2107, :2240, :2289)
        if n.is_subgraph_node() {
            return false;
        }
        let first = n.sd().first().map_or("", String::as_str);
        n.is_tattoo()
            || (n.ty() == NodeType::Normal && matches!(n.dn(), "Strength" | "Dexterity" | "Intelligence"))
            || (n.ty() == NodeType::Notable
                && !n.sd().is_empty()
                && (first.contains("+30 to Dexterity") || first.contains("+30 to Strength") || first.contains("+30 to Intelligence")))
            || n.ty() == NodeType::Keystone
    }

    /// The background walk of `CalcsTab:PowerBuilder` over a clone of the model (own sessions on `threads` threads): started on the UI thread, finished on its own.
    pub fn power_start(
        &self,
        stat: Option<&PowerStat>,
        max_depth: Option<f64>,
        threads: usize,
    ) -> Result<(Option<PowerResult>, Vec<ReportEntry>), String> {
        let result =
            pob_calc::power::build_power_threads(self.model(), pob_calc::CalculateOptions::default(), stat, max_depth, threads, None)?;
        let Some(result) = result else { return Ok((None, Vec::new())) };
        let report = self.power_report(stat, &result)?;
        Ok((Some(result), report))
    }

    /// TreeTab.lua:1060-1167 `BuildPowerReportList(currentStat)`.
    pub fn power_report(&self, stat: Option<&PowerStat>, result: &PowerResult) -> Result<Vec<ReportEntry>, String> {
        let ds = stat
            .and_then(|p| p.stat.as_deref())
            .and_then(|st| crate::stats::DISPLAY_STATS.iter().find(|d| d.stat == Some(st)));
        let fmt = match ds {
            None => None,
            Some(d) => Some(pob_calc::power::DisplayStatFmt {
                fmt: d
                    .fmt
                    .ok_or("Classes/TreeTab.lua:1092: attempt to concatenate field 'fmt' (a nil value)")?
                    .to_owned(),
                pc: d.pc,
                mod_: d.mod_,
                lower_is_better: d.lower_is_better,
            }),
        };
        let env = &self.result().env;
        let s = self.settings();
        let nf = pob_calc::power::NumberFormat {
            show_thousands_separators: s.show_thousands_separators,
            thousands_separator: s.thousands_separator.clone(),
            decimal_separator: s.decimal_separator.clone(),
        };
        Ok(pob_calc::power::build_power_report_list(
            &env.spec,
            &env.granted_passives,
            result,
            stat,
            fmt.as_ref(),
            &nf,
        ))
    }

    /// TreeTab.lua:1951-1976 `generateFallbackWeights(nodes, powerStat)`: `weight1`, `weight2`, .. of each node. `calcFunc({ addNodes = { [node] = true } })` over the build's calculator.
    pub fn fallback_weights(&self, nodes: &[FallbackNode], stat: &PowerStat) -> Result<Vec<Vec<f64>>, String> {
        let calc = self.misc_calculator()?;
        let spec = calc.session().shared().map_err(|e| e.to_string())?.spec.clone();
        let view_tree = self.view_mode_is_tree.get();
        let power = |run: &pob_calc::session::RunRef| pob_calc::power::get_from_output(&run.borrow().state.player.output, stat);
        let base_power = power(&calc.base);
        let mut out = Vec::with_capacity(nodes.len());
        for node in nodes {
            let mut weights = Vec::with_capacity(node.lines.len());
            for line in &node.lines {
                let ov = pob_calc::env::CalcOverride {
                    add_nodes: Some(vec![pob_calc::env::OverrideNode::Other(Rc::new(
                        spec.mod_list_node(line.mods.clone()),
                    ))]),
                    ..pob_calc::env::CalcOverride::default()
                };
                let run = calc.calc(&Rc::new(ov), None, view_tree).map_err(|e| e.0)?;
                let node_power = power(&run);
                // avoid infinity
                weights.push(if base_power == 0.0 {
                    0.0
                } else {
                    // normalize with absolute base power so that the result isn't negative
                    let gain = (node_power - base_power) / base_power.abs();
                    gain / line.divisor.or(node.divisor).unwrap_or(1.0)
                });
            }
            out.push(weights);
        }
        Ok(out)
    }

    /// `calcFunc({ spec = spec })` + `AddStatComparesToTooltip(tooltip, calcBase, output, "^7Switching to this tree will give you:")` (TreeTab.lua:67-71).
    pub fn spec_stat_compare(&self, i: usize) -> Result<Vec<String>, String> {
        let spec = Rc::new(self.load_spec(i)?);
        self.compare_override(
            pob_calc::env::CalcOverride {
                spec: Some(spec),
                ..pob_calc::env::CalcOverride::default()
            },
            "^7Switching to this tree will give you:",
        )
    }

    /// TreeTab.lua:72-86: `(respec, respecAscendancy, goldCost)` of switching to `specList[i]` (same class only).
    pub fn respec_counts(&self, i: usize) -> Result<(u32, u32, f64), String> {
        let other = self.load_spec(i)?;
        let cur = &self.result().env.spec;
        if other.cur_class_id != cur.cur_class_id {
            return Ok((0, 0, 0.0));
        }
        let (mut r, mut a) = (0, 0);
        for n in cur.alloc_nodes() {
            let in_cluster_map = cur.tree.cluster_node_map.contains_key(n.dn());
            if !n.ty().is_start()
                && (!in_cluster_map || n.ty() == NodeType::Keystone || n.tree_node().is_jewel_socket)
                && n.id < 65536
                && !other.alloc_nodes.contains_key(&n.id)
            {
                if n.ascendancy_name().is_some() {
                    a += 1;
                } else {
                    r += 1;
                }
            }
        }
        Ok((r, a, self.gold_respec_price()))
    }
}

/// One `nodes[i]` of `setupFallbackWeights` (TreeTab.lua:2012-2104): `lines` = `newNode.node or { newNode }`.
#[derive(Clone, Debug, Default)]
pub struct FallbackNode {
    pub divisor: Option<f64>,
    pub lines: Vec<FallbackLine>,
}

/// `modList` and `divisor` of a `nodeLines[i]`.
#[derive(Clone, Debug, Default)]
pub struct FallbackLine {
    pub mods: Vec<Rc<pob_mods::Mod>>,
    pub divisor: Option<f64>,
}

/// The background part of the power walk: over a clone of the model, on its own thread.
pub fn power_thread(
    model: &pob_build::BuildModel,
    live: &pob_tree::LiveSpecPaths,
    stat: Option<&PowerStat>,
    max_depth: Option<f64>,
    threads: usize,
    progress: &pob_calc::power::PowerProgress,
) -> Result<Option<PowerResult>, String> {
    pob_calc::power::build_power_walk(
        model,
        pob_calc::CalculateOptions::default(),
        stat,
        max_depth,
        pob_calc::power::PowerWalk {
            threads,
            cancel: None,
            live_paths: Some(live),
            progress: Some(progress),
        },
    )
}

impl Engine {
    /// `BuildPowerReportList`'s `t_sort` (TreeTab.lua:1156-1164) over the input it really has: `pairs(spec.nodes)`, then `pairs(tree.clusterNodeMap)` (`order`: the sample of a process of
    /// the original, R13/R27; without it: spec order and map order, one of the orders the original can have), each mastery's effects in `ipairs` order. `report` comes sorted stably
    /// (pob-calc), so ties are re-derived here with LuaJIT's unstable sort (L17).
    pub fn order_power_report(&self, report: &mut Vec<ReportEntry>, stat: Option<&PowerStat>, order: Option<&(Vec<u32>, Vec<String>)>) {
        let spec = &self.result().env.spec;
        let tree = &spec.tree;
        let node_rank: std::collections::HashMap<u32, usize> = match order {
            Some((nodes, _)) => nodes.iter().enumerate().map(|(i, id)| (*id, i)).collect(),
            None => spec.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect(),
        };
        let cluster_rank: std::collections::HashMap<&str, usize> = match order {
            Some((_, names)) => names.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect(),
            None => tree.cluster_node_map.keys().enumerate().map(|(i, n)| (n.as_str(), i)).collect(),
        };
        let cluster_dn: std::collections::HashMap<u32, &str> = tree
            .cluster_node_map
            .iter()
            .map(|(name, &ti)| (tree.nodes[ti].id, name.as_str()))
            .collect();
        let n_nodes = node_rank.len();
        let effect_rank = |r: &ReportEntry| -> usize {
            if r.ty != NodeType::Mastery {
                return 0;
            }
            let Some(node) = spec.node(r.id) else { return 0 };
            node.tree_node()
                .mastery_effect_ids
                .iter()
                .position(|e| {
                    pob_tree::tree::as_id(*e)
                        .and_then(|e| tree.mastery_effects.get(&u64::from(e)))
                        .is_some_and(|me| r.name.ends_with(&me.sd.join(" / ")))
                })
                .unwrap_or(usize::MAX)
        };
        let mut keyed: Vec<((usize, usize), ReportEntry)> = report
            .drain(..)
            .map(|r| {
                let k = match &r.path_dist {
                    pob_calc::power::ReportDist::Str("Cluster") => {
                        let rank = cluster_dn
                            .get(&r.id)
                            .and_then(|n| cluster_rank.get(n))
                            .copied()
                            .unwrap_or(usize::MAX);
                        (n_nodes + rank.min(usize::MAX - n_nodes), 0)
                    }
                    _ => (node_rank.get(&r.id).copied().unwrap_or(n_nodes), effect_rank(&r)),
                };
                (k, r)
            })
            .collect();
        keyed.sort_by_key(|(k, _)| *k);
        let mut input: Vec<ReportEntry> = keyed.into_iter().map(|(_, r)| r).collect();
        let lower = stat
            .and_then(|p| p.stat.as_deref())
            .and_then(|st| crate::stats::DISPLAY_STATS.iter().find(|d| d.stat == Some(st)))
            .is_some_and(|d| d.lower_is_better);
        crate::lua::lua_sort(&mut input, &mut |a: &ReportEntry, b: &ReportEntry| {
            if lower { a.power < b.power } else { a.power > b.power }
        });
        *report = input;
    }

    /// `itemsTab:AddItem(new("Item")(text), true)` (ItemsTab.lua:1686-1699) with no slot: the first free id, `itemOrderList`, what `ItemsTab:Save` writes.
    pub fn add_item_unequipped(&mut self, text: &str) -> Result<f64, String> {
        let raw = pob_build::sanitise_text(text);
        let mut item = pob_items::Item::parse(
            &raw,
            &pob_items::ItemOptions {
                parse_mode: ParseMode::Full,
                ..Default::default()
            },
        );
        if item.base.is_none() {
            return Err(format!("Classes/Item.lua: the item text is invalid: {text}"));
        }
        item.normalise_quality();
        let mut id = 1.0;
        while self.result().env.item(id).is_some() {
            id += 1.0;
        }
        let raw = item.build_raw();
        let mod_ranges = crate::save::mod_ranges(&item)
            .into_iter()
            .map(|(n, range)| pob_build::ModRangeModel { id: n as f64, range })
            .collect();
        self.items_table_set(id);
        let m = self.model_mut();
        m.items.push(pob_build::ItemModel {
            id,
            variant: item.variant,
            variant_alt: item.variant_alt[0],
            variant_alt2: item.variant_alt[1],
            variant_alt3: item.variant_alt[2],
            variant_alt4: item.variant_alt[3],
            variant_alt5: item.variant_alt[4],
            raw,
            mod_ranges,
        });
        m.item_order.push(id);
        self.recalc()?;
        Ok(id)
    }

    /// `build.timelessData`'s saved fields (Build.lua:81 defaults on first need).
    pub fn timeless_mut(&mut self) -> &mut pob_build::TimelessDataModel {
        self.model_mut().timeless_data.get_or_insert_with(|| pob_build::TimelessDataModel {
            jewel_type_id: None,
            conqueror_type_id: None,
            devotion_variant1: 1.0,
            devotion_variant2: 1.0,
            jewel_socket_id: None,
            fallback_weight_mode_idx: None,
            socket_filter: false,
            socket_allocate: false,
            socket_filter_distance: 0.0,
            search_list: Some(String::new()),
            search_list_fallback: Some(String::new()),
        })
    }

    /// `calcsTab.mainEnv.grantedPassives`.
    pub fn granted_passives_rc(&self) -> indexmap::IndexMap<u32, bool> {
        self.result().env.granted_passives.clone()
    }

    /// What [`power_thread`] must not rebuild from the model: `node.path` / `node.depends` / `node.pathDist` of the live `build.spec`.
    pub fn live_spec_paths(&self) -> pob_tree::LiveSpecPaths {
        pob_tree::LiveSpecPaths::of(&self.result().env.spec)
    }
}
