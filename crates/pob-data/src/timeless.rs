//! Timeless-jewel data (R5.5c, MINEFIELD W6): the binary LUTs, `data.nodeIDList` and the legion passives.
//!
//! * **LUTs** -- `Modules/DataJewelFileLoader.lua` + `Modules/DataLegionLookUpTableHelper.lua` (`loadTimelessJewel`, `readLUT`),
//!   line for line. The compressed files (`<stem>.zip`, else `<stem>.zip.part0..N` concatenated -- one RFC1950 zlib stream) are
//!   copied verbatim from the PoB checkout into `data/timeless-luts/` by `tools/sync-data.ps1`, which records the sha256 of every
//!   file in `data.lock` (`timelessLuts.files`). They are checked against the lock, inflated (`flate2`) on first use and kept
//!   for the process (`OnceLock` per jewel type) -- upstream's `.bin` cache file is not written.
//! * **`data.nodeIDList`** -- `Data/TimelessJewelData/NodeIndexMapping.lua` as converted to `data/node-id-list.json`.
//! * **Legion passives** -- `Data/TimelessJewelData/LegionPassives.lua` RAW (the table PassiveTree.lua:79 `require`s), a data patch
//!   taken from LuaJIT (`data-patches/legion-passives.json`, `tools/lua-corpus/export-legion.ts`); the processing of
//!   PassiveTree.lua:723-787 is pob-tree's.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::OnceLock;

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value as Json;

use crate::wire::OrderedMap;
use crate::{DataError, data_dir, ensure_data, lock_path, rules::rules, verify_lock};

const HELPER: &str = "Modules/DataLegionLookUpTableHelper.lua";

/// `data.nodeIDList[nodeId]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeIdEntry {
    pub index: u32,
    pub size: u32,
}

/// `data.nodeIDList` (NodeIndexMapping.lua).
#[derive(Debug)]
pub struct NodeIdList {
    pub nodes: HashMap<u32, NodeIdEntry>,
    /// `nodeIDList.size` (GV node count).
    pub size: u32,
    /// `nodeIDList.sizeNotable`.
    pub size_notable: u32,
    /// `nodeIDList.localIdToGlobalId[jewelType]` at `[jewelType - 1]`.
    pub local_id_to_global_id: Vec<HashMap<u32, u32>>,
    /// index -> node id (the `pairs(data.nodeIDList)` search of :50-56; indices are unique).
    by_index: Vec<u32>,
}

fn u32_of(j: &Json, what: &str) -> Result<u32, DataError> {
    j.as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| DataError(format!("pob-data: node-id-list.json: {what} is not a u32: {j}")))
}

fn parse_node_id_list(j: &Json) -> Result<NodeIdList, DataError> {
    let nodes_j = j["nodes"]
        .as_object()
        .ok_or_else(|| DataError("pob-data: node-id-list.json: no `nodes`".into()))?;
    let mut nodes = HashMap::new();
    for (k, v) in nodes_j {
        let id: u32 = k
            .parse()
            .map_err(|_| DataError(format!("pob-data: node-id-list.json: node key {k}")))?;
        nodes.insert(
            id,
            NodeIdEntry {
                index: u32_of(&v["index"], "index")?,
                size: u32_of(&v["size"], "size")?,
            },
        );
    }
    let size = u32_of(&j["size"], "size")?;
    let size_notable = u32_of(&j["sizeNotable"], "sizeNotable")?;
    let mut local_id_to_global_id = Vec::new();
    for m in j["localIdToGlobalId"].as_array().into_iter().flatten() {
        let mut map = HashMap::new();
        for (k, v) in m.as_object().into_iter().flatten() {
            if k == "size" {
                continue;
            }
            let local: u32 = k
                .parse()
                .map_err(|_| DataError(format!("pob-data: node-id-list.json: localIdToGlobalId key {k}")))?;
            map.insert(local, u32_of(v, "localIdToGlobalId value")?);
        }
        local_id_to_global_id.push(map);
    }
    let mut by_index = vec![u32::MAX; nodes.len()];
    for (&id, e) in &nodes {
        let slot = by_index
            .get_mut(e.index as usize)
            .ok_or_else(|| DataError(format!("pob-data: node-id-list.json: index {} out of range", e.index)))?;
        if *slot != u32::MAX {
            return Err(DataError(format!("pob-data: node-id-list.json: index {} twice", e.index)));
        }
        *slot = id;
    }
    Ok(NodeIdList {
        nodes,
        size,
        size_notable,
        local_id_to_global_id,
        by_index,
    })
}

