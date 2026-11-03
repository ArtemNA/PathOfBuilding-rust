//! `Classes/GemSelectControl.lua`: the gem combobox. `self.gems` holds the ids without `"Default:"` (only the callback strips it upstream); `self.dpsBuilder`, a
//! coroutine upstream, is the state in [`DpsBuilder`], stepped in `Draw` with the same 50 ms budget; a list entry is a gem id or `""` (`<No matches>`).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use pob_data::gems::GemEntry;
use pob_data::skills::GrantedEffect;
use pob_engine::skills_tab::GemEdit;
use pob_gfx::{Frame, Gfx};

use super::skills::{GroupRef, SkillsTab};
use crate::colors::color_code;
use crate::common::draw_check_mark;
use crate::control::{Anchor, Control, ControlBase, ControlRef, Prop, Rect, RectProps, anchor_weak};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::scroll_bar::ScrollBarControl;
use crate::gem_tooltip::{GemInst, add_gem_tooltip};
use crate::sg;

const TOOL_TIP_TEXT: &str = "Prefix tag searches with a colon and exclude tags with a dash. e.g. :fire:lightning:-cold:area";
const IMBUED_TOOLTIP_TEXT: &str =
    "\"Socketed in\" item must be set in order to add an imbued support.\nOnly one imbued support is allowed per item.";

/// `gemChangeFunc(gemId, addUndo, focusLost, bufMatchesGem)`.
pub type GemChangeFunc = Rc<dyn Fn(&mut Gfx, Option<String>, bool, bool, bool)>;

pub fn gem_of(id: &str) -> Option<&'static GemEntry> {
    pob_data::gems::gems().gems.0.get(id)
}

fn effect_of<'a>(skills: &'a pob_data::skills::Skills, gem: &GemEntry) -> &'a GrantedEffect {
    skills.get(&gem.granted_effect_id).expect("gem's granted effect")
}

fn lua_match(s: &str, pattern: &str) -> bool {
    pob_lua::match_(s.as_bytes(), pattern.as_bytes(), None)
        .unwrap_or_else(|e| panic!("Classes/GemSelectControl.lua:193: {e:?}"))
        .is_some()
}

/// `self.sortCache`.
pub struct SortCache {
    consider_gem_type: String,
    show_legacy_gems: bool,
    socket_group: Option<GroupRef>,
    gem_instance: Option<String>,
    output_revision: u64,
    default_level: String,
    default_quality: f64,
    character_level: f64,
    sort_type: String,
    pub can_support: HashSet<String>,
    pub dps: HashMap<String, f64>,
    pub dps_color: HashMap<String, &'static str>,
    base_dps: f64,
    dps_field: String,
    pending: Option<Vec<String>>,
}

/// The coroutine's locals between `yield`s.
struct DpsBuilder {
    next: usize,
    start: f64,
}

pub struct GemSelectControl {
    pub edit: Rc<EditControl>,
    me: Weak<Self>,
    tab: Weak<SkillsTab>,
    pub scroll_bar: Rc<ScrollBarControl>,
    pub index: Cell<usize>,
    pub gems: RefCell<HashSet<String>>,
    pub list: RefCell<Vec<String>>,
    pub gem_change_func: GemChangeFunc,
    force_tooltip: bool,
    pub imbued_select: bool,
    pub mode: RefCell<String>,
    pub dropped: Cell<bool>,
    pub sel_index: Cell<i64>,
    pub hover_sel: Cell<Option<usize>>,
    pub no_matches: Cell<bool>,
    pub search_str: RefCell<String>,
    pub gem_id: RefCell<Option<String>>,
    gem_name: RefCell<String>,
    initial_buf: RefCell<String>,
    pub sort_gems_by: RefCell<Option<&'static str>>,
    last_sort_gems_by: RefCell<Option<&'static str>>,
    pub sort_cache: RefCell<Option<SortCache>>,
    dps_build_flag: Cell<bool>,
    dps_builder: RefCell<Option<DpsBuilder>>,
    pub sort_percentage: RefCell<String>,
    last_hover_sel: Cell<Option<usize>>,
    hover_frame_count: Cell<i64>,
    /// `AddStatComparesToTooltip` of the hovered gem: upstream recalculates it every frame with the same result.
    #[allow(clippy::type_complexity)]
    hover_compare: RefCell<Option<((String, u64, usize, usize, String, String), Vec<String>)>>,
}

