//! The node tooltip of `PassiveTreeView` (:1047-1109, `AddNodeName` :1412-1470, `AddNodeTooltip` :1476-1770).
//!
//! `unimplemented`: `build.itemsTab:AddItemTooltip` of a socketed jewel (:1501, U6), `gemTooltip.AddGemTooltip` of a node's granted skill
//! (:1629-1643, `skillTooltip`, U5), the compare spec's sections of `AddNodeTooltip` (:1482-1511, :1574-1594: the jewel sections need U6), `launch.devModeAlt`.

use pob_engine::engine::Engine;
use pob_gfx::{Frame, Gfx};
use pob_tree::{NodeType, PassiveSpec, SpecNode};

use crate::colors::color_code;
use crate::common::{not_supported_mod_tooltips, show_flavour_text};
use crate::control::Rect;
use crate::tree_art::{TreeArt, overlay_size};
use crate::tree_view::PassiveTreeView;

fn cc(name: &str) -> String {
    color_code(name).unwrap_or_default()
}

/// :1047-1109 (the part inside the node loop); the caller checked the conditions of :1047.
#[allow(clippy::too_many_arguments)]
pub fn draw_node_tooltip(
    view: &mut PassiveTreeView,
    f: &mut Frame<'_>,
    e: &Engine,
    art: &TreeArt,
    spec: &PassiveSpec,
    i: usize,
    scr_x: f64,
    scr_y: f64,
    scale: f64,
    view_port: &Rect,
) {
    let node = &spec.nodes[i];
    crate::sg::set_draw_layer(f, None, 100);
    let size = (overlay_size(node.ty()).unwrap_or(0.0) * scale).floor();
    let params = (i, view.show_stat_differences, view.trace_path.clone(), e.output_revision());
    if view.tooltip.check_for_update(params) {
        add_node_tooltip(view, f, e, art, spec, node, i, None);
    }
    view.tooltip.center = true;
    let (tt_width, tt_height) = view.tooltip.get_dynamic_size(f, view_port);
    let (skill_width, skill_height) = view.skill_tooltip.get_dynamic_size(f, view_port);
    let fat_skill = skill_width > skill_height * 1.5;
    let has_skill = !view.skill_tooltip.lines.is_empty();
    let (total_width, total_height) = if fat_skill {
        (
            tt_width.max(if has_skill { skill_width } else { 0.0 }),
            tt_height + if has_skill { skill_height } else { 0.0 },
        )
    } else {
        (
            tt_width + if has_skill { skill_width } else { 0.0 },
            tt_height.max(if has_skill { skill_height } else { 0.0 }),
        )
    };
    let node_x = (scr_x + size).floor();
    let mut tt_x = node_x;
    let node_y = (scr_y - size).floor();
    let mut tt_y = node_y;
    let r_over_by = tt_x + total_width - view_port.x - view_port.width;
    if r_over_by > 0.0 {
        tt_x -= r_over_by;
    }
    let btm_over_by = tt_y + total_height - view_port.y - view_port.height;
    if btm_over_by > 0.0 {
        tt_y -= btm_over_by;
    }
    crate::sg::set_draw_layer(f, None, 100);
    if fat_skill {
        view.tooltip.draw(
            f,
            node_x.min(view_port.width - tt_width + view_port.x),
            tt_y.max(view_port.y),
            None,
            view_port,
        );
    } else {
        view.tooltip.draw(
            f,
            tt_x.max(view_port.x),
            node_y.min(view_port.height - tt_height + view_port.y),
            None,
            view_port,
        );
    }
    crate::sg::set_draw_layer(f, None, 99);
    if fat_skill {
        view.skill_tooltip.draw(f, tt_x, tt_y + tt_height, None, view_port);
    } else {
        view.skill_tooltip.draw(f, tt_x + tt_width + 5.0, tt_y, None, view_port);
    }
}

