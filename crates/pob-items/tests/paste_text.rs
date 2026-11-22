//! The text of an item copied in the game (Ctrl+C) has characters PoB does not draw: `ä` in "Doppelgänger", the em dash and the zero-width space
//! of "Grants Level 20 ​Unhinge — Unscalable Value". `new("Item", raw)` cleans them first (Item.lua:96 `sanitiseText`), so pasting the text in
//! the Items tab (Ctrl+V) gives the item that the "Create custom" box gives (its `pasteFilter` is the same `sanitiseText`, ItemsTab.lua:2814).

use pob_items::Item;

const GUISE: &str = "Item Class: Body Armours\nRarity: Unique\nDoppelg\u{e4}nger Guise\nSadist Garb\n--------\nEvasion Rating: 437\nEnergy Shield: 95\n--------\nRequirements:\nLevel: 68\nDex: 103\nInt: 109 (unmet)\n--------\nSockets: W-W-W \n--------\nItem Level: 86\n--------\n{ Unique Modifier }\nGrants Level 20 \u{200b}Unhinge \u{2014} Unscalable Value\n{ Unique Modifier \u{2014} Critical }\n60(40-60)% more Critical Strike Chance while Insane\n{ Unique Modifier }\nEnemies Killed by your Hits are destroyed while Insane\n{ Unique Modifier \u{2014} Physical, Chaos }\n39(40-30)% less Physical and Chaos Damage Taken while Sane\n{ Unique Modifier \u{2014} Life }\nRegenerate 10% Life over one second when Hit while Sane\n--------\nWithin each of us exists a being of pure malice,\nheld back only by the lies we tell ourselves.\n";

const VANITY: &str = "Item Class: Jewels\nRarity: Unique\nGlorious Vanity\nTimeless Jewel\n--------\nLimited to: 1 Historic\nRadius: Large\n--------\nItem Level: 80\n--------\n{ Unique Modifier }\nBathed in the blood of 2293(100-8000) sacrificed in the name of Xibaqua(...)\nPassives in radius are Conquered by the Vaal\n(Conquered Passive Skills cannot be modified by other Jewels)\nHistoric \u{2014} Unscalable Value\n--------\nThey believed themselves the pinnacle of civilisation, but that height toppled their empire.\n--------\nPlace into an allocated Jewel Socket on the Passive Skill Tree. Right click to remove from the Socket.\n--------\nNote: ~b/o 150 chaos\n";

#[test]
fn pasted_text_is_the_item_of_the_cleaned_text() {
    for raw in [GUISE, VANITY] {
        let pasted = Item::parse_with(raw, None, false);
        let cleaned = Item::parse_with(&pob_build::sanitise_text(raw), None, false);
        let (a, b) = (pasted.build_raw(), cleaned.build_raw());
        assert!(a.is_ascii(), "no character PoB draws as [U+XXXX]:\n{a}");
        assert_eq!(a, b);
    }
    let guise = Item::parse_with(GUISE, None, false);
    assert_eq!(guise.title.as_deref(), Some("Doppelganger Guise"));
    assert!(guise.build_raw().contains("Grants Level 20 Unhinge"), "{}", guise.build_raw());
}