/// `data.nodeIDList`.
pub fn node_id_list() -> &'static NodeIdList {
    static L: OnceLock<NodeIdList> = OnceLock::new();
    L.get_or_init(|| {
        let j: Json = crate::load("node-id-list.json");
        parse_node_id_list(&j).unwrap_or_else(|e| panic!("{e}"))
    })
}

/// `data.timelessJewelTypes[jewelType]`, `SeedMin`, `SeedMax` (Data.lua:914-952), 1-based `jewel_type`.
pub fn timeless_jewel_type(jewel_type: usize) -> Option<(&'static str, f64, f64)> {
    let r = rules();
    let i = jewel_type.checked_sub(1)?;
    let name = r.timeless_jewel_types.get(i)?.as_str()?;
    let min = r.timeless_jewel_seed_min.get(i)?.as_f64()?;
    let max = r.timeless_jewel_seed_max.get(i)?.as_f64()?;
    Some((name, min, max))
}

/// `data.timelessJewelAdditions` (`#legionAdditions`).
pub fn timeless_jewel_additions() -> f64 {
    rules().timeless_jewel_additions
}

/// `<data>/timeless-luts`.
pub fn lut_dir() -> PathBuf {
    data_dir().join("timeless-luts")
}

/// `data.lock` `timelessLuts.files` (name -> sha256).
fn locked_lut_files() -> Result<&'static IndexMap<String, String>, DataError> {
    static F: OnceLock<Result<IndexMap<String, String>, DataError>> = OnceLock::new();
    F.get_or_init(|| {
        let dir = ensure_data()?;
        let lock = lock_path(dir);
        verify_lock(dir, &lock)?;
        let text = std::fs::read_to_string(&lock).map_err(|e| DataError(format!("pob-data: {}: {e}", lock.display())))?;
        let j: Json = serde_json::from_str(&text).map_err(|e| DataError(format!("pob-data: {}: {e}", lock.display())))?;
        let files = j["timelessLuts"]["files"].as_object().ok_or_else(|| {
            DataError(format!(
                "pob-data: {} has no timelessLuts section -- run tools/sync-data.ps1",
                lock.display()
            ))
        })?;
        Ok(files
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
            .collect())
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// DataJewelFileLoader.lua:59-84: `<stem>.zip`, else `<stem>.zip.part0`, `.part1`, ... until one is missing, concatenated and
/// inflated. Every file read must be the one `data.lock` names (sha256). `None` = nothing to read ("Failed to load jewel data").
pub fn load_jewel_file(stem: &str) -> Result<Option<Vec<u8>>, DataError> {
    let locked = locked_lut_files()?;
    let dir = lut_dir();
    let read = |name: &str| -> Result<Option<Vec<u8>>, DataError> {
        let path = dir.join(name);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(DataError(format!("pob-data: {}: {e}", path.display()))),
        };
        let want = locked.get(name).ok_or_else(|| {
            DataError(format!(
                "pob-data: {} is not in data.lock -- run tools/sync-data.ps1",
                path.display()
            ))
        })?;
        let got = crate::sha256::sha256_hex(&bytes);
        if &got != want {
            return Err(DataError(format!(
                "pob-data: {} has sha256 {got}, data.lock has {want} -- run tools/sync-data.ps1",
                path.display()
            )));
        }
        Ok(Some(bytes))
    };
    let compressed = match read(&format!("{stem}.zip"))? {
        Some(b) => b,
        None => {
            let mut all = Vec::new();
            let mut part = 0;
            while let Some(b) = read(&format!("{stem}.zip.part{part}"))? {
                all.extend_from_slice(&b);
                part += 1;
            }
            all
        }
    };
    if compressed.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(&compressed[..])
        .read_to_end(&mut out)
        .map_err(|e| DataError(format!("pob-data: Inflate({stem}): {e}")))?;
    Ok(Some(out))
}

/// `data.timelessJewelLUTs[jewelType].data` (and `.sizes` for GV) per jewel type, inflated once per process.
static LUTS: [OnceLock<Result<Vec<u8>, DataError>>; 11] = [const { OnceLock::new() }; 11];

/// The inflated LUT of one jewel type (`loadTimelessJewel`, :43-90).
fn lut(jewel_type: usize) -> Result<&'static [u8], DataError> {
    let slot = LUTS
        .get(jewel_type.wrapping_sub(1))
        .ok_or_else(|| DataError(format!("{HELPER}:43: no timeless jewel type {jewel_type}")))?;
    slot.get_or_init(|| {
        let (name, _, _) = timeless_jewel_type(jewel_type).expect("checked by the caller");
        // `data.timelessJewelTypes[jewelType]:gsub("%s+", "")`
        let stem: String = name.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        match load_jewel_file(&stem)? {
            Some(b) => Ok(b),
            // :43-71 -- `jewelData` nil: the type's table stays without `data`, and readLUT's assert fires.
            None if jewel_type == 1 => Err(DataError(format!("{HELPER}:224: Error occurred loading Glorious Vanity data"))),
            None => Err(DataError(format!("{HELPER}:226: Error occurred loading Timeless Jewel data"))),
        }
    })
    .as_deref()
    .map_err(Clone::clone)
}