/// `AddCompareNodeTooltip` (:1772-1830) + `AddCompareNodeName` (:1832-1856).
pub fn add_compare_node_tooltip(view: &mut PassiveTreeView, g: &Gfx, _e: &Engine, art: &TreeArt, node: &SpecNode) {
    let flavour = show_flavour_text();
    let font_size_big = if flavour { 18.0 } else { 16.0 };
    view.tooltip.center = true;
    view.tooltip.max_width = Some(800.0);
    let an = art.nodes.get(&node.id);
    view.tooltip.recipe = an.filter(|n| !n.recipe.is_empty()).map(|n| n.recipe.clone());
    view.tooltip.tooltip_header = Some(
        match node.ty() {
            NodeType::Mastery => "MASTERYALLOC",
            NodeType::Notable | NodeType::Normal if node.ascendancy_name().is_some() => "ASCENDANCY",
            NodeType::Normal => "PASSIVE",
            NodeType::Notable => "NOTABLE",
            NodeType::Socket => "JEWEL",
            NodeType::Keystone => "KEYSTONE",
            _ => "UNKNOWN",
        }
        .to_string(),
    );
    let name = if flavour {
        format!("^xF8E6CA{}", node.dn())
    } else {
        node.dn().to_string()
    };
    line(view, g, 24.0, &name, Some("FONTIN"));
    view.tooltip.center = false;
    let only = format!("{}This node is only in the compared build", cc("DEXTERITY"));
    if node.ty() == NodeType::Socket {
        view.tooltip.add_separator(Some(14.0));
        line(view, g, 14.0, &only, None);
        return;
    }
    if !node.sd().is_empty() {
        line(view, g, 16.0, "", None);
        for (k, l) in node.sd().iter().enumerate() {
            let unsupported = node
                .mods()
                .get(k)
                .and_then(|m| m.as_ref())
                .is_some_and(|m| l != " " && (m.extra.is_some() || m.list.is_none()));
            let col = if unsupported { cc("UNSUPPORTED") } else { cc("MAGIC") };
            line(view, g, font_size_big, &format!("{col}{l}"), Some("FONTIN"));
        }
    }
    if let Some(rem) = node.reminder_text() {
        view.tooltip.add_separator(Some(14.0));
        for l in rem {
            line(view, g, 14.0, &format!("^xA0A080{l}"), None);
        }
    }
    if flavour
        && let Some(n) = an
        && !n.flavour_text.is_empty()
    {
        view.tooltip.add_separator(Some(14.0));
        for l in &n.flavour_text {
            line(view, g, font_size_big, &format!("{}{l}", cc("UNIQUE")), Some("FONTIN ITALIC"));
        }
    }
    view.tooltip.add_separator(Some(14.0));
    line(view, g, 14.0, &only, None);
}

/// `PassiveMasteryControl:AddValueTooltip` (PassiveMasteryControl.lua:50-56): the node's tooltip as if `effect` were chosen; drawn by the list.
pub fn mastery_effect_tooltip(
    view: &mut PassiveTreeView,
    tooltip: &mut crate::tooltip::Tooltip,
    g: &Gfx,
    e: &Engine,
    art: &TreeArt,
    spec: &PassiveSpec,
    node_id: u32,
    effect: f64,
) {
    let Some(i) = spec.node_idx(node_id) else { return };
    let mut node = spec.mastery_effect_node(i, effect as u64);
    node.alloc = spec.nodes[i].alloc;
    node.all_mastery_options = false;
    node.path.clone_from(&spec.nodes[i].path);
    node.path_dist = spec.nodes[i].path_dist;
    node.depends.clone_from(&spec.nodes[i].depends);
    node.linked.clone_from(&spec.nodes[i].linked);
    let temp = std::rc::Rc::new(node.clone());
    std::mem::swap(&mut view.tooltip, tooltip);
    view.tooltip.clear(false);
    add_node_tooltip(view, g, e, art, spec, &node, i, Some(temp));
    std::mem::swap(&mut view.tooltip, tooltip);
}

/// `tooltip:AddLine(size, text, font)`.
fn line(view: &mut PassiveTreeView, g: &Gfx, size: f64, text: &str, font: Option<&'static str>) {
    view.tooltip.add_line_ex(g, size, Some(text), font, false, None);
}

