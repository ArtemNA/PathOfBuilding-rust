//! `CompareTab:DrawCalcs`, `DrawCalcsSkillHeader`, `LayoutCalcsSkillControls`, `RefreshCalcsSkillControls` (CompareTab.lua:2262-2409, :4509-4931) and `Classes/CompareCalcsHelpers.lua`.

use std::collections::HashMap;
use std::rc::Rc;

use pob_calc::OutValue;
use pob_calc::env::Env;
use pob_calc::pob_build::InputValue;
use pob_engine::calcs::{CalcsActor, Cell, CheckCtx, Entry, Flags, check_flag, format_calc_str, minion_actor, player_actor, sections};
use pob_engine::compare_tab::{BuffRow, ModRowData, tabulate_entry};
use pob_engine::engine::Engine;
use pob_gfx::{Frame, Gfx};
use pob_lua::format::Arg;
use pob_mods::Value;

use super::{BUFF_MODES, CalcsSkillControls, CompareEntry, CompareTab, sel_buff_mode};
use crate::colors::color_code;
use crate::control::{Control, Rect};
use crate::modules::build::{BuildMode, SkillControls, apply_skill_select};
use crate::sg;
use crate::tooltip::Tooltip;

const CALCS_MAX_CARD_WIDTH: f64 = 400.0;
const CALCS_LABEL_WIDTH: f64 = 132.0;
const CALCS_SEP_W: f64 = 2.0;
const CALCS_HEADER_BAR_HEIGHT: f64 = 12.0;
const INFO: [(&str, &str, Option<&str>); 3] = [
    ("Aura/Buff Skills", "BuffList", Some("SkillBuffs")),
    ("Combat Buffs", "CombatList", None),
    ("Curses/Debuffs", "CurseList", Some("SkillDebuffs")),
];

fn code(n: &str) -> String {
    color_code(n).unwrap_or_else(|| panic!("colorCodes.{n}"))
}