/// Whether the LUT of `jewel_type` has been loaded in this process (the first `readLUT` of a type inflates it).
pub fn lut_loaded(jewel_type: usize) -> bool {
    LUTS.get(jewel_type.wrapping_sub(1)).is_some_and(|s| s.get().is_some())
}

/// The inflated bytes of `jewel_type`'s LUT (what upstream caches as `<stem>.bin`) -- for the byte check against the probe.
pub fn lut_bytes(jewel_type: usize) -> Result<&'static [u8], DataError> {
    timeless_jewel_type(jewel_type).ok_or_else(|| DataError(format!("{HELPER}:43: no timeless jewel type {jewel_type}")))?;
    lut(jewel_type)
}

/// `convertLocalIdToGlobalId` (:94-100).
fn convert_local_id_to_global_id(jewel_type: usize, local_id: u32) -> u32 {
    node_id_list()
        .local_id_to_global_id
        .get(jewel_type - 1)
        .and_then(|m| m.get(&local_id))
        .copied()
        .unwrap_or(local_id)
}

/// `s:byte(pos)` (1-based); `None` beyond the string (Lua returns nothing).
fn byte_at(s: &[u8], pos: i64, line: u32) -> Result<Option<u8>, DataError> {
    if pos < 1 {
        return Err(DataError(format!(
            "unimplemented: {HELPER}:{line}: string.byte at {pos} (seed outside the LUT range)"
        )));
    }
    Ok(usize::try_from(pos - 1).ok().and_then(|p| s.get(p)).copied())
}

/// A 0-based integer from a Lua number used as a string position (`seed - SeedMin`).
fn int_offset(x: f64, line: u32) -> Result<i64, DataError> {
    if x.fract() != 0.0 || !x.is_finite() {
        return Err(DataError(format!(
            "unimplemented: {HELPER}:{line}: non-integral string.byte position (seed offset {x})"
        )));
    }
    Ok(x as i64)
}

/// `data.readLUT(seed, nodeID, jewelType)` (DataLegionLookUpTableHelper.lua:221-261): the LUT bytes of `nodeID` for `seed`,
/// local ids converted to global ones; `{}` when the node has no entry. Errors: what upstream raises (a failed load, GV with a
/// node outside `nodeIDList` before any GV load), and unported edge cases (non-integral or out-of-range positions).
pub fn read_lut(seed: f64, node_id: u32, jewel_type: usize) -> Result<Vec<u32>, DataError> {
    let (_, seed_min, seed_max) =
        timeless_jewel_type(jewel_type).ok_or_else(|| DataError(format!("{HELPER}:221: no timeless jewel type {jewel_type}")))?;
    let list = node_id_list();
    let entry = list.nodes.get(&node_id).copied();
    // loadTimelessJewel (:11-14): GV without an index does not load; readLUT's assert then indexes a nil table.
    if jewel_type == 1 && entry.is_none() && !lut_loaded(1) {
        return Err(DataError(format!(
            "{HELPER}:224: attempt to index field '?' (a nil value) (Glorious Vanity LUT never loaded)"
        )));
    }
    let data = lut(jewel_type)?;
    let seed = if jewel_type == 5 { seed / 20.0 } else { seed };
    let seed_offset = int_offset(seed - seed_min, 232)?;
    let seed_size = int_offset(seed_max - seed_min, 233)? + 1;
    let Some(entry) = entry else {
        // :257 ConPrintf("ERROR: Missing Index lookup for nodeID: ..")
        return Ok(Vec::new());
    };
    let index = i64::from(entry.index);
    if jewel_type == 1 {
        let gv = gv_node(data, list, entry.index, seed_size)?;
        // :239 `sizes:byte(index * seedSize + seedOffset + 1)`
        let Some(data_length) = byte_at(gv.sizes, index * seed_size + seed_offset + 1, 239)? else {
            return Err(DataError(format!("{HELPER}:241: 'for' limit must be a number (no size byte)")));
        };
        let start = gv.seed_start(seed_offset)?;
        let mut result = Vec::with_capacity(data_length as usize);
        for i in 0..usize::from(data_length) {
            // `data[index + 1][seedOffset + 1]:byte(i)` -- the seed's slice of the node's raw block.
            match gv.raw.get(start + i).copied() {
                Some(b) => result.push(u32::from(b)),
                None => {
                    return Err(DataError(format!(
                        "unimplemented: {HELPER}:240: GV slice shorter than its size byte (node {node_id}, seed {seed})"
                    )));
                }
            }
        }
        if data_length == 2 || data_length == 3 {
            result[0] = convert_local_id_to_global_id(1, result[0]);
        } else if data_length == 6 || data_length == 8 {
            for r in result.iter_mut().take(usize::from(data_length / 2)) {
                *r = convert_local_id_to_global_id(1, *r);
            }
        }
        return Ok(result);
    }
    if entry.index <= list.size_notable {
        // :253 `data:byte(index * seedSize + seedOffset + 1)`; nothing there -> `{ convert(nil) }` = `{}`.
        return Ok(byte_at(data, index * seed_size + seed_offset + 1, 253)?
            .map(|b| vec![convert_local_id_to_global_id(jewel_type, u32::from(b))])
            .unwrap_or_default());
    }
    Ok(Vec::new())
}

