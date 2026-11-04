//! `CompareTab:DrawSummary` and `DrawStatList` (CompareTab.lua:3162-3398).

use std::rc::Rc;

use pob_engine::display::{DisplayActor, DisplayEnv, MinionBorrow, format_num_sep, match_flags};
use pob_engine::lua::LuaV;
use pob_engine::stats::{DISPLAY_STATS, DisplayStat, MINION_DISPLAY_STATS};
use pob_gfx::Frame;
use pob_lua::format::Arg;

use super::layout::*;
use super::{CompareEntry, CompareTab};
use crate::colors::color_code;
use crate::control::Rect;
use crate::modules::build::BuildMode;
use crate::sg;

fn code(n: &str) -> String {
    color_code(n).unwrap_or_else(|| panic!("colorCodes.{n}"))
}

fn fmt(f: &str, v: f64) -> String {
    pob_lua::format::format(f, &[Arg::Num(v)]).unwrap_or_else(|e| panic!("{e}"))
}

/// `actor.mainSkill.minion and mainEnv.minion` over a build.
fn actor<'a, 'b>(d: &'b DisplayEnv<'a>, use_minion: bool) -> &'b DisplayActor<'a> {
    match (use_minion, d.minion.as_ref()) {
        (true, Some(m)) => m,
        _ => &d.player,
    }
}

impl CompareTab {
    /// `DrawSummary` (:3162-3306).
    pub(super) fn draw_summary(&self, f: &mut Frame<'_>, vp: &Rect, entry: &Rc<CompareEntry>) {
        let primary = self.primary();
        let (peng, ceng) = (primary.engine.borrow(), entry.b.engine.borrow());
        let (pe, ce) = (peng.as_ref().expect("engine"), ceng.as_ref().expect("engine"));
        let (pm, cm) = (MinionBorrow::new(pe.result()), MinionBorrow::new(ce.result()));
        let pd = DisplayEnv::new(pe.result(), &pm).unwrap_or_else(|e| panic!("{e}"));
        let cd = DisplayEnv::new(ce.result(), &cm).unwrap_or_else(|e| panic!("{e}"));
        let (p_minion, c_minion) = (pd.minion.is_some(), cd.minion.is_some());
        let header_height = 22.0;
        let header_reserve = SUMMARY_HEADER_HEIGHT;
        let scroll_view_h = vp.height - header_reserve;
        let col1 = SUMMARY_COL1;
        let col2r = SUMMARY_COL2_RIGHT;
        let primary_name = Self::short_build_name(primary.build_name.borrow().as_deref());
        let compare_name = &entry.label;
        let compare_name_w = sg::draw_string_width(f, header_height, "VAR", compare_name);
        let min_col3r = col2r + compare_name_w + 16.0;
        let max_col3r = vp.width - 200.0;
        let col3r = SUMMARY_COL3_RIGHT.max(min_col3r).min(max_col3r);
        let col4 = col3r + 20.0;
        sg::set_viewport(f, Some([vp.x, vp.y, vp.width, header_reserve]));
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_string(f, col1, 4.0, Some("LEFT"), header_height, "VAR", "^7Stat");
        sg::draw_string(
            f,
            col2r,
            4.0,
            Some("RIGHT_X"),
            header_height,
            "VAR",
            &format!("{}{primary_name}", code("POSITIVE")),
        );
        sg::draw_string(
            f,
            col3r,
            4.0,
            Some("RIGHT_X"),
            header_height,
            "VAR",
            &format!("{}{compare_name}", code("WARNING")),
        );
        sg::draw_string(f, col4, 4.0, Some("LEFT"), header_height, "VAR", "^7Difference");
        sg::set_draw_color(f, 0.5, 0.5, 0.5);
        sg::draw_rect(f, 4.0, header_height + 8.0, vp.width - 8.0, 2.0);
        sg::set_viewport(f, Some([vp.x, vp.y + header_reserve, vp.width, scroll_view_h]));
        let mut draw_y = -self.scroll_y.get();
        let stats: &[DisplayStat] = if p_minion || c_minion {
            MINION_DISPLAY_STATS
        } else {
            DISPLAY_STATS
        };
        let settings = BuildMode::display_settings();
        draw_y = draw_stat_list(
            f,
            draw_y,
            stats,
            actor(&pd, p_minion),
            actor(&cd, c_minion),
            col1,
            col4,
            col2r,
            col3r,
            &settings,
        );
        draw_y += 16.0;
        sg::set_draw_color(f, 0.5, 0.5, 0.5);
        sg::draw_rect(f, 4.0, draw_y, vp.width - 8.0, 2.0);
        draw_y += 8.0;
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_string(f, POWER_REPORT_LEFT, draw_y, Some("LEFT"), 20.0, "VAR", "^7Compare Power Report");
        draw_y += 24.0;
        // the coroutine: `calcFunc` of both engines, without the borrows of this frame's rows
        drop(pd);
        drop(cd);
        drop(pm);
        drop(cm);
        drop(peng);
        drop(ceng);
        self.run_compare_power_report(f, entry);
        let c = self.c();
        let control_y = vp.y + header_reserve + draw_y;
        let base_x = vp.x + POWER_REPORT_LEFT;
        c.power_stat_select.base.x.set(base_x + 60.0);
        c.power_stat_select.base.y.set(control_y);
        sg::draw_string(f, POWER_REPORT_LEFT, draw_y, Some("LEFT"), 16.0, "VAR", "^7Metric:");
        let mut check_x = base_x + 280.0;
        let checks = [
            &c.power_tree_check,
            &c.power_items_check,
            &c.power_gems_check,
            &c.power_support_gems_check,
            &c.power_config_check,
        ];
        for (i, ch) in checks.into_iter().enumerate() {
            let lw = ch.label_width.get();
            ch.base.x.set(check_x + lw);
            ch.base.y.set(control_y);
            if i < 4 {
                check_x += lw + 26.0;
            }
        }
        draw_y += 28.0;
        self.sync_power_list();
        let list_height = 250.0;
        let list = &c.power_report_list;
        list.base.x.set(vp.x + POWER_REPORT_LEFT);
        list.base.y.set(vp.y + header_reserve + draw_y);
        list.base.width.set(740.0);
        list.base.height.set(list_height);
        draw_y += list_height + 20.0;
        self.summary_total_content_height.set(draw_y + self.scroll_y.get() + 36.0);
        sg::set_viewport(f, None);
    }
}