/// `AddNodeName` (:1412-1470).
fn add_node_name(view: &mut PassiveTreeView, g: &Gfx, e: &Engine, art: &TreeArt, node: &SpecNode) {
    let flavour = show_flavour_text();
    let font_size_big = if flavour { 18.0 } else { 16.0 };
    let an = art.nodes.get(&node.id);
    view.tooltip.recipe = an.filter(|n| !n.recipe.is_empty()).map(|n| n.recipe.clone());
    view.tooltip.tooltip_header = Some(
        match node.ty() {
            NodeType::Mastery => {
                if node.alloc {
                    "MASTERYALLOC"
                } else {
                    "MASTERY"
                }
            }
            NodeType::Notable | NodeType::Normal if node.ascendancy_name().is_some() => "ASCENDANCY",
            NodeType::Normal => "PASSIVE",
            NodeType::Notable => "NOTABLE",
            NodeType::Socket => "JEWEL",
            NodeType::Keystone => "KEYSTONE",
            _ => "UNKNOWN",
        }
        .to_string(),
    );
    let name = if flavour {
        format!("^xF8E6CA{}", node.dn())
    } else {
        node.dn().to_string()
    };
    view.tooltip.center = true;
    line(view, g, 24.0, &name, Some("FONTIN"));
    view.tooltip.center = false;
    if node.ty() == NodeType::Socket
        && let Some(nir) = node.nodes_in_radius()
    {
        // :1446-1452 `pairs(node.nodesInRadius[2])`: a sum
        let mut totals = [0.0; 3];
        if let Some(list) = nir.get(1) {
            for id in list {
                for (k, a) in ["Str", "Dex", "Int"].iter().enumerate() {
                    totals[k] += e.node_attrib_sum(*id, a);
                }
            }
        }
        view.tooltip.add_separator(Some(14.0));
        for (k, (code, name)) in [
            ("STRENGTH", "Strength"),
            ("DEXTERITY", "Dexterity"),
            ("INTELLIGENCE", "Intelligence"),
        ]
        .iter()
        .enumerate()
        {
            if totals[k] >= 40.0 {
                line(
                    view,
                    g,
                    font_size_big,
                    &format!("^7Can support {}{name} ^7threshold jewels", cc(code)),
                    Some("FONTIN"),
                );
            }
        }
    }
    if node.ty() == NodeType::Socket
        && node.alloc
        && let Some(d) = node.distance_to_class_start
        && d > 0.0
    {
        view.tooltip.add_separator(Some(14.0));
        line(view, g, 16.0, &format!("^7Distance to start: {}", d as i64), Some("FONTIN"));
    }
}

/// `addModInfoToTooltip` (:1543-1566).
fn add_mod_info(view: &mut PassiveTreeView, g: &Gfx, node: &SpecNode, i: usize, text: &str, font_size_big: f64) {
    let Some(Some(m)) = node.mods().get(i) else { return };
    if text != " " && (m.extra.is_some() || m.list.is_none()) {
        let mut l = format!("{}{text}", cc("UNSUPPORTED"));
        if not_supported_mod_tooltips() {
            l.push_str(" ^8(Not supported in PoB yet)");
        }
        line(view, g, font_size_big, &l, Some("FONTIN"));
    } else {
        line(view, g, font_size_big, &format!("{}{text}", cc("MAGIC")), Some("FONTIN"));
    }
}