/// One GV node, as `loadTimelessJewel` lays it out (:57-86): `sizes` = the first `nodeCount * seedSize + 1` bytes, then each
/// node's raw block (`nodeIDList[id].size` bytes, in index order), cut per seed by the size bytes. Upstream cuts every node at load; here a node on its first read,
/// after which a read costs what upstream's does.
struct GvNode {
    sizes: &'static [u8],
    raw: &'static [u8],
    /// Byte offset in `raw` of each seed's slice (`seedKey` 1..=seedSize+1, :66-71), 0-based.
    starts: Vec<u32>,
}

impl GvNode {
    fn seed_start(&self, seed_offset: i64) -> Result<usize, DataError> {
        usize::try_from(seed_offset)
            .ok()
            .and_then(|o| self.starts.get(o))
            .map(|&s| s as usize)
            .ok_or_else(|| {
                DataError(format!(
                    "unimplemented: {HELPER}:240: GV seed offset {seed_offset} outside the node"
                ))
            })
    }
}

/// Node `index`'s block and the start of every seed's slice in it.
fn gv_node(data: &'static [u8], list: &NodeIdList, index: u32, seed_size: i64) -> Result<&'static GvNode, DataError> {
    static OFFSETS: OnceLock<Vec<usize>> = OnceLock::new();
    static NODES: OnceLock<Vec<OnceLock<GvNode>>> = OnceLock::new();
    let ss = usize::try_from(seed_size).map_err(|_| DataError(format!("{HELPER}:60: seed size {seed_size}")))?;
    let node_count = list.size as usize;
    let size_offset = node_count * ss;
    let i = index as usize;
    let nodes = NODES.get_or_init(|| (0..list.by_index.len()).map(|_| OnceLock::new()).collect());
    let Some(slot) = nodes.get(i) else {
        return Err(DataError(format!("{HELPER}:240: GV node index {index} outside nodeIDList")));
    };
    Ok(slot.get_or_init(|| {
        // :61 `jewelData:sub(1, sizeOffset + 1)` (clamped like `sub`).
        let sizes = &data[..(size_offset + 1).min(data.len())];
        // :64-75 -- node i's raw block starts after the sizes and every earlier node's block.
        let offsets = OFFSETS.get_or_init(|| {
            let mut v = Vec::with_capacity(node_count + 1);
            let mut off = size_offset;
            for i in 0..node_count {
                v.push(off);
                let id = list.by_index.get(i).copied().unwrap_or(u32::MAX);
                off += list.nodes.get(&id).map_or(0, |e| e.size as usize);
            }
            v.push(off);
            v
        });
        let (from, to) = (
            offsets.get(i).copied().unwrap_or(usize::MAX),
            offsets.get(i + 1).copied().unwrap_or(usize::MAX),
        );
        let raw = &data[from.min(data.len())..to.min(data.len())];
        // :66-71 -- `for seedKey = 1, seedSize + 1`: slice `seedKey` starts after the lengths of the earlier ones.
        let mut starts = Vec::with_capacity(ss + 1);
        let mut off = 0u32;
        for seed_key in 1..=ss + 1 {
            starts.push(off);
            off += u32::from(sizes.get(i * ss + seed_key - 1).copied().unwrap_or(0));
        }
        GvNode { sizes, raw, starts }
    }))
}

// --- Abyss jewels -----------------------------------------------------------------------------------------------------------

const ABYSS_HELPER: &str = "Modules/DataAbyssJewelLookUpTableHelper.lua";

/// One part of a modification (:31-44): `type` 1 replaces the node, 2 adds to it; `id` is global.
#[derive(Clone, Debug, PartialEq)]
pub struct AbyssComponent {
    pub ty: u8,
    pub id: u32,
    pub rolls: Vec<f64>,
}

/// `data.getAbyssJewelComponentRoll(component, node, statIndex)` (:59-73) over `node.stats`: the stat with `index == stat_index` and its roll.
pub fn abyss_component_roll<'s>(
    component: &AbyssComponent,
    stats: &'s OrderedMap<LegionStat>,
    stat_index: usize,
) -> (Option<&'s LegionStat>, Option<f64>) {
    let Some(stat) = stats.values().find(|m| m.index == stat_index as f64) else {
        return (None, None);
    };
    let roll = stat_index.checked_sub(1).and_then(|i| component.rolls.get(i)).copied();
    // :69 A negative stored value is shown as a positive number in "reduced" descriptions.
    (Some(stat), roll.map(|r| if r < 0.0 && stat.min >= 0.0 { -r } else { r }))
}