impl GemSelectControl {
    /// :28-68
    pub fn new(
        g: &Gfx,
        anchor: Option<Anchor>,
        rect: RectProps,
        tab: Weak<SkillsTab>,
        index: usize,
        change: GemChangeFunc,
        force_tooltip: bool,
        imbued: bool,
    ) -> Rc<Self> {
        let edit = EditControl::new(
            g,
            anchor,
            rect,
            EditArgs {
                filter: Some("^ %a':-".into()),
                ..Default::default()
            },
        );
        let height = edit.base.height.raw().expect("height");
        let wedit: Weak<dyn Control> = Rc::downgrade(&(edit.clone() as ControlRef));
        let scroll_bar = ScrollBarControl::new(
            anchor_weak("TOPRIGHT", wedit, "TOPRIGHT"),
            [(-1.0).into(), Prop::Val(0.0), 18.0.into(), 0.0.into()],
            Some((height - 4.0) * 4.0),
            None,
            false,
        );
        edit.host.add("scrollBar", scroll_bar.clone());
        let c = Rc::new_cyclic(|me: &Weak<Self>| Self {
            edit: edit.clone(),
            me: me.clone(),
            tab,
            scroll_bar: scroll_bar.clone(),
            index: Cell::new(index),
            gems: RefCell::new(HashSet::new()),
            list: RefCell::new(Vec::new()),
            gem_change_func: change,
            force_tooltip,
            imbued_select: imbued,
            mode: RefCell::new(String::new()),
            dropped: Cell::new(false),
            sel_index: Cell::new(0),
            hover_sel: Cell::new(None),
            no_matches: Cell::new(false),
            search_str: RefCell::new(String::new()),
            gem_id: RefCell::new(None),
            gem_name: RefCell::new(String::new()),
            initial_buf: RefCell::new(String::new()),
            sort_gems_by: RefCell::new(None),
            last_sort_gems_by: RefCell::new(None),
            sort_cache: RefCell::new(None),
            dps_build_flag: Cell::new(false),
            dps_builder: RefCell::new(None),
            sort_percentage: RefCell::new(String::new()),
            last_hover_sel: Cell::new(None),
            hover_frame_count: Cell::new(0),
            hover_compare: RefCell::new(None),
        });
        let w = Rc::downgrade(&c);
        scroll_bar
            .base
            .y
            .set_fn(move |g| w.upgrade().expect("gem select").edit.get_size(g).1 + 1.0);
        let w = Rc::downgrade(&c);
        scroll_bar.base.height.set_fn(move |_| {
            let s = w.upgrade().expect("gem select");
            (height - 4.0) * s.list.borrow().len().min(15) as f64 + 2.0
        });
        let w = Rc::downgrade(&c);
        scroll_bar.base.shown.set_fn(move |g| {
            let s = w.upgrade().expect("gem select");
            s.dropped.get() && s.scroll_bar.base.enabled.get(g)
        });
        let w = Rc::downgrade(&c);
        *edit.change_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, _buf: &str, _| {
            let s = w.upgrade().expect("gem select");
            if !s.dropped.get() {
                s.dropped.set(true);
                s.update_sort_cache(g);
            }
            s.sel_index.set(0);
            let buf = s.edit.buf.borrow().clone();
            s.build_list(&buf);
            s.update_gem(g, false, false, false);
        }));
        c.populate_gem_list();
        c
    }

    fn tab(&self) -> Rc<SkillsTab> {
        self.tab.upgrade().expect("skillsTab")
    }

    /// :116-146
    pub fn populate_gem_list(&self) {
        let tab = self.tab();
        let (show, match_level, character_level, legacy) = tab.read(|e| {
            let m = &e.model().skills;
            (
                m.show_support_gem_types.clone(),
                e.skills_tab().default_gem_level == "characterLevel",
                e.model().level,
                m.show_legacy_gems.unwrap_or(false),
            )
        });
        let (show_all, show_exc, show_norm) = (show == "ALL", show == "EXCEPTIONAL", show == "NORMAL");
        let skills = pob_data::skills::skills();
        let by = *self.sort_gems_by.borrow();
        let mut out = self.gems.borrow_mut();
        out.clear();
        for gem in pob_data::gems::gems().gems.0.values() {
            let ge = effect_of(&skills, gem);
            if ge.hide_from_gem_list || !by.is_none_or(|t| gem.tags.get(t).copied() == Some(true)) {
                continue;
            }
            let req = ge.level(1.0).and_then(|l| l.level_requirement).unwrap_or(1.0);
            if character_level >= req || !match_level {
                let legacy_awakened = ge.legacy && ge.plus_version_of.is_some();
                let exceptional = gem.tag_string.contains("Exceptional");
                if self.imbued_select {
                    let sec_ok = gem
                        .secondary_granted_effect_id
                        .as_ref()
                        .is_none_or(|s| skills.get(s).is_some_and(|x| x.support));
                    if ge.support && !exceptional && !legacy_awakened && sec_ok {
                        out.insert(gem.id.clone());
                    }
                } else if (show_exc || show_all) && (legacy_awakened || exceptional) {
                    if legacy || !ge.legacy {
                        out.insert(gem.id.clone());
                    }
                } else if show_norm || show_all {
                    out.insert(gem.id.clone());
                }
            }
        }
    }

    /// :148-163
    fn filter_support(&self, gem: &GemEntry, ge: &GrantedEffect, show: &str, legacy: bool) -> bool {
        let legacy_awakened = ge.legacy && ge.plus_version_of.is_some();
        if ge.legacy && !legacy {
            return false;
        }
        if self.imbued_select {
            return self.sort_cache.borrow().as_ref().is_some_and(|c| c.can_support.contains(&gem.id));
        }
        let exceptional = legacy_awakened || gem.tag_string.contains("Exceptional");
        !ge.support || show == "ALL" || (show == "NORMAL" && !exceptional) || (show == "EXCEPTIONAL" && exceptional)
    }

    /// :165-264
    #[allow(clippy::too_many_lines)]
    pub fn build_list(&self, buf: &str) {
        let tab = self.tab();
        let (show, legacy) = tab.read(|e| {
            let m = &e.model().skills;
            (m.show_support_gem_types.clone(), m.show_legacy_gems.unwrap_or(false))
        });
        self.scroll_bar.offset.set(0.0);
        let search = format!("{buf}{}", self.mode.borrow());
        *self.search_str.borrow_mut() = search.clone();
        self.mode.borrow_mut().clear();
        let skills = pob_data::skills::skills();
        let gems: Vec<&'static GemEntry> = self.gems.borrow().iter().filter_map(|id| gem_of(id)).collect();
        let mut list: Vec<String> = Vec::new();
        if search.is_empty() {
            for gem in &gems {
                if self.filter_support(gem, effect_of(&skills, gem), &show, legacy) {
                    list.push(gem.id.clone());
                }
            }
            self.sort_gem_list(&mut list);
        } else {
            let mut added: HashSet<String> = HashSet::new();
            // `string:split(":")` (Common.lua:1001-1010)
            let mut tags: Vec<String> = Vec::new();
            if search.starts_with(':') {
                tags.push(String::new());
            }
            tags.extend(search.split(':').filter(|x| !x.is_empty()).map(str::to_string));
            let mut search_term = tags
                .first()
                .cloned()
                .expect("Classes/GemSelectControl.lua:183: attempt to index local 'searchTerm' (a nil value)");
            tags.remove(0);
            let lower = search_term.to_ascii_lowercase();
            let mut abbreviated = String::from("^");
            for c in lower.chars() {
                if c.is_ascii_alphabetic() {
                    abbreviated.push(' ');
                    abbreviated.push(c);
                    abbreviated.push_str("%l+");
                } else {
                    abbreviated.push(c);
                }
            }
            abbreviated.push('$');
            let patterns = [format!("^ {lower}$"), abbreviated, format!("^ {lower}"), lower.clone()];
            for pattern in &patterns {
                let mut matches: Vec<String> = Vec::new();
                for gem in &gems {
                    let ge = effect_of(&skills, gem);
                    let filter = self.filter_support(gem, ge, &show, legacy);
                    let name = format!(" {}", gem.name.to_ascii_lowercase());
                    if filter && !added.contains(&gem.id) && lua_match(&name, pattern) {
                        let mut add = true;
                        for tag in &tags {
                            let mut tag_name: String = tag.split_ascii_whitespace().collect::<String>().to_ascii_lowercase();
                            let negate = tag_name.starts_with('-');
                            if negate {
                                tag_name.remove(0);
                            }
                            let tag_name = match tag_name.as_str() {
                                "active" => "grants_active_skill",
                                "int" => "intelligence",
                                "str" => "strength",
                                "dex" => "dexterity",
                                "aoe" => "area",
                                o => o,
                            };
                            let has = gem.tags.get(tag_name).copied() == Some(true);
                            if has == negate {
                                add = false;
                            }
                        }
                        if add {
                            matches.push(gem.id.clone());
                            added.insert(gem.id.clone());
                        }
                    } else {
                        match search_term.as_str() {
                            "active" => search_term = "grants_active_skill".into(),
                            "int" => search_term = "intelligence".into(),
                            "str" => search_term = "strength".into(),
                            "dex" => search_term = "dexterity".into(),
                            _ => {}
                        }
                        if filter
                            && !added.contains(&gem.id)
                            && gem.tags.get(search_term.to_ascii_lowercase().as_str()).copied() == Some(true)
                        {
                            matches.push(gem.id.clone());
                            added.insert(gem.id.clone());
                        }
                    }
                }
                self.sort_gem_list(&mut matches);
                list.extend(matches);
            }
        }
        if list.is_empty() {
            list.push(String::new());
            self.no_matches.set(true);
        } else {
            self.no_matches.set(false);
        }
        *self.list.borrow_mut() = list;
    }

    /// :266-373
    pub fn update_sort_cache(&self, g: &Gfx) {
        let _ = g;
        let tab = self.tab();
        let display = tab.display_token();
        let idx = self.index.get();
        let (show, legacy, default_level, default_quality, character_level, field, revision, gem_instance, group) = tab.read(|e| {
            let m = &e.model().skills;
            let group = tab.display_index();
            (
                m.show_support_gem_types.clone(),
                m.show_legacy_gems.unwrap_or(false),
                e.skills_tab().default_gem_level.clone(),
                e.skills_tab().default_gem_quality,
                e.model().level,
                m.sort_gems_by_dps_field.clone(),
                e.output_revision(),
                group.and_then(|g| e.gem_id_at(g, idx)),
                group,
            )
        });
        let same_by = *self.sort_gems_by.borrow() == *self.last_sort_gems_by.borrow();
        {
            let c = self.sort_cache.borrow();
            if same_by
                && let Some(c) = c.as_ref()
                && c.socket_group == display
                && c.gem_instance == gem_instance
                && c.output_revision == revision
                && c.default_level == default_level
                && (c.character_level == character_level || default_level != "characterLevel")
                && c.default_quality == default_quality
                && c.sort_type == field
                && c.consider_gem_type == show
                && c.show_legacy_gems == legacy
            {
                return;
            }
            let repopulate = !same_by
                || c.as_ref().is_none_or(|c| {
                    c.consider_gem_type != show
                        || c.show_legacy_gems != legacy
                        || c.default_quality != default_quality
                        || c.default_level != default_level
                        || (c.character_level != character_level && default_level == "characterLevel")
                });
            drop(c);
            if repopulate {
                *self.last_sort_gems_by.borrow_mut() = *self.sort_gems_by.borrow();
                self.populate_gem_list();
            }
        }
        let group = group.expect("Classes/GemSelectControl.lua:271: attempt to index field 'displayGroup' (a nil value)");
        let ids: Vec<String> = self.gems.borrow().iter().cloned().collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let (can_support, base_dps) = tab.read(|e| {
            (
                e.gem_can_support(group, &refs, self.imbued_select)
                    .unwrap_or_else(|er| panic!("{er}")),
                e.gem_base_dps(&field).unwrap_or_else(|er| panic!("{er}")),
            )
        });
        tab.edit(|e| {
            e.mark_slot_enabled();
            Ok(())
        });
        let skills = pob_data::skills::skills();
        let mut dps = HashMap::new();
        let mut dps_color = HashMap::new();
        let mut pending = Vec::new();
        for id in &ids {
            dps.insert(id.clone(), base_dps);
            let gem = gem_of(id).expect("gem");
            let ge = effect_of(&skills, gem);
            if can_support.contains(id) || (ge.has_global_effect.get() && !ge.support) {
                pending.push(id.clone());
            }
            dps_color.insert(id.clone(), "");
        }
        *self.sort_cache.borrow_mut() = Some(SortCache {
            consider_gem_type: show,
            show_legacy_gems: legacy,
            socket_group: display,
            gem_instance,
            output_revision: revision,
            default_level,
            default_quality,
            character_level,
            sort_type: field.clone(),
            can_support,
            dps,
            dps_color,
            base_dps,
            dps_field: field,
            pending: Some(pending),
        });
        self.dps_build_flag.set(true);
    }

    /// :375-395
    fn sort_gem_list(&self, list: &mut [String]) {
        let cache = self.sort_cache.borrow();
        let Some(c) = cache.as_ref() else {
            panic!("Classes/GemSelectControl.lua:385: attempt to index upvalue 'sortCache' (a nil value)");
        };
        let by_dps = self.tab().read(|e| e.model().skills.sort_gems_by_dps.unwrap_or(true));
        let name = |id: &str| gem_of(id).map_or(id.to_string(), |g| g.name.clone());
        list.sort_by(|a, b| {
            let (sa, sb) = (c.can_support.contains(a), c.can_support.contains(b));
            if sa != sb {
                return if sa {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                };
            }
            let (da, db) = (c.dps.get(a), c.dps.get(b));
            if by_dps && da != db {
                return db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal);
            }
            name(a).as_bytes().cmp(name(b).as_bytes())
        });
    }

    /// :397-406
    pub fn sync_selection(&self, g: &Gfx) {
        self.sel_index.set(0);
        let buf = self.edit.buf.borrow().to_ascii_lowercase();
        let list = self.list.borrow().clone();
        for (i, id) in list.iter().enumerate() {
            if self.gems.borrow().contains(id) && gem_of(id).is_some_and(|x| x.name.to_ascii_lowercase() == buf) {
                self.sel_index.set(i as i64 + 1);
                self.scroll_sel_into_view(g);
                break;
            }
        }
    }

    /// :408-413
    fn sort_current_list(&self, g: &Gfx) {
        if self.search_str.borrow().is_empty() {
            let mut l = std::mem::take(&mut *self.list.borrow_mut());
            self.sort_gem_list(&mut l);
            *self.list.borrow_mut() = l;
            self.sync_selection(g);
        }
    }

    /// :415-452, one `coroutine.resume`.
    fn dps_builder_step(&self, g: &Gfx) {
        let tab = self.tab();
        let Some(group) = tab.display_index() else {
            *self.dps_builder.borrow_mut() = None;
            return;
        };
        let index = self.effective_index(group);
        loop {
            let (id, n, field) = {
                let c = self.sort_cache.borrow();
                let Some(c) = c.as_ref() else {
                    *self.dps_builder.borrow_mut() = None;
                    return;
                };
                let Some(p) = c.pending.as_ref() else {
                    *self.dps_builder.borrow_mut() = None;
                    return;
                };
                let next = self.dps_builder.borrow().as_ref().map_or(0, |b| b.next);
                if next >= p.len() {
                    (None, p.len(), c.dps_field.clone())
                } else {
                    (Some(p[next].clone()), p.len(), c.dps_field.clone())
                }
            };
            let Some(id) = id else {
                self.sort_current_list(g);
                if let Some(c) = self.sort_cache.borrow_mut().as_mut() {
                    c.pending = None;
                }
                *self.dps_builder.borrow_mut() = None;
                return;
            };
            if self.gems.borrow().contains(&id) {
                // a Lua error kills the coroutine in silence (`launch.devMode` is off, :514)
                let Ok(v) = tab.read(|e| e.gem_candidate_dps(group, index, &id, self.imbued_select, &field)) else {
                    *self.dps_builder.borrow_mut() = None;
                    return;
                };
                if let Some(c) = self.sort_cache.borrow_mut().as_mut() {
                    c.dps.insert(id.clone(), v);
                    let col = if v > c.base_dps {
                        "^x228866"
                    } else if v < c.base_dps {
                        "^xFF4422"
                    } else {
                        "^xFFFF66"
                    };
                    c.dps_color.insert(id.clone(), col);
                }
            }
            let now = g.get_time() as f64;
            let mut b = self.dps_builder.borrow_mut();
            let st = b.as_mut().expect("builder");
            st.next += 1;
            if now - st.start > 50.0 {
                let done = st.next;
                st.start = now;
                drop(b);
                self.sort_current_list(g);
                *self.sort_percentage.borrow_mut() = format!("{}%", ((done * 100) as f64 / n as f64).floor());
                return;
            }
        }
    }

    /// `self.index`: :76-78 the imbued control follows the end of the group's list.
    fn effective_index(&self, group: usize) -> usize {
        if self.imbued_select {
            let n = self
                .tab()
                .read(|e| e.socket_groups().get(group - 1).map_or(0, |g| g.gem_list.len()));
            self.index.set(n + 1);
        }
        self.index.get()
    }

    /// :454-470
    pub fn update_gem(&self, g: &mut Gfx, set_text: bool, add_undo: bool, focus_lost: bool) {
        let gem_id = {
            let list = self.list.borrow();
            list.get((self.sel_index.get().max(1) - 1) as usize).cloned().unwrap_or_default()
        };
        let known = self.gems.borrow().contains(&gem_id);
        let buf = self.edit.buf.borrow().clone();
        let buf_matches = known && gem_of(&gem_id).is_some_and(|x| x.name.eq_ignore_ascii_case(&buf));
        let has_text = buf.bytes().any(|c| !c.is_ascii_whitespace());
        *self.gem_id.borrow_mut() = (has_text && known).then_some(gem_id);
        let name = if buf_matches {
            self.gem_id
                .borrow()
                .as_deref()
                .and_then(gem_of)
                .map_or(String::new(), |x| x.name.clone())
        } else {
            String::new()
        };
        *self.gem_name.borrow_mut() = name.clone();
        if set_text {
            self.edit.set_text(g, &name, false);
        }
        let f = self.gem_change_func.clone();
        let id = self.gem_id.borrow().clone();
        f(g, id, add_undo && name != *self.initial_buf.borrow(), focus_lost, buf_matches);
    }

    /// :472-478
    fn scroll_sel_into_view(&self, g: &Gfx) {
        let (_, height) = self.edit.get_size(g);
        let n = self.list.borrow().len() as f64;
        let drop_height = (height - 4.0) * n.min(15.0);
        self.scroll_bar.set_content_dimension(g, (height - 4.0) * n, drop_height);
        self.scroll_bar
            .scroll_into_view((self.sel_index.get() as f64 - 2.0) * (height - 4.0), 3.0 * (height - 4.0));
    }

    /// :480-501
    fn mouse_over_comp(&self, g: &Gfx) -> (bool, Option<&'static str>) {
        if !self.is_shown(g) {
            return (false, None);
        }
        if self.edit.host.get_mouse_over_control(g).is_some() {
            return (true, None);
        }
        let (x, y) = self.get_pos(g);
        let (width, height) = self.get_size(g);
        let (cx, cy) = g.get_cursor_pos();
        let drop_extra = if self.dropped.get() {
            (height - 4.0) * self.list.borrow().len().min(15) as f64 + 2.0
        } else {
            0.0
        };
        let m = cx >= x && cy >= y && cx < x + width && cy < y + height + drop_extra;
        (m, m.then_some(if cy < y + height { "BODY" } else { "DROP" }))
    }

    /// :701-704
    fn check_supporting(group: &pob_calc::skills_tab::SocketGroup, g_idx: usize, a: usize, b: usize) -> bool {
        let skills = pob_data::skills::skills();
        let (ga, gb) = (&group.gem_list[a], &group.gem_list[b]);
        let (Some(da), Some(db)) = (ga.gem_data.as_deref().and_then(gem_of), gb.gem_data.as_deref().and_then(gem_of)) else {
            return false;
        };
        let (ea, eb) = (effect_of(&skills, da), effect_of(&skills, db));
        let sec_support = da
            .secondary_granted_effect_id
            .as_ref()
            .and_then(|s| skills.get(s))
            .is_some_and(|s| s.support);
        (ea.support || sec_support)
            && !eb.support
            && ga
                .support_effect
                .as_ref()
                .is_some_and(|s| s.borrow().is_supporting.as_ref().is_some_and(|l| l.contains(&(g_idx, b))))
    }

    /// :640-697 the tooltip of the closed control and the two overlay buttons.
    #[allow(clippy::too_many_lines)]
    fn draw_closed(&self, f: &mut Frame<'_>, view_port: &Rect, no_tooltip: bool, x: f64, y: f64, width: f64, height: f64, m_over: bool) {
        let tab = self.tab();
        let sel = tab.host.sel();
        let hover_control = match sel.as_ref().and_then(|s| tab.gem_select_of(s)) {
            Some(h) => Some(h),
            None => tab.host.get_mouse_over_control(f).and_then(|m| tab.gem_select_of(&m)),
        };
        let idx = self.index.get();
        if let Some(h) = &hover_control
            && let Some(group) = tab.display_index()
        {
            let hi = h.index.get();
            let glows = tab.read(|e| {
                let gs = &e.socket_groups()[group - 1];
                let (Some(a), Some(b)) = (gs.gem_list.get(idx.wrapping_sub(1)), gs.gem_list.get(hi.wrapping_sub(1))) else {
                    return false;
                };
                a.enabled
                    && b.enabled
                    && a.gem_data.is_some()
                    && b.gem_data.is_some()
                    && (Self::check_supporting(gs, group - 1, idx - 1, hi - 1) || Self::check_supporting(gs, group - 1, hi - 1, idx - 1))
            });
            if glows {
                sg::set_draw_color_a(f, 0.33, 1.0, 0.33, 0.25);
                sg::draw_rect(f, x, y, width, height);
            }
        }
        let sel_dropped = sel.as_ref().and_then(|s| tab.gem_select_of(s)).is_some_and(|s| s.dropped.get());
        if m_over && !sel_dropped && (!no_tooltip || self.force_tooltip) {
            let (cursor_x, _) = f.get_cursor_pos();
            let mut tt = self.edit.tip.tooltip.borrow_mut();
            tt.clear(false);
            let mut drawn = false;
            let imbued_gem = hover_control
                .as_ref()
                .filter(|h| h.imbued_select)
                .map(|h| h.gem_id.borrow().clone());
            tab.read(|e| match &imbued_gem {
                Some(id) => {
                    if let Some(gem) = id.as_deref().and_then(gem_of) {
                        let skills = pob_data::skills::skills();
                        add_gem_tooltip(
                            &mut tt,
                            f,
                            e,
                            &GemInst {
                                gem: Some(gem),
                                effect: effect_of(&skills, gem),
                                level: 1.0,
                                quality: 0.0,
                                display: None,
                                skip_requirements: false,
                            },
                        );
                        drawn = true;
                    }
                }
                None => {
                    if let Some(group) = tab.display_index()
                        && let Some(gi) = e.socket_groups()[group - 1].gem_list.get(idx.wrapping_sub(1))
                        && let Some(gem) = gi.gem_data.as_deref().and_then(gem_of)
                    {
                        let skills = pob_data::skills::skills();
                        add_gem_tooltip(
                            &mut tt,
                            f,
                            e,
                            &GemInst {
                                gem: Some(gem),
                                effect: effect_of(&skills, gem),
                                level: gi.level.unwrap_or(1.0),
                                quality: gi.quality.unwrap_or(0.0),
                                display: gi.display_effect.as_ref().map(|d| d.borrow().clone()),
                                skip_requirements: false,
                            },
                        );
                        drawn = true;
                    }
                }
            });
            if !drawn {
                tt.add_line(f, 16.0, Some(if self.imbued_select { IMBUED_TOOLTIP_TEXT } else { TOOL_TIP_TEXT }));
            }
            if !self.imbued_select {
                let (mut color_s, mut color_a) = (0.5, 0.5);
                if cursor_x > x + width - 18.0 {
                    color_s = 1.0;
                    tt.clear(false);
                    tt.add_line(f, 16.0, Some("Only show Support gems"));
                } else if cursor_x > x + width - 40.0 && cursor_x < cursor_x + width - 20.0 {
                    color_a = 1.0;
                    tt.clear(false);
                    tt.add_line(f, 16.0, Some("Only show Active gems"));
                }
                let sx = x + width - 16.0 - 2.0;
                sg::set_draw_color(f, color_s, color_s, color_s);
                sg::draw_rect(f, sx, y + 2.0, 16.0, height - 4.0);
                sg::set_draw_color(f, 0.0, 0.0, 0.0);
                sg::draw_rect(f, sx + 1.0, y + 2.0, 16.0 - 2.0, height - 4.0);
                sg::set_draw_color(f, color_s, color_s, color_s);
                sg::draw_string(f, sx + 8.0, y, Some("CENTER_X"), height - 2.0, "VAR", "S");
                let sx = x + width - 16.0 * 2.0 - 2.0 * 2.0;
                sg::set_draw_color(f, color_a, color_a, color_a);
                sg::draw_rect(f, sx, y + 2.0, 16.0, height - 4.0);
                sg::set_draw_color(f, 0.0, 0.0, 0.0);
                sg::draw_rect(f, sx + 1.0, y + 2.0, 16.0 - 2.0, height - 4.0);
                sg::set_draw_color(f, color_a, color_a, color_a);
                sg::draw_string(f, sx + 8.0, y, Some("CENTER_X"), height - 2.0, "VAR", "A");
            }
            sg::set_draw_layer(f, None, 10);
            tt.draw(f, x, y, Some((width, height)), view_port);
            sg::set_draw_layer(f, None, 0);
        }
    }

    /// :584-621 the tooltip of the hovered row.
    fn draw_hover(&self, f: &mut Frame<'_>, view_port: &Rect, hover: usize, x: f64, y: f64, width: f64, height: f64) {
        if Some(hover) == self.last_hover_sel.get() {
            self.hover_frame_count.set(self.hover_frame_count.get() + 1);
        } else {
            self.last_hover_sel.set(Some(hover));
            self.hover_frame_count.set(0);
        }
        if self.hover_frame_count.get() < 2 {
            return;
        }
        let tab = self.tab();
        let Some(group) = tab.display_index() else { return };
        let index = self.effective_index(group);
        let id = self.list.borrow()[hover - 1].clone();
        let gem = gem_of(&id).expect("gem");
        let skills = pob_data::skills::skills();
        let mut tt = self.edit.tip.tooltip.borrow_mut();
        tt.clear(false);
        tab.read(|e| {
            let field = e.model().skills.sort_gems_by_dps_field.clone();
            let key = (
                id.clone(),
                e.output_revision(),
                group,
                index,
                e.skills_tab().default_gem_level.clone(),
                field.clone(),
            );
            let mut cache = self.hover_compare.borrow_mut();
            if cache.as_ref().is_none_or(|c| c.0 != key) {
                let edit = GemEdit::Candidate {
                    gem_id: id.clone(),
                    imbued: self.imbued_select,
                };
                let lines = e
                    .gem_stat_compare(group, index, &edit, "^7Selecting this gem will give you:", Some(field == "FullDPS"))
                    .unwrap_or_else(|er| panic!("{er}"));
                *cache = Some((key, lines));
            }
            let level = pob_calc::process_gem_level(
                gem,
                self.imbued_select,
                &e.skills_tab().default_gem_level,
                Some(e.model().level),
                &skills,
            )
            .unwrap_or_else(|er| panic!("{er}"));
            add_gem_tooltip(
                &mut tt,
                f,
                e,
                &GemInst {
                    gem: Some(gem),
                    effect: effect_of(&skills, gem),
                    level,
                    quality: e.skills_tab().default_gem_quality,
                    display: None,
                    skip_requirements: false,
                },
            );
            tt.add_separator(Some(10.0));
            for l in &cache.as_ref().expect("just set").1 {
                tt.add_line(f, 14.0, Some(l));
            }
        });
        let off = self.scroll_bar.offset.get();
        tt.draw(
            f,
            x,
            y + height + 2.0 + (hover as f64 - 1.0) * (height - 4.0) - off,
            Some((width, height - 4.0)),
            view_port,
        );
    }

    /// `self.tooltip`'s text lines.
    pub fn debug_tooltip(&self) -> Vec<String> {
        self.edit
            .tip
            .tooltip
            .borrow()
            .lines
            .iter()
            .map(|l| l.text.clone().unwrap_or_else(|| "<sep>".to_string()))
            .collect()
    }
}

