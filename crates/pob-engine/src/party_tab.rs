//! The data half of `Classes/PartyTab.lua` for the tab (U8a).
//!
//! Upstream keeps two things apart: the edit buffers (what `PartyTab:Save` writes, `extras.party`) and what `ParseBuffs` left in `partyTab.actor` / `partyTab.enemyModList`
//! (what the calc reads). An edit of a buffer changes nothing in the calc until Import or "Rebuild All"; "Disable Party Effects" empties the actor and keeps the buffers.
//! The model holds the second as the list of `ParseBuffs` calls in order (`model.party_imported_buffs`, read by `pob_calc::party::PartyTab::load` on every run), so that
//! what accumulates upstream (`actor.modDB` and `actor.output`, which `clearInputText` does not wipe) accumulates here too.

use pob_calc::party::PartyTab;
use pob_calc::pob_build::{PartyImportedBuffs, PartyText};

use crate::engine::Engine;
use crate::save::PartyState;

/// The buffer names of `ParseBuffs` that belong to `actor`'s lists.
pub const ENEMY_BUFFERS: [&str; 2] = ["EnemyConditions", "EnemyMods"];

impl Engine {
    /// `extras.party`: the controls and the buffers as `PartyTab:Save` will write them.
    pub fn party_state(&self) -> &PartyState {
        &self.extras().party
    }

    /// The buffers, the destination and the two check boxes of the tab (`Save` reads them from the controls).
    pub fn party_set_state(&mut self, state: PartyState) {
        self.extras_mut().party = state;
    }

    /// `ExportedBuffs` state: `enablePartyExportBuffs` (ImportTab.lua:507-510) reads the model's flag.
    pub fn import_export_party(&self) -> bool {
        self.extras().import.export_party
    }

    pub fn import_set_export_party(&mut self, state: bool) {
        self.extras_mut().import.export_party = state;
    }

    /// `wipeTable(self.actor[name]); self.actor[name] = { }` / `wipeTable(self.enemyModList)`: the parses of these buffers are gone.
    pub fn party_wipe(&mut self, names: &[&str]) {
        self.model_mut().party_imported_buffs.retain(|b| !names.contains(&b.name.as_str()));
    }

    /// :354-358 `wipeTable(self.actor)`, `wipeTable(self.enemyModList)`: everything `ParseBuffs` left.
    pub fn party_wipe_all(&mut self) {
        self.model_mut().party_imported_buffs.clear();
    }

    /// `self:ParseBuffs(list, buf, type, label)` over the current state. `Err` = the Lua error the parse raises (nothing is kept then).
    pub fn party_parse(&mut self, name: &str, buf: &str) -> Result<(), String> {
        let entry = PartyImportedBuffs {
            name: name.to_owned(),
            text: PartyText::Text(buf.to_owned()),
        };
        self.model_mut().party_imported_buffs.push(entry);
        let parsed = PartyTab::load(&self.model().party_imported_buffs);
        if let Err(e) = parsed {
            self.model_mut().party_imported_buffs.pop();
            return Err(e);
        }
        Ok(())
    }

    /// Build.lua:83 `self.characterLevel = ...`: what `Init` writes before a `LoadDB` that fails; the frame of that failure ends over this build.
    pub fn set_level_of_failed_init(&mut self, level: f64) {
        self.model_mut().level = level;
    }

    /// The state of `actor` / `enemyModList` as it stands (the `label` readers).
    pub fn party_actor(&self) -> Result<PartyTab, String> {
        PartyTab::load(&self.model().party_imported_buffs)
    }
}
