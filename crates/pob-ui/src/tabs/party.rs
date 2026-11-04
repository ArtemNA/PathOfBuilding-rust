//! `Classes/PartyTab.lua` (U8a). The edit buffers live on the controls (what `Save` writes, `Engine::party_state`); what `ParseBuffs` left in `actor` / `enemyModList` is the engine's
//! (`pob_engine::party_tab`): an edit reaches the calc only through Import or "Rebuild All", as in the Lua. The export of buffs (`exportBuffs`, :996-1047) is `pob_calc::perform::exports`, written by `SaveDB`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_build::XmlNode;
use pob_engine::party_tab::ENEMY_BUFFERS;
use pob_engine::save::PartyState;
use pob_gfx::{Frame, Gfx};
use serde_json::{Value as Json, json};

use crate::colors::color_code;
use crate::control::{ControlRef, Rect, anchor, rect};
use crate::controls::basic::{ButtonControl, CheckBoxControl, LabelControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::host::{ControlHost, InputEvent, InputEvents, ipairs};
use crate::launch::launch;
use crate::modules::build::BuildMode;
use crate::modules::main::{BareControl, main};
use crate::sg;
use crate::tabs::import::{CodeInput, WEBSITES, classify_code, dump_host};

/// :39 `partyDestinations`.
const DESTINATIONS: [&str; 8] = [
    "All",
    "Party Member Stats",
    "Aura",
    "Curse",
    "Warcry Skills",
    "Link Skills",
    "EnemyConditions",
    "EnemyMods",
];

const NOTES_DESC: &str = "^7To import a build it must be exported with \"Export support\" enabled in the import/export tab\n\tAuras with the highest effect will take priority, your curses will take priority over a support's\n\t\n\tAll of these effects can be found in the Calcs tab";

thread_local! {
    /// R13: `pairs(enemyModList)` of the oracle's process (:752), for the gate.
    static ENEMY_MODS_ORDER: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// The gate: the names of the Enemy Modifiers label in the order of the oracle's process.
pub fn set_enemy_mods_order_sample(order: Option<Vec<String>>) {
    ENEMY_MODS_ORDER.with_borrow_mut(|o| *o = order);
}

/// :44-52 `theme.lineCounter`.
fn line_counter(label: &str) -> f64 {
    label.bytes().filter(|c| *c == b'\n').count() as f64 * 16.0
}

fn colored(code: &str, text: &str) -> String {
    format!("{}{text}", color_code(code).unwrap_or_else(|| panic!("colorCodes.{code}")))
}

/// :28-37 `self.lastContent`: `PartyMemberStats` is nil until a `Load` or `ResetModFlags`.
#[derive(Default)]
struct LastContent {
    party_member_stats: Option<String>,
    aura: String,
    curse: String,
    warcry: String,
    link: String,
    enemy_cond: String,
    enemy_mods: String,
    enable_export_buffs: bool,
    show_advanced_tools: bool,
}

pub struct PartyTab {
    pub anchor: Rc<BareControl>,
    pub host: ControlHost,
    build: Weak<BuildMode>,
    pub import_code_in: Rc<EditControl>,
    pub import_code_destination: Rc<DropDownControl>,
    pub append_not_replace: Rc<CheckBoxControl>,
    pub show_advance_tools: Rc<CheckBoxControl>,
    pub edit_auras: Rc<EditControl>,
    pub edit_warcries: Rc<EditControl>,
    pub edit_links: Rc<EditControl>,
    pub edit_party_member_stats: Rc<EditControl>,
    pub enemy_cond: Rc<EditControl>,
    pub enemy_mods: Rc<EditControl>,
    pub edit_curses: Rc<EditControl>,
    pub simple_auras: Rc<LabelControl>,
    pub simple_warcries: Rc<LabelControl>,
    pub simple_links: Rc<LabelControl>,
    pub simple_enemy_cond: Rc<LabelControl>,
    pub simple_enemy_mods: Rc<LabelControl>,
    pub simple_curses: Rc<LabelControl>,
    import_code_site: Cell<Option<usize>>,
    import_code_detail: RefCell<String>,
    import_code_xml: RefCell<Option<String>>,
    /// `self.importCodeFetching`
    import_code_fetching: Cell<bool>,
    me: RefCell<Weak<Self>>,
    import_code_valid: Cell<bool>,
    last_content: RefCell<LastContent>,
    pub mod_flag: Cell<bool>,
    /// What the engine has: `Save` reads the controls.
    synced: RefCell<Option<PartyState>>,
}

impl PartyTab {
    /// :16-514, then `Load` (:516-573).
    #[allow(clippy::too_many_lines)]
    pub fn new(g: &mut Gfx, build: Weak<BuildMode>) -> Rc<Self> {
        let host = ControlHost::new();
        let a = BareControl::new(None, rect(0.0, 0.0, 0.0, 0.0));
        let tab: ControlRef = a.clone();
        let label = |c: &ControlRef, p: &'static str, op: &'static str, rc: [f64; 4], text: &str| {
            LabelControl::new(anchor(p, c, op), rect(rc[0], rc[1], rc[2], rc[3]), text.to_string())
        };
        let buffer = |g: &Gfx, c: &ControlRef, op: &'static str, y: f64| {
            EditControl::new(
                g,
                anchor("TOPLEFT", c, op),
                rect(0.0, y, 0.0, 0.0),
                EditArgs {
                    init: Some(String::new()),
                    filter: Some("^%C\t\n".into()),
                    line_height: Some(14.0),
                    allow_zoom: true,
                    ..EditArgs::default()
                },
            )
        };
        let notes_desc = label(&tab, "TOPLEFT", "TOPLEFT", [8.0, 8.0, 150.0, 16.0], NOTES_DESC);
        let nd: ControlRef = notes_desc.clone();
        host.add("notesDesc", notes_desc.clone());
        let import_code_header = label(
            &nd,
            "TOPLEFT",
            "BOTTOMLEFT",
            [0.0, 32.0, 0.0, 16.0],
            "^7Enter a build code/URL below:",
        );
        let ich: ControlRef = import_code_header.clone();
        host.add("importCodeHeader", import_code_header.clone());
        let import_code_in = EditControl::new(
            g,
            anchor("TOPLEFT", &ich, "BOTTOMLEFT"),
            rect(0.0, 4.0, 328.0, 20.0),
            EditArgs {
                init: Some(String::new()),
                ..EditArgs::default()
            },
        );
        let ici: ControlRef = import_code_in.clone();
        host.add("importCodeIn", import_code_in.clone());
        let import_code_state = LabelControl::new(anchor("LEFT", &ici, "RIGHT"), rect(8.0, 0.0, 0.0, 16.0), String::new());
        host.add("importCodeState", import_code_state.clone());
        let import_code_destination = DropDownControl::new(
            g,
            anchor("TOPLEFT", &ici, "BOTTOMLEFT"),
            rect(0.0, 4.0, 160.0, 20.0),
            DESTINATIONS.iter().map(|s| DropItem::str(s)).collect(),
            None,
            Some("Destination for Import/clear\nCurrently Links Skills do not export"),
            false,
        );
        let icd: ControlRef = import_code_destination.clone();
        host.add("importCodeDestination", import_code_destination.clone());
        let import_code_go = ButtonControl::new(
            anchor("LEFT", &icd, "RIGHT"),
            rect(8.0, 0.0, 160.0, 20.0),
            "Import".to_string(),
            |_| {},
        );
        let icg: ControlRef = import_code_go.clone();
        host.add("importCodeGo", import_code_go.clone());
        let append_not_replace = CheckBoxControl::new(
            g,
            anchor("LEFT", &icg, "RIGHT"),
            [60.0, 0.0, 20.0],
            Some("Append"),
            None,
            Some("This sets the import button to append to the current party lists instead of replacing them (curses will still replace)"),
            false,
        );
        let anr: ControlRef = append_not_replace.clone();
        host.add("appendNotReplace", append_not_replace.clone());
        let clear = ButtonControl::new(
            anchor("LEFT", &anr, "RIGHT"),
            rect(8.0, 0.0, 160.0, 20.0),
            "Clear".to_string(),
            |_| {},
        );
        clear
            .tip
            .tooltip_text
            .set(Some("^7Clears all the party tab imported data".to_string()));
        host.add("clear", clear.clone());
        let show_advance_tools = CheckBoxControl::new(
            g,
            anchor("TOPLEFT", &icd, "BOTTOMLEFT"),
            [140.0, 4.0, 20.0],
            Some("^7Show Advanced Info"),
            None,
            Some(
                "This shows the advanced info like what stats each aura/curse etc are adding, as well as enables the ability to edit them without a re-export\nDo not edit any boxes unless you know what you are doing, use copy/paste or import instead",
            ),
            false,
        );
        let sat: ControlRef = show_advance_tools.clone();
        host.add("ShowAdvanceTools", show_advance_tools.clone());
        let remove_effects = ButtonControl::new(
            anchor("LEFT", &sat, "RIGHT"),
            rect(8.0, 0.0, 160.0, 20.0),
            "Disable Party Effects".to_string(),
            |_| {},
        );
        remove_effects.tip.tooltip_text.set(Some(
            "^7Removes the effects of the supports, without removing the data\nUse \"rebuild all\" to apply the effects again".to_string(),
        ));
        let re: ControlRef = remove_effects.clone();
        host.add("removeEffects", remove_effects.clone());
        let rebuild = ButtonControl::new(
            anchor("LEFT", &re, "RIGHT"),
            rect(8.0, 0.0, 160.0, 20.0),
            "^7Rebuild All".to_string(),
            |_| {},
        );
        rebuild.tip.tooltip_text.set(Some(
            "^7Reparse all the inputs in case they have been disabled or they have changed since loading the build or importing"
                .to_string(),
        ));
        host.add("rebuild", rebuild.clone());
        let edit_auras_label = label(&sat, "TOPLEFT", "TOPLEFT", [-140.0, 40.0, 0.0, 16.0], "^7Auras");
        let eal: ControlRef = edit_auras_label.clone();
        host.add("editAurasLabel", edit_auras_label.clone());
        let edit_auras = buffer(g, &eal, "TOPLEFT", 18.0);
        host.add("editAuras", edit_auras.clone());
        let simple_auras = label(&eal, "TOPLEFT", "TOPLEFT", [0.0, 18.0, 0.0, 16.0], "");
        host.add("simpleAuras", simple_auras.clone());
        let edit_warcries_label = label(&eal, "TOPLEFT", "BOTTOMLEFT", [0.0, 8.0, 0.0, 16.0], "^7Warcry Skills");
        let ewl: ControlRef = edit_warcries_label.clone();
        host.add("editWarcriesLabel", edit_warcries_label.clone());
        let edit_warcries = buffer(g, &ewl, "TOPLEFT", 18.0);
        host.add("editWarcries", edit_warcries.clone());
        let simple_warcries = label(&ewl, "TOPLEFT", "TOPLEFT", [0.0, 18.0, 0.0, 16.0], "");
        host.add("simpleWarcries", simple_warcries.clone());
        let edit_links_label = label(&ewl, "TOPLEFT", "BOTTOMLEFT", [0.0, 8.0, 0.0, 16.0], "^7Link Skills");
        let ell: ControlRef = edit_links_label.clone();
        host.add("editLinksLabel", edit_links_label.clone());
        let edit_links = buffer(g, &ell, "TOPLEFT", 18.0);
        host.add("editLinks", edit_links.clone());
        let simple_links = label(&ell, "TOPLEFT", "TOPLEFT", [0.0, 18.0, 0.0, 16.0], "");
        host.add("simpleLinks", simple_links.clone());
        let stats_label = label(&nd, "TOPLEFT", "TOPRIGHT", [8.0, 0.0, 0.0, 16.0], "^7Party Member Stats");
        let msl: ControlRef = stats_label.clone();
        host.add("editPartyMemberStatsLabel", stats_label);
        let edit_party_member_stats = buffer(g, &msl, "BOTTOMLEFT", 2.0);
        host.add("editPartyMemberStats", edit_party_member_stats.clone());
        let enemy_cond_label = label(&msl, "TOPLEFT", "BOTTOMLEFT", [0.0, 8.0, 0.0, 16.0], "^7Enemy Conditions");
        let ecl: ControlRef = enemy_cond_label.clone();
        host.add("enemyCondLabel", enemy_cond_label.clone());
        let enemy_cond = buffer(g, &ecl, "BOTTOMLEFT", 2.0);
        host.add("enemyCond", enemy_cond.clone());
        let simple_enemy_cond = label(
            &ecl,
            "TOPLEFT",
            "TOPLEFT",
            [0.0, 18.0, 0.0, 16.0],
            "^7---------------------------\n",
        );
        host.add("simpleEnemyCond", simple_enemy_cond.clone());
        let enemy_mods_label = label(&ecl, "TOPLEFT", "BOTTOMLEFT", [0.0, 8.0, 0.0, 16.0], "^7Enemy Modifiers");
        let eml: ControlRef = enemy_mods_label.clone();
        host.add("enemyModsLabel", enemy_mods_label.clone());
        let enemy_mods = buffer(g, &eml, "BOTTOMLEFT", 2.0);
        host.add("enemyMods", enemy_mods.clone());
        let simple_enemy_mods = label(&eml, "TOPLEFT", "TOPLEFT", [0.0, 18.0, 0.0, 16.0], "\n");
        host.add("simpleEnemyMods", simple_enemy_mods.clone());
        let edit_curses_label = label(&eml, "TOPLEFT", "BOTTOMLEFT", [0.0, 8.0, 0.0, 16.0], "^7Curses");
        let cl: ControlRef = edit_curses_label.clone();
        host.add("editCursesLabel", edit_curses_label.clone());
        let edit_curses = buffer(g, &cl, "BOTTOMLEFT", 2.0);
        host.add("editCurses", edit_curses.clone());
        let simple_curses = label(&cl, "TOPLEFT", "TOPLEFT", [0.0, 18.0, 0.0, 16.0], "");
        host.add("simpleCurses", simple_curses.clone());

        let t = Rc::new(Self {
            anchor: a.clone(),
            host,
            build,
            import_code_in: import_code_in.clone(),
            import_code_destination,
            append_not_replace: append_not_replace.clone(),
            show_advance_tools: show_advance_tools.clone(),
            edit_auras: edit_auras.clone(),
            edit_warcries: edit_warcries.clone(),
            edit_links: edit_links.clone(),
            edit_party_member_stats: edit_party_member_stats.clone(),
            enemy_cond: enemy_cond.clone(),
            enemy_mods: enemy_mods.clone(),
            edit_curses: edit_curses.clone(),
            simple_auras: simple_auras.clone(),
            simple_warcries: simple_warcries.clone(),
            simple_links: simple_links.clone(),
            simple_enemy_cond: simple_enemy_cond.clone(),
            simple_enemy_mods: simple_enemy_mods.clone(),
            simple_curses: simple_curses.clone(),
            import_code_site: Cell::new(None),
            import_code_detail: RefCell::new(String::new()),
            import_code_xml: RefCell::new(None),
            import_code_valid: Cell::new(false),
            import_code_fetching: Cell::new(false),
            me: RefCell::new(Weak::new()),
            last_content: RefCell::new(LastContent::default()),
            mod_flag: Cell::new(false),
            synced: RefCell::new(None),
        });

        // --- the function properties (the tab's own width and height are read through the anchor)
        let w = Rc::downgrade(&a);
        let width = move |g: &Gfx| w.upgrade().expect("partyTab").base.width.get(g);
        let w = Rc::downgrade(&a);
        let height = move |g: &Gfx| w.upgrade().expect("partyTab").base.height.get(g);
        // :69-76
        let last_width = Cell::new(None::<f64>);
        let nl = Rc::downgrade(&notes_desc);
        let wd = width.clone();
        notes_desc.base.width.set_fn(move |g| {
            let width = wd(g) / 2.0 - 16.0;
            if last_width.get() != Some(width) {
                last_width.set(Some(width));
                let wrapped = crate::common::wrap_string(g, NOTES_DESC, 16.0, width - 50.0).join("\n");
                nl.upgrade().expect("notesDesc").label.set(wrapped);
            }
            width
        });
        let nl = notes_desc.clone();
        import_code_header.base.y.set_fn(move |g| line_counter(&nl.label.get(g)) + 4.0);
        let wd = width.clone();
        import_code_in
            .base
            .width
            .set_fn(move |g| if wd(g) > 880.0 { 328.0 } else { wd(g) / 2.0 - 100.0 });
        let wd = width.clone();
        append_not_replace
            .base
            .x
            .set_fn(move |g| if wd(g) > 1350.0 { 60.0 } else { -276.0 });
        let wd = width.clone();
        append_not_replace.base.y.set_fn(move |g| if wd(g) > 1350.0 { 0.0 } else { 24.0 });
        let wd = width.clone();
        show_advance_tools.base.y.set_fn(move |g| if wd(g) > 1350.0 { 4.0 } else { 28.0 });
        let wd = width.clone();
        rebuild.base.x.set_fn(move |g| if wd(g) > 1350.0 { 8.0 } else { -328.0 });
        let wd = width.clone();
        rebuild.base.y.set_fn(move |g| if wd(g) > 1350.0 { 0.0 } else { 24.0 });
        let wd = width.clone();
        edit_auras_label
            .base
            .y
            .set_fn(move |g| 36.0 + if wd(g) <= 1350.0 { 24.0 } else { 0.0 });
        // :55-58 `theme.bufferHeightLeft`
        let buffer_height_left = {
            let (wd, hg, ich, eal) = (width.clone(), height.clone(), import_code_header.clone(), edit_auras_label.clone());
            move |g: &Gfx| hg(g) - 378.0 - if wd(g) > 1350.0 { 0.0 } else { 24.0 } - ich.base.y.get(g) - eal.base.y.get(g)
        };
        let half = |e: &Rc<EditControl>| {
            let wd = width.clone();
            e.base.width.set_fn(move |g| wd(g) / 2.0 - 16.0);
        };
        for e in [
            &edit_auras,
            &edit_warcries,
            &edit_links,
            &edit_party_member_stats,
            &enemy_cond,
            &enemy_mods,
            &edit_curses,
        ] {
            half(e);
            let s = show_advance_tools.clone();
            e.base.shown.set_fn(move |_| s.state.get());
        }
        for l in [
            &simple_auras,
            &simple_warcries,
            &simple_links,
            &simple_enemy_cond,
            &simple_enemy_mods,
            &simple_curses,
        ] {
            let s = show_advance_tools.clone();
            l.base.shown.set_fn(move |_| !s.state.get());
        }
        const SMALL: f64 = 106.0;
        const RIGHT: f64 = 434.0;
        let (w_, l_, bl) = (edit_warcries.clone(), edit_links.clone(), buffer_height_left.clone());
        edit_auras.base.height.set_fn(move |g| {
            if w_.base.has_focus.get() || l_.base.has_focus.get() {
                SMALL
            } else {
                bl(g)
            }
        });
        let (e_, bl) = (edit_warcries.clone(), buffer_height_left.clone());
        edit_warcries
            .base
            .height
            .set_fn(move |g| if e_.base.has_focus.get() { bl(g) } else { SMALL });
        let (e_, bl) = (edit_links.clone(), buffer_height_left.clone());
        edit_links
            .base
            .height
            .set_fn(move |g| if e_.base.has_focus.get() { bl(g) } else { SMALL });
        let right_focus = |e: &Rc<EditControl>| {
            let (e_, hg) = (e.clone(), height.clone());
            e.base
                .height
                .set_fn(move |g| if e_.base.has_focus.get() { hg(g) - RIGHT } else { SMALL });
        };
        right_focus(&edit_party_member_stats);
        right_focus(&enemy_cond);
        right_focus(&enemy_mods);
        let (c_, m_, p_, hg) = (
            enemy_cond.clone(),
            enemy_mods.clone(),
            edit_party_member_stats.clone(),
            height.clone(),
        );
        edit_curses.base.height.set_fn(move |g| {
            if c_.base.has_focus.get() || m_.base.has_focus.get() || p_.base.has_focus.get() {
                SMALL
            } else {
                hg(g) - RIGHT
            }
        });
        // :406-409 and the like: a label's y follows the row above
        let row_y = |l: &Rc<LabelControl>, above: &Rc<EditControl>, simple: &Rc<LabelControl>| {
            let (s, e, sl) = (show_advance_tools.clone(), above.clone(), simple.clone());
            l.base.y.set_fn(move |g| {
                if s.state.get() {
                    e.base.height.get(g) + 8.0
                } else {
                    line_counter(&sl.label.get(g)) + 4.0
                }
            });
        };
        row_y(&edit_warcries_label, &edit_auras, &simple_auras);
        row_y(&edit_links_label, &edit_warcries, &simple_warcries);
        row_y(&enemy_mods_label, &enemy_cond, &simple_enemy_cond);
        row_y(&edit_curses_label, &enemy_mods, &simple_enemy_mods);
        let (s, e) = (show_advance_tools.clone(), edit_party_member_stats.clone());
        enemy_cond_label
            .base
            .y
            .set_fn(move |g| if s.state.get() { e.base.height.get(g) + 8.0 } else { 4.0 });

        // --- the callbacks
        let weak = Rc::downgrade(&t);
        *t.me.borrow_mut() = weak.clone();
        let f = move || weak.upgrade().expect("partyTab");
        let h = f.clone();
        import_code_state.label.set_fn(move |_| h().import_code_detail.borrow().clone());
        let h = f.clone();
        *import_code_in.change_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, buf: &str, _placeholder: bool| {
            h().import_code_handle(g, buf);
        }));
        let h = f.clone();
        *import_code_in.enter_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, _buf: &str| {
            if h().import_code_valid.get() {
                h().on_import_go(g);
            }
        }));
        let h = f.clone();
        *import_code_go.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| h().on_import_go(g));
        let h = f.clone();
        import_code_go.base.enabled.set_fn(move |_| {
            let tab = h();
            tab.import_code_valid.get() && !tab.import_code_fetching.get()
        });
        let h = f.clone();
        *import_code_go.enter_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx| {
            if h().import_code_valid.get() {
                h().on_import_go(g);
            }
        }));
        let h = f.clone();
        *clear.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| {
            let t = h();
            // :339-344
            t.clear_input_text(g);
            t.with_engine(|e| e.party_wipe(&ENEMY_BUFFERS));
            t.flag();
        });
        let h = f.clone();
        *remove_effects.on_click.borrow_mut() = Rc::new(move |_g: &mut Gfx| {
            let t = h();
            // :353-360
            t.with_engine(pob_engine::engine::Engine::party_wipe_all);
            t.flag();
        });
        let h = f.clone();
        *rebuild.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| h().on_rebuild(g));

        // --- :516-573 Load
        t.host.select_control(g, Some(t.edit_auras.clone()));
        t.load(g);
        t
    }

    fn with_engine<R>(&self, f: impl FnOnce(&mut pob_engine::engine::Engine) -> R) -> R {
        let b = self.build.upgrade().expect("buildMode");
        let mut e = b.engine.borrow_mut();
        f(e.as_mut().expect("engine"))
    }

    fn flag(&self) {
        self.build.upgrade().expect("buildMode").build_flag.set(true);
    }

    fn edit_of(&self, name: &str) -> &Rc<EditControl> {
        match name {
            "PartyMemberStats" => &self.edit_party_member_stats,
            "Aura" => &self.edit_auras,
            "Curse" => &self.edit_curses,
            "Warcry Skills" => &self.edit_warcries,
            "Link Skills" => &self.edit_links,
            "EnemyConditions" => &self.enemy_cond,
            "EnemyMods" => &self.enemy_mods,
            other => panic!("buffer {other}"),
        }
    }

    /// :516-573: the engine read the section; the controls follow it.
    fn load(&self, g: &mut Gfx) {
        let state = self.with_engine(|e| e.party_state().clone());
        for (name, text) in &state.buffers {
            self.edit_of(name).set_text(g, text, false);
        }
        let parsed: Vec<String> = self.with_engine(|e| e.model().party_imported_buffs.iter().map(|b| b.name.clone()).collect());
        for name in &parsed {
            let buf = self.edit_of(name).buf.borrow().clone();
            self.set_label(name, &buf).unwrap_or_else(|e| panic!("{e}"));
        }
        self.import_code_destination.sel_by_label(&state.destination);
        self.append_not_replace.state.set(state.append);
        self.show_advance_tools.state.set(state.show_advance_tools);
        if self.with_engine(|e| e.extras().party_section) {
            *self.last_content.borrow_mut() = self.content();
        }
        *self.synced.borrow_mut() = Some(state);
    }

    fn content(&self) -> LastContent {
        let buf = |e: &Rc<EditControl>| e.buf.borrow().clone();
        LastContent {
            party_member_stats: Some(buf(&self.edit_party_member_stats)),
            aura: buf(&self.edit_auras),
            curse: buf(&self.edit_curses),
            warcry: buf(&self.edit_warcries),
            link: buf(&self.edit_links),
            enemy_cond: buf(&self.enemy_cond),
            enemy_mods: buf(&self.enemy_mods),
            enable_export_buffs: self.build.upgrade().is_some_and(|b| {
                b.engine
                    .borrow()
                    .as_ref()
                    .is_some_and(pob_engine::engine::Engine::import_export_party)
            }),
            show_advanced_tools: self.show_advance_tools.state.get(),
        }
    }

    /// The `label.label` a `ParseBuffs` of this buffer writes (:735-764, :901-984).
    fn set_label(&self, name: &str, buf: &str) -> Result<(), String> {
        let (label, text) = match name {
            "EnemyMods" => {
                let order = ENEMY_MODS_ORDER.with_borrow(Clone::clone);
                (&self.simple_enemy_mods, pob_calc::party::enemy_mods_label(buf, order.as_deref())?)
            }
            "Aura" => (&self.simple_auras, self.with_engine(|e| e.party_actor())?.aura.aura_label()?),
            "Warcry Skills" => (
                &self.simple_warcries,
                self.with_engine(|e| e.party_actor())?.warcry.plain_label("Warcry")?,
            ),
            "Link Skills" => (&self.simple_links, self.with_engine(|e| e.party_actor())?.link.plain_label("Link")?),
            "Curse" => (
                &self.simple_curses,
                self.with_engine(|e| e.party_actor())?.curse.plain_label("Curse")?,
            ),
            _ => return Ok(()),
        };
        label.label.set(text);
        Ok(())
    }

    /// `self:ParseBuffs(...)` of a buffer: a Lua error stays one (Launch.lua's prompt).
    fn parse_buffs(&self, name: &str, buf: &str) {
        self.with_engine(|e| e.party_parse(name, buf)).unwrap_or_else(|e| panic!("{e}"));
        self.set_label(name, buf).unwrap_or_else(|e| panic!("{e}"));
    }

    fn destination_is(&self, what: &str) -> bool {
        let d = DESTINATIONS[self.import_code_destination.sel_index.get() - 1];
        d == "All" || d == what
    }

    /// :82-118 `clearInputText`.
    fn clear_input_text(&self, g: &mut Gfx) {
        if self.destination_is("Party Member Stats") {
            self.edit_party_member_stats.set_text(g, "", false);
        }
        for (dest, label, edit, actor) in [
            ("Aura", &self.simple_auras, &self.edit_auras, "Aura"),
            ("Curse", &self.simple_curses, &self.edit_curses, "Curse"),
            ("Warcry Skills", &self.simple_warcries, &self.edit_warcries, "Warcry Skills"),
            ("Link Skills", &self.simple_links, &self.edit_links, "Link Skills"),
        ] {
            if self.destination_is(dest) {
                label.label.set(String::new());
                edit.set_text(g, "", false);
                self.with_engine(|e| e.party_wipe(&[actor]));
            }
        }
        if self.destination_is("EnemyConditions") {
            self.simple_enemy_cond.label.set("^7---------------------------\n".to_string());
            self.enemy_cond.set_text(g, "", false);
        }
        if self.destination_is("EnemyMods") {
            self.simple_enemy_mods.label.set("\n".to_string());
            self.enemy_mods.set_text(g, "", false);
        }
    }

    /// :120-160 `importCodeHandle`.
    fn import_code_handle(&self, g: &mut Gfx, buf: &str) {
        self.import_code_site.set(None);
        self.import_code_detail.borrow_mut().clear();
        *self.import_code_xml.borrow_mut() = None;
        self.import_code_valid.set(false);
        match classify_code(buf) {
            CodeInput::Empty => {}
            CodeInput::Invalid => *self.import_code_detail.borrow_mut() = colored("NEGATIVE", "Invalid input"),
            CodeInput::Url(j, url_text) => {
                self.import_code_valid.set(true);
                *self.import_code_detail.borrow_mut() = colored("POSITIVE", &format!("URL is valid ({})", WEBSITES[j - 1].0));
                self.import_code_site.set(Some(j));
                if buf != url_text {
                    self.import_code_in.set_text(g, &url_text, false);
                }
            }
            CodeInput::Xml(xml) => {
                if launch().dev_mode.get() && sg::is_key_down(g, "SHIFT") {
                    g.copy(&xml);
                }
                self.import_code_valid.set(true);
                *self.import_code_detail.borrow_mut() = colored("POSITIVE", "Code is valid");
                *self.import_code_xml.borrow_mut() = Some(xml);
            }
        }
    }

    /// :302-321
    fn on_import_go(&self, g: &mut Gfx) {
        if let Some(site) = self.import_code_site.get()
            && self.import_code_xml.borrow().is_none()
        {
            self.import_code_fetching.set(true);
            let w = self.me.borrow().clone();
            let link = self.import_code_in.buf.borrow().clone();
            crate::build_sites::download_build(&link, site, move |g, ok, data| {
                let Some(tab) = w.upgrade() else { return };
                tab.import_code_fetching.set(false);
                if ok {
                    tab.import_code_handle(g, &data);
                    tab.finish_import(g);
                } else {
                    *tab.import_code_detail.borrow_mut() = colored("NEGATIVE", &data);
                    tab.import_code_valid.set(false);
                }
            });
            return;
        }
        self.finish_import(g);
    }

    /// :162-285 `finishImport`.
    #[allow(clippy::too_many_lines)]
    fn finish_import(&self, g: &mut Gfx) {
        if !self.import_code_valid.get() {
            return;
        }
        let mut current_curse_buffer = None;
        let mut current_link_buffer = None;
        if self.append_not_replace.state.get() {
            if self.destination_is("Aura") {
                self.with_engine(|e| e.party_wipe(&["Aura"]));
            }
            if self.destination_is("Curse") {
                current_curse_buffer = Some(self.edit_curses.buf.borrow().clone());
                self.edit_curses.set_text(g, "", false);
                self.with_engine(|e| e.party_wipe(&["Curse"]));
            }
            if self.destination_is("Warcry Skills") {
                self.with_engine(|e| e.party_wipe(&["Warcry Skills"]));
            }
            if self.destination_is("Link Skills") {
                current_link_buffer = Some(self.edit_links.buf.borrow().clone());
                self.edit_links.set_text(g, "", false);
                self.with_engine(|e| e.party_wipe(&["Link Skills"]));
            }
        } else {
            self.clear_input_text(g);
        }
        // :196-203 (`fileName` is a global nobody sets)
        let xml = self.import_code_xml.borrow().clone().unwrap_or_default();
        let doc = match pob_build::parse_xml_document(&xml) {
            Ok(d) => d,
            Err(e) => {
                launch().show_err_msg(&format!("^1Error loading 'nil': {e}"));
                return;
            }
        };
        let root = match doc.first() {
            Some(XmlNode::Element(e)) if e.name == "PathOfBuilding" => e,
            _ => {
                launch().show_err_msg("^1Error parsing 'nil': 'PathOfBuilding' root element missing");
                return;
            }
        };
        // :206-284: the first `<Party>`
        let Some(party) = root.child_elements().find(|n| n.name == "Party") else {
            return;
        };
        // :209-273 in document order; a buffer takes the old text first
        let joined = |edit: &Rc<EditControl>, text: Option<&str>| -> String {
            let old = edit.buf.borrow();
            if old.is_empty() {
                text.unwrap_or("").to_string()
            } else {
                format!("{old}\n{}", text.unwrap_or(""))
            }
        };
        for node in party.child_elements().filter(|n| n.name == "ExportedBuffs") {
            let Some(name) = node.attr("name") else {
                g.con_printf("missing name");
                continue;
            };
            let text: Option<&str> = match node.children.first() {
                Some(XmlNode::Text(t)) => Some(t),
                _ => None,
            };
            match name {
                "PartyMemberStats" if self.destination_is("Party Member Stats") => {
                    let t = joined(&self.edit_party_member_stats, text);
                    self.edit_party_member_stats.set_text(g, &t, false);
                    let buf = self.edit_party_member_stats.buf.borrow().clone();
                    self.parse_buffs("PartyMemberStats", &buf);
                }
                "Aura" if self.destination_is("Aura") => {
                    let t = joined(&self.edit_auras, text);
                    self.edit_auras.set_text(g, &t, false);
                    let buf = self.edit_auras.buf.borrow().clone();
                    self.parse_buffs("Aura", &buf);
                }
                "Curse" if self.destination_is("Curse") => {
                    let mut t = joined(&self.edit_curses, text);
                    if let Some(c) = &current_curse_buffer
                        && t == "--- Curse Limit ---\n1"
                    {
                        t.clone_from(c);
                    }
                    self.edit_curses.set_text(g, &t, false);
                    let buf = self.edit_curses.buf.borrow().clone();
                    self.parse_buffs("Curse", &buf);
                }
                "Warcry Skills" if self.destination_is("Warcry Skills") => {
                    let t = joined(&self.edit_warcries, text);
                    self.edit_warcries.set_text(g, &t, false);
                    let buf = self.edit_warcries.buf.borrow().clone();
                    self.parse_buffs("Warcry Skills", &buf);
                }
                "Link Skills" if self.destination_is("Link Skills") => {
                    let mut t = joined(&self.edit_links, text);
                    if let Some(c) = &current_link_buffer
                        && t.is_empty()
                    {
                        t.clone_from(c);
                    }
                    self.edit_links.set_text(g, &t, false);
                    let buf = self.edit_links.buf.borrow().clone();
                    self.parse_buffs("Link Skills", &buf);
                }
                "EnemyConditions" if self.destination_is("EnemyConditions") => {
                    let t = joined(&self.enemy_cond, text);
                    self.enemy_cond.set_text(g, &t, false);
                }
                "EnemyMods" if self.destination_is("EnemyMods") => {
                    let t = joined(&self.enemy_mods, text);
                    self.enemy_mods.set_text(g, &t, false);
                }
                _ => {}
            }
        }
        // :275-280
        if self.destination_is("EnemyConditions") || self.destination_is("EnemyMods") {
            self.with_engine(|e| e.party_wipe(&ENEMY_BUFFERS));
            let (cond, mods) = (self.enemy_cond.buf.borrow().clone(), self.enemy_mods.buf.borrow().clone());
            self.parse_buffs("EnemyConditions", &cond);
            self.parse_buffs("EnemyMods", &mods);
        }
        self.flag();
    }

    /// :363-377
    fn on_rebuild(&self, _g: &mut Gfx) {
        self.with_engine(pob_engine::engine::Engine::party_wipe_all);
        for name in [
            "PartyMemberStats",
            "Aura",
            "Curse",
            "Warcry Skills",
            "Link Skills",
            "EnemyConditions",
            "EnemyMods",
        ] {
            let buf = self.edit_of(name).buf.borrow().clone();
            self.parse_buffs(name, &buf);
        }
        self.flag();
    }

    /// What `Save` reads (:575-659), into the engine.
    pub fn sync(&self) {
        let mut buffers = IndexMap::new();
        for name in [
            "PartyMemberStats",
            "Aura",
            "Curse",
            "Warcry Skills",
            "Link Skills",
            "EnemyConditions",
            "EnemyMods",
        ] {
            let buf = self.edit_of(name).buf.borrow();
            if !buf.is_empty() {
                buffers.insert(name.to_string(), buf.clone());
            }
        }
        let state = PartyState {
            buffers,
            destination: DESTINATIONS[self.import_code_destination.sel_index.get() - 1].to_string(),
            append: self.append_not_replace.state.get(),
            show_advance_tools: self.show_advance_tools.state.get(),
        };
        if self.synced.borrow().as_ref() != Some(&state) {
            let b = self.build.upgrade().expect("buildMode");
            // the frame of an import that failed goes on over a closed build
            if let Some(e) = b.engine.borrow_mut().as_mut() {
                e.party_set_state(state.clone());
            }
            *self.synced.borrow_mut() = Some(state);
        }
    }

    /// :661-672
    pub fn reset_mod_flags(&self) {
        *self.last_content.borrow_mut() = self.content();
        self.mod_flag.set(false);
    }

    /// :674-720
    pub fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, input_events: &mut InputEvents) {
        let a = &self.anchor.base;
        a.x.set(view_port.x);
        a.y.set(view_port.y);
        a.width.set(view_port.width);
        a.height.set(view_port.height);
        let downs: Vec<String> = ipairs(input_events)
            .filter_map(|(_, e)| match e {
                InputEvent::KeyDown { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect();
        for key in downs {
            if (key == "z" || key == "y") && sg::is_key_down(f, "CTRL") {
                let focused = [&self.edit_auras, &self.edit_curses, &self.edit_links, &self.edit_party_member_stats]
                    .into_iter()
                    .find(|e| e.base.has_focus.get());
                if let Some(e) = focused {
                    if key == "z" {
                        e.undo_g(f);
                    } else {
                        e.redo_g(f);
                    }
                }
            }
        }
        self.host.process_controls_input(f, input_events, view_port);
        main().draw_background(f, view_port);
        self.host.draw_controls(f, view_port, None);
        let now = self.content();
        let last = self.last_content.borrow();
        self.mod_flag.set(
            last.aura != now.aura
                || last.curse != now.curse
                || last.warcry != now.warcry
                || last.link != now.link
                || last.party_member_stats != now.party_member_stats
                || last.enemy_cond != now.enemy_cond
                || last.enemy_mods != now.enemy_mods
                || last.enable_export_buffs != now.enable_export_buffs
                || last.show_advanced_tools != now.show_advanced_tools,
        );
        drop(last);
        self.sync();
    }

    /// The state the gate compares (the same fields as `import-runner.lua`).
    pub fn debug_dump(&self, g: &Gfx) -> Json {
        json!({
            "controls": dump_host(&self.host, g),
            "importCodeDetail": self.import_code_detail.borrow().clone(),
            "importCodeValid": self.import_code_valid.get(),
            "importCodeSite": self.import_code_site.get(),
            "hasXml": self.import_code_xml.borrow().is_some(),
            "modFlag": self.mod_flag.get(),
        })
    }
}