impl Control for GemSelectControl {
    fn base(&self) -> &ControlBase {
        &self.edit.base
    }

    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        Some(self.mouse_over_comp(g).0)
    }

    fn has_draw(&self) -> bool {
        true
    }

    fn imbued_dropped(&self) -> bool {
        self.imbued_select && self.dropped.get()
    }

    /// :503-699
    #[allow(clippy::too_many_lines)]
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, no_tooltip: bool) {
        if self.dps_build_flag.get() {
            self.dps_build_flag.set(false);
            *self.dps_builder.borrow_mut() = Some(DpsBuilder {
                next: 0,
                start: f.get_time() as f64,
            });
        }
        if self.dps_builder.borrow().is_some() {
            self.dps_builder_step(f);
        }
        self.edit.draw(f, view_port, no_tooltip && !self.force_tooltip);
        let (x, y) = self.get_pos(f);
        let (width, height) = self.get_size(f);
        let (m_over, m_over_comp) = self.mouse_over_comp(f);
        let n = self.list.borrow().len() as f64;
        let drop_height = (height - 4.0) * n.min(15.0);
        self.scroll_bar.set_content_dimension(f, (height - 4.0) * n, drop_height);
        if !self.dropped.get() {
            self.draw_closed(f, view_port, no_tooltip, x, y, width, height, m_over);
            return;
        }
        sg::set_draw_layer(f, None, 5);
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_rect(f, x, y + height, width, drop_height + 4.0);
        sg::set_draw_color(f, 0.0, 0.0, 0.0);
        sg::draw_rect(f, x + 1.0, y + height + 1.0, width - 2.0, drop_height + 2.0);
        sg::set_draw_layer(f, None, 0);
        sg::set_draw_layer(f, None, 5);
        if self.dps_builder.borrow().is_some() {
            sg::set_draw_color(f, 0.75, 0.75, 0.75);
            let s = format!("Sorting {}", self.sort_percentage.borrow());
            sg::draw_string(f, x + width - 4.0, y, Some("RIGHT_X"), height - 2.0, "VAR", &s);
        }
        let (_, cursor_y) = f.get_cursor_pos();
        let off = self.scroll_bar.offset.get();
        let mut hover = (m_over_comp == Some("DROP")).then(|| ((cursor_y - y - height + off) / (height - 4.0)).floor() as i64 + 1);
        if let Some(h) = hover {
            let ok = h >= 1
                && self
                    .list
                    .borrow()
                    .get((h - 1) as usize)
                    .is_some_and(|id| self.gems.borrow().contains(id));
            if !ok {
                hover = None;
            }
        }
        let hover = hover.map(|h| h as usize);
        self.hover_sel.set(hover);
        sg::set_viewport(f, Some([x + 2.0, y + height + 2.0, width - 4.0, drop_height]));
        let min_index = (off / 16.0 + 1.0).floor() as i64;
        let max_index = ((off + drop_height) / 16.0 + 1.0).floor().min(n) as i64;
        let skills = pob_data::skills::skills();
        let enabled = self.scroll_bar.base.enabled.get(f);
        let cache = self.sort_cache.borrow();
        let list = self.list.borrow().clone();
        for index in min_index..=max_index {
            let row_y = (index as f64 - 1.0) * (height - 4.0) - off;
            if hover == Some(index as usize) || self.sel_index.get() == index || (index == 1 && self.sel_index.get() == 0) {
                sg::set_draw_color(f, 0.2, 0.2, 0.2);
                sg::draw_rect(f, 0.0, row_y, width - 4.0, height - 4.0);
            }
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            let id = &list[(index - 1) as usize];
            let gem = gem_of(id).filter(|_| self.gems.borrow().contains(id));
            let ge = gem.map(|x| effect_of(&skills, x));
            if let Some(ge) = ge {
                let code = match ge.color {
                    Some(1.0) => Some("STRENGTH"),
                    Some(2.0) => Some("DEXTERITY"),
                    Some(3.0) => Some("INTELLIGENCE"),
                    _ => None,
                };
                if let Some(code) = code {
                    sg::set_draw_color_str(f, &color_code(code).unwrap_or_default());
                }
            }
            let text = gem.map_or("<No matches>", |x| x.name.as_str());
            sg::draw_string(f, 0.0, row_y, Some("LEFT"), height - 4.0, "VAR", text);
            if let (Some(ge), Some(c)) = (ge, cache.as_ref()) {
                let col = c.dps_color.get(id).copied().unwrap_or("");
                let right = width - 4.0 - height / 2.0 - if enabled { 18.0 } else { 0.0 };
                if ge.support && c.can_support.contains(id) && !col.is_empty() {
                    sg::set_draw_color_str(f, col);
                    draw_check_mark(f, right, row_y + (height - 4.0) / 2.0, (height - 4.0) * 0.8);
                } else if ge.has_global_effect.get() && !col.is_empty() {
                    sg::set_draw_color_str(f, col);
                    sg::draw_string(f, right, row_y - 2.0, Some("CENTER_X"), height, "VAR", "+");
                }
            }
        }
        drop(cache);
        sg::set_viewport(f, None);
        let me: Option<ControlRef> = (no_tooltip && !self.force_tooltip)
            .then(|| self.me.upgrade().map(|m| m as ControlRef))
            .flatten();
        self.edit.host.draw_controls(f, view_port, me.as_ref());
        if let Some(h) = hover {
            self.draw_hover(f, view_port, h, x, y, width, height);
        } else {
            self.last_hover_sel.set(None);
            self.hover_frame_count.set(0);
        }
        sg::set_draw_layer(f, None, 0);
    }

    /// :710-717
    fn on_focus_gained(&self, g: &mut Gfx) {
        self.edit.on_focus_gained(g);
        self.dropped.set(true);
        self.update_sort_cache(g);
        self.build_list("");
        self.sync_selection(g);
        *self.initial_buf.borrow_mut() = self.edit.buf.borrow().clone();
    }

    /// :727-731
    fn on_focus_lost(&self, g: &mut Gfx) {
        if self.dropped.get() {
            self.cancel_selection(g);
        }
    }

    fn has_on_key_down(&self) -> bool {
        true
    }

    /// :733-819
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        let (x, y) = self.get_pos(g);
        let (width, height) = self.get_size(g);
        let (cursor_x, cursor_y) = g.get_cursor_pos();
        if !self.imbued_select && key == "LEFTBUTTON" && cursor_y > y && cursor_y < y + height {
            *self.sort_gems_by.borrow_mut() = if cursor_x > x + width - 18.0 {
                Some("support")
            } else if cursor_x > x + width - 40.0 && cursor_x < cursor_x + width - 20.0 {
                Some("grants_active_skill")
            } else {
                None
            };
        }
        if let Some(m) = self.edit.host.get_mouse_over_control(g).filter(|m| m.has_on_key_down()) {
            *self.edit.host.sel_control.borrow_mut() = Some(m.clone());
            let n = m.on_key_down(g, key, false);
            return n.map(|_| self as ControlRef);
        }
        *self.edit.host.sel_control.borrow_mut() = None;
        let me: ControlRef = self.clone();
        if self.dropped.get() {
            if key.contains("BUTTON") && !self.mouse_over_comp(g).0 {
                self.cancel_selection(g);
                return None;
            }
            if key == "LEFTBUTTON" {
                if let Some(h) = self.hover_sel.get() {
                    let id = self.list.borrow().get(h - 1).cloned().unwrap_or_default();
                    if self.gems.borrow().contains(&id) {
                        self.dropped.set(false);
                        self.sel_index.set(h as i64);
                        let name = gem_of(&id).map_or(String::new(), |x| x.name.clone());
                        self.edit.set_text(g, &name, false);
                        self.update_gem(g, false, true, false);
                        return None;
                    }
                }
            } else if key == "RETURN" {
                self.dropped.set(false);
                if self.no_matches.get() {
                    self.edit.set_text(g, "", false);
                }
                self.sel_index.set(self.sel_index.get().max(1));
                let id = self
                    .list
                    .borrow()
                    .get((self.sel_index.get() - 1) as usize)
                    .cloned()
                    .unwrap_or_default();
                if self.gems.borrow().contains(&id) {
                    let name = gem_of(&id).map_or(String::new(), |x| x.name.clone());
                    self.edit.set_text(g, &name, false);
                }
                self.update_gem(g, true, true, true);
                return None;
            } else if key == "ESCAPE" {
                self.cancel_selection(g);
                return None;
            } else if ScrollBarControl::is_scroll_up_key(key) {
                self.scroll_bar.scroll(-1.0);
            } else if ScrollBarControl::is_scroll_down_key(key) {
                self.scroll_bar.scroll(1.0);
            } else if key == "DOWN" {
                if (self.sel_index.get() as usize) < self.list.borrow().len() && !self.no_matches.get() {
                    self.sel_index.set(self.sel_index.get() + 1);
                    let id = self.list.borrow()[(self.sel_index.get() - 1) as usize].clone();
                    let name = gem_of(&id).map_or(String::new(), |x| x.name.clone());
                    self.edit.set_text(g, &name, false);
                    self.update_gem(g, false, false, false);
                    self.scroll_sel_into_view(g);
                }
            } else if key == "UP" && self.sel_index.get() > 0 && !self.no_matches.get() {
                self.sel_index.set(self.sel_index.get() - 1);
                if self.sel_index.get() == 0 {
                    let s = self.search_str.borrow().clone();
                    self.edit.set_text(g, &s, false);
                } else {
                    let id = self.list.borrow()[(self.sel_index.get() - 1) as usize].clone();
                    let name = gem_of(&id).map_or(String::new(), |x| x.name.clone());
                    self.edit.set_text(g, &name, false);
                }
                self.update_gem(g, false, false, false);
                self.scroll_sel_into_view(g);
            }
        } else if key == "RETURN" || key == "RIGHTBUTTON" {
            self.dropped.set(true);
            self.update_sort_cache(g);
            *self.initial_buf.borrow_mut() = self.edit.buf.borrow().clone();
            return Some(me);
        }
        let edit: ControlRef = self.edit.clone();
        let n = Rc::clone(&self.edit).on_key_down(g, key, double_click);
        n.map(|c| if crate::control::same(&*c, &*edit) { me } else { c })
    }

    fn has_on_key_up(&self) -> bool {
        true
    }

    /// :821-854
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        let sel = self.edit.host.sel();
        if let Some(s) = sel {
            let n = if s.has_on_key_up() { s.on_key_up(g, key) } else { None };
            if n.is_some() {
                return Some(self as ControlRef);
            }
            *self.edit.host.sel_control.borrow_mut() = None;
        }
        if key == "F1" && self.mouse_over_comp(g).0 {
            crate::modules::main::note_not_ported("Classes/GemSelectControl.lua:833-851 itemLib.wiki.openGem (U6)");
        }
        let me: ControlRef = self.clone();
        let edit: ControlRef = self.edit.clone();
        let n = Rc::clone(&self.edit).on_key_up(g, key);
        n.map(|c| if crate::control::same(&*c, &*edit) { me } else { c })
    }

    fn has_on_char(&self) -> bool {
        true
    }

    fn on_char(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        let me: ControlRef = self.clone();
        let edit: ControlRef = self.edit.clone();
        let n = Rc::clone(&self.edit).on_char(g, key);
        n.map(|c| if crate::control::same(&*c, &*edit) { me } else { c })
    }
}

impl GemSelectControl {
    /// :719-725
    pub fn cancel_selection(&self, g: &mut Gfx) {
        self.dropped.set(false);
        let b = self.initial_buf.borrow().clone();
        *self.edit.buf.borrow_mut() = b;
        self.build_list("");
        self.sync_selection(g);
        self.update_gem(g, false, true, true);
    }
}