/// `jewelLUT.seedOffsets[blockKey]` of one block (:192-201).
struct AbyssBlock {
    offset: usize,
    seeds: OnceLock<Result<Vec<u32>, DataError>>,
}

impl AbyssBlock {
    fn new(offset: usize) -> AbyssBlock {
        AbyssBlock {
            offset,
            seeds: OnceLock::new(),
        }
    }
}

/// `data.timelessJewelLUTs[jewelType]` of an Abyss file (:103-112).
struct AbyssLut {
    data: &'static [u8],
    /// `format == "ABYN"` (Zorath: one block per passive node) or `"ABYS"` (one block per socket).
    by_node: bool,
    seed_minimum: f64,
    seed_maximum: f64,
    seed_increment: f64,
    seed_count: usize,
    blocks: HashMap<u32, AbyssBlock>,
    ascendancies: HashMap<Vec<u8>, AbyssBlock>,
}

/// `s_byte(jewelData, offset)`, 0-based here.
fn abyss_byte(data: &[u8], offset: usize, line: u32) -> Result<usize, DataError> {
    data.get(offset).map(|&b| usize::from(b)).ok_or_else(|| {
        DataError(format!(
            "{ABYSS_HELPER}:{line}: attempt to perform arithmetic on a nil value (offset {offset})"
        ))
    })
}

/// `bytesToUShort` (Common.lua:384-387): a missing byte reads 0.
fn abyss_u16(data: &[u8], offset: usize) -> u32 {
    let b = |o: usize| u32::from(data.get(o).copied().unwrap_or(0));
    b(offset) + b(offset + 1) * 256
}

/// `skipModification` (:16-24).
fn skip_modification(data: &[u8], mut offset: usize) -> Result<usize, DataError> {
    let count = abyss_byte(data, offset, 18)?;
    offset += 1;
    for _ in 0..count {
        offset += 3 + abyss_byte(data, offset + 2, 21)? * 2;
    }
    Ok(offset)
}

/// `readModification` (:26-47).
fn read_modification(data: &[u8], mut offset: usize, jewel_type: usize) -> Result<(Vec<AbyssComponent>, usize), DataError> {
    let count = abyss_byte(data, offset, 28)?;
    let mut out = Vec::with_capacity(count);
    offset += 1;
    for _ in 0..count {
        let ty = abyss_byte(data, offset, 31)? as u8;
        let local = abyss_byte(data, offset + 1, 31)? as u32;
        let stat_count = abyss_byte(data, offset + 2, 39)?;
        offset += 3;
        let mut rolls = Vec::with_capacity(stat_count);
        for _ in 0..stat_count {
            let roll = abyss_u16(data, offset);
            offset += 2;
            rolls.push(if roll >= 32768 {
                f64::from(roll) - 65536.0
            } else {
                f64::from(roll)
            });
        }
        out.push(AbyssComponent {
            ty,
            id: convert_local_id_to_global_id(jewel_type, local),
            rolls,
        });
    }
    Ok((out, offset))
}

/// `skipSocketRecord` (:75-82).
fn skip_socket_record(data: &[u8], mut offset: usize) -> Result<usize, DataError> {
    let count = abyss_byte(data, offset, 76)?;
    offset += 1;
    for _ in 0..count {
        offset = skip_modification(data, offset + 2)?;
    }
    Ok(offset)
}

