//! The `pairs` order of a LuaJIT 2.1 table built from `{}` by assigning integer-valued number keys (D13) -- what
//! `for add, val in pairs(additions)` (PassiveSpec.lua:1314) walks. A model of `lj_tab.c` restricted to that case: the array
//! part (slots `0..asize`, slot 0 included), the hash part (`2^hbits` nodes, `hashnum` main positions, `freetop` scanning
//! down, the collision relinking of `lj_tab_newkey`), `rehashtab`/`bestasize`/`lj_tab_resize`, and `lj_tab_next` (array first,
//! then the nodes in index order). Values are never nil here, so only presence is tracked.
//!
//! The model is checked against LuaJIT on a corpus of key sequences (`tests/timeless_pairs.rs`, fixture
//! `tests/fixtures/timeless-pairs.json` from `tools/lua-corpus/export-timeless.ts`).

/// `LJ_MAX_ASIZE` (`(1 << (LJ_MAX_ABITS - 1)) + 1`, LJ_MAX_ABITS = 28).
const LJ_MAX_ABITS: usize = 28;
const LJ_MAX_ASIZE: u32 = (1 << (LJ_MAX_ABITS - 1)) + 1;

#[derive(Clone, Copy, Debug, Default)]
struct Node {
    key: Option<f64>,
    val: bool,
    next: Option<usize>,
}

/// A LuaJIT table with number keys (integral or not).
#[derive(Debug)]
pub struct LjTable {
    /// Array slots `0..asize`: present?
    array: Vec<bool>,
    /// `None` = the shared `nilnode` (hmask 0, no free node).
    nodes: Option<Vec<Node>>,
    freetop: usize,
}

/// `lj_fls`: index of the highest set bit.
fn fls(x: u32) -> u32 {
    31 - x.leading_zeros()
}

/// `hashrot` (lj_tab.h, the x86/x64 variant).
fn hashrot(mut lo: u32, mut hi: u32) -> u32 {
    lo ^= hi;
    hi = hi.rotate_left(14);
    lo = lo.wrapping_sub(hi);
    hi = hi.rotate_left(5);
    hi ^= lo;
    hi = hi.wrapping_sub(lo.rotate_left(13));
    hi
}

/// `hashnum`: `hashrot(lo, hi << 1)` of the double's bits.
fn hashnum(k: f64) -> u32 {
    let bits = k.to_bits();
    hashrot(bits as u32, ((bits >> 32) as u32) << 1)
}

/// `countint`: the bin of an integral key that fits the array part.
fn countint(k: f64, bins: &mut [u32; LJ_MAX_ABITS]) -> u32 {
    let ki = k as i32;
    if f64::from(ki) == k && (ki as u32) < LJ_MAX_ASIZE {
        let ki = ki as u32;
        bins[if ki > 2 { fls(ki - 1) as usize } else { 0 }] += 1;
        1
    } else {
        0
    }
}

/// `bestasize`.
fn bestasize(bins: &[u32; LJ_MAX_ABITS], narray: &mut u32) -> u32 {
    let (mut na, mut sz, nn) = (0u32, 0u32, *narray);
    let mut sum = 0u32;
    let mut b = 0usize;
    while b < LJ_MAX_ABITS && 2 * nn > (1u32 << b) && sum != nn {
        if bins[b] > 0 {
            sum += bins[b];
            if 2 * sum > (1u32 << b) {
                sz = (2u32 << b) + 1;
                na = sum;
            }
        }
        b += 1;
    }
    *narray = sz;
    na
}

/// `hsize2hbits`.
fn hsize2hbits(s: u32) -> u32 {
    match s {
        0 => 0,
        1 => 1,
        _ => 1 + fls(s - 1),
    }
}

impl Default for LjTable {
    fn default() -> Self {
        LjTable::new()
    }
}

impl LjTable {
    /// `{}` -- `lj_tab_new(L, 0, 0)`.
    pub fn new() -> LjTable {
        LjTable {
            array: Vec::new(),
            nodes: None,
            freetop: 0,
        }
    }

    fn hmask(&self) -> usize {
        self.nodes.as_ref().map_or(0, |n| n.len() - 1)
    }

    fn main_pos(&self, k: f64) -> usize {
        (hashnum(k) as usize) & self.hmask()
    }

