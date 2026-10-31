//! Jewel radii: `data.setJewelRadiiGlobally` (Modules/Data.lua:583-601) and the `nodesInRadius` lists PassiveTree builds for
//! every socket and keystone (Classes/PassiveTree.lua:654-711).
//!
//! MINEFIELD D7: upstream's `data.jewelRadius` is a global the last `setJewelRadiiGlobally` call left behind. `main:LoadTree`
//! (Main.lua:352-358) sets it to the tree's own version right before `new("PassiveTree")`, so a tree's `nodesInRadius` uses the
//! table of ITS version (3_13 -> `jewelRadii["3_15"]`, 8 entries), while items are parsed after `Build.lua:606` reset it to the
//! latest version (pob-items, `jewelRadiusIndex` from the 3_16 table). Both tables come from `data/rules.json`.
//!
//! A list is upstream's hash `{ [node.id] = node }`; here the ids in ascending order (D13: `pairs` over it is a hash order,
//! the set is what matters -- `tests/radius.rs` checks the sets against LuaJIT, `tools/lua-corpus/export-radius.ts`).

use crate::tree::{PassiveTree, TreeNode};

/// One entry of `data.jewelRadius` after `setJewelRadiiGlobally` (`outerSquared`/`innerSquared` added, Data.lua:592-593).
#[derive(Clone, Debug, PartialEq)]
pub struct RadiusInfo {
    pub inner: f64,
    pub outer: f64,
    pub inner_squared: f64,
    pub outer_squared: f64,
    pub label: String,
}

/// Data.lua:583-601 `data.setJewelRadiiGlobally(treeVersion)`: the table and `data.maxJewelRadius`.
///
/// `treeVersion:match("(%d+)_(%d+)")` then `tonumber(major) <= 3 and tonumber(minor) <= 15` -- e.g. "3_6" is 3_15 too (L-style
/// quirk kept). A version without the pattern is Lua's `attempt to compare nil with number`.
pub fn set_jewel_radii(tree_version: &str) -> (Vec<RadiusInfo>, f64) {
    let caps = pob_lua::match_(tree_version.as_bytes(), b"(%d+)_(%d+)", None)
        .unwrap_or_else(|e| panic!("Modules/Data.lua:584: {}", e.0))
        .unwrap_or_else(|| panic!("Modules/Data.lua:585: attempt to compare nil with number (treeVersion {tree_version:?})"));
    let num = |i: usize| -> f64 {
        let b = caps.get(i).and_then(|c| c.bytes(tree_version.as_bytes())).expect("two captures");
        pob_lua::tonumber::tonumber(std::str::from_utf8(b).expect("ASCII digits")).expect("%d+ is a number")
    };
    let key = if num(0) <= 3.0 && num(1) <= 15.0 { "3_15" } else { "3_16" };
    let table = pob_data::rules::rules()
        .jewel_radii
        .get(key)
        .unwrap_or_else(|| panic!("data.jewelRadii[\"{key}\"] is nil (Data.lua:603)"));
    let mut max = 0.0;
    let list = table
        .iter()
        .map(|r| {
            if r.outer > max {
                max = r.outer;
            }
            RadiusInfo {
                inner: r.inner,
                outer: r.outer,
                inner_squared: r.inner * r.inner,
                outer_squared: r.outer * r.outer,
                label: r.label.clone(),
            }
        })
        .collect();
    (list, max)
}

/// `node.nodesInRadius`: entry `i` is upstream's `nodesInRadius[i + 1]`, ids ascending.
pub type NodesInRadius = Vec<Vec<u32>>;

impl PassiveTree {
    /// PassiveTree.lua:654-711 -- for every socket (a "Charm Socket" gets none, :657) and every keystone, the nodes whose
    /// squared distance lies in `[innerSquared, outerSquared]` of each radius (the bounding-box prefilter of :667-670 kept, its
    /// `>=`/`<=` included). Sockets exclude masteries, proxies and blighted nodes; keystones also `node.isSocket` -- a key no
    /// tree JSON has, so always nil.
    pub(crate) fn build_nodes_in_radius(&mut self) {
        let (radii, max_r) = set_jewel_radii(&self.tree_version);
        let mut out = std::collections::HashMap::new();
        let sockets: Vec<usize> = self.sockets.values().copied().collect();
        for idx in sockets {
            let socket = &self.nodes[idx];
            if socket.name.as_deref() == Some("Charm Socket") {
                continue;
            }
            out.insert(socket.id, self.in_radius(idx, &radii, max_r));
        }
        // `pairs(self.keystoneMap)` -- each node once (`if not keystone.nodesInRadius`), under `dn` and `dn:lower()`.
        let keystones: Vec<usize> = self.keystone_map.values().copied().collect();
        for idx in keystones {
            let id = self.nodes[idx].id;
            if out.contains_key(&id) {
                continue;
            }
            let k = &self.nodes[idx];
            let list = if k.x.is_some() && k.y.is_some() {
                self.in_radius(idx, &radii, max_r)
            } else {
                vec![Vec::new(); radii.len()]
            };
            out.insert(id, list);
        }
        self.nodes_in_radius = out;
        self.jewel_radius = radii;
        self.max_jewel_radius = max_r;
    }

    fn in_radius(&self, centre: usize, radii: &[RadiusInfo], max_r: f64) -> NodesInRadius {
        let c = &self.nodes[centre];
        let (cx, cy) = (c.x.expect("socket.x"), c.y.expect("socket.y"));
        let (min_x, max_x) = (cx - max_r, cx + max_r);
        let (min_y, max_y) = (cy - max_r, cy + max_r);
        let mut lists = vec![Vec::new(); radii.len()];
        for (i, node) in self.nodes.iter().enumerate() {
            if i == centre || !self.candidate(node) {
                continue;
            }
            let (Some(x), Some(y)) = (node.x, node.y) else { continue };
            if !(x >= min_x && x <= max_x && y >= min_y && y <= max_y) {
                continue;
            }
            let (vx, vy) = (x - cx, y - cy);
            let dist_squared = vx * vx + vy * vy;
            for (r, info) in radii.iter().enumerate() {
                if dist_squared <= info.outer_squared && info.inner_squared <= dist_squared {
                    lists[r].push(node.id);
                }
            }
        }
        // `self.nodes` is ascending id already; keep the documented order regardless.
        for l in &mut lists {
            l.sort_unstable();
        }
        lists
    }

    /// `not node.isBlighted and node.group and not node.isProxy and not node.group.isProxy and not node.isMastery`.
    fn candidate(&self, node: &TreeNode) -> bool {
        let Some(g) = node.group else { return false };
        !node.is_blighted && !node.is_proxy && !self.groups[&g].is_proxy && !node.is_mastery
    }

    /// `self.nodes[id].nodesInRadius` of a tree socket or keystone (`nil` for any other node and for a Charm Socket).
    pub fn nodes_in_radius(&self, id: u32) -> Option<&NodesInRadius> {
        self.nodes_in_radius.get(&id)
    }
}