/// `skipAscendancyRecord` (:84-86).
fn skip_ascendancy_record(data: &[u8], offset: usize) -> Result<usize, DataError> {
    Ok(offset + 1 + abyss_byte(data, offset, 85)? * 2)
}

/// `parseAbyssJewel(jewelType, jewelData)` (:88-164).
fn parse_abyss_jewel(jewel_type: usize, data: &'static [u8]) -> Result<AbyssLut, DataError> {
    let format = data.get(..4).unwrap_or(data);
    let version = data.get(4).copied();
    let stored = data.get(5).copied().map(usize::from);
    let seed_minimum = abyss_u16(data, 6);
    let seed_maximum = abyss_u16(data, 8);
    let seed_increment = abyss_u16(data, 10);
    let mut offset = 12;
    if version != Some(1) || stored != Some(jewel_type) || seed_increment == 0 {
        return Err(DataError(format!(
            "{ABYSS_HELPER}:100: Invalid Abyss timeless jewel header for jewel type {jewel_type}"
        )));
    }
    let seed_count_f = (f64::from(seed_maximum) - f64::from(seed_minimum)) / f64::from(seed_increment) + 1.0;
    let seed_count = seed_count_f as usize;
    let mut blocks = HashMap::new();
    let mut ascendancies = HashMap::new();
    let by_node = match format {
        b"ABYS" => {
            // :114-128 Each socket has one entry for every seed.
            let socket_count = abyss_byte(data, offset, 116)?;
            offset += 2;
            let mut ids = Vec::with_capacity(socket_count);
            for _ in 0..socket_count {
                ids.push(abyss_u16(data, offset));
                offset += 2;
            }
            for id in ids {
                blocks.insert(id, AbyssBlock::new(offset));
                for _ in 0..seed_count {
                    offset = skip_socket_record(data, offset)?;
                }
            }
            false
        }
        b"ABYN" => {
            // :129-159 Zorath has one block per passive node, so only allocated path nodes are read later.
            let node_count = abyss_u16(data, offset) as usize;
            offset += 2;
            let mut ids = Vec::with_capacity(node_count);
            for _ in 0..node_count {
                ids.push(abyss_u16(data, offset));
                offset += 2;
            }
            for id in ids {
                blocks.insert(id, AbyssBlock::new(offset));
                for _ in 0..seed_count {
                    offset = skip_modification(data, offset)?;
                }
            }
            if data.get(offset..offset + 4) != Some(b"ASCS") {
                return Err(DataError(format!(
                    "{ABYSS_HELPER}:145: Missing Abyss ascendancy section for jewel type {jewel_type}"
                )));
            }
            offset += 4;
            let ascendancy_count = abyss_u16(data, offset) as usize;
            offset += 2;
            for _ in 0..ascendancy_count {
                let name_length = abyss_byte(data, offset, 152)?;
                let name = data.get(offset + 1..offset + 1 + name_length).unwrap_or_default().to_vec();
                offset += name_length + 1;
                ascendancies.insert(name, AbyssBlock::new(offset));
                for _ in 0..seed_count {
                    offset = skip_ascendancy_record(data, offset)?;
                }
            }
            true
        }
        other => {
            return Err(DataError(format!(
                "{ABYSS_HELPER}:161: Unknown Abyss timeless jewel format {}",
                String::from_utf8_lossy(other)
            )));
        }
    };
    Ok(AbyssLut {
        data,
        by_node,
        seed_minimum: f64::from(seed_minimum),
        seed_maximum: f64::from(seed_maximum),
        seed_increment: f64::from(seed_increment),
        seed_count,
        blocks,
        ascendancies,
    })
}

/// `loadAbyssJewel(jewelType)` (:166-178): `None` = no file.
fn load_abyss_jewel(jewel_type: usize) -> Result<Option<&'static AbyssLut>, DataError> {
    static LOADED: [OnceLock<Result<Option<AbyssLut>, DataError>>; 5] = [const { OnceLock::new() }; 5];
    let slot = jewel_type
        .checked_sub(7)
        .and_then(|i| LOADED.get(i))
        .ok_or_else(|| DataError(format!("{ABYSS_HELPER}:166: no Abyss jewel type {jewel_type}")))?;
    slot.get_or_init(|| {
        let (name, _, _) = timeless_jewel_type(jewel_type).expect("the Abyss jewel types are in the table");
        let stem: String = name.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        let cell = LUTS.get(jewel_type - 1).expect("a jewel type");
        let data =
            cell.get_or_init(|| load_jewel_file(&stem)?.ok_or_else(|| DataError(format!("{ABYSS_HELPER}:173: no jewel data: {stem}"))));
        match data {
            Ok(d) => parse_abyss_jewel(jewel_type, d).map(Some),
            // :173-175 the file is not there: an empty result, not an error
            Err(e) if e.0.starts_with(ABYSS_HELPER) => Ok(None),
            Err(e) => Err(e.clone()),
        }
    })
    .as_ref()
    .map(Option::as_ref)
    .map_err(Clone::clone)
}