/// `DrawStatList` (:3309-3398).
#[allow(clippy::too_many_arguments)]
fn draw_stat_list(
    f: &mut Frame<'_>,
    mut draw_y: f64,
    stats: &[DisplayStat],
    p: &DisplayActor<'_>,
    c: &DisplayActor<'_>,
    col1: f64,
    col4: f64,
    col2r: f64,
    col3r: f64,
    settings: &pob_engine::display::DisplaySettings,
) -> f64 {
    let line_height = 16.0;
    for sd in stats {
        if sd.stat.is_none() && sd.label.is_none() {
            draw_y += 6.0;
        } else if sd.stat == Some("SkillDPS")
            || sd.hide_stat
            || (!match_flags(sd.flag, sd.not_flag, p.skill_flags) && !match_flags(sd.flag, sd.not_flag, c.skill_flags))
        {
        } else if let Some(stat) = sd.stat {
            let val = |a: &DisplayActor<'_>| -> f64 {
                let v = a.view.get(stat);
                let v = match sd.child_stat {
                    Some(ch) => match v {
                        LuaV::Table(_) => v.index(ch).unwrap_or(LuaV::Nil),
                        _ => LuaV::Num(0.0),
                    },
                    None => v,
                };
                match v {
                    LuaV::Num(n) => n,
                    _ => 0.0,
                }
            };
            let (pv, cv) = (val(p), val(c));
            let cond = |v: f64, a: &DisplayActor<'_>| match sd.cond_func {
                None => true,
                Some(cf) => cf(&LuaV::Num(v), &a.view).unwrap_or_else(|e| panic!("{e}")).truthy(),
            };
            if (pv != 0.0 || cv != 0.0) && (sd.cond_func.is_none() || cond(pv, p) || cond(cv, c)) {
                let stat_fmt = sd.fmt.unwrap_or("d");
                let mult = if sd.pc || sd.mod_ { 100.0 } else { 1.0 };
                let ps = format_num_sep(&fmt(&format!("%{stat_fmt}"), pv * mult), settings);
                let cs = format_num_sep(&fmt(&format!("%{stat_fmt}"), cv * mult), settings);
                let diff = cv - pv;
                let mut diff_str = String::new();
                let mut diff_color = "^7".to_string();
                if !(-0.001..=0.001).contains(&diff) {
                    let better = (sd.lower_is_better && diff < 0.0) || (!sd.lower_is_better && diff > 0.0);
                    diff_color = code(if better { "POSITIVE" } else { "NEGATIVE" });
                    diff_str = format_num_sep(&fmt(&format!("%+{stat_fmt}"), diff * mult), settings);
                    if pv != 0.0 {
                        let pc = cv / pv * 100.0 - 100.0;
                        diff_str.push_str(&fmt(" (%+.1f%%)", pc));
                    }
                }
                let label_color = sd.color.unwrap_or("^7");
                sg::draw_string(
                    f,
                    col1,
                    draw_y,
                    Some("LEFT"),
                    line_height,
                    "VAR",
                    &format!("{label_color}{}", sd.label.unwrap_or(stat)),
                );
                sg::draw_string(f, col2r, draw_y, Some("RIGHT_X"), line_height, "VAR", &format!("^7{ps}"));
                sg::draw_string(
                    f,
                    col3r,
                    draw_y,
                    Some("RIGHT_X"),
                    line_height,
                    "VAR",
                    &format!("{}{cs}", code("SPLITPERSONALITY")),
                );
                if !diff_str.is_empty() {
                    sg::draw_string(
                        f,
                        col4,
                        draw_y,
                        Some("LEFT"),
                        line_height,
                        "VAR",
                        &format!("{diff_color}{diff_str}"),
                    );
                }
                draw_y += line_height + 1.0;
            }
        } else if let (Some(label), Some(cf)) = (sd.label, sd.cond_func) {
            // `statData.condFunc(primaryOutput)`: the first argument is the output
            let shown = |a: &DisplayActor<'_>| cf(&LuaV::Nil, &a.view).unwrap_or_else(|e| panic!("{e}")).truthy();
            let (ps, cs) = (shown(p), shown(c));
            if ps || cs {
                let label_color = sd.color.unwrap_or("^7");
                let v = sd.val.unwrap_or("");
                sg::draw_string(f, col1, draw_y, Some("LEFT"), line_height, "VAR", &format!("{label_color}{label}"));
                sg::draw_string(
                    f,
                    col2r,
                    draw_y,
                    Some("RIGHT_X"),
                    line_height,
                    "VAR",
                    &format!("^7{}", if ps { v } else { "-" }),
                );
                sg::draw_string(
                    f,
                    col3r,
                    draw_y,
                    Some("RIGHT_X"),
                    line_height,
                    "VAR",
                    &format!("{}{}", code("WARNING"), if cs { v } else { "-" }),
                );
                draw_y += line_height + 1.0;
            }
        }
    }
    draw_y
}
