//! `CompareTab:DrawSkills` (CompareTab.lua:4049-4491).

use std::collections::BTreeSet;
use std::rc::Rc;

use pob_engine::engine::Engine;
use pob_gfx::Frame;

use super::layout::*;
use super::{CompareEntry, CompareTab};
use crate::colors::color_code;
use crate::control::Rect;
use crate::modules::build::BuildMode;
use crate::sg;
use pob_calc::skills_tab::SocketGroup;

const COMPARE_COL_GAP: f64 = 48.0;

fn code(n: &str) -> String {
    color_code(n).unwrap_or_else(|| panic!("colorCodes.{n}"))
}

/// `tostring` of a number after `..`.
fn num(v: f64) -> String {
    pob_lua::tostring_num(v)
}

/// A gem or the synthesized imbued entry (:4071-4078).
#[derive(Clone)]
struct G {
    /// `(side, group, gem)`, the identity; `gem == usize::MAX`: the imbued entry.
    key: (u8, usize, usize),
    name: Option<String>,
    color: Option<String>,
    level: Option<f64>,
    quality: Option<f64>,
    imbued: bool,
}

#[derive(Clone)]
struct Line {
    gem: Option<G>,
    name: String,
    status: u8, // 0 common, 1 additional, 2 missing
}

/// `getImbuedGem` (:4061-4081).
fn imbued_gem(e: &Engine, side: u8, gi: usize, group: &SocketGroup) -> Option<G> {
    let (effect_name, color) = e.compare_imbued_effect(group)?;
    Some(G {
        key: (side, gi, usize::MAX),
        name: effect_name.or_else(|| group.imbued_support.clone()),
        color: color.map(skill_color),
        level: Some(1.0),
        quality: Some(0.0),
        imbued: true,
    })
}

/// `data.skillColorMap` (Data.lua).
fn skill_color(c: f64) -> String {
    code(match c as i64 {
        1 => "STRENGTH",
        2 => "DEXTERITY",
        3 => "INTELLIGENCE",
        _ => "NORMAL",
    })
}

/// `getGemsWithImbued` (:4216-4227).
fn gems_with_imbued(e: &Engine, side: u8, gi: usize, group: &SocketGroup) -> Vec<G> {
    let mut out: Vec<G> = group
        .gem_list
        .iter()
        .enumerate()
        .map(|(k, g)| G {
            key: (side, gi, k),
            name: super::power::gem_name(e, g),
            color: g.color.clone(),
            level: g.level,
            quality: g.quality,
            imbued: false,
        })
        .collect();
    out.extend(imbued_gem(e, side, gi, group));
    out
}

/// `getGemNameSet` (:4084-4100).
fn name_set(gems: &[G]) -> BTreeSet<String> {
    gems.iter().filter_map(|g| g.name.clone()).collect()
}