impl AbyssLut {
    /// `getSeedIndex` (:180-186), 0-based.
    fn seed_index(&self, seed: f64) -> Option<usize> {
        let seed_offset = seed - self.seed_minimum;
        if seed_offset < 0.0 || seed > self.seed_maximum || seed_offset % self.seed_increment != 0.0 {
            return None;
        }
        Some((seed_offset / self.seed_increment) as usize)
    }

    /// `getRecordOffsets` (:188-203).
    fn record_offsets<'a>(
        &self,
        block: &'a AbyssBlock,
        skip: fn(&[u8], usize) -> Result<usize, DataError>,
    ) -> Result<&'a [u32], DataError> {
        block
            .seeds
            .get_or_init(|| {
                let mut offsets = Vec::with_capacity(self.seed_count);
                let mut offset = block.offset;
                for _ in 0..self.seed_count {
                    offsets.push(offset as u32);
                    offset = skip(self.data, offset)?;
                }
                Ok(offsets)
            })
            .as_deref()
            .map_err(Clone::clone)
    }

    /// `readNode` (:205-213), past its checks.
    fn read_node(&self, seed_index: usize, node_id: u32, jewel_type: usize) -> Result<Vec<AbyssComponent>, DataError> {
        let Some(block) = self.blocks.get(&node_id) else {
            return Ok(Vec::new());
        };
        let offsets = self.record_offsets(block, skip_modification)?;
        Ok(read_modification(self.data, offsets[seed_index] as usize, jewel_type)?.0)
    }
}

/// `data.readAbyssJewelLUT(seed, socketId, jewelType, path, ascendancyName)` (:215-273): the affected nodes in the order of their first assignment (the table's `pairs` order is the caller's:
/// `pob_tree::lj_table`). `path`: the keys of `path`, in `pairs` order.
pub fn read_abyss_jewel_lut(
    seed: f64,
    socket_id: u32,
    jewel_type: usize,
    path: Option<&[u32]>,
    ascendancy_name: Option<&str>,
) -> Result<IndexMap<u32, Vec<AbyssComponent>>, DataError> {
    let mut affected = IndexMap::new();
    if (7..=10).contains(&jewel_type) {
        let Some(lut) = load_abyss_jewel(jewel_type)? else {
            return Ok(affected);
        };
        let Some(seed_index) = lut.seed_index(seed).filter(|_| !lut.by_node) else {
            return Ok(affected);
        };
        let Some(block) = lut.blocks.get(&socket_id) else {
            return Ok(affected);
        };
        let mut offset = lut.record_offsets(block, skip_socket_record)?[seed_index] as usize;
        let count = abyss_byte(lut.data, offset, 228)?;
        offset += 1;
        for _ in 0..count {
            let node_id = abyss_u16(lut.data, offset);
            let (modification, next) = read_modification(lut.data, offset + 2, jewel_type)?;
            offset = next;
            affected.insert(node_id, modification);
        }
        return Ok(affected);
    }
    let (11, Some(path)) = (jewel_type, path) else {
        return Ok(affected);
    };
    let Some(lut) = load_abyss_jewel(jewel_type)? else {
        return Ok(affected);
    };
    let Some(seed_index) = lut.seed_index(seed).filter(|_| lut.by_node) else {
        // :206-210 `readNode` gives `{ }` for every node of the path, and :250 skips the ascendancies
        return Ok(affected);
    };
    for &node_id in path {
        let modification = lut.read_node(seed_index, node_id, jewel_type)?;
        if !modification.is_empty() {
            affected.insert(node_id, modification);
        }
    }
    // :251-270 A Zorath seed selects one node in every ascendancy; a name limits the result to that section.
    let Some(name) = ascendancy_name else {
        return Err(DataError(format!(
            "unimplemented: {ABYSS_HELPER}:257 every ascendancy of a Zorath seed (`pairs(ascendancyOffsets)`): only PassiveSpec.lua:1142 asks for it"
        )));
    };
    if let Some(block) = lut.ascendancies.get(name.as_bytes()) {
        let mut offset = lut.record_offsets(block, skip_ascendancy_record)?[seed_index] as usize;
        let selected = abyss_byte(lut.data, offset, 262)?;
        offset += 1;
        for _ in 0..selected {
            let node_id = abyss_u16(lut.data, offset);
            offset += 2;
            let modification = lut.read_node(seed_index, node_id, jewel_type)?;
            affected.insert(node_id, modification);
        }
    }
    Ok(affected)
}
// --- legion passives ------------------------------------------------------------------------------------------------------