    /// `t[k] = <non-nil>` (`lj_tab_set`; an existing key keeps its place).
    pub fn set(&mut self, k: f64) {
        let ki = k as i32;
        if f64::from(ki) == k {
            // lj_tab_setint: the array part if `(MSize)key < asize`.
            if (ki as u32 as usize) < self.array.len() {
                self.array[ki as u32 as usize] = true;
                return;
            }
        }
        let k = if k == 0.0 { 0.0 } else { k }; // -0 -> 0 (tvismzero)
        if let Some(nodes) = &self.nodes {
            let mut n = Some(self.main_pos(k));
            while let Some(i) = n {
                if nodes[i].key == Some(k) {
                    // an existing key (its value may have been nil -- not in this model)
                    if let Some(nodes) = &mut self.nodes {
                        nodes[i].val = true;
                    }
                    return;
                }
                n = nodes[i].next;
            }
        }
        self.newkey(k);
    }

    /// `lj_tab_newkey`.
    fn newkey(&mut self, k: f64) {
        let Some(nodes) = self.nodes.as_mut() else {
            self.rehash(k);
            return self.set(k);
        };
        let hmask = nodes.len() - 1;
        let mut n = (hashnum(k) as usize) & hmask;
        if nodes[n].val {
            // Main position taken: find a free node from freetop downwards.
            let mut free = self.freetop;
            loop {
                if free == 0 {
                    self.rehash(k);
                    return self.set(k);
                }
                free -= 1;
                if nodes[free].key.is_none() {
                    break;
                }
            }
            self.freetop = free;
            let occupant = nodes[n].key.expect("a taken node has a key");
            let mut collide = (hashnum(occupant) as usize) & hmask;
            if collide != n {
                // Colliding node not in its main position: move it to the free node.
                while nodes[collide].next != Some(n) {
                    collide = nodes[collide].next.expect("chain reaches the main node");
                }
                nodes[collide].next = Some(free);
                nodes[free] = nodes[n];
                nodes[n].next = None;
                nodes[n].val = false;
            } else {
                // Otherwise use the free node.
                nodes[free].next = nodes[n].next;
                nodes[n].next = Some(free);
                n = free;
            }
        }
        nodes[n].key = Some(k);
        nodes[n].val = true;
    }

    /// `rehashtab(L, t, ek)` + `lj_tab_resize`.
    fn rehash(&mut self, ek: f64) {
        let mut bins = [0u32; LJ_MAX_ABITS];
        // countarray
        let mut asize = 0u32;
        if !self.array.is_empty() {
            let mut i = 0usize;
            for (b, bin) in bins.iter_mut().enumerate() {
                let mut top = 2usize << b;
                if top >= self.array.len() {
                    top = self.array.len() - 1;
                    if i > top {
                        break;
                    }
                }
                let mut n = 0;
                while i <= top {
                    if self.array[i] {
                        n += 1;
                    }
                    i += 1;
                }
                *bin += n;
                asize += n;
            }
        }
        let mut total = 1 + asize;
        // counthash
        if let Some(nodes) = &self.nodes {
            for n in nodes.iter().filter(|n| n.val) {
                asize += countint(n.key.expect("a live node has a key"), &mut bins);
                total += 1;
            }
        }
        asize += countint(ek, &mut bins);
        let na = bestasize(&bins, &mut asize);
        total -= na;
        self.resize(asize as usize, hsize2hbits(total));
    }

    /// `lj_tab_resize(L, t, asize, hbits)`.
    fn resize(&mut self, asize: usize, hbits: u32) {
        let old_array = std::mem::take(&mut self.array);
        let old_nodes = self.nodes.take();
        let mut array = old_array.clone();
        if asize > array.len() {
            array.resize(asize, false);
        }
        if hbits > 0 {
            let hsize = 1usize << hbits;
            self.nodes = Some(vec![Node::default(); hsize]);
            self.freetop = hsize;
        } else {
            self.nodes = None;
            self.freetop = 0;
        }
        if asize < old_array.len() {
            array.truncate(asize);
            self.array = array;
            for (i, &present) in old_array.iter().enumerate().skip(asize) {
                if present {
                    self.set(i as f64);
                }
            }
        } else {
            self.array = array;
        }
        if let Some(old) = old_nodes {
            for n in old.iter().filter(|n| n.val) {
                self.set(n.key.expect("a live node has a key"));
            }
        }
    }

    /// `pairs(t)` -- `lj_tab_next`: array slots 0.. first, then the hash nodes in index order.
    pub fn pairs_keys(&self) -> Vec<f64> {
        let mut out: Vec<f64> = (0..self.array.len()).filter(|&i| self.array[i]).map(|i| i as f64).collect();
        if let Some(nodes) = &self.nodes {
            out.extend(nodes.iter().filter(|n| n.val).map(|n| n.key.expect("a live node has a key")));
        }
        out
    }
}

/// The `pairs` order of `{}` after `t[k] = v` for `keys` in this order (repeats update in place).
pub fn pairs_order(keys: &[f64]) -> Vec<f64> {
    let mut t = LjTable::new();
    for &k in keys {
        t.set(k);
    }
    t.pairs_keys()
}