/// `groupSimilarity` (:4103-4117).
fn similarity(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// `buildAlignedGemLists` (:4229-4291).
fn aligned(p: Option<&[G]>, c: Option<&[G]>, ps: &BTreeSet<String>, cs: &BTreeSet<String>) -> (Vec<Line>, Vec<Line>) {
    let (pg, cg) = (p.unwrap_or(&[]), c.unwrap_or(&[]));
    let (mut pd, mut cd) = (Vec::new(), Vec::new());
    let mut emitted: BTreeSet<&str> = BTreeSet::new();
    for gem in pg {
        if let Some(name) = gem.name.as_deref()
            && cs.contains(name)
            && emitted.insert(name)
        {
            pd.push(Line {
                gem: Some(gem.clone()),
                name: name.to_string(),
                status: 0,
            });
            cd.push(Line {
                gem: cg.iter().find(|g| g.name.as_deref() == Some(name)).cloned(),
                name: name.to_string(),
                status: 0,
            });
        }
    }
    for gem in pg {
        if let Some(name) = gem.name.as_deref()
            && !cs.contains(name)
        {
            pd.push(Line {
                gem: Some(gem.clone()),
                name: name.to_string(),
                status: 1,
            });
        }
    }
    for gem in cg {
        if let Some(name) = gem.name.as_deref()
            && !ps.contains(name)
        {
            cd.push(Line {
                gem: Some(gem.clone()),
                name: name.to_string(),
                status: 1,
            });
        }
    }
    if p.is_some() && c.is_some() {
        for name in cs.difference(ps) {
            pd.push(Line {
                gem: None,
                name: name.clone(),
                status: 2,
            });
        }
        for name in ps.difference(cs) {
            cd.push(Line {
                gem: None,
                name: name.clone(),
                status: 2,
            });
        }
    }
    (pd, cd)
}

/// `buildGemDisplayString` (:4192-4207).
fn display_string(l: &Line) -> String {
    if l.status == 2 {
        return format!("{}- {}^7", code("NEGATIVE"), l.name);
    }
    let Some(g) = &l.gem else { return String::new() };
    let name = g.name.as_deref().unwrap_or("?");
    let color = g.color.clone().unwrap_or_else(|| code("GEM"));
    let level = g.level.map_or_else(String::new, |v| format!(" {}", num(v)));
    let qual = g.quality.map_or_else(String::new, |v| format!("/{}", num(v)));
    let prefix = if l.status == 1 {
        format!("{}+ ", code("POSITIVE"))
    } else {
        String::new()
    };
    format!("{prefix}{color}{name}^7{level}{qual}")
}

/// `getGroupLabel` (:4323-4329).
fn group_label(g: &SocketGroup, idx: usize) -> String {
    let mut l = g
        .display_label
        .clone()
        .or_else(|| g.label.clone())
        .unwrap_or_else(|| format!("Group {idx}"));
    if let Some(s) = &g.slot {
        l = format!("{l} ({s})");
    }
    l
}

const HEADER_X: f64 = 10.0;
const ICON: f64 = 16.0;
const TEXT_X: f64 = HEADER_X + ICON + 2.0;
const GEM_FONT: f64 = 16.0;
const GEM_LINE: f64 = 18.0;

impl CompareTab {
    /// `DrawSkills` (:4049-4491).
    pub(super) fn draw_skills(&self, f: &mut Frame<'_>, vp: &Rect, entry: &Rc<CompareEntry>) {
        let line_height = 18.0;
        let primary = self.primary();
        let (peng, ceng) = (primary.engine.borrow(), entry.b.engine.borrow());
        let (pe, ce) = (peng.as_ref().expect("engine"), ceng.as_ref().expect("engine"));
        let (pgs, cgs) = (pe.socket_groups(), ce.socket_groups());
        let p_gems: Vec<Vec<G>> = pgs.iter().enumerate().map(|(i, g)| gems_with_imbued(pe, 0, i, g)).collect();
        let c_gems: Vec<Vec<G>> = cgs.iter().enumerate().map(|(i, g)| gems_with_imbued(ce, 1, i, g)).collect();
        let p_sets: Vec<BTreeSet<String>> = p_gems.iter().map(|g| name_set(g)).collect();
        let c_sets: Vec<BTreeSet<String>> = c_gems.iter().map(|g| name_set(g)).collect();
        // :4130-4141
        let mut score_pairs: Vec<(usize, usize, f64)> = Vec::new();
        for (pi, ps) in p_sets.iter().enumerate() {
            for (ci, cs) in c_sets.iter().enumerate() {
                let s = similarity(ps, cs);
                if s > 0.0 {
                    score_pairs.push((pi, ci, s));
                }
            }
        }
        pob_engine::lua::lua_sort(&mut score_pairs, &mut |a: &(usize, usize, f64), b: &(usize, usize, f64)| a.2 > b.2);
        // :4144-4166
        let (mut p_matched, mut c_matched) = (vec![false; pgs.len()], vec![false; cgs.len()]);
        let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
        for (pi, ci, _) in &score_pairs {
            if !p_matched[*pi] && !c_matched[*ci] {
                pairs.push((Some(*pi), Some(*ci)));
                p_matched[*pi] = true;
                c_matched[*ci] = true;
            }
        }
        for (i, m) in p_matched.iter().enumerate() {
            if !m {
                pairs.push((Some(i), None));
            }
        }
        for (i, m) in c_matched.iter().enumerate() {
            if !m {
                pairs.push((None, Some(i)));
            }
        }
        // :4355-4374
        let empty = BTreeSet::new();
        let lists: Vec<(Vec<Line>, Vec<Line>)> = pairs
            .iter()
            .map(|(pi, ci)| {
                aligned(
                    pi.map(|i| p_gems[i].as_slice()),
                    ci.map(|i| c_gems[i].as_slice()),
                    pi.map_or(&empty, |i| &p_sets[i]),
                    ci.map_or(&empty, |i| &c_sets[i]),
                )
            })
            .collect();
        let mut max_primary_w: f64 = 0.0;
        for ((pi, _), (pd, _)) in pairs.iter().zip(&lists) {
            if let Some(i) = pi {
                let w = TEXT_X + sg::draw_string_width(f, 18.0, "VAR", &format!("^7{}", group_label(&pgs[*i], i + 1)));
                max_primary_w = max_primary_w.max(w);
            }
            for l in pd {
                max_primary_w = max_primary_w.max(TEXT_X + sg::draw_string_width(f, GEM_FONT, "VAR", &display_string(l)));
            }
        }
        let primary_name = Self::short_build_name(primary.build_name.borrow().as_deref());
        let header_w = 10.0 + sg::draw_string_width(f, 18.0, "VAR", &format!("{}{primary_name}", code("POSITIVE")));
        max_primary_w = max_primary_w.max(header_w);
        let col_width = max_primary_w + COMPARE_COL_GAP;
        let content_width = col_width * 2.0;
        let needs_h = content_width > vp.width;
        let gem_text_width = col_width - (TEXT_X + 10.0);
        let header_reserve = SKILLS_HEADER_HEIGHT;
        let c = self.c();
        let h = &c.skills_h_scroll_bar;
        h.base.x.set(vp.x);
        h.base.y.set(vp.y + vp.height - SKILLS_H_SCROLL_BAR_HEIGHT);
        h.base.width.set(vp.width);
        h.set_content_dimension(f, content_width, vp.width);
        let bottom = if needs_h { SKILLS_H_SCROLL_BAR_HEIGHT } else { 0.0 };
        let scroll_view_h = vp.height - header_reserve - bottom;
        let off_x = -h.offset.get();
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_string(
            f,
            vp.x + off_x + 10.0,
            vp.y + 4.0,
            Some("LEFT"),
            18.0,
            "VAR",
            &format!("{}{primary_name}", code("POSITIVE")),
        );
        sg::draw_string(
            f,
            vp.x + off_x + col_width + 10.0,
            vp.y + 4.0,
            Some("LEFT"),
            18.0,
            "VAR",
            &format!("{}{}", code("WARNING"), entry.label),
        );
        sg::set_draw_color(f, 0.5, 0.5, 0.5);
        sg::draw_rect(f, vp.x + 4.0, vp.y + header_reserve, vp.width - 8.0, 2.0);
        sg::set_viewport(f, Some([vp.x, vp.y + header_reserve, vp.width, scroll_view_h]));
        let mut draw_y = 4.0 - self.scroll_y.get();
        // :4408-4423 the positions
        let mut entries: Vec<(G, f64, f64)> = Vec::new();
        let mut pre_y = draw_y;
        for (i, (pd, cd)) in lists.iter().enumerate() {
            if i > 0 {
                pre_y += 2.0;
            }
            let collect = |entries: &mut Vec<(G, f64, f64)>, l: &[Line], x: f64| {
                let mut y = pre_y + line_height;
                for e in l {
                    if let Some(g) = &e.gem {
                        entries.push((g.clone(), x, y));
                    }
                    y += GEM_LINE;
                }
                y
            };
            let p_y = collect(&mut entries, pd, off_x + TEXT_X);
            let c_y = collect(&mut entries, cd, off_x + col_width + TEXT_X);
            pre_y += (p_y - pre_y).max(c_y - pre_y) + 6.0;
        }
        // :4426-4438
        let (cx, cy) = f.get_cursor_pos();
        let (lx, ly) = (cx - vp.x, cy - (vp.y + header_reserve));
        let mut hovered: Option<&(G, f64, f64)> = None;
        if lx >= 0.0 && lx < vp.width && ly >= 0.0 && ly < scroll_view_h {
            hovered = entries
                .iter()
                .find(|(_, x, y)| lx >= *x && lx < *x + gem_text_width && ly >= *y && ly < *y + GEM_LINE);
        }
        // :4441-4456
        let mut highlight: Vec<(u8, usize, usize)> = Vec::new();
        if let Some((hg, _, _)) = hovered {
            highlight.push(hg.key);
            for (g, _, _) in &entries {
                // `entry.group == hoveredEntry.group`: the same table, so the same side and index
                if g.key.0 == hg.key.0 && g.key.1 == hg.key.1 && g.key != hg.key {
                    let e = if g.key.0 == 0 { pe } else { ce };
                    if check_supporting(e, hg, g) || check_supporting(e, g, hg) {
                        highlight.push(g.key);
                    }
                }
            }
            if highlight.len() <= 1 {
                highlight.clear();
            }
        }
        // :4459-4486
        for (i, (pair, (pd, cd))) in pairs.iter().zip(&lists).enumerate() {
            if i > 0 {
                sg::set_draw_color(f, 0.3, 0.3, 0.3);
                sg::draw_rect(f, 0.0, draw_y, vp.width, 1.0);
                draw_y += 2.0;
            }
            if let Some(gi) = pair.0 {
                draw_group_header(f, &primary, &pgs[gi], gi + 1, off_x + HEADER_X, draw_y);
            }
            if let Some(gi) = pair.1 {
                draw_group_header(f, &entry.b, &cgs[gi], gi + 1, off_x + col_width + HEADER_X, draw_y);
            }
            let p_y = draw_gem_list(f, pd, off_x + TEXT_X, draw_y + line_height, &highlight, gem_text_width);
            let c_y = draw_gem_list(f, cd, off_x + col_width + TEXT_X, draw_y + line_height, &highlight, gem_text_width);
            draw_y += (p_y - draw_y).max(c_y - draw_y) + 6.0;
        }
        self.skills_total_content_height.set(draw_y + self.scroll_y.get() + 36.0);
        sg::set_viewport(f, None);
    }
}

/// `checkSupporting` (:4169-4186).
fn check_supporting(e: &Engine, a: &G, b: &G) -> bool {
    let groups = e.socket_groups();
    let inst = |g: &G| {
        (!g.imbued)
            .then(|| groups.get(g.key.1).and_then(|grp| grp.gem_list.get(g.key.2)))
            .flatten()
    };
    if a.imbued {
        return inst(b).and_then(|g| e.compare_gem_effect(g)).is_some_and(|x| !x.1);
    }
    let (Some(ga), Some(gb)) = (inst(a), inst(b)) else { return false };
    e.compare_is_supporting(ga, gb, (b.key.1, b.key.2))
}

/// `drawGroupHeader` (:4345-4353).
fn draw_group_header(f: &mut Frame<'_>, b: &BuildMode, g: &SocketGroup, idx: usize, x: f64, y: f64) {
    if let Some(icon) = b.skills_group_icon(idx) {
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_image(f, Some(&icon), x, y, ICON, ICON, None);
    }
    sg::draw_string(
        f,
        x + ICON + 2.0,
        y,
        Some("LEFT"),
        18.0,
        "VAR",
        &format!("^7{}", group_label(g, idx)),
    );
}

/// `drawGemList` (:4306-4320).
fn draw_gem_list(f: &mut Frame<'_>, l: &[Line], x: f64, start: f64, highlight: &[(u8, usize, usize)], width: f64) -> f64 {
    let mut y = start;
    for e in l {
        if e.gem.as_ref().is_some_and(|g| highlight.contains(&g.key)) {
            sg::set_draw_color_a(f, 0.33, 1.0, 0.33, 0.25);
            sg::draw_rect(f, x, y, width, GEM_LINE);
        }
        let line = display_string(e);
        if !line.is_empty() {
            sg::draw_string(f, x, y, Some("LEFT"), GEM_FONT, "VAR", &line);
        }
        y += GEM_LINE;
    }
    y
}
