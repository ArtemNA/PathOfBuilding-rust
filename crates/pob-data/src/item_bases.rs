//! `data.itemBases`, `data.itemBaseLists`, `data.itemBaseTypeList` (Modules/Data.lua:660-720) -- pob-ts `src/data/itemBases.ts`.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::wire::OrderedMap;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemBaseReq {
    pub level: Option<f64>,
    pub str: Option<f64>,
    pub dex: Option<f64>,
    pub int: Option<f64>,
}

/// Item.lua:2276-2340 reads exactly these.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(non_snake_case)]
pub struct ItemBaseWeapon {
    pub PhysicalMin: Option<f64>,
    pub PhysicalMax: Option<f64>,
    pub CritChanceBase: f64,
    pub AttackRateBase: f64,
    pub Range: f64,
}

/// Item.lua:2341-2410.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(non_snake_case)]
pub struct ItemBaseArmour {
    pub ArmourBaseMin: Option<f64>,
    pub ArmourBaseMax: Option<f64>,
    pub EvasionBaseMin: Option<f64>,
    pub EvasionBaseMax: Option<f64>,
    pub EnergyShieldBaseMin: Option<f64>,
    pub EnergyShieldBaseMax: Option<f64>,
    pub WardBaseMin: Option<f64>,
    pub WardBaseMax: Option<f64>,
    pub BlockChance: Option<f64>,
    pub MovementPenalty: Option<f64>,
}

/// Item.lua:2411-2460; `buff` lines become `buffModLines` (:1060).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemBaseFlask {
    pub duration: f64,
    pub charges_used: f64,
    pub charges_max: f64,
    pub life: Option<f64>,
    pub mana: Option<f64>,
    pub buff: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemBaseTincture {
    pub cooldown: f64,
    pub mana_burn: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemBase {
    #[serde(rename = "type")]
    pub ty: String,
    pub sub_type: Option<String>,
    pub socket_limit: Option<f64>,
    pub tags: OrderedMap<bool>,
    pub influence_tags: Option<OrderedMap<String>>,
    pub implicit: Option<String>,
    pub implicit_mod_types: Vec<Vec<String>>,
    pub implicit_ids: Option<Vec<String>>,
    pub enchant: Option<String>,
    pub enchant_mod_types: Option<Vec<Vec<String>>>,
    pub enchant_ids: Option<Vec<String>>,
    pub req: ItemBaseReq,
    pub weapon: Option<ItemBaseWeapon>,
    pub armour: Option<ItemBaseArmour>,
    pub flask: Option<ItemBaseFlask>,
    pub tincture: Option<ItemBaseTincture>,
    pub cannot_be_anointed: Option<bool>,
    pub flavour_text: Option<Vec<String>>,
    pub hidden: Option<bool>,
}

/// One `data.itemBaseLists[type][i]` (`base` dropped by the converter -- it is `itemBases[name]`).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemBaseListEntry {
    pub label: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemBases {
    pub bases: OrderedMap<ItemBase>,
    pub lists: OrderedMap<Vec<ItemBaseListEntry>>,
    pub type_list: Vec<String>,
}

impl ItemBases {
    /// `data.itemBases[name]`.
    pub fn base(&self, name: &str) -> Option<&ItemBase> {
        self.bases.get(name)
    }

    /// The base names in byte order -- the fixed tie-break for the one `pairs(data.itemBases)` whose order can matter
    /// (Item.lua:972-983 partial match, D13; same order as pob-ts `itemBaseNames`).
    pub fn names_sorted(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.bases.keys().map(String::as_str).collect();
        v.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        v
    }
}

pub fn item_bases() -> &'static ItemBases {
    static B: OnceLock<ItemBases> = OnceLock::new();
    B.get_or_init(|| crate::load("item-bases.json"))
}