/// One `stats[key]` entry of a legion node/addition.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegionStat {
    pub fmt: Option<String>,
    pub index: f64,
    pub max: f64,
    pub min: f64,
    pub stat_order: Option<f64>,
}

/// `legion.additions[i]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegionAddition {
    pub id: String,
    pub dn: String,
    pub sd: Vec<String>,
    pub sorted_stats: Vec<String>,
    /// `pairs(stats)` order: the exporter's key order (byte-sorted); D13 -- see pob-tree `timeless`.
    pub stats: OrderedMap<LegionStat>,
}

/// `legion.nodes[i]` -- the raw tree-JSON-like node.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegionNode {
    pub id: String,
    pub dn: String,
    pub sd: Vec<String>,
    #[serde(rename = "sortedStats")]
    pub sorted_stats: Vec<String>,
    pub stats: OrderedMap<LegionStat>,
    pub icon: String,
    pub ks: bool,
    pub m: bool,
    #[serde(rename = "not")]
    pub not_: bool,
    pub g: f64,
    pub o: f64,
    pub oidx: f64,
    pub da: f64,
    pub ia: f64,
    pub sa: f64,
    #[serde(rename = "passivePointsGranted")]
    pub passive_points_granted: f64,
    #[serde(rename = "isJewelSocket")]
    pub is_jewel_socket: bool,
    #[serde(rename = "isMultipleChoice")]
    pub is_multiple_choice: bool,
    #[serde(rename = "isMultipleChoiceOption")]
    pub is_multiple_choice_option: bool,
    #[serde(rename = "in")]
    pub in_: Vec<Json>,
    pub out: Vec<Json>,
    pub spc: Vec<Json>,
}

/// `Data/TimelessJewelData/LegionPassives.lua`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegionPassives {
    format: u32,
    #[allow(dead_code)]
    producer: Json,
    manifest_sha256: String,
    pob_sha: String,
    pub additions: Vec<LegionAddition>,
    /// Display only (`groups[1000000000]`).
    pub groups: Json,
    pub nodes: Vec<LegionNode>,
}

/// `<repo>/data-patches/legion-passives.json`.
pub fn legion_patch_path() -> PathBuf {
    data_dir().join("..").join("data-patches").join("legion-passives.json")
}

/// Parse the legion patch and check it against the lock (`manifestSha256` and `pobSha`).
pub fn parse_legion(text: &str, lock_sha: &str, lock_pob: &str) -> Result<LegionPassives, DataError> {
    let l: LegionPassives = serde_json::from_str(text).map_err(|e| DataError(format!("pob-data: legion-passives.json: {e}")))?;
    if l.format != 1 {
        return Err(DataError(format!(
            "pob-data: legion-passives.json: format {}, expected 1",
            l.format
        )));
    }
    if l.manifest_sha256 != lock_sha || l.pob_sha != lock_pob {
        return Err(DataError(format!(
            "pob-data: legion-passives.json was taken from data {} / PoB {}, but data.lock has {lock_sha} / {lock_pob} -- re-take it: \
             node tools/lua-corpus/export-legion.ts",
            l.manifest_sha256, l.pob_sha
        )));
    }
    Ok(l)
}

/// The legion passives of this process (read once, after the `data.lock` check).
pub fn legion_passives() -> Result<&'static LegionPassives, DataError> {
    static L: OnceLock<Result<LegionPassives, DataError>> = OnceLock::new();
    L.get_or_init(|| {
        let dir = ensure_data()?;
        let lock = verify_lock(dir, &lock_path(dir))?;
        let path = legion_patch_path();
        let text = std::fs::read_to_string(&path).map_err(|e| DataError(format!("pob-data: {}: {e}", path.display())))?;
        parse_legion(&text, &lock.manifest_sha256, &lock.pob_sha)
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// `data.abyssNotableNames` (AbyssNotableNames.lua): stat id -> name.
pub fn abyss_notable_names() -> &'static OrderedMap<String> {
    static N: OnceLock<OrderedMap<String>> = OnceLock::new();
    N.get_or_init(|| crate::load("abyss-notable-names.json"))
}
