//! Readers and the mutator of `Classes/TradeQuery.lua` (U8d): `statSortSelectionList` as `<TradeSearchWeights>` writes it, the calculator's base.

use crate::engine::Engine;
use crate::save::TradeWeight;

/// A gem as `addMoreWEMods` reads it (TradeQueryGenerator.lua:888-906).
#[derive(Clone, Debug, PartialEq)]
pub struct AuraGem {
    pub name_spec: String,
    /// `gem.nameSpec:find("Vaal") and gem.enableGlobal2`.
    pub vaal_global2: bool,
    /// `gem.gemData and gem.gemData.tags.aura or gem.fromItem`.
    pub is_aura_or_from_item: bool,
}

impl Engine {
    /// `tradeQuery.statSortSelectionList`: `None` before `ItemsTab:Load` ran (R58).
    pub fn trade_weights(&self) -> Option<&[TradeWeight]> {
        (!self.extras().no_trade_weights).then(|| self.extras().trade_search_weights.as_slice())
    }

    /// `self.statSortSelectionList = list` (TradeQuery.lua:417-420, :770, :778, :789-790). From now on `Save` writes the element, even for a build whose XML had no `<Items>` (R58).
    pub fn set_trade_weights(&mut self, list: Vec<TradeWeight>) {
        let x = self.extras_mut();
        x.no_trade_weights = false;
        x.trade_search_weights = list;
    }

    /// `baseOutput` of `calcsTab:GetMiscCalculator()`.
    pub fn misc_base_run(&self) -> Result<pob_calc::session::RunRef, String> {
        Ok(self.misc_calculator()?.base.clone())
    }

    /// The gems of `skillsTab.socketGroupList` that are enabled, in order.
    pub fn trade_aura_gems(&self) -> Vec<AuraGem> {
        let data = pob_data::gems::gems();
        let mut out = Vec::new();
        for group in self.socket_groups() {
            for gem in &group.gem_list {
                if !gem.enabled {
                    continue;
                }
                let name = gem.name_spec.clone().unwrap_or_default();
                let aura = gem
                    .gem_data
                    .as_ref()
                    .and_then(|i| data.gems.get(i))
                    .is_some_and(|d| d.tags.get("aura").copied().unwrap_or(false));
                out.push(AuraGem {
                    vaal_global2: name.contains("Vaal") && gem.enable_global2 == Some(true),
                    is_aura_or_from_item: aura || gem.from_item == Some(true),
                    name_spec: name,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?><PathOfBuilding><Build level="1" targetVersion="3_0" className="Scion" ascendClassName="None"/><Tree activeSpec="1"><Spec treeVersion="3_29" classId="0" ascendClassId="0" nodes=""/></Tree></PathOfBuilding>"#;

    #[test]
    fn weights_come_to_the_saved_build_even_without_items_in_the_xml() {
        let mut e = Engine::load_xml(XML).expect("load");
        assert_eq!(e.trade_weights(), None);
        assert!(!e.save_xml().expect("save").contains("TradeSearchWeights"));
        e.set_trade_weights(vec![
            TradeWeight {
                label: "Full DPS".into(),
                stat: Some("FullDPS".into()),
                weight_mult: Some(1.0),
            },
            TradeWeight {
                label: "Effective Hit Pool".into(),
                stat: Some("TotalEHP".into()),
                weight_mult: Some(0.5),
            },
        ]);
        assert_eq!(e.trade_weights().map(<[_]>::len), Some(2));
        let saved = e.save_xml().expect("save");
        assert!(saved.contains(r#"stat="FullDPS""#), "{saved}");
        assert!(saved.contains(r#"weightMult="0.50""#), "{saved}");
        // the same as a build that comes with them in its XML
        let again = Engine::load_xml(&saved).expect("reload");
        assert_eq!(again.trade_weights().map(<[_]>::len), Some(2));
    }
}
