//! The node tooltip of `PassiveTreeView` (:1047-1109, `AddNodeName` :1412-1470, `AddNodeTooltip` :1476-1770).

use pob_engine::engine::Engine;
use pob_gfx::{Frame, Gfx};
use pob_tree::tree::NodeModLine;
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
    let params = (
        i,
        view.show_stat_differences,
        view.trace_path.clone(),
        crate::launch::launch().dev_mode_alt.get(),
        e.output_revision(),
    );
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

/// `addCompareJewelSection` (:1482-1495): the jewel of the compared build in this socket, through that build's items tab with the socket of this one.
fn add_compare_jewel_section(
    view: &mut PassiveTreeView,
    g: &Gfx,
    node_id: u32,
    c_jewel: Option<&std::rc::Rc<pob_items::item::Item>>,
    with_label: bool,
) -> bool {
    let allocated = view
        .compare_spec
        .as_ref()
        .is_some_and(|c| c.node_idx(node_id).is_some_and(|k| c.nodes[k].alloc));
    let Some(jewel) = c_jewel.filter(|_| allocated) else {
        return false;
    };
    if with_label {
        view.tooltip.add_separator(Some(14.0));
        line(view, g, 14.0, &format!("{}Compared build:", cc("DEXTERITY")), None);
    }
    // `self.compareSpec.build.itemsTab`, on the tab of one build its own
    let owner = view.compare_owner.as_ref().and_then(std::rc::Weak::upgrade);
    let own = view.owner_build();
    let socket_tab = own.items_tab.borrow().clone();
    let tab = owner.map_or_else(|| socket_tab.clone(), |b| b.items_tab.borrow().clone());
    if let (Some(tab), Some(socket_tab)) = (tab, socket_tab) {
        tab.add_foreign_socket_tooltip(&mut view.tooltip, g, jewel, &socket_tab, node_id);
    }
    true
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
/// `self.viewer:AddNodeTooltip(tooltip, node, build)` of a node of the spec, into `tooltip`.
pub fn plain_node_tooltip(
    view: &mut PassiveTreeView,
    tooltip: &mut crate::tooltip::Tooltip,
    g: &Gfx,
    e: &Engine,
    art: &TreeArt,
    spec: &PassiveSpec,
    node_id: u32,
) {
    let Some(i) = spec.node_idx(node_id) else { return };
    std::mem::swap(&mut view.tooltip, tooltip);
    view.tooltip.clear(false);
    add_node_tooltip(view, g, e, art, spec, &spec.nodes[i], i, None);
    std::mem::swap(&mut view.tooltip, tooltip);
}

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
    // :1435-1444 the ids of `launch.devModeAlt`
    let dev = crate::launch::launch().dev_mode_alt.get();
    let name = if dev { format!("{name} [{}]", node.id) } else { name };
    view.tooltip.center = true;
    line(view, g, 24.0, &name, Some("FONTIN"));
    view.tooltip.center = false;
    if dev && node.id > 65535 {
        // decompose the cluster node id
        let (id, l) = (node.id, pob_lua::tostring_num);
        let text = format!(
            "^7Cluster node index: {}, size: {}, large index: {}, medium index: {}",
            l(f64::from(id & 0xF)),
            l(f64::from((id >> 4) & 0x3)),
            l(f64::from((id >> 6) & 0x7)),
            l(f64::from((id >> 9) & 0x3))
        );
        line(view, g, font_size_big, &text, None);
    }
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

/// `AddNodeName` from outside the view's tree tooltip (ItemsTab.lua:875, NotableDBControl.lua:247).
pub fn add_node_name_pub(view: &mut PassiveTreeView, g: &Gfx, e: &Engine, node: &SpecNode) {
    let art = view.art(&crate::launch::launch().cfg.script_path, &e.spec_rc().tree);
    add_node_name(view, g, e, &art, node);
}

/// What `self.build.spec.tree.keystoneMap[name]` holds (PassiveTree.lua:739-741): a node of the tree, or a legacy keystone of `legion.nodes`.
enum TreeKeystone<'a> {
    Tree(&'a pob_tree::tree::TreeNode),
    Legion(&'a pob_tree::timeless::LegionNode),
}

/// `self.viewer:AddNodeTooltip(tt, keyStoneNode, self.build, true)` (TreeTab.lua:1595-1598): the name, the lines and the texts of the node up to `returnEarly` (:1666-1668). The node is not one
/// of the spec, so it has no `alloc`, `power` or `finalModList`; legion nodes have no `recipe`, `reminderText` or `flavourText` in the data (LegionPassives.lua).
pub fn keystone_tooltip(
    view: &mut PassiveTreeView,
    tooltip: &mut crate::tooltip::Tooltip,
    g: &Gfx,
    e: &Engine,
    art: &TreeArt,
    spec: &PassiveSpec,
    name: &str,
) {
    let tree = &spec.tree;
    let node = if let Some(&i) = tree.keystone_map.get(name) {
        TreeKeystone::Tree(&tree.nodes[i])
    } else {
        let legion = tree.legion().unwrap_or_else(|err| panic!("{}", err.0));
        match legion.keystone_map.get(name) {
            Some(&i) => TreeKeystone::Legion(&legion.nodes[i]),
            None => return,
        }
    };
    std::mem::swap(&mut view.tooltip, tooltip);
    add_keystone_tooltip(view, g, e, art, &node);
    std::mem::swap(&mut view.tooltip, tooltip);
}

fn add_keystone_tooltip(view: &mut PassiveTreeView, g: &Gfx, e: &Engine, art: &TreeArt, node: &TreeKeystone<'_>) {
    let flavour = show_flavour_text();
    let font_size_big = if flavour { 18.0 } else { 16.0 };
    let dev = crate::launch::launch().dev_mode_alt.get();
    view.skill_tooltip.clear(false);
    view.tooltip.center = true;
    view.tooltip.max_width = Some(800.0);
    let (dn, sd, stats, reminder, an) = match node {
        TreeKeystone::Tree(n) => (&n.dn, &n.sd, &n.stats, n.reminder_text.as_deref(), art.nodes.get(&n.id)),
        TreeKeystone::Legion(n) => (&n.dn, &n.sd, &n.stats, None, None),
    };
    // :1412-1444 `AddNodeName`
    view.tooltip.recipe = an.filter(|n| !n.recipe.is_empty()).map(|n| n.recipe.clone());
    view.tooltip.tooltip_header = Some("KEYSTONE".to_string());
    let name = if flavour { format!("^xF8E6CA{dn}") } else { dn.clone() };
    let name = match (dev, node) {
        (true, TreeKeystone::Tree(n)) => format!("{name} [{}]", n.id),
        (true, TreeKeystone::Legion(n)) => format!("{name} [{}]", n.id),
        (false, _) => name,
    };
    line(view, g, 24.0, &name, Some("FONTIN"));
    view.tooltip.center = false;
    if dev && let TreeKeystone::Legion(_) = node {
        // :1437 `node.id > 65535`, and the id of a legion node is a string
        panic!("Classes/PassiveTreeView.lua:1437: attempt to compare number with string");
    }
    // :1621-1647
    if !sd.is_empty() {
        line(view, g, 16.0, "", None);
        for (k, l) in sd.iter().enumerate() {
            add_mod_info(view, g, &stats.mods, k, l, font_size_big);
        }
        add_granted_skills(view, g, e, stats.mod_list.mods.iter());
    }
    // :1651-1664
    if let Some(rem) = reminder {
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
}

/// :1626-1646 the child tooltip of a granted skill.
fn add_granted_skills<'a>(view: &mut PassiveTreeView, g: &Gfx, e: &Engine, mods: impl Iterator<Item = &'a std::rc::Rc<pob_mods::Mod>>) {
    let granted: Vec<(String, f64)> = mods
        .filter(|m| matches!(&*m.name, "ExtraSkill" | "ExtraSupport"))
        .filter_map(|m| {
            let id = m.value.get("skillId");
            Some((id.as_str()?.to_string(), m.value.get("level").as_num().unwrap_or(1.0)))
        })
        .collect();
    let skills = pob_data::skills::skills();
    for (id, level) in granted {
        let Some(effect) = skills.get(&id) else { continue };
        let data = pob_data::gems::gems();
        let gem = data.gem_for_skill.get(&effect.id).and_then(|x| data.gems.0.get(x));
        crate::gem_tooltip::add_gem_tooltip(
            &mut view.skill_tooltip,
            g,
            e,
            &crate::gem_tooltip::GemInst {
                gem,
                effect,
                level,
                quality: 0.0,
                display: None,
                skip_requirements: gem.is_none(),
            },
        );
    }
}

/// `addModInfoToTooltip` (:1543-1566).
fn add_mod_info(view: &mut PassiveTreeView, g: &Gfx, mods: &[Option<NodeModLine>], i: usize, text: &str, font_size_big: f64) {
    let Some(Some(m)) = mods.get(i) else { return };
    // :1545-1556 modifier debugging info
    let mut owned = text.to_string();
    if crate::launch::launch().dev_mode_alt.get()
        && let Some(list) = &m.list
    {
        let mut mod_str: Option<String> = None;
        for x in list {
            let s = pob_mods::format::format_mod(x);
            mod_str = Some(match mod_str {
                Some(a) => format!("{a}, {s}"),
                None => format!("^2{s}"),
            });
        }
        if let Some(extra) = &m.extra {
            let e = format!("{}{extra}", cc("NEGATIVE"));
            mod_str = Some(match mod_str {
                Some(a) => format!("{a}  {e}"),
                None => e,
            });
        }
        if let Some(s) = mod_str {
            owned = format!("{owned}  {s}");
        }
    }
    let text = owned.as_str();
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
        let jewel = spec
            .jewels
            .get(f64::from(node.id))
            .copied()
            .and_then(|id| e.result().env.item(id).cloned());
        let c_jewel = view.compare_jewel(e, spec, node.id);
        if let Some(jewel) = &jewel {
            let tab = view.owner_build().items_tab.borrow().clone();
            if let Some(tab) = tab {
                tab.add_socket_tooltip(&mut view.tooltip, g, jewel, node.id);
            }
            if !crate::tree_view::jewels_equal(Some(jewel), c_jewel.as_ref()) {
                add_compare_jewel_section(view, g, node.id, c_jewel.as_ref(), true);
            }
        } else if !add_compare_jewel_section(view, g, node.id, c_jewel.as_ref(), false) {
            add_node_name(view, g, e, art, node);
        }
        if c_jewel.is_none()
            && view
                .compare_spec
                .as_ref()
                .is_some_and(|c| c.node_idx(node.id).is_some_and(|k| c.nodes[k].alloc))
        {
            view.tooltip.add_separator(Some(14.0));
            line(view, g, 16.0, &format!("{}Compared build: ^7(empty socket)", cc("WARNING")), None);
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

    // :1525-1531 an unallocated socket: the jewel of the compared build
    if ty == NodeType::Socket {
        let c_jewel = view.compare_jewel(e, spec, node.id);
        if add_compare_jewel_section(view, g, node.id, c_jewel.as_ref(), false) {
            line(view, g, 14.0, &format!("{tip}Tip: Hold Shift or Ctrl to hide this tooltip."), None);
            return;
        }
    }

    // :1534
    add_node_name(view, g, e, art, node);
    view.tooltip.center = false;
    // :1536-1541 power debugging info
    if crate::launch::launch().dev_mode_alt.get()
        && let Some(tab) = view.owner_build().tree_tab.borrow().clone()
        && let Some(power) = tab.power.borrow().as_ref().and_then(|r| r.nodes.get(i).cloned())
        && let Some(offence) = power.offence
    {
        let defence = power
            .defence
            .expect("Classes/PassiveTreeView.lua:1539: bad argument #3 to 'format' (number expected, got nil)");
        let text = pob_lua::format::format(
            "DPS power: %g   Defence power: %g",
            &[pob_lua::format::Arg::Num(offence), pob_lua::format::Arg::Num(defence)],
        )
        .unwrap_or_else(|err| panic!("{err}"));
        line(view, g, 16.0, &text, None);
    }

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
                        add_mod_info(view, g, node.mods(), line_count - 1, l, font_size_big);
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
            add_mod_info(view, g, node.mods(), k, l, font_size_big);
        }
        add_granted_skills(view, g, e, node.mods().iter().flatten().filter_map(|m| m.list.as_ref()).flatten());
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