/// `AddNodeTooltip` (:1476-1770), without `returnEarly`; `temp`: the mastery popup's node with the hovered effect.
#[allow(clippy::too_many_lines)]
fn add_node_tooltip(
    view: &mut PassiveTreeView,
    g: &Gfx,
    e: &Engine,
    art: &TreeArt,
    spec: &PassiveSpec,
    node: &SpecNode,
    i: usize,
    temp: Option<std::rc::Rc<SpecNode>>,
) {
    let flavour = show_flavour_text();
    let font_size_big = if flavour { 18.0 } else { 16.0 };
    view.skill_tooltip.clear(false);
    view.tooltip.center = true;
    view.tooltip.max_width = Some(800.0);
    let tip = cc("TIP");
    let ty = node.ty();
    let tn = node.tree_node();

    // :1497-1522 sockets
    if ty == NodeType::Socket && node.alloc {
        let has_jewel = spec
            .jewels
            .get(f64::from(node.id))
            .copied()
            .and_then(|id| e.result().env.item(id).cloned())
            .is_some();
        if has_jewel {
            view.note_unimplemented("PassiveTreeView.lua:1501 itemsTab:AddItemTooltip of the socketed jewel (U6)");
        } else {
            add_node_name(view, g, e, art, node);
        }
        if let Some(d) = node.distance_to_class_start
            && d > 0.0
        {
            view.tooltip.add_separator(Some(14.0));
            line(view, g, 16.0, &format!("^7Distance to start: {}", d as i64), None);
        }
        view.tooltip.add_separator(Some(14.0));
        // :1517 `socket:IsEnabled()`: a socket slot is enabled when its node is allocated
        line(
            view,
            g,
            14.0,
            &format!("{tip}Tip: Right click this socket to go to the items page and choose the jewel for this socket."),
            None,
        );
        line(view, g, 14.0, &format!("{tip}Tip: Hold Shift or Ctrl to hide this tooltip."), None);
        return;
    }

    // :1534
    add_node_name(view, g, e, art, node);
    view.tooltip.center = false;

    // :1596-1647 (no compare spec: `mNode` = `node`, `masteryColor` = "")
    let sd = node.sd();
    let is_runegraft = node.override_type() == Some("AlternateMastery");
    if !sd.is_empty() && node.all_mastery_options && !is_runegraft {
        view.tooltip.add_separator(Some(14.0));
        line(view, g, 14.0, "^7Available Mastery node options are:", None);
        line(view, g, 6.0, "", None);
        let mut line_count = 0;
        let effects = &tn.mastery_effect_ids;
        for (n, effect) in effects.iter().enumerate() {
            let existing = spec.mastery_selections.values().any(|v| v == effect);
            if let Some(data) = spec.tree.mastery_effect(*effect) {
                for l in &data.sd {
                    line_count += 1;
                    if !existing {
                        add_mod_info(view, g, node, line_count - 1, l, font_size_big);
                    }
                }
            }
            if !existing && n + 1 < effects.len() {
                line(view, g, 6.0, "", None);
            }
        }
        view.tooltip.add_separator(Some(14.0));
    }
    if !sd.is_empty() && (!node.all_mastery_options || is_runegraft) {
        line(view, g, 16.0, "", None);
        for (k, l) in sd.iter().enumerate() {
            add_mod_info(view, g, node, k, l, font_size_big);
        }
        if node
            .mods()
            .iter()
            .flatten()
            .filter_map(|m| m.list.as_ref())
            .flatten()
            .any(|m| matches!(&*m.name, "ExtraSkill" | "ExtraSupport"))
        {
            view.note_unimplemented("PassiveTreeView.lua:1629-1643 gemTooltip.AddGemTooltip of a granted skill (U5)");
        }
    }

    // :1651-1656 reminder text
    if let Some(rem) = node.reminder_text() {
        view.tooltip.add_separator(Some(14.0));
        for l in rem {
            line(view, g, 14.0, &format!("^xA0A080{l}"), None);
        }
    }
    // :1659-1664 flavour text
    if flavour
        && let Some(n) = art.nodes.get(&node.id)
        && !n.flavour_text.is_empty()
    {
        view.tooltip.add_separator(Some(14.0));
        for l in &n.flavour_text {
            line(view, g, font_size_big, &format!("{}{l}", cc("UNIQUE")), Some("FONTIN ITALIC"));
        }
    }

    // :1671-1680 tattoo editing
    let stat_line = |s: &str| {
        ["+30 to Dexterity", "+30 to Strength", "+30 to Intelligence"]
            .iter()
            .any(|p| s.contains(p))
    };
    if (node.is_tattoo()
        || (ty == NodeType::Normal && matches!(node.dn(), "Strength" | "Dexterity" | "Intelligence"))
        || (ty == NodeType::Notable && !sd.is_empty() && stat_line(&sd[0]))
        || ty == NodeType::Keystone
        || ty == NodeType::Mastery)
        && !node.is_subgraph_node()
    {
        view.tooltip.add_separator(Some(14.0));
        let t = if ty == NodeType::Mastery { "runegraft" } else { "tattoo" };
        line(view, g, 14.0, &format!("{tip}Tip: Right click to edit the {t} for this node"), None);
    }

    // :1683-1734 mod differences
    let trace = view.trace_path.clone();
    let path_nodes: Vec<usize> = if node.alloc {
        node.depends.clone()
    } else if let Some(t) = &trace {
        t.clone()
    } else {
        node.path.clone().unwrap_or_default()
    };
    let path_length = path_nodes.len();
    if view.show_stat_differences {
        view.tooltip.add_separator(Some(14.0));
        match e.node_stat_compare(i, trace.as_deref(), temp) {
            Ok(c) => {
                for l in &c.lines {
                    line(view, g, 14.0, l, None);
                }
                if c.count == 0 {
                    if c.is_granted {
                        line(
                            view,
                            g,
                            14.0,
                            "^7This node is granted by an item. Removing it will cause no changes",
                            None,
                        );
                    } else {
                        // :1727 `node.intuitiveLeapLikesAffecting == 0` compares a table with 0: never true
                        line(
                            view,
                            g,
                            14.0,
                            &format!(
                                "^7No changes from {} this node.",
                                if node.alloc { "unallocating" } else { "allocating" }
                            ),
                            None,
                        );
                    }
                }
            }
            Err(err) => panic!("{err}"),
        }
        line(
            view,
            g,
            14.0,
            &format!("{tip}Tip: Press Ctrl+D to disable the display of stat differences."),
            None,
        );
    } else {
        view.tooltip.add_separator(Some(14.0));
        line(
            view,
            g,
            14.0,
            &format!("{tip}Tip: Press Ctrl+D to enable the display of stat differences."),
            None,
        );
    }

    // :1737-1750 pathing distance
    view.tooltip.add_separator(Some(14.0));
    if let Some(path) = node.path.as_ref().filter(|p| !p.is_empty()) {
        if view.trace_mode && trace.as_ref().is_some_and(|t| t.contains(&i)) {
            line(
                view,
                g,
                14.0,
                &format!("^7{} nodes in trace path", trace.as_ref().map_or(0, Vec::len)),
                None,
            );
            line(view, g, 14.0, &tip, None);
        } else {
            let leap = if node.intuitive_leap_likes_affecting.is_empty() {
                ""
            } else {
                " ^8(Can be allocated without pathing to it)"
            };
            line(
                view,
                g,
                14.0,
                &format!("^7{} points to node{leap}", pob_lua::tostring_num(node.path_dist)),
                None,
            );
            line(view, g, 14.0, &tip, None);
            if path.len() > 1 {
                line(
                    view,
                    g,
                    14.0,
                    "Tip: To reach this node by a different path, hold Shift, then trace the path and click this node",
                    None,
                );
            }
        }
    }
    let _ = path_length;
    // :1751-1763 gold
    let mut gold_cost = e.gold_respec_price();
    if node.ascendancy_name().is_some() {
        gold_cost *= 5.0;
    }
    let nf = |v: f64| pob_engine::display::format_num_sep(&pob_lua::tostring_num(v), e.settings());
    if node.depends.len() > 1 {
        view.tooltip.add_separator(Some(14.0));
        line(
            view,
            g,
            14.0,
            &format!("^7{} points gained from unallocating these nodes", node.depends.len()),
            None,
        );
        line(
            view,
            g,
            14.0,
            &format!(
                "^xFFD700{} Gold ^7required to unallocate these nodes",
                nf(node.depends.len() as f64 * gold_cost)
            ),
            None,
        );
        line(view, g, 14.0, &tip, None);
    } else if node.alloc {
        line(
            view,
            g,
            14.0,
            &format!(
                "^xFFD700{} Gold ^7required to unallocate this node",
                nf(node.depends.len() as f64 * gold_cost)
            ),
            None,
        );
        line(view, g, 14.0, &tip, None);
    }
    if ty == NodeType::Socket {
        line(view, g, 14.0, &format!("{tip}Tip: Hold Shift or Ctrl to hide this tooltip."), None);
    } else {
        line(view, g, 14.0, &format!("{tip}Tip: Hold Ctrl to hide this tooltip."), None);
        line(view, g, 14.0, &format!("{tip}Tip: Press Ctrl+C to copy this node's text."), None);
    }
}