fn sfmt(f: &str, a: &[Arg<'_>]) -> String {
    pob_lua::format::format(f, a).unwrap_or_else(|e| panic!("{e}"))
}

/// `wrapInfoLine` (:25-47).
fn wrap_info_line(g: &Gfx, s: &str, width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    if s.is_empty() || width <= 0.0 {
        return lines;
    }
    let b = s.as_bytes();
    let is_space = |c: u8| c.is_ascii_whitespace();
    let (mut line_start, mut pos) = (0usize, 0usize);
    while pos < b.len() {
        let ws = (pos..b.len()).find(|&i| is_space(b[i]));
        let word_end = ws.unwrap_or(b.len());
        if line_start < pos && sg::draw_string_width(g, 14.0, "VAR", &s[line_start..word_end]) > width {
            lines.push(s[line_start..pos].trim_end().to_string());
            line_start = pos;
        }
        let Some(ws) = ws else {
            lines.push(s[line_start..].to_string());
            break;
        };
        let we = (ws..b.len()).find(|&i| !is_space(b[i])).unwrap_or(b.len());
        pos = we;
    }
    lines
}

fn out_str(env_out: &pob_calc::Output, key: &str) -> Option<String> {
    match env_out.get(key) {
        Some(OutValue::Str(s)) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

/// `FormatCalcModValue` (CompareCalcsHelpers.lua:13-35).
fn format_mod_value(v: &Value, ty: &str) -> String {
    let n = v.as_num();
    match (ty, n) {
        ("BASE", Some(x)) => sfmt("%+g base", &[Arg::Num(x)]),
        ("INC", Some(x)) if x >= 0.0 => format!("{}% increased", pob_lua::tostring_num(x)),
        ("INC", Some(x)) => format!("{}% reduced", pob_lua::tostring_num(-x)),
        ("MORE", Some(x)) if x >= 0.0 => format!("{}% more", pob_lua::tostring_num(x)),
        ("MORE", Some(x)) => format!("{}% less", pob_lua::tostring_num(-x)),
        ("OVERRIDE", _) => format!("Override: {}", value_tostring(v)),
        ("FLAG", _) => if matches!(v, Value::Bool(false)) { "False" } else { "True" }.to_string(),
        _ => value_tostring(v),
    }
}

fn value_tostring(v: &Value) -> String {
    match v {
        Value::Num(n) => pob_lua::tostring_num(*n),
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => s.to_string(),
        other => pob_mods::format::format_value(other),
    }
}

/// `FormatCalcModName` (:38-40).
fn format_mod_name(name: &str) -> String {
    let g = |s: &[u8], p: &[u8]| pob_lua::gsub(s, p, pob_lua::Repl::Str(b"%1 %2"), None).expect("pattern").0;
    String::from_utf8_lossy(&g(&g(name.as_bytes(), b"([%l%d]:?)(%u)"), b"(%l)(%d)")).into_owned()
}

/// `ResolveSourceName` (:45-76), `resolveModSource` (:329-360): `sourceType`, the name.
fn source_of(src: Option<&str>, e: &Engine) -> (String, String) {
    let Some(src) = src else { return ("?".into(), String::new()) };
    let ty = src.split(':').next().unwrap_or("").to_string();
    let rest = src.split_once(':').map_or("", |x| x.1);
    let name = match ty.as_str() {
        "Item" => {
            let id = rest.split(':').next().and_then(|x| x.parse::<f64>().ok());
            id.filter(|_| rest.contains(':'))
                .and_then(|i| e.item_by_id(i))
                .map_or_else(String::new, |i| format!("{}{}", color_code(&i.rarity).unwrap_or_default(), i.name))
        }
        "Tree" => rest
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|x| x.parse::<u32>().ok())
            .and_then(|id| {
                let spec = e.spec_rc();
                spec.node(id)
                    .map(|n| n.tree_node().dn.clone())
                    .or_else(|| spec.tree.node(id).map(|n| n.dn.clone()))
            })
            .unwrap_or_default(),
        "Skill" => e.compare_skill_name(rest).unwrap_or_default(),
        "Pantheon" | "Spectre" => rest.to_string(),
        _ => String::new(),
    };
    (ty, name)
}

/// `FormatModRow` (:124-140).
fn format_mod_row(r: &ModRowData, sd: &Entry, e: &Engine) -> (String, String, String, String) {
    let display = if sd.mod_type.is_none() {
        format_mod_value(&r.value, &r.ty)
    } else {
        match r.value.as_num() {
            Some(x) => pob_lua::tostring_num((x * 100.0 + 0.5).floor() / 100.0),
            None => value_tostring(&r.value),
        }
    };
    let (ty, name) = source_of(r.source.as_deref(), e);
    let mod_name = if sd.mod_name_is_list {
        format!("  {}", format_mod_name(&r.name))
    } else {
        String::new()
    };
    (display, ty, name, mod_name)
}

/// `ModRowKey` (:113-121).
fn row_key(r: &ModRowData) -> String {
    let src = r.source.as_deref().unwrap_or("");
    let norm = pob_lua::gsub(src.as_bytes(), b"^(Item):%d+:", pob_lua::Repl::Str(b"%1:"), None)
        .expect("pattern")
        .0;
    format!("{}|{}|{}", String::from_utf8_lossy(&norm), r.name, r.ty)
}

/// The value both ways of `table.sort` here compare (:261-266, :284-289, :300-305).
fn by_value(a: &ModRowData, b: &ModRowData) -> bool {
    matches!((a.value.as_num(), b.value.as_num()), (Some(x), Some(y)) if x > y)
}

/// `calcRowMatchesBetweenBuilds` (:4639-4675).
fn row_matches(cell: &Cell, pa: &CalcsActor<'_>, ca: &CalcsActor<'_>, pe: &Engine, ce: &Engine) -> bool {
    let Some(fmt) = &cell.format else { return false };
    let st = BuildMode::display_settings();
    let (p, c) = (format_calc_str(fmt, pa, Some(cell), &st), format_calc_str(fmt, ca, Some(cell), &st));
    match (p, c) {
        (Ok(a), Ok(b)) if a == b => {}
        _ => return false,
    }
    for sd in &cell.entries {
        if !sd.mod_name.is_empty() {
            let (pr, cr) = (tabulate_entry(sd, pa), tabulate_entry(sd, ca));
            if pr.len() != cr.len() {
                return false;
            }
            let mut counts: HashMap<String, usize> = HashMap::new();
            for r in &pr {
                *counts.entry(format!("{}|{}", row_key(r), value_tostring(&r.value))).or_insert(0) += 1;
            }
            for r in &cr {
                let k = format!("{}|{}", row_key(r), value_tostring(&r.value));
                match counts.get_mut(&k) {
                    None => return false,
                    Some(n) if *n > 1 => *n -= 1,
                    Some(_) => {
                        counts.remove(&k);
                    }
                }
            }
        }
        if let Some(b) = &sd.breakdown
            && pe.compare_breakdown_lines(b) != ce.compare_breakdown_lines(b)
        {
            return false;
        }
    }
    true
}

/// `actor = input.showMinion and env.minion or env.player` and `player` over one engine.
fn with_actors<R>(e: &Engine, f: impl FnOnce(&CalcsActor<'_>, &CalcsActor<'_>, &Env) -> R) -> R {
    let run = e.calcs_run();
    let player = player_actor(&run.env, &run.state.player).unwrap_or_else(|err| panic!("{err}"));
    if e.calcs_show_minion()
        && let Some(ms) = &run.state.minion
    {
        let ms = ms.borrow();
        if let Some(m) = minion_actor(&run.env, &ms).unwrap_or_else(|err| panic!("{err}")) {
            return f(&m, &player, &run.env);
        }
    }
    f(&player, &player, &run.env)
}

struct SubInfo {
    label: String,
    rows: Vec<&'static pob_engine::calcs::Row>,
    extra: Option<&'static str>,
}

struct SecInfo {
    colour: &'static str,
    subs: Vec<SubInfo>,
    height: f64,
    x: f64,
    y: f64,
}

impl CompareTab {
    /// `RefreshCalcsSkillControls` (:2262-2318).
    fn refresh_calcs_skill_controls(&self, g: &mut Gfx, entry: &CompareEntry) {
        let c = self.c();
        let primary = self.primary();
        let w = self.me.clone();
        for (b, cs, own) in [(&primary, &c.prim_calcs, false), (&entry.b, &c.cmp_calcs, true)] {
            let eng = b.engine.borrow();
            let e = eng.as_ref().expect("engine");
            // the primary build: `Build:RefreshSkillSelectControls`; the entry: its own
            let sel = if own {
                e.compare_skill_select(true)
            } else {
                e.skill_select_calcs()
            }
            .unwrap_or_else(|err| panic!("{err}"));
            let library = c.cmp_minion_library.clone();
            apply_skill_select(
                g,
                &SkillControls {
                    main_socket_group: &cs.socket_group,
                    main_skill: &cs.main_skill,
                    main_skill_part: &cs.skill_part,
                    main_skill_stage_count: &cs.stage_count,
                    main_skill_mine_count: &cs.mine_count,
                    main_skill_minion: &cs.minion,
                    main_skill_minion_library: &library,
                    main_skill_minion_skill: &cs.minion_skill,
                },
                &sel,
            );
            cs.socket_group.base.shown.set(true);
            cs.mode.base.shown.set(true);
            let input = e.calcs_input();
            let mode = match input.get("misc_buffMode") {
                Some(InputValue::Str(s)) => Some(s.to_string()),
                _ => None,
            };
            sel_buff_mode(&cs.mode, mode.as_deref());
            cs.show_minion.base.shown.set(cs.minion.base.shown.get(g));
            cs.show_minion.state.set(e.calcs_show_minion());
            // :2300-2317 the `.shown` booleans gated by the view mode
            gate(g, cs, &w);
        }
        let _ = BUFF_MODES;
    }

    /// `LayoutCalcsSkillControls` (:2321-2409): the height of the header.
    pub(super) fn layout_calcs_skill_controls(&self, g: &mut Gfx, vp: &Rect, entry: &CompareEntry) -> f64 {
        self.refresh_calcs_skill_controls(g, entry);
        let c = self.c();
        let col_width = ((vp.width - 20.0) / 2.0).floor();
        let left_x = vp.x + 4.0;
        let right_x = left_x + col_width + 12.0;
        let label_w = 120.0;
        let control_w = col_width - label_w - 8.0;
        let row_h = 22.0;
        let (mut left_y, mut right_y) = (vp.y + 4.0 + row_h, vp.y + 4.0 + row_h);
        let layout = |g: &Gfx, ctl: &dyn Control, x: f64, y: f64, width: Option<f64>| -> bool {
            if !ctl.is_shown(g) {
                return false;
            }
            let b = ctl.base();
            b.x.set(x + label_w);
            b.y.set(y);
            b.width.set(width.unwrap_or(control_w).min(b.width.get(g)));
            true
        };
        let rows = |c: &CalcsSkillControls| -> [(Rc<dyn Control>, bool, bool); 9] {
            [
                (c.socket_group.clone(), true, true),
                (c.main_skill.clone(), true, false),
                (c.skill_part.clone(), true, false),
                (c.stage_count.clone(), false, false),
                (c.mine_count.clone(), false, false),
                (c.show_minion.clone(), false, false),
                (c.minion.clone(), true, false),
                (c.minion_skill.clone(), true, false),
                (c.mode.clone(), false, true),
            ]
        };
        for ((p, use_w, always), (m, _, _)) in rows(&c.prim_calcs).into_iter().zip(rows(&c.cmp_calcs)) {
            let width = use_w.then_some(control_w);
            let ps = layout(g, &*p, left_x, left_y, width);
            let cs = layout(g, &*m, right_x, right_y, width);
            if ps || always {
                left_y += row_h;
            }
            if cs || always {
                right_y += row_h;
            }
        }
        // :2377-2400
        let mut text_h = 2.0;
        let primary = self.primary();
        let (peng, ceng) = (primary.engine.borrow(), entry.b.engine.borrow());
        let (po, co) = (
            &peng.as_ref().expect("engine").calcs_run().state.player.output,
            &ceng.as_ref().expect("engine").calcs_run().state.player.output,
        );
        let wrap_w = col_width - 8.0;
        for (label, key, _) in INFO {
            let (pv, cv) = (out_str(po, key), out_str(co, key));
            if pv.is_some() || cv.is_some() {
                let n = |v: &Option<String>| v.as_ref().map_or(0, |s| wrap_info_line(g, &format!("{label}: {s}"), wrap_w).len());
                text_h += n(&pv).max(n(&cv)).max(1) as f64 * 18.0;
            }
        }
        let header = left_y.max(right_y) - vp.y + text_h + 4.0;
        let check = &c.calcs_show_only_differences_check;
        check.state.set(self.calcs_show_only_differences.get());
        check.base.x.set(left_x + check.label_width.get() + 2.0);
        check.base.y.set(vp.y + header + 4.0);
        header
    }

    /// `DrawCalcsSkillHeader` (:4509-4637).
    fn draw_calcs_skill_header(
        &self,
        f: &mut Frame<'_>,
        vp: &Rect,
        entry: &CompareEntry,
        header_height: f64,
    ) -> Option<(bool, &'static str, (f64, f64, f64, f64))> {
        let col_width = ((vp.width - 20.0) / 2.0).floor();
        let left_x = vp.x + 4.0;
        let right_x = left_x + col_width + 12.0;
        let row_h = 22.0;
        let y = vp.y + 4.0;
        let primary = self.primary();
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        let name = Self::short_build_name(primary.build_name.borrow().as_deref());
        sg::draw_string(
            f,
            left_x,
            y + 2.0,
            Some("LEFT"),
            18.0,
            "VAR",
            &format!("{}{name}", code("POSITIVE")),
        );
        sg::draw_string(
            f,
            right_x,
            y + 2.0,
            Some("LEFT"),
            18.0,
            "VAR",
            &format!("{}{}", code("WARNING"), entry.label),
        );
        let y = y + row_h;
        let c = self.c();
        let (mut ly, mut ry) = (y, y);
        let label = |f: &mut Frame<'_>, text: &str, x: f64, cy: f64, ctl: &dyn Control| -> bool {
            if !ctl.is_shown(f) {
                return false;
            }
            sg::draw_string(f, x, cy + 2.0, Some("LEFT"), 14.0, "VAR", &format!("^7{text}:"));
            true
        };
        let (p, m) = (&c.prim_calcs, &c.cmp_calcs);
        label(f, "Socket Group", left_x, ly, &*p.socket_group);
        label(f, "Socket Group", right_x, ry, &*m.socket_group);
        ly += row_h;
        ry += row_h;
        let rows: [(&str, Rc<dyn Control>, Rc<dyn Control>); 7] = [
            ("Active Skill", p.main_skill.clone(), m.main_skill.clone()),
            ("Skill Part", p.skill_part.clone(), m.skill_part.clone()),
            ("Stages", p.stage_count.clone(), m.stage_count.clone()),
            ("Mines", p.mine_count.clone(), m.mine_count.clone()),
            ("Show Minion Stats", p.show_minion.clone(), m.show_minion.clone()),
            ("Minion", p.minion.clone(), m.minion.clone()),
            ("Minion Skill", p.minion_skill.clone(), m.minion_skill.clone()),
        ];
        for (t, a, b) in rows {
            if label(f, t, left_x, ly, &*a) {
                ly += row_h;
            }
            if label(f, t, right_x, ry, &*b) {
                ry += row_h;
            }
        }
        label(f, "Calc Mode", left_x, ly, &*p.mode);
        label(f, "Calc Mode", right_x, ry, &*m.mode);
        ly += row_h;
        ry += row_h;
        let mut text_y = ly.max(ry) + 2.0;
        let (peng, ceng) = (primary.engine.borrow(), entry.b.engine.borrow());
        let (po, co) = (
            &peng.as_ref().expect("engine").calcs_run().state.player.output,
            &ceng.as_ref().expect("engine").calcs_run().state.player.output,
        );
        let (cx, cy) = f.get_cursor_pos();
        let wrap_w = col_width - 8.0;
        let mut hover = None;
        for (lab, key, breakdown) in INFO {
            let (pv, cv) = (out_str(po, key), out_str(co, key));
            if pv.is_none() && cv.is_none() {
                continue;
            }
            let lines = |g: &Gfx, v: &Option<String>| {
                v.as_ref()
                    .map_or_else(Vec::new, |s| wrap_info_line(g, &format!("{lab}: {s}"), wrap_w))
            };
            let (pl, cl) = (lines(f, &pv), lines(f, &cv));
            let (ph, ch) = (pl.len() as f64 * 18.0, cl.len() as f64 * 18.0);
            let rh = ph.max(ch).max(18.0);
            if let Some(b) = breakdown
                && cy >= text_y
                && cy < text_y + rh
            {
                let on_left = cx >= left_x && cx < right_x && ph > 0.0 && cy < text_y + ph;
                let on_right = cx >= right_x && cx < vp.x + vp.width && ch > 0.0 && cy < text_y + ch;
                if on_left {
                    sg::set_draw_color(f, 0.15, 0.25, 0.15);
                    sg::draw_rect(f, left_x, text_y, col_width, ph);
                    hover = Some((true, b, (left_x, text_y, col_width, ph)));
                } else if on_right {
                    sg::set_draw_color(f, 0.15, 0.25, 0.15);
                    sg::draw_rect(f, right_x, text_y, col_width, ch);
                    hover = Some((false, b, (right_x, text_y, col_width, ch)));
                }
            }
            for (i, l) in pl.iter().enumerate() {
                sg::draw_string(
                    f,
                    left_x,
                    text_y + 1.0 + i as f64 * 18.0,
                    Some("LEFT"),
                    14.0,
                    "VAR",
                    &format!("^7{l}"),
                );
            }
            for (i, l) in cl.iter().enumerate() {
                sg::draw_string(
                    f,
                    right_x,
                    text_y + 1.0 + i as f64 * 18.0,
                    Some("LEFT"),
                    14.0,
                    "VAR",
                    &format!("^7{l}"),
                );
            }
            text_y += rh;
        }
        sg::set_draw_color(f, 0.4, 0.4, 0.4);
        sg::draw_rect(f, vp.x + 2.0, vp.y + header_height - 2.0, vp.width - 4.0, 2.0);
        hover
    }

    /// `DrawCalcs` (:4684-4931).
    #[allow(clippy::too_many_lines)]
    pub(super) fn draw_calcs(&self, f: &mut Frame<'_>, vp: &Rect, entry: &Rc<CompareEntry>, skill_header: f64) {
        let header_hover = if skill_header > 0.0 {
            self.draw_calcs_skill_header(f, vp, entry, skill_header)
        } else {
            None
        };
        let c = self.c();
        let primary = self.primary();
        let peng = primary.engine.borrow();
        let ceng = entry.b.engine.borrow();
        let (pe, ce) = (peng.as_ref().expect("engine"), ceng.as_ref().expect("engine"));
        let grid_width = vp.width - 20.0;
        let card_width = CALCS_MAX_CARD_WIDTH.min(grid_width - 16.0);
        let val_w = ((card_width - 140.0) / 2.0).floor();
        let val1 = CALCS_LABEL_WIDTH + CALCS_SEP_W * 2.0;
        let val2 = val1 + val_w + CALCS_SEP_W;
        let max_col = ((grid_width / (card_width + 8.0)).floor() as usize).max(1);
        let base_y = CALCS_HEADER_BAR_HEIGHT
            + if c.calcs_show_only_differences_check.is_shown(f) {
                12.0
            } else {
                0.0
            };
        let only_diff = self.calcs_show_only_differences.get();
        let st = BuildMode::display_settings();
        let (mut hover_cell, mut hover_label, mut hover_rect): (Option<&'static Cell>, &str, (f64, f64)) = (None, "", (0.0, 0.0));
        with_actors(pe, |pa, pp, _| {
            with_actors(ce, |ca, _, _| {
                let pctx = CheckCtx {
                    actor: pa,
                    player_flags: pp.skill_flags,
                };
                // `primaryBuild.calcsTab:CheckFlag(rowData, compareActor)`: `playerFlag` reads the primary build
                let cctx = CheckCtx {
                    actor: ca,
                    player_flags: pp.skill_flags,
                };
                let mut secs: Vec<SecInfo> = Vec::new();
                // `self.calcSections`: the module, without the section `CalcsTab:CalcsTab` makes itself (CalcsTab.lua:45)
                for sec in sections().iter().filter(|s| s.id != "SkillSelect") {
                    let first = &sec.sub_sections[0];
                    let flags = Flags {
                        flag: first.flag.clone(),
                        not_flag: first.not_flag.clone(),
                        have_output: first.have_output.clone(),
                        ..Flags::default()
                    };
                    if !check_flag(&pctx, &flags).unwrap_or(false) {
                        continue;
                    }
                    let mut subs = Vec::new();
                    for sub in &sec.sub_sections {
                        let mut rows = Vec::new();
                        for row in &sub.rows {
                            let Some(cell) = row.cells.first() else { continue };
                            if row.label.is_none() || cell.format.is_none() {
                                continue;
                            }
                            let (pv, cv) = (
                                check_flag(&pctx, &row.flags).unwrap_or(false),
                                check_flag(&cctx, &row.flags).unwrap_or(false),
                            );
                            if pv || cv {
                                let keep = !(only_diff && pv && cv) || !row_matches(cell, pa, ca, pe, ce);
                                if keep {
                                    rows.push(row);
                                }
                            }
                        }
                        let extra_differs = only_diff
                            && sub.extra.as_ref().is_some_and(|x| {
                                match (format_calc_str(x, pa, None, &st), format_calc_str(x, ca, None, &st)) {
                                    (Ok(a), Ok(b)) => a != b,
                                    _ => true,
                                }
                            });
                        if !rows.is_empty() || extra_differs {
                            subs.push(SubInfo {
                                label: sub.label.clone(),
                                rows,
                                extra: sub.extra.as_deref(),
                            });
                        }
                    }
                    if !subs.is_empty() {
                        let mut height = 2.0;
                        for s in &subs {
                            height += 22.0 + s.rows.len() as f64 * 18.0;
                            if !s.rows.is_empty() {
                                height += 2.0;
                            }
                        }
                        secs.push(SecInfo {
                            colour: &sec.colour,
                            subs,
                            height,
                            x: 0.0,
                            y: 0.0,
                        });
                    }
                }
                // :4773-4788
                let mut col_y: Vec<Option<f64>> = vec![None; max_col];
                let mut max_y = base_y;
                for s in &mut secs {
                    let mut col = 0;
                    let mut min_y = col_y[0].unwrap_or(base_y);
                    for (i, y) in col_y.iter().enumerate().skip(1) {
                        if y.unwrap_or(base_y) < min_y {
                            col = i;
                            min_y = y.unwrap_or(base_y);
                        }
                    }
                    s.x = 4.0 + (card_width + 8.0) * col as f64;
                    s.y = col_y[col].unwrap_or(base_y);
                    col_y[col] = Some(s.y + s.height + 8.0);
                    max_y = max_y.max(s.y + s.height + 8.0);
                }
                let sb = &c.calcs_scroll_bar;
                sb.base.x.set(vp.x + vp.width - 18.0);
                sb.base.y.set(vp.y + skill_header);
                sb.base.height.set(vp.height - skill_header);
                sb.set_content_dimension(f, max_y + 26.0, vp.height - skill_header);
                sg::set_viewport(f, Some([vp.x, vp.y + skill_header, grid_width, vp.height - skill_header]));
                let (cx, cy) = f.get_cursor_pos();
                let (vx, vy) = (cx - vp.x, cy - (vp.y + skill_header));
                for s in &secs {
                    let x = s.x;
                    let y = s.y - sb.offset.get();
                    if !(y + s.height >= 0.0 && y < vp.height) {
                        continue;
                    }
                    sg::set_draw_layer(f, None, -10);
                    sg::set_draw_color_str(f, s.colour);
                    sg::draw_rect(f, x, y, card_width, s.height);
                    sg::set_draw_color(f, 0.10, 0.10, 0.10);
                    sg::draw_rect(f, x + 2.0, y + 2.0, card_width - 4.0, s.height - 4.0);
                    sg::set_draw_layer(f, None, 0);
                    let mut line_y = y;
                    for sub in &s.subs {
                        sg::set_draw_color_str(f, s.colour);
                        sg::draw_rect(f, x + 2.0, line_y, card_width - 4.0, 2.0);
                        sg::draw_string(
                            f,
                            x + 3.0,
                            line_y + 3.0,
                            Some("LEFT"),
                            16.0,
                            "VAR BOLD",
                            &format!("^7{}:", sub.label),
                        );
                        if let Some(extra) = sub.extra {
                            let w = sg::draw_string_width(f, 16.0, "VAR BOLD", &format!("{}:", sub.label));
                            if let (Ok(a), Ok(b)) = (format_calc_str(extra, pa, None, &st), format_calc_str(extra, ca, None, &st)) {
                                sg::draw_string(
                                    f,
                                    x + 3.0 + w + 8.0,
                                    line_y + 3.0,
                                    Some("LEFT"),
                                    16.0,
                                    "VAR",
                                    &format!("{}{a}  ^8|  {}{b}", code("POSITIVE"), code("WARNING")),
                                );
                            }
                        }
                        sg::set_draw_color_str(f, s.colour);
                        sg::draw_rect(f, x + 2.0, line_y + 20.0, card_width - 4.0, 2.0);
                        line_y += 22.0;
                        for row in &sub.rows {
                            let cell = &row.cells[0];
                            let text_size = row.text_size.unwrap_or(14.0);
                            let hovered =
                                vx >= x && vx < x + card_width && vy >= line_y && vy < line_y + 18.0 && vy >= 0.0 && vy < vp.height;
                            let bg = row.bg_col.as_deref().unwrap_or("^0");
                            if hovered {
                                sg::set_draw_color(f, 0.25, 1.0, 0.25);
                                sg::draw_rect(f, x + 2.0, line_y, card_width - 4.0, 18.0);
                                sg::set_draw_color_str(f, bg);
                                sg::draw_rect(f, x + 3.0, line_y + 1.0, card_width - 6.0, 16.0);
                                hover_cell = Some(cell);
                                hover_label = row.label.as_deref().unwrap_or("");
                                hover_rect = (x, line_y);
                            } else {
                                sg::set_draw_color_str(f, bg);
                                sg::draw_rect(f, x + 2.0, line_y, CALCS_LABEL_WIDTH - 2.0, 18.0);
                            }
                            let tc = row.color.as_deref().unwrap_or("^7");
                            sg::draw_string(
                                f,
                                x + CALCS_LABEL_WIDTH,
                                line_y + 1.0,
                                Some("RIGHT_X"),
                                16.0,
                                "VAR",
                                &format!("{tc}{}^7:", row.label.as_deref().unwrap_or("")),
                            );
                            for (vx0, actor) in [(val1, pa), (val2, ca)] {
                                if !hovered {
                                    sg::set_draw_color_str(f, s.colour);
                                    sg::draw_rect(f, x + vx0 - CALCS_SEP_W, line_y, CALCS_SEP_W, 18.0);
                                    sg::set_draw_color_str(f, bg);
                                    sg::draw_rect(f, x + vx0, line_y, val_w, 18.0);
                                }
                                if let Some(fmt) = &cell.format
                                    && let Ok(v) = format_calc_str(fmt, actor, Some(cell), &st)
                                {
                                    sg::draw_string(
                                        f,
                                        x + vx0 + 2.0,
                                        line_y + 9.0 - text_size / 2.0,
                                        Some("LEFT"),
                                        text_size,
                                        "VAR",
                                        &format!("^7{v}"),
                                    );
                                }
                            }
                            line_y += 18.0;
                        }
                        if !sub.rows.is_empty() {
                            line_y += 2.0;
                        }
                    }
                }
            });
        });
        sg::set_viewport(f, None);
        if let Some(cell) = hover_cell {
            let p_label = Self::short_build_name(primary.build_name.borrow().as_deref());
            self.draw_calcs_tooltip(
                f,
                pe,
                ce,
                &p_label,
                &entry.label,
                cell,
                hover_label,
                (hover_rect.0 + vp.x, hover_rect.1 + vp.y + skill_header, card_width, 18.0),
                vp,
            );
        } else if let Some((left, key, rect)) = header_hover {
            draw_skill_breakdown_panel(f, if left { pe } else { ce }, key, rect, vp);
        }
    }

    /// `DrawCalcsTooltip` (CompareCalcsHelpers.lua:173-324).
    #[allow(clippy::too_many_arguments)]
    fn draw_calcs_tooltip(
        &self,
        f: &mut Frame<'_>,
        pe: &Engine,
        ce: &Engine,
        p_label: &str,
        c_label: &str,
        cell: &'static Cell,
        row_label: &str,
        rect: (f64, f64, f64, f64),
        vp: &Rect,
    ) {
        let mut t = self.calcs_tooltip.borrow_mut();
        if t.check_for_update((std::ptr::from_ref(cell) as usize, row_label.to_string())) {
            let rows_of = |e: &Engine, sd: &Entry| -> Vec<ModRowData> {
                let run = e.calcs_run();
                let a = player_actor(&run.env, &run.state.player).unwrap_or_else(|err| panic!("{err}"));
                tabulate_entry(sd, &a)
            };
            t.add_line(f, 16.0, Some(&format!("^7{row_label}")));
            t.add_separator(Some(10.0));
            let pos = code("POSITIVE");
            let warn = code("WARNING");
            for sd in &cell.entries {
                if let Some(b) = &sd.breakdown {
                    let (pl, cl) = (pe.compare_breakdown_lines(b), ce.compare_breakdown_lines(b));
                    if let Some(l) = &pl {
                        t.add_line(f, 14.0, Some(&format!("{pos}{p_label}:")));
                        for x in l {
                            t.add_line(f, 14.0, Some(&format!("^7  {x}")));
                        }
                    }
                    if let Some(l) = &cl {
                        t.add_line(f, 14.0, Some(&format!("{warn}{c_label}:")));
                        for x in l {
                            t.add_line(f, 14.0, Some(&format!("^7  {x}")));
                        }
                    }
                    if pl.is_some() || cl.is_some() {
                        t.add_separator(Some(10.0));
                    }
                }
                if sd.mod_name.is_empty() {
                    continue;
                }
                let (pr, cr) = (rows_of(pe, sd), rows_of(ce, sd));
                if pr.is_empty() && cr.is_empty() {
                    continue;
                }
                let by_key: HashMap<String, &ModRowData> = cr.iter().map(|r| (row_key(r), r)).collect();
                let mut common: Vec<(ModRowData, ModRowData)> = Vec::new();
                let mut p_only: Vec<ModRowData> = Vec::new();
                let mut matched: Vec<String> = Vec::new();
                for r in &pr {
                    let k = row_key(r);
                    if let Some(o) = by_key.get(&k) {
                        common.push((r.clone(), (*o).clone()));
                        matched.push(k);
                    } else {
                        p_only.push(r.clone());
                    }
                }
                let mut c_only: Vec<ModRowData> = cr.iter().filter(|r| !matched.contains(&row_key(r))).cloned().collect();
                t.add_line(f, 14.0, Some(&format!("^7{}:", sd.label.as_deref().unwrap_or("Player modifiers"))));
                if !common.is_empty() {
                    pob_engine::lua::lua_sort(&mut common, &mut |a: &(ModRowData, ModRowData), b: &(ModRowData, ModRowData)| {
                        by_value(&a.0, &b.0)
                    });
                    t.add_line(f, 12.0, Some("^x808080  Common:"));
                    for (a, b) in &common {
                        let (pv, ty, name, mn) = format_mod_row(a, sd, pe);
                        let (cv, _, _, _) = format_mod_row(b, sd, ce);
                        let vs = if pv == cv {
                            sfmt("^7%-10s", &[Arg::Str(&pv)])
                        } else {
                            format!("{pos}{}^7/{warn}{}", sfmt("%-5s", &[Arg::Str(&pv)]), sfmt("%-5s", &[Arg::Str(&cv)]))
                        };
                        let l = sfmt(
                            "    %s ^7%-6s ^7%s%s",
                            &[Arg::Str(&vs), Arg::Str(&ty), Arg::Str(&name), Arg::Str(&mn)],
                        );
                        t.add_line(f, 12.0, Some(&l));
                    }
                }
                let only = |t: &mut Tooltip, f: &mut Frame<'_>, rows: &mut Vec<ModRowData>, color: &str, label: &str, e: &Engine| {
                    if rows.is_empty() {
                        return;
                    }
                    pob_engine::lua::lua_sort(rows, &mut |a: &ModRowData, b: &ModRowData| by_value(a, b));
                    t.add_line(f, 12.0, Some(&format!("{color}  {label} only:")));
                    for r in rows.iter() {
                        let (v, ty, name, mn) = format_mod_row(r, sd, e);
                        let l = sfmt(
                            "    ^7%-10s ^7%-6s ^7%s%s",
                            &[Arg::Str(&v), Arg::Str(&ty), Arg::Str(&name), Arg::Str(&mn)],
                        );
                        t.add_line(f, 12.0, Some(&l));
                    }
                };
                only(&mut t, f, &mut p_only, &pos, p_label, pe);
                only(&mut t, f, &mut c_only, &warn, c_label, ce);
                t.add_separator(Some(6.0));
            }
        }
        sg::set_draw_layer(f, None, 100);
        t.draw(f, rect.0, rect.1, Some((rect.2, rect.3)), vp);
        sg::set_draw_layer(f, None, 0);
    }
}

/// `.shown` of a row of the calcs controls behind the view mode (:2308-2317).
fn gate(g: &Gfx, cs: &CalcsSkillControls, w: &std::rc::Weak<CompareTab>) {
    let list: [&dyn Control; 9] = [
        &*cs.socket_group,
        &*cs.main_skill,
        &*cs.skill_part,
        &*cs.stage_count,
        &*cs.mine_count,
        &*cs.show_minion,
        &*cs.minion,
        &*cs.minion_skill,
        &*cs.mode,
    ];
    for ctl in list {
        let v = ctl.base().shown.get(g);
        let w = w.clone();
        ctl.base().shown.set_fn(move |_| {
            let t = w.upgrade().expect("compareTab");
            *t.view_mode.borrow() == "CALCS" && t.active_compare().is_some() && v
        });
    }
}

/// `DrawSkillBreakdownPanel` (CompareCalcsHelpers.lua:364-474).
fn draw_skill_breakdown_panel(f: &mut Frame<'_>, e: &Engine, key: &str, cell: (f64, f64, f64, f64), vp: &Rect) {
    let Some(mut list): Option<Vec<BuffRow>> = e.compare_breakdown_mods(key) else {
        return;
    };
    if list.is_empty() {
        return;
    }
    pob_engine::lua::lua_sort(&mut list, &mut |a: &BuffRow, b: &BuffRow| {
        a.name > b.name || (a.name == b.name && matches!((a.value.as_num(), b.value.as_num()), (Some(x), Some(y)) if x > y))
    });
    let rows: Vec<[String; 4]> = list
        .iter()
        .map(|r| {
            let (ty, name) = source_of(r.source.as_deref(), e);
            [format_mod_value(&r.value, &r.ty), format_mod_name(&r.name), ty, name]
        })
        .collect();
    let labels = ["Value", "Stat", "Source", "Source Name"];
    let mut widths = [0.0f64; 4];
    for (i, l) in labels.iter().enumerate() {
        widths[i] = sg::draw_string_width(f, 16.0, "VAR", l) + 6.0;
        for r in &rows {
            widths[i] = widths[i].max(sg::draw_string_width(f, 12.0, "VAR", &r[i]) + 6.0);
        }
    }
    let pad = 4.0;
    let panel_w = pad + widths.iter().sum::<f64>();
    let panel_h = 20.0 + rows.len() as f64 * 14.0 + 4.0;
    let (cx, cy, cw, _) = cell;
    let mut x = cx + cw + 5.0;
    if x + panel_w > vp.x + vp.width {
        x = vp.x.max(cx - 5.0 - panel_w);
    }
    let y = cy.min(vp.y + vp.height - panel_h);
    sg::set_draw_layer(f, None, 10);
    sg::set_draw_color_a(f, 0.0, 0.0, 0.0, 0.9);
    sg::draw_rect(f, x + 2.0, y + 2.0, panel_w - 4.0, panel_h - 4.0);
    sg::set_draw_layer(f, None, 11);
    sg::set_draw_color(f, 0.33, 0.66, 0.33);
    sg::draw_rect(f, x, y, panel_w, 2.0);
    sg::draw_rect(f, x, y + panel_h - 2.0, panel_w, 2.0);
    sg::draw_rect(f, x, y, 2.0, panel_h);
    sg::draw_rect(f, x + panel_w - 2.0, y, 2.0, panel_h);
    sg::set_draw_layer(f, None, 10);
    let mut col_x = x + pad;
    let mut xs = [0.0; 4];
    for (i, l) in labels.iter().enumerate() {
        xs[i] = col_x;
        if i > 0 {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
            sg::draw_rect(f, col_x - 2.0, y + 2.0, 1.0, panel_h - 4.0);
        }
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_string(f, col_x, y + 2.0, Some("LEFT"), 16.0, "VAR", l);
        col_x += widths[i];
    }
    let mut row_y = y + 20.0;
    for r in &rows {
        sg::set_draw_color(f, 0.5, 0.5, 0.5);
        sg::draw_rect(f, x + 2.0, row_y - 1.0, panel_w - 4.0, 1.0);
        for i in 0..4 {
            if !r[i].is_empty() {
                sg::draw_string(f, xs[i], row_y + 1.0, Some("LEFT"), 12.0, "VAR", &format!("^7{}", r[i]));
            }
        }
        row_y += 14.0;
    }
    sg::set_draw_layer(f, None, 0);
}
