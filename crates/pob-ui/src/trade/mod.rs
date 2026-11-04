//! `Classes/TradeQuery.lua`: the "Trader" popup of the Items tab, its rows, the search weights and the results, `TradeStatWeightMultiplierListControl.lua`.
//! Requests: `requests.rs`; the weights of the mods: `generator.rs`; `Buy similar`: `buy_similar.rs`.
//! Not ported: `PullLeagueList` (:79-108, no caller upstream); the slider's `tooltip.Draw` replacement (:733-741, a slider without a tooltip text never calls it).

pub mod buy_similar;
pub mod dkjson;
pub mod generator;
pub mod helpers;
pub mod requests;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_calc::env::{CalcOverride, OverrideNode};
use pob_engine::save::TradeWeight;
use pob_gfx::Gfx;
use pob_items::Item;
use serde_json::{Value as Json, json};

use self::generator::{Out, TradeQueryGenerator, get_stat, weighted_ratio_outputs};
use self::helpers::b64_gzip_encode;
use self::requests::{HOST_NAME_PATTERN, TradeItem, TradeQueryRequests, build_url};
use crate::colors::color_code;
use crate::control::{ControlRef, anchor, anchor_nil, rect};
use crate::controls::basic::{ButtonControl, LabelControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::list::{ListControl, ListHooks};
use crate::controls::scroll_bar::{ScrollBarControl, SliderControl};
use crate::launch::{PageParams, launch};
use crate::lua::lmatch;
use crate::modules::main::main;
use crate::poe_api::{self, os_time};
use crate::tabs::items::{ItemSlotControl, ItemsTab};
use crate::tooltip::Tooltip;
use crate::undo::UndoHost;

const HOST_NAME: &str = "https://www.pathofexile.com/";

/// `baseSlots` (:21).
const BASE_SLOTS: [&str; 18] = [
    "Weapon 1",
    "Weapon 2",
    "Weapon 1 Swap",
    "Weapon 2 Swap",
    "Helmet",
    "Body Armour",
    "Gloves",
    "Boots",
    "Amulet",
    "Ring 1",
    "Ring 2",
    "Ring 3",
    "Belt",
    "Flask 1",
    "Flask 2",
    "Flask 3",
    "Flask 4",
    "Flask 5",
];

/// `self.realmIds` (:55-59): the order of the constructor; `pairs` over it is of the process (R13).
const REALM_IDS: [(&str, &str); 3] = [("PC", "pc"), ("Xbox", "xbox"), ("Sony", "sony")];

thread_local! {
    /// R13: `pairs(self.realmIds)` of a process of the original, for the gate.
    static REALM_ORDER_SAMPLE: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
    /// R13: `pairs(main.onFrameFuncs)` (Main.lua:489-491) of that process: whether a query's search goes in the frame it was made or in the next.
    static FRAME_ORDER_SAMPLE: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// The gate's order of `pairs(self.realmIds)`.
pub fn set_realm_order_sample(sample: Option<Vec<String>>) {
    REALM_ORDER_SAMPLE.with(|s| *s.borrow_mut() = sample);
}

/// The gate's order of `pairs(main.onFrameFuncs)`.
pub fn set_frame_order_sample(sample: Option<Vec<String>>) {
    FRAME_ORDER_SAMPLE.with(|s| *s.borrow_mut() = sample);
}

fn apply_frame_order() {
    FRAME_ORDER_SAMPLE.with(|s| {
        if let Some(order) = &*s.borrow() {
            main()
                .on_frame_funcs
                .borrow_mut()
                .sort_by_cached_key(|k, _| order.iter().position(|x| x == k).unwrap_or(usize::MAX));
        }
    });
}

fn realm_pairs() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = REALM_IDS.iter().map(|(a, b)| ((*a).to_string(), (*b).to_string())).collect();
    REALM_ORDER_SAMPLE.with(|s| {
        if let Some(order) = &*s.borrow() {
            out.sort_by_key(|(k, _)| order.iter().position(|x| x == k).unwrap_or(usize::MAX));
        }
    });
    out
}

fn cc(name: &str) -> String {
    color_code(name).unwrap_or_default()
}

/// An entry of `statSortSelectionList`.
#[derive(Clone, Debug, PartialEq)]
pub struct TradeStat {
    pub stat: String,
    pub label: String,
    pub weight_mult: Option<f64>,
    /// `transform` (`function(value) return -value end`).
    pub negate: bool,
}

impl TradeStat {
    fn of(w: &TradeWeight) -> Option<Self> {
        let stat = w.stat.clone()?;
        let negate = pob_calc::power::find_power_stat(&stat).is_some_and(|p| p.transform.as_deref() == Some("negate"));
        Some(Self {
            stat,
            label: w.label.clone(),
            weight_mult: w.weight_mult,
            negate,
        })
    }
}

/// `initStatSortSelectionList` (:259-270).
fn default_stat_list() -> Vec<TradeStat> {
    vec![
        TradeStat {
            label: "Full DPS".to_string(),
            stat: "FullDPS".to_string(),
            weight_mult: Some(1.0),
            negate: false,
        },
        TradeStat {
            label: "Effective Hit Pool".to_string(),
            stat: "TotalEHP".to_string(),
            weight_mult: Some(0.5),
            negate: false,
        },
    ]
}

/// `isSameAsDefaultList` (:273-277).
fn is_same_as_default(list: &[TradeStat]) -> bool {
    list.len() == 2
        && list[0].stat == "FullDPS"
        && list[0].weight_mult == Some(1.0)
        && list[1].stat == "TotalEHP"
        && list[1].weight_mult == Some(0.5)
}

/// `TradeQuerySlotTable` (:537-544).
#[derive(Debug, Default)]
pub struct SlotTable {
    pub slot_name: String,
    pub full_name: Option<String>,
    pub node_id: Option<u32>,
    pub unique: bool,
    pub already_corrupted: bool,
    pub selected_jewel_node_id: Cell<Option<u32>>,
    pub selected_slot_name: RefCell<Option<String>>,
}

/// One row of `result.evaluation`.
#[derive(Clone, Debug)]
pub struct Evaluation {
    pub output: HashMap<String, f64>,
    pub weight: f64,
    pub dns: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
struct ResultEntry {
    item: TradeItem,
    evaluation: Option<Vec<Evaluation>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Sorted {
    index: usize,
    output_attr: f64,
}

/// `self.pbCurrencyConversion[realm]`.
#[derive(Clone, Debug, Default)]
struct RealmRates {
    timestamp: f64,
    leagues: BTreeMap<String, BTreeMap<String, f64>>,
}

/// The controls of one row.
struct Row {
    uri: Rc<EditControl>,
    price: Rc<ButtonControl>,
    result: Rc<DropDownControl>,
    name: Rc<LabelControl>,
}

pub struct TradeQuery {
    me: RefCell<Weak<Self>>,
    pub items_tab: Weak<ItemsTab>,
    pub requests: Rc<TradeQueryRequests>,
    pub generator: RefCell<Option<Rc<TradeQueryGenerator>>>,
    total_price: RefCell<BTreeMap<usize, (Option<String>, Option<f64>)>>,
    result_tbl: RefCell<HashMap<usize, Vec<ResultEntry>>>,
    sorted_result_tbl: RefCell<HashMap<usize, Vec<Sorted>>>,
    item_index_tbl: RefCell<HashMap<usize, usize>>,
    only_weighted_base_output: RefCell<HashMap<(usize, usize), HashMap<String, f64>>>,
    last_compared_weight_list: RefCell<HashMap<(usize, usize), Vec<TradeStat>>>,
    slot_tables: RefCell<BTreeMap<usize, Rc<SlotTable>>>,
    item_sort_index: Cell<usize>,
    currency: RefCell<HashMap<String, RealmRates>>,
    pub pb_realm: RefCell<String>,
    pb_realm_index: Cell<usize>,
    pb_league_index: Cell<usize>,
    pub pb_league: RefCell<Option<String>>,
    all_leagues: RefCell<HashMap<String, Vec<String>>>,
    realm_drop_list: RefCell<Vec<String>>,
    league_drop_list: RefCell<Vec<String>>,
    backoff_finish: Rc<Cell<Option<f64>>>,
    last_queries: RefCell<HashMap<usize, String>>,
    click_time: Cell<Option<f64>>,
    trade_type_index: Cell<Option<usize>>,
    max_fetch_pages: Cell<Option<f64>>,
    /// `self.statSortSelectionList`: from the engine until the first assignment here; the engine's copy is what `Save` writes.
    stat_sort: RefCell<Option<Vec<TradeStat>>>,
    // the controls `self.controls.X` of the latest `PriceItem` (the Lua table outlives a popup)
    pub(crate) trade_type_selection: RefCell<Option<Rc<DropDownControl>>>,
    pb_notice: RefCell<Option<Rc<LabelControl>>>,
    realm: RefCell<Option<Rc<DropDownControl>>>,
    league: RefCell<Option<Rc<DropDownControl>>>,
    full_price: RefCell<Option<Rc<LabelControl>>>,
    scroll_bar: RefCell<Option<Rc<ScrollBarControl>>>,
    rows: RefCell<BTreeMap<usize, Row>>,
    effective_rows_height: Cell<f64>,
    pane_height: Cell<f64>,
}

impl TradeQuery {
    /// `TradeQueryClass:TradeQuery(itemsTab)` (:27-74).
    pub fn new(items_tab: Weak<ItemsTab>) -> Rc<Self> {
        let t = Rc::new(Self {
            me: RefCell::new(Weak::new()),
            items_tab,
            requests: Rc::new(TradeQueryRequests::default()),
            generator: RefCell::new(None),
            total_price: RefCell::new(BTreeMap::new()),
            result_tbl: RefCell::new(HashMap::new()),
            sorted_result_tbl: RefCell::new(HashMap::new()),
            item_index_tbl: RefCell::new(HashMap::new()),
            only_weighted_base_output: RefCell::new(HashMap::new()),
            last_compared_weight_list: RefCell::new(HashMap::new()),
            slot_tables: RefCell::new(BTreeMap::new()),
            item_sort_index: Cell::new(1),
            currency: RefCell::new(HashMap::new()),
            pb_realm: RefCell::new(String::new()),
            pb_realm_index: Cell::new(1),
            pb_league_index: Cell::new(1),
            pb_league: RefCell::new(None),
            all_leagues: RefCell::new(HashMap::new()),
            realm_drop_list: RefCell::new(Vec::new()),
            league_drop_list: RefCell::new(Vec::new()),
            backoff_finish: Rc::new(Cell::new(None)),
            last_queries: RefCell::new(HashMap::new()),
            click_time: Cell::new(None),
            trade_type_index: Cell::new(None),
            max_fetch_pages: Cell::new(None),
            stat_sort: RefCell::new(None),
            trade_type_selection: RefCell::new(None),
            pb_notice: RefCell::new(None),
            realm: RefCell::new(None),
            league: RefCell::new(None),
            full_price: RefCell::new(None),
            scroll_bar: RefCell::new(None),
            rows: RefCell::new(BTreeMap::new()),
            effective_rows_height: Cell::new(0.0),
            pane_height: Cell::new(0.0),
        });
        *t.me.borrow_mut() = Rc::downgrade(&t);
        t
    }

    fn rc(&self) -> Rc<Self> {
        self.me.borrow().upgrade().expect("tradeQuery")
    }

    fn tab(&self) -> Rc<ItemsTab> {
        self.items_tab.upgrade().expect("itemsTab")
    }

    /// `self.statSortSelectionList` (`None`: nil).
    fn stat_list(&self) -> Option<Vec<TradeStat>> {
        if let Some(l) = &*self.stat_sort.borrow() {
            return Some(l.clone());
        }
        self.tab()
            .read(|e| e.trade_weights().map(|w| w.iter().filter_map(TradeStat::of).collect()))
    }

    /// `self.statSortSelectionList = list`: `ItemsTab:Save` reads it.
    fn set_stat_list(&self, list: Vec<TradeStat>) {
        let saved = list
            .iter()
            .map(|s| TradeWeight {
                label: s.label.clone(),
                stat: Some(s.stat.clone()),
                weight_mult: s.weight_mult,
            })
            .collect();
        self.tab().edit(|e| {
            e.set_trade_weights(saved);
            Ok(())
        });
        *self.stat_sort.borrow_mut() = Some(list);
    }

    fn notice(&self) -> Rc<LabelControl> {
        self.pb_notice.borrow().clone().expect("controls.pbNotice")
    }

    /// `:SetNotice(notice_control, msg)` (:798-805).
    fn set_notice(&self, msg: &str) {
        self.notice().label.set(notice_text(msg));
    }

    fn currency_map(&self) -> Option<BTreeMap<String, f64>> {
        let league = self.pb_league.borrow().clone()?;
        self.currency
            .borrow()
            .get(&*self.pb_realm.borrow())
            .and_then(|r| r.leagues.get(&league))
            .cloned()
    }

    /// `:ConvertCurrencyToDivs(currencyId, amount)` (:113-118).
    fn convert_currency_to_divs(&self, id: &str, amount: f64) -> Option<f64> {
        self.currency_map().and_then(|m| m.get(id).map(|r| amount * r))
    }

    /// `:PullCXData()` (:126-257).
    fn pull_cx_data(&self) {
        let realm = self.pb_realm.borrow().clone();
        if realm.is_empty() {
            return;
        }
        let now = os_time();
        if self.currency.borrow().get(&realm).is_some_and(|r| now - r.timestamp < 61.0 * 60.0) {
            return;
        }
        let me = self.rc();
        launch().download_page(
            &format!("{HOST_NAME}api/trade/data/static"),
            PageParams::default(),
            move |_g, response, err| {
                if let Some(e) = err {
                    me.set_notice(&format!("Error: {e}"));
                    return;
                }
                let Some(stat) = dkjson::decode(&response.body) else {
                    me.set_notice("Could not decode static trade data");
                    return;
                };
                let mut url = "https://web.poecdn.com/api/currency-exchange".to_string();
                if realm != "pc" {
                    url.push('/');
                    url.push_str(&realm);
                }
                let hour = 60.0 * 60.0;
                url.push_str(&format!("/{}", pob_lua::format::tostring_num(((now / hour).floor() - 1.0) * hour)));
                let me2 = me.clone();
                let realm2 = realm.clone();
                launch().download_page(&url, PageParams::default(), move |g, response, err| {
                    if let Some(e) = err {
                        me2.set_notice(&format!("Error: {e}"));
                        return;
                    }
                    let Some(j) = dkjson::decode(&response.body) else {
                        me2.set_notice("Malformed CX API response");
                        return;
                    };
                    if !j["error"].is_null() && j["error"] != json!(false) {
                        me2.set_notice(&format!("CX error: {}", j["error"]["message"].as_str().unwrap_or("")));
                        return;
                    }
                    match currency_exchange(&stat, &j) {
                        Ok(leagues) => {
                            me2.currency
                                .borrow_mut()
                                .insert(realm2.clone(), RealmRates { timestamp: now, leagues });
                        }
                        Err(e) => {
                            me2.set_notice("Failed to process CX response");
                            g.con_printf(&format!("CX error: {e}"));
                        }
                    }
                });
            },
        );
    }

    /// `:PriceItem()` (:280-689).
    pub fn price_item(&self, g: &mut Gfx) {
        let me = self.rc();
        let generator = TradeQueryGenerator::new(&me);
        *self.generator.borrow_mut() = Some(generator.clone());
        main()
            .on_frame_funcs
            .borrow_mut()
            .insert("TradeQueryGenerator".to_string(), Rc::new(move |g: &mut Gfx| generator.on_frame(g)));
        let tab = self.tab();
        let row_height = 20.0;
        let pad = 4.0;
        let margins_h = 16.0;
        let margins_v = 16.0;
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        // :293-305
        let order: Vec<f64> = tab.read(|e| e.item_sets().iter().map(|s| s.id).collect());
        let list: Vec<DropItem> = tab.read(|e| {
            e.item_sets()
                .iter()
                .map(|s| DropItem::str(s.title.as_deref().unwrap_or("Default")))
                .collect()
        });
        let t = self.items_tab.clone();
        let set_order = order.clone();
        let set_select = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(margins_h, margins_v, 188.0, row_height),
            list,
            Some(Rc::new(move |g: &mut Gfx, i: usize, _v: &DropItem| {
                let t = t.upgrade().expect("itemsTab");
                t.set_active_item_set(g, set_order[i - 1]);
                t.add_undo_state(false);
            })),
            None,
            false,
        );
        set_select.enable_dropped_width.set(true);
        let n_sets = order.len();
        set_select.base.enabled.set_fn(move |_| n_sets > 1);
        let ssr: ControlRef = set_select.clone();
        controls.insert("setSelect".into(), set_select);

        // :307-361
        if poe_api::is_authorized() {
            poe_api::validate_auth(g, |_g, valid, _e| {
                if !valid {
                    poe_api::reset_details();
                }
            });
        }
        let m = self.me.borrow().clone();
        let auth = ButtonControl::new(
            anchor("TOPLEFT", &ssr, "TOPLEFT"),
            rect(0.0, row_height + pad, 188.0, row_height),
            crate::control::Prop::func(move |_g| m.upgrade().expect("tradeQuery").login_status()),
            {
                let m = self.me.borrow().clone();
                move |_g| {
                    let t = m.upgrade().expect("tradeQuery");
                    if poe_api::is_authorized() {
                        // LOGOUT
                        {
                            let mm = main();
                            let mut a = mm.accounts.borrow_mut();
                            a.last_token = None;
                            a.last_refresh_token = None;
                            a.token_expiry = None;
                        }
                        main().save_settings();
                    } else {
                        // LOGIN
                        let m2 = Rc::downgrade(&t);
                        poe_api::fetch_auth_token(move |_g, _err| {
                            if poe_api::is_authorized()
                                && let Some(t) = m2.upgrade()
                            {
                                main().save_settings();
                                t.set_notice("");
                            }
                        });
                        t.click_time.set(Some(os_time()));
                    }
                }
            },
        );
        auth.tip.tooltip_text.set(Some(AUTH_TOOLTIP.to_string()));
        let ar: ControlRef = auth.clone();
        controls.insert("tradeAuthButton".into(), auth);
        // :380-393
        let list: Vec<DropItem> = [
            "Instant buyout",
            "Instant buyout and in person",
            "In person (online in league)",
            "In person (online)",
            "Any (includes offline)",
        ]
        .map(DropItem::str)
        .to_vec();
        let m = self.me.borrow().clone();
        let trade_type = DropDownControl::new(
            g,
            anchor("TOPLEFT", &ar, "BOTTOMLEFT"),
            rect(0.0, pad, 188.0, row_height),
            list,
            Some(Rc::new(move |_g: &mut Gfx, i: usize, _v: &DropItem| {
                m.upgrade().expect("tradeQuery").trade_type_index.set(Some(i));
            })),
            None,
            false,
        );
        trade_type.set_sel(g, self.trade_type_index.get().unwrap_or(1) as i64, false);
        *self.trade_type_selection.borrow_mut() = Some(trade_type.clone());
        let ttr: ControlRef = trade_type.clone();
        controls.insert("tradeTypeSelection".into(), trade_type);
        // :396-413
        const MAX_FETCH_DEFAULT: f64 = 2.0;
        let focus_value = Rc::new(Cell::new(MAX_FETCH_DEFAULT));
        let (m, fv) = (self.me.borrow().clone(), focus_value.clone());
        let fetch = EditControl::new(
            g,
            anchor_nil("TOPRIGHT", "TOPRIGHT"),
            rect(-12.0, 19.0, 150.0, row_height),
            EditArgs {
                init: Some(String::new()),
                prompt: Some("Fetch Pages".to_string()),
                filter: Some("%D".to_string()),
                limit: Some(3),
                change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _p: bool| {
                    let t = m.upgrade().expect("tradeQuery");
                    let pages = pob_lua::tonumber(buf).unwrap_or(MAX_FETCH_DEFAULT).clamp(1.0, 10.0);
                    t.max_fetch_pages.set(Some(pages));
                    t.requests.max_fetch_per_search.set((10.0 * pages) as usize);
                    fv.set(pages);
                })),
                ..EditArgs::default()
            },
        );
        self.requests.max_fetch_per_search.set((10.0 * MAX_FETCH_DEFAULT) as usize);
        fetch.set_text(
            g,
            &pob_lua::format::tostring_num(self.max_fetch_pages.get().unwrap_or(MAX_FETCH_DEFAULT)),
            false,
        );
        focus_value.set(MAX_FETCH_DEFAULT);
        let (w, fv) = (Rc::downgrade(&fetch), focus_value.clone());
        *fetch.on_focus_lost_fn.borrow_mut() = Some(Rc::new(move |g: &mut Gfx| {
            if let Some(f) = w.upgrade() {
                f.set_text(g, &pob_lua::format::tostring_num(fv.get()), false);
            }
        }));
        fetch.tip.set_tooltip_func(|tooltip: &mut Tooltip, g: &Gfx, ()| {
            tooltip.clear(false);
            tooltip.add_line(
                g,
                16.0,
                Some("Specify maximum number of item pages to retrieve per search from PoE Trade."),
            );
            tooltip.add_line(g, 16.0, Some("Each page fetches up to 10 items."));
            tooltip.add_line(g, 16.0, Some("Acceptable Range is: 1 to 10"));
        });
        let fr: ControlRef = fetch.clone();
        controls.insert("fetchCountEdit".into(), fetch);
        // :417-432 R58: the list is made here even when `ItemsTab:Load` never ran
        if self.stat_list().is_none_or(|l| l.is_empty()) {
            self.set_stat_list(default_stat_list());
        }
        let m = self.me.borrow().clone();
        let weights_button = ButtonControl::new(
            anchor("TOPRIGHT", &fr, "BOTTOMRIGHT"),
            rect(0.0, pad, 150.0, row_height),
            "^7Adjust search weights".to_string(),
            move |g| {
                let t = m.upgrade().expect("tradeQuery");
                t.tab().undo.mod_flag.set(true);
                t.set_stat_weights(g, Vec::new());
            },
        );
        let m = self.me.borrow().clone();
        weights_button.tip.set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, ()| {
            tooltip.clear(false);
            tooltip.add_line(g, 16.0, Some("Sorts the weights by the stats selected multiplied by a value"));
            tooltip.add_line(g, 16.0, Some("Currently sorting by:"));
            let t = m.upgrade().expect("tradeQuery");
            for s in t.stat_list().unwrap_or_default() {
                let line = pob_lua::format::format(
                    "%s: %.2f",
                    &[
                        pob_lua::format::Arg::Str(&s.label),
                        pob_lua::format::Arg::Num(s.weight_mult.unwrap_or(0.0)),
                    ],
                )
                .expect("format");
                tooltip.add_line(g, 16.0, Some(&line));
            }
        });
        let wr: ControlRef = weights_button.clone();
        controls.insert("StatWeightMultipliersButton".into(), weights_button);
        // :433-461
        let list: Vec<DropItem> = SORT_MODES.map(DropItem::str).to_vec();
        let m = self.me.borrow().clone();
        let item_sort = DropDownControl::new(
            g,
            anchor("TOPRIGHT", &wr, "TOPLEFT"),
            rect(-8.0, 0.0, 170.0, row_height),
            list,
            Some(Rc::new(move |g: &mut Gfx, i: usize, _v: &DropItem| {
                let t = m.upgrade().expect("tradeQuery");
                t.item_sort_index.set(i);
                let rows: Vec<usize> = t.result_tbl.borrow().keys().copied().collect();
                for r in rows {
                    t.update_controls_with_items(g, r);
                }
            })),
            Some(
                "Weighted Sum searches will always sort using descending weighted sum\nAdditional post filtering options can be done these include:\nHighest Stat Value - Sort from highest to lowest Stat Value change of equipping item\nHighest Stat Value / Price - Sorts from highest to lowest by estimated Stat Value per currency\nLowest Price - Sorts from lowest to highest price of retrieved items\nHighest Weight - Displays the order retrieved from trade",
            ),
            false,
        );
        item_sort.set_sel(g, self.item_sort_index.get() as i64, true);
        let isr: ControlRef = item_sort.clone();
        controls.insert("itemSortSelection".into(), item_sort);
        controls.insert(
            "itemSortSelectionLabel".into(),
            LabelControl::new(
                anchor("TOPRIGHT", &isr, "TOPLEFT"),
                rect(-4.0, 0.0, 56.0, 16.0),
                "^7Sort By:".to_string(),
            ),
        );
        // :464-504
        let realm_label = LabelControl::new(
            anchor("LEFT", &ssr, "RIGHT"),
            rect(18.0, 0.0, 20.0, row_height - 4.0),
            "^7Realm:".to_string(),
        );
        let rlr: ControlRef = realm_label.clone();
        controls.insert("realmLabel".into(), realm_label);
        let realm_items: Vec<DropItem> = self.realm_drop_list.borrow().iter().map(|s| DropItem::str(s)).collect();
        let m = self.me.borrow().clone();
        let realm = DropDownControl::new(
            g,
            anchor("LEFT", &rlr, "RIGHT"),
            rect(6.0, 0.0, 150.0, row_height),
            realm_items,
            Some(Rc::new(move |g: &mut Gfx, i: usize, v: &DropItem| {
                m.upgrade().expect("tradeQuery").on_realm(g, i, v.label.as_deref().unwrap_or(""));
            })),
            None,
            false,
        );
        *self.realm.borrow_mut() = Some(realm.clone());
        realm.set_sel(g, self.pb_realm_index.get() as i64, false);
        let w = Rc::downgrade(&realm);
        realm
            .base
            .enabled
            .set_fn(move |_| w.upgrade().expect("realm").list.borrow().len() > 1);
        controls.insert("realm".into(), realm);
        // :507-515
        let league_label = LabelControl::new(
            anchor("TOPRIGHT", &rlr, "TOPRIGHT"),
            rect(0.0, row_height + pad, 20.0, row_height - 4.0),
            "^7League:".to_string(),
        );
        let llr: ControlRef = league_label.clone();
        controls.insert("leagueLabel".into(), league_label);
        let league_items: Vec<DropItem> = self.league_drop_list.borrow().iter().map(|s| DropItem::str(s)).collect();
        let m = self.me.borrow().clone();
        let league = DropDownControl::new(
            g,
            anchor("LEFT", &llr, "RIGHT"),
            rect(6.0, 0.0, 150.0, row_height),
            league_items,
            Some(Rc::new(move |_g: &mut Gfx, i: usize, v: &DropItem| {
                let t = m.upgrade().expect("tradeQuery");
                t.pb_league_index.set(i);
                *t.pb_league.borrow_mut() = v.label.clone();
            })),
            None,
            false,
        );
        *self.league.borrow_mut() = Some(league.clone());
        league.set_sel(g, self.pb_league_index.get() as i64, false);
        let w = Rc::downgrade(&league);
        league
            .base
            .enabled
            .set_fn(move |_| w.upgrade().expect("league").list.borrow().len() > 1);
        controls.insert("league".into(), league);
        // :521-572
        let shown = |g: &Gfx, s: &ItemSlotControl| s.drop.base.shown.get(g);
        let mut abyssal: HashMap<String, Vec<Rc<ItemSlotControl>>> = HashMap::new();
        for (name, slot) in tab.slots.borrow().iter() {
            if name.contains("Abyssal")
                && shown(g, slot)
                && let Some(parent) = slot.parent_slot_name()
            {
                abyssal.entry(parent).or_default().push(slot.clone());
            }
        }
        for l in abyssal.values_mut() {
            l.sort_by(|a, b| a.label.borrow().cmp(&b.label.borrow()));
        }
        let mut slot_tables: Vec<SlotTable> = Vec::new();
        for name in BASE_SLOTS {
            if tab.slots.borrow().get(name).is_some_and(|s| shown(g, s)) {
                slot_tables.push(SlotTable {
                    slot_name: name.to_string(),
                    ..SlotTable::default()
                });
            }
            if let Some(list) = abyssal.get(name) {
                for s in list {
                    slot_tables.push(SlotTable {
                        slot_name: s.label.borrow().clone(),
                        full_name: Some(s.slot_name.clone()),
                        ..SlotTable::default()
                    });
                }
            }
        }
        let mut active_sockets: Vec<u32> = tab
            .sockets
            .borrow()
            .iter()
            .filter(|(_, s)| !s.inactive.get())
            .map(|(id, _)| *id)
            .collect();
        active_sockets.sort_unstable();
        // `spec.nodes[nodeId].containJewelSocket` is read in three places upstream and written in none
        let active_unique_socket = active_sockets.first().copied();
        for id in &active_sockets {
            slot_tables.push(SlotTable {
                slot_name: tab.sockets.borrow()[id].label.borrow().clone(),
                node_id: Some(*id),
                ..SlotTable::default()
            });
        }
        let n_slots = slot_tables.len();
        let section = LabelControl::new(anchor("LEFT", &ttr, "LEFT"), rect(0.0, pad, 0.0, 0.0), String::new());
        let top: ControlRef = section.clone();
        controls.insert("sectionAnchor".into(), section.clone());
        let scroll_bar_shown = n_slots > 21;
        let rwp = row_height + pad;
        let m = self.me.borrow().clone();
        let hide: Rc<dyn Fn(&Gfx, usize) -> bool> = Rc::new(move |g: &Gfx, index: usize| {
            if !scroll_bar_shown {
                return true;
            }
            let t = m.upgrade().expect("tradeQuery");
            let sb = t.scroll_bar.borrow().clone().expect("scrollBar");
            let max_items = (sb.base.height.get(g) / rwp).floor() - 2.0;
            let off = sb.offset.get();
            let i = index as f64;
            (i <= max_items && off < rwp * (i - 1.0) + pad)
                || (i >= max_items + 1.0 && off > rwp * (i - max_items) && off < rwp * (i - 1.0))
        });
        self.rows.borrow_mut().clear();
        self.slot_tables.borrow_mut().clear();
        for (i, st) in slot_tables.into_iter().enumerate() {
            let index = i + 1;
            self.slot_tables.borrow_mut().insert(index, Rc::new(st));
            self.price_item_row_display(g, index, &top, pad, row_height, &mut controls);
            let h = hide.clone();
            self.rows.borrow()[&index].name.base.shown.set_fn(move |g| h(g, index));
        }
        let other = LabelControl::new(
            anchor("TOPLEFT", &top, "TOPLEFT"),
            rect(0.0, (n_slots as f64 + 1.0) * rwp, 100.0, 16.0),
            "^8Other trades:".to_string(),
        );
        let h = hide.clone();
        other.base.shown.set_fn(move |g| h(g, n_slots + 1));
        controls.insert("otherTradesLabel".into(), other);
        // :605-628
        let mut row_count = n_slots + 1;
        let specials = [
            ("Megalomaniac", true, active_unique_socket),
            ("Watcher's Eye", false, active_unique_socket),
            ("Pearl of Tsoatha", false, None),
        ];
        for (k, (name, corrupted, socket)) in specials.into_iter().enumerate() {
            if k > 0 {
                row_count += 1;
            }
            let st = SlotTable {
                slot_name: name.to_string(),
                unique: true,
                already_corrupted: corrupted,
                selected_jewel_node_id: Cell::new(socket),
                ..SlotTable::default()
            };
            self.slot_tables.borrow_mut().insert(row_count, Rc::new(st));
            self.price_item_row_display(g, row_count, &top, pad, row_height, &mut controls);
            let rows = self.rows.borrow();
            let name_ctl = &rows[&row_count].name;
            name_ctl.base.y.set(name_ctl.base.y.get(g) + rwp);
            // `hideRowFunc(self, row_count)`: the upvalue, as it is after :638
            let h = hide.clone();
            let rc = n_slots + 5;
            name_ctl.base.shown.set_fn(move |g| h(g, rc));
        }
        // :638-643
        let row_count = row_count + 2;
        let n = n_slots as f64;
        let effective_row_count = row_count as f64 - if scroll_bar_shown && n_slots >= 19 { n - 19.0 } else { 0.0 } + 2.0 + 2.0;
        self.effective_rows_height
            .set(row_height * (effective_row_count - n + (18.0 - if n_slots > 37 { 3.0 } else { 0.0 })));
        self.pane_height.set(rwp * effective_row_count + 3.0 * margins_v + row_height / 2.0);
        let pane_width = 885.0 + if scroll_bar_shown { 25.0 } else { 0.0 };
        let sb = ScrollBarControl::new(
            anchor("TOPRIGHT", &wr, "TOPRIGHT"),
            rect(0.0, 25.0, 18.0, 0.0),
            Some(50.0),
            Some("VERTICAL"),
            false,
        );
        sb.base.shown.set(scroll_bar_shown);
        *self.scroll_bar.borrow_mut() = Some(sb.clone());
        controls.insert("scrollBar".into(), sb.clone());
        let full_price = LabelControl::new(
            anchor_nil("BOTTOM", "BOTTOM"),
            rect(0.0, -row_height - margins_v - pad, pane_width - 2.0 * margins_h, row_height),
            String::new(),
        );
        *self.full_price.borrow_mut() = Some(full_price.clone());
        controls.insert("fullPrice".into(), full_price);
        controls.insert(
            "close".into(),
            ButtonControl::new(
                anchor_nil("BOTTOM", "BOTTOM"),
                rect(0.0, -margins_v, 90.0, row_height),
                "Done".to_string(),
                |_g| {
                    main().close_popup();
                },
            ),
        );
        let notice = LabelControl::new(
            anchor_nil("BOTTOMRIGHT", "BOTTOMRIGHT"),
            rect(-row_height - margins_v - pad, -margins_v, 300.0, row_height),
            String::new(),
        );
        *self.pb_notice.borrow_mut() = Some(notice.clone());
        controls.insert("pbNotice".into(), notice);
        // the league and realm lists arrive here: `UpdateRealms` needs `pbNotice`, the Lua's table has it when a reply comes
        if self.pb_realm.borrow().is_empty() {
            self.update_realms(g);
        }
        let m = self.me.borrow().clone();
        let scroll_func: crate::controls::popup::PopupFunc = Rc::new(move |g: &Gfx| {
            let t = m.upgrade().expect("tradeQuery");
            let sb = t.scroll_bar.borrow().clone().expect("scrollBar");
            sb.base.height.set(crate::control::Prop::Val(t.pane_height.get() - 100.0));
            sb.set_content_dimension(g, t.pane_height.get() - 100.0, t.effective_rows_height.get());
            section.base.y.set(crate::control::Prop::Val(-sb.offset.get()));
        });
        // :662-686
        let notice_time = self.backoff_finish.clone();
        let on_rate_limit: Rc<dyn Fn(f64)> = Rc::new(move |backoff: f64| {
            notice_time.set(Some(os_time() + backoff));
        });
        let m = self.me.borrow().clone();
        main().on_frame_funcs.borrow_mut().insert(
            "TradeQueryRequests".to_string(),
            Rc::new(move |_g: &mut Gfx| {
                let t = m.upgrade().expect("tradeQuery");
                t.requests.process_queue(&on_rate_limit);
                t.count_down();
            }),
        );
        apply_frame_order();
        self.pull_cx_data();
        let popup_h = self.pane_height.get();
        main().open_popup(
            g,
            pane_width,
            popup_h,
            "Trader",
            controls,
            None,
            None,
            Some("close"),
            scroll_bar_shown.then_some(scroll_func),
        );
    }

    /// The `countDown` coroutine (:664-676): one resume per frame; each `onRateLimit` replaces it.
    fn count_down(&self) {
        let Some(finish) = self.backoff_finish.get() else { return };
        let now = os_time();
        if finish < now + 0.5 {
            self.backoff_finish.set(None);
            self.set_notice("");
            return;
        }
        let msg = format!(
            "{}Rate limited. Retrying after {} seconds...",
            cc("WARNING"),
            pob_lua::format::tostring_num(finish - now)
        );
        self.set_notice(&msg);
    }

    /// `self.loginStatus` (:307-322).
    fn login_status(&self) -> String {
        if poe_api::is_authorized() {
            self.click_time.set(None);
            return "Authenticated".to_string();
        }
        if let Some(c) = self.click_time.get() {
            let left = (c + 60.0 - os_time()).max(0.0);
            if left == 0.0 {
                self.click_time.set(None);
                return "Not authenticated".to_string();
            }
            return format!("Logging in... ({})", pob_lua::format::tostring_num(left));
        }
        format!("{}Not authenticated", cc("WARNING"))
    }

    /// The `selFunc` of the realm dropdown (:465-500).
    fn on_realm(&self, g: &mut Gfx, index: usize, value: &str) {
        self.pb_realm_index.set(index);
        let id = REALM_IDS.iter().find(|(k, _)| *k == value).map_or("", |(_, v)| *v);
        if *self.pb_realm.borrow() != id {
            *self.pb_realm.borrow_mut() = id.to_string();
            self.pull_cx_data();
        }
        let realm = id.to_string();
        if self.all_leagues.borrow().contains_key(&realm) {
            self.set_league_drop_list(g);
            return;
        }
        let m = self.me.borrow().clone();
        self.requests.fetch_leagues(
            &realm,
            Rc::new(move |g: &mut Gfx, leagues: Vec<String>, err: Option<String>| {
                let t = m.upgrade().expect("tradeQuery");
                if let Some(e) = err {
                    t.set_notice(&format!("Error while fetching league list: {e}"));
                    return;
                }
                let mut sorted: Vec<String> = leagues
                    .into_iter()
                    .filter(|l| !matches!(l.as_str(), "Standard" | "Ruthless" | "Hardcore" | "Hardcore Ruthless"))
                    .collect();
                for l in ["Standard", "Hardcore", "Ruthless", "Hardcore Ruthless"] {
                    sorted.push(l.to_string());
                }
                let realm_now = t.pb_realm.borrow().clone();
                t.all_leagues.borrow_mut().insert(realm_now, sorted);
                t.set_league_drop_list(g);
            }),
        );
    }

    /// `setLeagueDropList` (:471-477).
    fn set_league_drop_list(&self, g: &mut Gfx) {
        let list = self.all_leagues.borrow().get(&*self.pb_realm.borrow()).cloned().unwrap_or_default();
        *self.league_drop_list.borrow_mut() = list.clone();
        let league = self.league.borrow().clone().expect("controls.league");
        league.set_list(g, Some(list.iter().map(|s| DropItem::str(s)).collect()));
        league.sel_index.set(0);
        league.set_sel(g, self.pb_league_index.get() as i64, false);
    }

    /// `:SetStatWeights(previousSelectionList)` (:692-795).
    fn set_stat_weights(&self, g: &mut Gfx, previous: Vec<TradeStat>) {
        let popup_height = 500.0;
        let list_height = popup_height - 45.0 - 30.0 - 10.0;
        let mut stat_list: Vec<WeightRow> = Vec::new();
        for s in pob_data::rules::rules().power_stat_list.as_array().expect("powerStatList") {
            let label = s["label"].as_str().unwrap_or("");
            let Some(stat) = s["stat"].as_str() else { continue };
            if s["ignoreForItems"].as_bool() == Some(true) || label == "Name" {
                continue;
            }
            stat_list.push(WeightRow {
                label: format!("0      :  {label}"),
                stat: TradeStat {
                    label: label.to_string(),
                    stat: stat.to_string(),
                    negate: s["transform"].as_str() == Some("negate"),
                    weight_mult: Some(0.0),
                },
            });
        }
        let stat_list = Rc::new(RefCell::new(stat_list));
        let index = Rc::new(Cell::new(1usize));
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        let first = stat_list.borrow()[0].stat.label.clone();
        let slider_label = LabelControl::new(
            anchor_nil("TOPLEFT", "TOPRIGHT"),
            rect(-410.0, 20.0, 0.0, 16.0),
            format!("^7{first}:"),
        );
        let slr: ControlRef = slider_label.clone();
        let value_cell: Rc<RefCell<Option<Rc<LabelControl>>>> = Rc::new(RefCell::new(None));
        let (sl, ix, vc) = (stat_list.clone(), index.clone(), value_cell.clone());
        let slider = SliderControl::new(
            anchor("TOPLEFT", &slr, "TOPRIGHT"),
            rect(20.0, 0.0, 150.0, 16.0),
            Some(Rc::new(move |_g: &mut Gfx, value: f64| {
                let mut l = sl.borrow_mut();
                let row = &mut l[ix.get() - 1];
                let label = vc.borrow().clone();
                if value == 0.0 {
                    if let Some(c) = label {
                        c.label.set("^7Disabled".to_string());
                    }
                    row.stat.weight_mult = Some(0.0);
                    row.label = format!("0      :  {}", row.stat.label);
                } else {
                    let v = 0.01 + value * 0.99;
                    if let Some(c) = label {
                        c.label.set(fmt2("^7%.2f", v));
                    }
                    row.stat.weight_mult = Some(v);
                    row.label = format!("{}{}", fmt2("%.2f :  ", v), row.stat.label);
                }
            })),
            None,
        );
        let sr: ControlRef = slider.clone();
        let value = LabelControl::new(
            anchor("TOPLEFT", &sr, "TOPRIGHT"),
            rect(20.0, 0.0, 0.0, 16.0),
            "^7Disabled".to_string(),
        );
        *value_cell.borrow_mut() = Some(value.clone());
        let initial_rows: Vec<usize> = (0..stat_list.borrow().len()).collect();
        let list = ListControl::new(
            anchor_nil("TOPLEFT", "TOPRIGHT"),
            rect(-410.0, 45.0, 400.0, list_height),
            16.0,
            Some("VERTICAL"),
            false,
            initial_rows,
            false,
            Box::new(WeightList {
                rows: stat_list.clone(),
                index: index.clone(),
                slider: slider.clone(),
                slider_label: slider_label.clone(),
            }),
        );
        list.sel_index.set(None);
        controls.insert("ListControl".into(), list);
        controls.insert("SliderLabel".into(), slider_label);
        controls.insert("Slider".into(), slider.clone());
        controls.insert("SliderValue".into(), value);
        // :746-756
        for base in self.stat_list().unwrap_or_default() {
            let mult = base
                .weight_mult
                .unwrap_or_else(|| panic!("Classes/TradeQuery.lua:749: bad argument #2 to 'format' (number expected, got nil)"));
            let mut set_slider = false;
            {
                let mut rows = stat_list.borrow_mut();
                for r in rows.iter_mut() {
                    if r.stat.stat == base.stat {
                        r.stat.weight_mult = Some(mult);
                        r.label = format!("{}{}", fmt2("%.2f :  ", mult), base.label);
                    }
                }
                if rows[index.get() - 1].stat.stat == base.stat {
                    set_slider = true;
                }
            }
            if set_slider {
                slider.set_val(g, if mult == 1.0 { 1.0 } else { mult - 0.01 });
            }
        }
        // :758-775
        let m = self.me.borrow().clone();
        let sl = stat_list.clone();
        controls.insert(
            "finalise".into(),
            ButtonControl::new(
                anchor_nil("BOTTOM", "BOTTOM"),
                rect(-90.0, -10.0, 80.0, 20.0),
                "Save".to_string(),
                move |g| {
                    main().close_popup();
                    let t = m.upgrade().expect("tradeQuery");
                    let list: Vec<TradeStat> = sl
                        .borrow()
                        .iter()
                        .filter(|r| r.stat.weight_mult.unwrap_or(0.0) > 0.0)
                        .map(|r| r.stat.clone())
                        .collect();
                    if !list.is_empty() {
                        t.set_stat_list(list);
                    }
                    let rows: Vec<usize> = t.result_tbl.borrow().keys().copied().collect();
                    for r in rows {
                        t.update_controls_with_items(g, r);
                    }
                },
            ),
        );
        let m = self.me.borrow().clone();
        let prev = previous.clone();
        controls.insert(
            "cancel".into(),
            ButtonControl::new(
                anchor_nil("BOTTOM", "BOTTOM"),
                rect(0.0, -10.0, 80.0, 20.0),
                "Cancel".to_string(),
                move |_g| {
                    if !prev.is_empty() {
                        m.upgrade().expect("tradeQuery").set_stat_list(prev.clone());
                    }
                    main().close_popup();
                },
            ),
        );
        let m = self.me.borrow().clone();
        controls.insert(
            "reset".into(),
            ButtonControl::new(
                anchor_nil("BOTTOM", "BOTTOM"),
                rect(90.0, -10.0, 80.0, 20.0),
                "Reset".to_string(),
                move |g| {
                    let t = m.upgrade().expect("tradeQuery");
                    let cur = t.stat_list().unwrap_or_default();
                    let previous_selection = if is_same_as_default(&cur) { previous.clone() } else { cur };
                    t.set_stat_list(default_stat_list());
                    main().close_popup();
                    t.set_stat_weights(g, previous_selection);
                },
            ),
        );
        main().open_popup(g, 420.0, popup_height, "Stat Weight Multipliers", controls, None, None, None, None);
    }

    /// `:ReduceOutput(output)` (:808-819).
    fn reduce_output(&self, output: Out<'_>, list: &[TradeStat]) -> HashMap<String, f64> {
        let mut small = HashMap::new();
        for s in list {
            small.insert(s.stat.clone(), get_stat(output, &s.stat));
            let has_full = match output {
                Out::Full(o) => o.opt_num("FullDPS").is_some() || o.get("FullDPS").is_some(),
                Out::Small(m) => m.contains_key("FullDPS"),
            };
            if s.stat == "FullDPS" && !has_full {
                for k in ["TotalDPS", "TotalDotDPS", "CombinedDPS"] {
                    small.insert(k.to_string(), get_stat(output, k));
                }
            }
        }
        small
    }

    /// `:GetResultEvaluation(row_idx, result_index)` (:822-881).
    fn get_result_evaluation(&self, row: usize, index: usize) -> Vec<Evaluation> {
        let tab = self.tab();
        let list = self.stat_list().unwrap_or_default();
        let (cached, raw) = {
            let t = self.result_tbl.borrow();
            let e = &t[&row][index - 1];
            (e.evaluation.clone(), e.item.item_string.clone())
        };
        let slot_tbl = self.slot_tables.borrow()[&row].clone();
        let result = tab.read(|e| -> Result<Vec<Evaluation>, String> {
            let base = e.misc_base_run()?;
            let b = base.borrow();
            let base_out = Out::Full(&b.state.player.output);
            let only = self.reduce_output(base_out, &list);
            if let Some(c) = &cached
                && self.only_weighted_base_output.borrow().get(&(row, index)) == Some(&only)
                && self.last_compared_weight_list.borrow().get(&(row, index)) == Some(&list)
            {
                return Ok(c.clone());
            }
            self.only_weighted_base_output.borrow_mut().insert((row, index), only);
            self.last_compared_weight_list.borrow_mut().insert((row, index), list.clone());
            if slot_tbl.slot_name == "Megalomaniac" {
                let spec = e.spec_rc();
                let mut added = Vec::new();
                for caps in pob_items::lua::gmatch(&format!("{raw}\r\n"), "1 Added Passive Skill is (.-)\r?\n") {
                    if let Some(&ti) = caps.first().and_then(|n| spec.tree.cluster_node_map.get(n)) {
                        added.push(ti);
                    }
                }
                let eval = |nodes: &[usize], div: f64| -> Result<Evaluation, String> {
                    let run = e.item_override_run(CalcOverride {
                        add_nodes: Some(
                            nodes
                                .iter()
                                .map(|&t| OverrideNode::Other(Rc::new(spec.tree_node_view(t))))
                                .collect(),
                        ),
                        ..CalcOverride::default()
                    })?;
                    let r = run.borrow();
                    let output = self.reduce_output(Out::Full(&r.state.player.output), &list);
                    let weight = weighted_ratio_outputs(base_out, Out::Small(&output), &list) / div;
                    Ok(Evaluation {
                        output,
                        weight,
                        dns: Some(nodes.iter().map(|&t| spec.tree.nodes[t].dn.clone()).collect()),
                    })
                };
                let [a, b2, c] = added[..] else {
                    return Err("Classes/TradeQuery.lua:847: table index is nil".to_string());
                };
                let mut out = vec![
                    eval(&[a, b2], 4.0)?,
                    eval(&[a, c], 4.0)?,
                    eval(&[b2, c], 4.0)?,
                    eval(&[a, b2, c], 5.0)?,
                ];
                pob_engine::lua::lua_sort(&mut out, &mut |x: &Evaluation, y: &Evaluation| x.weight > y.weight);
                return Ok(out);
            }
            if slot_tbl.slot_name == "Pearl of Tsoatha" && slot_tbl.selected_slot_name.borrow().is_none() {
                for i in 1..=3 {
                    let name = format!("Ring {i}");
                    if tab.slots.borrow().contains_key(&name) && tab.read_slot_shown(&name) {
                        *slot_tbl.selected_slot_name.borrow_mut() = Some(name);
                        break;
                    }
                }
            }
            let jewel = slot_tbl.node_id.or(slot_tbl.selected_jewel_node_id.get());
            let slot_name = match jewel {
                Some(j) => format!("Jewel {j}"),
                None => slot_tbl
                    .selected_slot_name
                    .borrow()
                    .clone()
                    .unwrap_or_else(|| slot_tbl.slot_name.clone()),
            };
            let item = Item::parse_with(&raw, None, false);
            let run = e.item_override_run(CalcOverride {
                rep_slot_name: Some(slot_name),
                rep_item: Some(Rc::new(item)),
                ..CalcOverride::default()
            })?;
            let r = run.borrow();
            let output = self.reduce_output(Out::Full(&r.state.player.output), &list);
            let weight = weighted_ratio_outputs(base_out, Out::Small(&output), &list);
            Ok(vec![Evaluation { output, weight, dns: None }])
        });
        let v = result.unwrap_or_else(|e| panic!("{e}"));
        self.result_tbl.borrow_mut().get_mut(&row).expect("row")[index - 1].evaluation = Some(v.clone());
        v
    }

    /// `:UpdateDropdownList(row_idx)` (:884-899).
    fn update_dropdown_list(&self, g: &Gfx, row: usize) {
        let rows = self.result_tbl.borrow();
        let Some(results) = rows.get(&row) else { return };
        let mut labels = Vec::new();
        let sorted = self.sorted_result_tbl.borrow();
        for i in 0..results.len() {
            let pb = sorted[&row][i].index;
            let r = &results[pb - 1];
            let price = pob_lua::format::format(
                " %s(%d %s)",
                &[
                    pob_lua::format::Arg::Str(&cc("CURRENCY")),
                    pob_lua::format::Arg::Num(r.item.amount.unwrap_or(0.0)),
                    pob_lua::format::Arg::Str(r.item.currency.as_deref().unwrap_or("")),
                ],
            )
            .expect("format");
            let item = Item::parse_with(&r.item.item_string, None, false);
            labels.push(DropItem::str(&format!("{}{}{price}", cc(&item.rarity), item.name)));
        }
        let rows2 = self.rows.borrow();
        let d = &rows2[&row].result;
        d.sel_index.set(1);
        d.set_list(g, Some(labels));
    }

    /// `:ResetResultRow(rowIdx)` (:900-907).
    fn reset_result_row(&self, g: &Gfx, row: usize) {
        self.item_index_tbl.borrow_mut().remove(&row);
        self.sorted_result_tbl.borrow_mut().remove(&row);
        self.result_tbl.borrow_mut().remove(&row);
        self.total_price.borrow_mut().remove(&row);
        self.update_dropdown_list(g, row);
        self.set_full_price();
    }

    fn set_full_price(&self) {
        if let Some(l) = &*self.full_price.borrow() {
            l.label.set(format!("^7Total Price: {}", self.get_total_price_string()));
        }
    }

    /// `:UpdateControlsWithItems(row_idx)` (:908-936).
    fn update_controls_with_items(&self, g: &Gfx, row: usize) {
        let mode = SORT_MODES[self.item_sort_index.get() - 1];
        let (sorted, err) = self.sort_fetch_results(row, mode);
        let sorted = match err.as_deref() {
            Some("MissingConversionRates") => {
                self.set_notice("^4Currency rates unavailable. Falling back to Stat Value sort.");
                self.sort_fetch_results(row, SORT_MODES[0]).0
            }
            Some(e) => {
                self.set_notice(&format!("Error: {e}"));
                return;
            }
            None => {
                self.set_notice("");
                sorted
            }
        };
        let sorted = sorted.unwrap_or_default();
        let first = sorted.first().map(|s| s.index);
        self.sorted_result_tbl.borrow_mut().insert(row, sorted);
        let Some(pb) = first else {
            self.reset_result_row(g, row);
            self.set_notice("^4No compatible items found for this slot.");
            return;
        };
        self.item_index_tbl.borrow_mut().insert(row, pb);
        self.rows.borrow()[&row]
            .price
            .tip
            .tooltip_text
            .set(Some(format!("Sorted by {mode}")));
        let (currency, amount) = {
            let t = self.result_tbl.borrow();
            let i = &t[&row][pb - 1].item;
            (i.currency.clone(), i.amount)
        };
        self.total_price.borrow_mut().insert(row, (currency, amount));
        self.set_full_price();
        self.update_dropdown_list(g, row);
    }

    /// `:SetFetchResultReturn(row_idx, index)` (:939-947).
    fn set_fetch_result_return(&self, row: usize, index: usize) {
        let p = self
            .result_tbl
            .borrow()
            .get(&row)
            .and_then(|r| r.get(index - 1))
            .map(|e| (e.item.currency.clone(), e.item.amount));
        if let Some(p) = p {
            self.total_price.borrow_mut().insert(row, p);
            self.set_full_price();
        }
    }

    /// `:SortFetchResults(row_idx, mode)` (:950-1023).
    fn sort_fetch_results(&self, row: usize, mode: &str) -> (Option<Vec<Sorted>>, Option<String>) {
        let count = self.result_tbl.borrow().get(&row).map_or(0, Vec::len);
        let weight = |i: usize| self.get_result_evaluation(row, i).iter().map(|e| e.weight).sum::<f64>();
        let price_table = || -> Option<Vec<f64>> {
            let t = self.result_tbl.borrow();
            let mut out = Vec::new();
            for e in t.get(&row)? {
                match (&e.item.currency, e.item.amount) {
                    (Some(c), Some(a)) => out.push(self.convert_currency_to_divs(c, a)?),
                    _ => return None,
                }
            }
            Some(out)
        };
        let mut out = Vec::new();
        let desc = |a: &Sorted, b: &Sorted| a.output_attr > b.output_attr;
        match mode {
            SORT_WEIGHT => {
                for i in 1..=count {
                    out.push(Sorted {
                        index: i,
                        output_attr: i as f64,
                    });
                }
                return (Some(out), None);
            }
            SORT_STAT_VALUE => {
                for i in 1..=count {
                    out.push(Sorted {
                        index: i,
                        output_attr: weight(i),
                    });
                }
                pob_engine::lua::lua_sort(&mut out, &mut |a, b| desc(a, b));
            }
            SORT_STAT_VALUE_PRICE => {
                let Some(prices) = price_table() else {
                    return (None, Some("MissingConversionRates".to_string()));
                };
                for i in 1..=count {
                    // `math.log(x, 10)` (lib_math.c): `lj_vm_log2(x) * (1 / lj_vm_log2(10))`
                    let log = prices[i - 1].log2() * (1.0 / 10f64.log2());
                    out.push(Sorted {
                        index: i,
                        output_attr: weight(i) - 0.1 * log,
                    });
                }
                pob_engine::lua::lua_sort(&mut out, &mut |a, b| desc(a, b));
            }
            SORT_PRICE => {
                let Some(prices) = price_table() else {
                    return (None, Some("MissingConversionRates".to_string()));
                };
                for (i, p) in prices.iter().enumerate() {
                    out.push(Sorted {
                        index: i + 1,
                        output_attr: *p,
                    });
                }
                pob_engine::lua::lua_sort(&mut out, &mut |a, b| a.output_attr < b.output_attr);
            }
            _ => return (None, Some("InvalidSort".to_string())),
        }
        (Some(out), None)
    }

    /// `:FilterToSafeItems(itemEntries, slotName)` (:1029-1038).
    fn filter_to_safe_items(&self, items: Vec<TradeItem>, slot_name: Option<&str>) -> Vec<TradeItem> {
        let tab = self.tab();
        items
            .into_iter()
            .filter(|entry| {
                let item = Item::parse_with(&entry.item_string, None, false);
                item.base.is_some() && slot_name.is_none_or(|s| tab.read(|e| e.is_item_valid_for_slot(&item, s)))
            })
            .collect()
    }

    fn set_results(&self, row: usize, items: Vec<TradeItem>) {
        self.result_tbl
            .borrow_mut()
            .insert(row, items.into_iter().map(|item| ResultEntry { item, evaluation: None }).collect());
        // a new `result` table has no `evaluation`; the upvalue tables stay, keyed by index
    }

    /// The slot of a row: `getSelectedSlot` (:1049-1052).
    fn selected_slot(&self, st: &SlotTable, active: Option<&Rc<ItemSlotControl>>) -> Option<Rc<ItemSlotControl>> {
        let tab = self.tab();
        let node = st.node_id.or(st.selected_jewel_node_id.get());
        node.and_then(|n| tab.sockets.borrow().get(&n).cloned()).or_else(|| active.cloned())
    }

    fn tooltip_slot(&self, st: &SlotTable, active: Option<&Rc<ItemSlotControl>>) -> Option<crate::item_tooltip::CompareSlot> {
        let tab = self.tab();
        st.selected_jewel_node_id
            .get()
            .and_then(|n| tab.sockets.borrow().get(&n).cloned())
            .or_else(|| active.cloned())
            .map(|s| s.compare_info())
    }

    /// `:PriceItemRowDisplay(row_idx, top_pane_alignment_ref, row_vertical_padding, row_height)` (:1040-1338).
    fn price_item_row_display(
        &self,
        g: &mut Gfx,
        row: usize,
        top: &ControlRef,
        pad: f64,
        row_height: f64,
        controls: &mut IndexMap<String, ControlRef>,
    ) {
        let tab = self.tab();
        let st = self.slot_tables.borrow()[&row].clone();
        let node_id = st.node_id.or(st.selected_jewel_node_id.get());
        let active_slot: Option<Rc<ItemSlotControl>> = node_id.and_then(|n| tab.sockets.borrow().get(&n).cloned()).or_else(|| {
            let slots = tab.slots.borrow();
            slots
                .get(&st.slot_name)
                .or_else(|| st.full_name.as_ref().and_then(|f| slots.get(f)))
                .cloned()
        });
        let name_color = if st.unique { cc("UNIQUE") } else { "^7".to_string() };
        let name = LabelControl::new(
            anchor("TOPLEFT", top, "TOPLEFT"),
            rect(0.0, row as f64 * (row_height + pad), 135.0, row_height - 4.0),
            format!("{name_color}{}", st.slot_name),
        );
        let nr: ControlRef = name.clone();
        controls.insert(format!("name{row}"), name.clone());
        // :1055-1140
        let m = self.me.borrow().clone();
        let (st2, active2) = (st.clone(), active_slot.clone());
        let best = ButtonControl::new(
            anchor("LEFT", &nr, "LEFT"),
            rect(143.0, 0.0, 80.0, row_height),
            "Find best".to_string(),
            move |g| {
                let t = m.upgrade().expect("tradeQuery");
                t.find_best(g, row, st2.clone(), active2.clone());
            },
        );
        let m = self.me.borrow().clone();
        best.base
            .shown
            .set_fn(move |_| !m.upgrade().expect("tradeQuery").result_tbl.borrow().contains_key(&row));
        let m = self.me.borrow().clone();
        best.base
            .enabled
            .set_fn(move |_| m.upgrade().expect("tradeQuery").pb_league.borrow().is_some());
        best.tip.tooltip_text.set(Some("Creates a weighted search to find the highest Stat Value items for this slot.\nNote that even if you are authenticated, you can click this button again to show the search link.\nIf you have additional requirements that the trade tool doesn't cover (e.g. Adorned Magic jewels),\nyou can add them, copy the link here, and press \"Price Item\" to evaluate the items.".to_string()));
        if let Some(node) = st.node_id {
            // :1129-1140 `onHover`: the box's bottom on the top of the button
            let (w, tb) = (Rc::downgrade(&best), self.items_tab.clone());
            *best.on_hover_frame.borrow_mut() = Some(Rc::new(move |f: &mut pob_gfx::Frame<'_>| {
                let b = w.upgrade().expect("bestButton");
                let (x, y) = crate::control::Control::get_pos(&*b, f);
                let (bw, _) = crate::control::Control::get_size(&*b, f);
                let size = 250.0;
                tb.upgrade()
                    .expect("itemsTab")
                    .draw_socket_viewer(f, node, x - size / 2.0 + bw / 2.0, y - size - 4.0, size, size);
            }));
        }
        let br: ControlRef = best.clone();
        controls.insert(format!("bestButton{row}"), best.clone());
        // :1142-1168
        let valid_url = Rc::new(Cell::new(false));
        let vu = valid_url.clone();
        let uri = EditControl::new(
            g,
            anchor("TOPLEFT", &br, "TOPRIGHT"),
            rect(8.0, 0.0, 514.0, row_height),
            EditArgs {
                filter: Some("^%C\t\n".to_string()),
                change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _p: bool| {
                    let subpath = lmatch(buf, &format!("{HOST_NAME_PATTERN}trade/search/(.+)$")).unwrap_or_default();
                    let paths = subpath.split('/').filter(|p| !p.is_empty()).count();
                    vu.set(paths == 2 || paths == 3);
                    // :1154-1157 `activeItemSet[nodeId] = { pbURL = "" }`: `Save` writes the same empty attribute with or without it
                })),
                ..EditArgs::default()
            },
        );
        uri.set_placeholder(g, "Paste trade URL here...", false);
        let w = Rc::downgrade(&uri);
        uri.tip.set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, ()| {
            tooltip.clear(false);
            let u = w.upgrade().expect("uri");
            if crate::lua::find(&u.buf.borrow(), &format!("^{HOST_NAME_PATTERN}trade/search/"), None, false).is_some() {
                tooltip.add_line(g, 16.0, Some("Control + click to open in web-browser"));
            }
        });
        let ur: ControlRef = uri.clone();
        controls.insert(format!("uri{row}"), uri.clone());
        // :1169-1212
        let m = self.me.borrow().clone();
        let (st2, active2, w) = (st.clone(), active_slot.clone(), Rc::downgrade(&uri));
        let price = ButtonControl::new(
            anchor("TOPLEFT", &ur, "TOPRIGHT"),
            rect(8.0, 0.0, 100.0, row_height),
            "Price Item".to_string(),
            move |g| {
                let t = m.upgrade().expect("tradeQuery");
                t.rows.borrow()[&row].price.label.set("Searching...".to_string());
                let mut url = w.upgrade().expect("uri").buf.borrow().clone();
                if !url.starts_with("https://") {
                    url = format!("https://{url}");
                }
                let t2 = t.clone();
                let (st3, active3) = (st2.clone(), active2.clone());
                let requests = t.requests.clone();
                requests.search_with_url(
                    g,
                    &url,
                    Rc::new(move |g: &mut Gfx, items, err, query| {
                        if let Some(e) = err {
                            t2.set_notice(&format!("Error: {e}"));
                        } else {
                            t2.set_notice("");
                            if let Some(q) = query {
                                t2.last_queries.borrow_mut().insert(row, q);
                            }
                            let selected = t2.selected_slot(&st3, active3.as_ref());
                            let safe = t2.filter_to_safe_items(items.unwrap_or_default(), selected.as_ref().map(|s| s.slot_name.as_str()));
                            t2.set_results(row, safe);
                            t2.update_controls_with_items(g, row);
                        }
                        t2.rows.borrow()[&row].price.label.set("Price Item".to_string());
                    }),
                );
            },
        );
        let (st2, vu, w, tb) = (st.clone(), valid_url.clone(), Rc::downgrade(&price), self.items_tab.clone());
        price.base.enabled.set_fn(move |g| {
            let is_searching = w.upgrade().expect("price").label.get(g) == "Searching...";
            let requires = !st2.unique || is_jewel_unique(&st2.slot_name);
            let selected = st2
                .selected_jewel_node_id
                .get()
                .and_then(|n| tb.upgrade().expect("tab").sockets.borrow().get(&n).cloned());
            let has = !st2.unique || selected.is_some_and(|s| !s.inactive.get());
            poe_api::is_authorized() && vu.get() && !is_searching && (has || !requires)
        });
        let (st2, vu, tb) = (st.clone(), valid_url.clone(), self.items_tab.clone());
        price.tip.set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, ()| {
            tooltip.clear(false);
            if !poe_api::is_authorized() {
                tooltip.add_line(g, 16.0, Some("You must log in to use the search feature"));
            } else if !vu.get() {
                tooltip.add_line(g, 16.0, Some("Enter a valid trade URL"));
            } else if is_jewel_unique(&st2.slot_name) {
                let ok = st2
                    .selected_jewel_node_id
                    .get()
                    .and_then(|n| tb.upgrade().expect("tab").sockets.borrow().get(&n).cloned())
                    .is_some_and(|s| !s.inactive.get());
                if !ok {
                    tooltip.add_line(g, 16.0, Some("Requires an active Jewel Socket"));
                }
            }
        });
        controls.insert(format!("priceButton{row}"), price.clone());
        // :1216-1224
        let m = self.me.borrow().clone();
        let change = ButtonControl::new(
            anchor("LEFT", &nr, "LEFT"),
            rect(143.0, 0.0, 80.0, row_height),
            "<< Search".to_string(),
            move |g| {
                m.upgrade().expect("tradeQuery").reset_result_row(g, row);
            },
        );
        let m = self.me.borrow().clone();
        change
            .base
            .shown
            .set_fn(move |_| m.upgrade().expect("tradeQuery").result_tbl.borrow().contains_key(&row));
        let cr: ControlRef = change.clone();
        controls.insert(format!("changeButton{row}"), change.clone());
        let m = self.me.borrow().clone();
        let result = DropDownControl::new(
            g,
            anchor("TOPLEFT", &cr, "TOPRIGHT"),
            rect(8.0, 0.0, 351.0, row_height),
            Vec::new(),
            Some(Rc::new(move |_g: &mut Gfx, i: usize, _v: &DropItem| {
                let t = m.upgrade().expect("tradeQuery");
                let idx = t.sorted_result_tbl.borrow()[&row][i - 1].index;
                t.item_index_tbl.borrow_mut().insert(row, idx);
                t.set_fetch_result_return(row, idx);
            })),
            None,
            false,
        );
        let rr: ControlRef = result.clone();
        controls.insert(format!("resultDropdown{row}"), result.clone());
        let m = self.me.borrow().clone();
        let (st2, active2) = (st.clone(), active_slot.clone());
        result
            .tip
            .set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, (_mode, index, _v)| {
                let t = m.upgrade().expect("tradeQuery");
                let pb = t
                    .sorted_result_tbl
                    .borrow()
                    .get(&row)
                    .and_then(|s| s.get(index.wrapping_sub(1)))
                    .map(|s| s.index);
                let Some(pb) = pb else { return };
                let raw = t.result_tbl.borrow().get(&row).and_then(|r| r.get(pb - 1)).map(|e| e.item.clone());
                let Some(entry) = raw else { return };
                let item = Rc::new(Item::parse_with(&entry.item_string, None, false));
                tooltip.clear(false);
                t.tab()
                    .add_tooltip(tooltip, g, &item, t.tooltip_slot(&st2, active2.as_ref()).as_ref(), false);
                t.megalomaniac_tooltip(tooltip, g, &st2, row, pb);
                tooltip.add_separator(Some(10.0));
                tooltip.add_line(
                    g,
                    16.0,
                    Some(&format!(
                        "^7Price: {} {}",
                        entry.amount.map_or("nil".to_string(), pob_lua::format::tostring_num),
                        entry.currency.as_deref().unwrap_or("nil")
                    )),
                );
            });
        // :1257-1285
        let m = self.me.borrow().clone();
        let st2 = st.clone();
        let import = ButtonControl::new(
            anchor("TOPLEFT", &rr, "TOPRIGHT"),
            rect(8.0, 0.0, 100.0, row_height),
            "Import Item".to_string(),
            move |g| {
                let t = m.upgrade().expect("tradeQuery");
                let tab = t.tab();
                let raw = {
                    let i = t.item_index_tbl.borrow()[&row];
                    t.result_tbl.borrow()[&row][i - 1].item.item_string.clone()
                };
                tab.create_display_item_from_raw(g, &raw, false);
                let item = tab.display_item.borrow().clone();
                let id = tab.add_display_item(g, true);
                let jewel = st2.node_id.or(st2.selected_jewel_node_id.get());
                let slot = match jewel {
                    Some(n) => tab.sockets.borrow().get(&n).cloned(),
                    None => tab.slots.borrow().get(&st2.slot_name).cloned(),
                };
                if let (Some(slot), Some(item), Some(id)) = (slot, item, id)
                    && (jewel.is_some() || st2.slot_name == *slot.label.borrow())
                    && crate::control::Control::is_shown(&*slot, g)
                    && tab.read(|e| e.is_item_valid_for_slot(&item, &slot.slot_name))
                {
                    slot.set_sel_item_id(id);
                    tab.populate_slots(g);
                    tab.add_undo_state(false);
                    tab.build().build_flag.set(true);
                }
            },
        );
        let m = self.me.borrow().clone();
        let (st2, active2) = (st.clone(), active_slot.clone());
        import.tip.set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, ()| {
            tooltip.clear(false);
            let t = m.upgrade().expect("tradeQuery");
            let idx = t.item_index_tbl.borrow().get(&row).copied();
            let Some(idx) = idx else {
                panic!("Classes/TradeQuery.lua:1275: attempt to index a nil value");
            };
            let raw = t.result_tbl.borrow()[&row][idx - 1].item.item_string.clone();
            let item = Rc::new(Item::parse_with(&raw, None, false));
            t.tab()
                .add_tooltip(tooltip, g, &item, t.tooltip_slot(&st2, active2.as_ref()).as_ref(), true);
            t.megalomaniac_tooltip(tooltip, g, &st2, row, idx);
        });
        let m = self.me.borrow().clone();
        import
            .base
            .enabled
            .set_fn(move |_| m.upgrade().expect("tradeQuery").item_index_tbl.borrow().contains_key(&row));
        let ir: ControlRef = import.clone();
        controls.insert(format!("importButton{row}"), import);
        // :1287-1337
        let m = self.me.borrow().clone();
        let label = crate::control::Prop::func(move |_g| {
            let t = m.upgrade().expect("tradeQuery");
            let Some(item) = t.current_result(row) else { return String::new() };
            let price = t.total_price.borrow().get(&row).map(|(c, a)| {
                format!(
                    "{} {}",
                    a.map_or("nil".to_string(), pob_lua::format::tostring_num),
                    c.as_deref().unwrap_or("nil")
                )
            });
            if item.whisper.is_some() && item.price_type.as_deref() != Some("~b/o") {
                price.map_or("Whisper".to_string(), |p| format!("Whisper for {p}"))
            } else {
                price.map_or("Search".to_string(), |p| format!("Search for {p}"))
            }
        });
        let m = self.me.borrow().clone();
        let whisper = ButtonControl::new(
            anchor("TOPLEFT", &ir, "TOPRIGHT"),
            rect(8.0, 0.0, 155.0, row_height),
            label,
            move |g| {
                m.upgrade().expect("tradeQuery").whisper_click(g, row);
            },
        );
        let m = self.me.borrow().clone();
        whisper.tip.set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, ()| {
            tooltip.clear(false);
            tooltip.center = true;
            let t = m.upgrade().expect("tradeQuery");
            let has = t
                .current_result(row)
                .unwrap_or_else(|| panic!("Classes/TradeQuery.lua:1334: attempt to index local 'itemResult' (a nil value)"))
                .whisper
                .is_some();
            tooltip.add_line(
                g,
                16.0,
                Some(if has {
                    "Copies the item purchase whisper to the clipboard"
                } else {
                    "Opens the search page to show the item"
                }),
            );
        });
        controls.insert(format!("whisperButton{row}"), whisper);
        self.rows.borrow_mut().insert(row, Row { uri, price, result, name });
        self.update_dropdown_list(g, row);
    }

    /// `self.itemIndexTbl[row_idx] and self.resultTbl[row_idx][self.itemIndexTbl[row_idx]]`.
    fn current_result(&self, row: usize) -> Option<TradeItem> {
        let i = *self.item_index_tbl.borrow().get(&row)?;
        self.result_tbl
            .borrow()
            .get(&row)
            .and_then(|r| r.get(i - 1))
            .map(|e| e.item.clone())
    }

    /// `addMegalomaniacCompareToTooltipIfApplicable(tooltip, result_index)` (:1225-1238).
    fn megalomaniac_tooltip(&self, tooltip: &mut Tooltip, g: &Gfx, st: &SlotTable, row: usize, index: usize) {
        if st.slot_name != "Megalomaniac" {
            return;
        }
        for entry in self.get_result_evaluation(row, index) {
            tooltip.add_separator(Some(10.0));
            let dns = entry.dns.as_ref().expect("evaluationEntry.DNs");
            let node_combo = dns.join(" ^8+^7 ");
            let base = self.only_weighted_base_output.borrow().get(&(row, index)).cloned().unwrap_or_default();
            let lines = self
                .tab()
                .read(|e| {
                    e.stat_compare_small(
                        &base,
                        &entry.output,
                        &format!("^8Allocating ^7{node_combo}^8 will give You:"),
                        dns.len() as f64 + 2.0,
                    )
                })
                .unwrap_or_else(|er| panic!("{er}"));
            for l in lines {
                tooltip.add_line(g, 14.0, Some(&l));
            }
        }
    }

    /// The click of `whisperButton` (:1303-1328).
    fn whisper_click(&self, g: &mut Gfx, row: usize) {
        let Some(item) = self.current_result(row) else {
            panic!("Classes/TradeQuery.lua:1305: attempt to index local 'itemResult' (a nil value)");
        };
        if let Some(w) = item.whisper.as_ref().filter(|_| item.price_type.as_deref() != Some("~b/o")) {
            g.copy(w);
            return;
        }
        let last = self.last_queries.borrow().get(&row).cloned().unwrap_or_default();
        let mut exact = dkjson::decode(&last)
            .unwrap_or_else(|| panic!("Classes/TradeQuery.lua:1312: attempt to index local 'exactQuery' (a nil value)"));
        let q = &mut exact["query"];
        if q["stats"][0]["type"] == "weight" {
            let w = pob_lua::tonumber(&item.weight).unwrap_or(0.0);
            q["stats"][0]["value"] = json!({"min": dkjson::n(pob_lua::number::floor(w, Some(1.0)) - 1.0), "max": dkjson::n(pob_lua::number::round(w, Some(1.0)) + 1.0)});
        }
        if q["filters"].is_null() || q["filters"].as_array().is_some_and(Vec::is_empty) {
            q["filters"] = json!({});
        }
        if q["filters"]["trade_filters"].is_null() {
            q["filters"]["trade_filters"] = json!({"filters": {}});
        }
        if q["filters"]["trade_filters"]["filters"].is_null() {
            q["filters"]["trade_filters"]["filters"] = json!({});
        }
        q["filters"]["trade_filters"]["filters"]["account"] = json!({"input": item.trader});
        let url = build_url(
            &format!("{HOST_NAME}trade/search"),
            Some(&self.pb_realm.borrow()),
            self.pb_league.borrow().as_deref().unwrap_or(""),
            Some(&b64_gzip_encode(&dkjson::encode(q))),
        );
        g.copy(&url);
        g.open_url(&url);
    }

    /// The click of `bestButton` (:1055-1122).
    fn find_best(&self, g: &mut Gfx, row: usize, st: Rc<SlotTable>, active: Option<Rc<ItemSlotControl>>) {
        let generator = self.generator.borrow().clone().expect("tradeQueryGenerator");
        let list = self.stat_list().unwrap_or_default();
        let t = self.rc();
        let st2 = st.clone();
        let handler: generator::RequesterCb = Rc::new(move |g: &mut Gfx, query: Option<Json>, err: Option<String>| {
            if let Some(e) = err {
                t.set_notice(&format!("{}{e}", cc("NEGATIVE")));
                return;
            }
            t.set_notice("");
            let query = query.expect("query");
            let realm = t.pb_realm.borrow().clone();
            let league = t.pb_league.borrow().clone().unwrap_or_default();
            if !poe_api::is_authorized() {
                let url = format!(
                    "{}/{}",
                    build_url(&format!("{HOST_NAME}trade/search"), Some(&realm), &league, None),
                    b64_gzip_encode(&dkjson::encode(&query))
                );
                t.rows.borrow()[&row].uri.set_text(g, &url, true);
                return;
            }
            t.rows.borrow()[&row].price.label.set("Searching...".to_string());
            let text = dkjson::encode(&json!({"query": query, "sort": {"statgroup.0": "desc"}, "engine": "new"}));
            let (t2, t3) = (t.clone(), t.clone());
            let (st3, active3) = (st2.clone(), active.clone());
            let (r2, l2) = (realm.clone(), league.clone());
            t.requests.search_with_query_weight_adjusted(
                &realm,
                &league,
                text,
                Rc::new(move |g: &mut Gfx, items, err, final_query| {
                    if let Some(e) = err {
                        t2.set_notice(&format!("{}{e}", cc("NEGATIVE")));
                        t2.rows.borrow()[&row].price.label.set("Price Item".to_string());
                        return;
                    }
                    t2.set_notice("");
                    if let Some(q) = final_query {
                        t2.last_queries.borrow_mut().insert(row, q);
                    }
                    let selected = t2.selected_slot(&st3, active3.as_ref());
                    let mut safe = t2.filter_to_safe_items(items.unwrap_or_default(), selected.as_ref().map(|s| s.slot_name.as_str()));
                    // :1092-1107
                    let generator = t2.generator.borrow().clone().expect("generator");
                    let eldritch = generator.last_include_eldritch.borrow().clone();
                    let enchant = generator.last_copy_enchant_mode.borrow().clone();
                    for s in &mut safe {
                        let mut item = Item::parse_with(&s.item_string, None, false);
                        item.normalise_quality();
                        if eldritch.as_deref() == Some("Copy Current") || enchant.as_deref() == Some("Copy Current") {
                            t2.tab().copy_anoints_ex(&mut item, true, true, Some(&st3.slot_name));
                        } else if eldritch.as_deref() == Some("Remove") {
                            if item.influence("tangle") || item.influence("cleansing") {
                                item.implicit_mod_lines.clear();
                            }
                        } else if enchant.as_deref() == Some("Remove") {
                            item.enchant_mod_lines.clear();
                        }
                        s.item_string = item.build_raw();
                    }
                    t2.set_results(row, safe);
                    t2.update_controls_with_items(g, row);
                    t2.rows.borrow()[&row].price.label.set("Price Item".to_string());
                }),
                Rc::new(move |g: &mut Gfx, id: String| {
                    let url = build_url(&format!("{HOST_NAME}trade/search"), Some(&r2), &l2, Some(&id));
                    t3.rows.borrow()[&row].uri.set_text(g, &url, true);
                }),
            );
        });
        let active_slot = {
            let tab = self.tab();
            let node = st.node_id.or(st.selected_jewel_node_id.get());
            node.and_then(|n| tab.sockets.borrow().get(&n).cloned()).or_else(|| {
                let slots = tab.slots.borrow();
                slots
                    .get(&st.slot_name)
                    .or_else(|| st.full_name.as_ref().and_then(|f| slots.get(f)))
                    .cloned()
            })
        };
        generator.request_query(g, active_slot, st, list, handler);
    }

    /// `:GetTotalPriceString()` (:1341-1377).
    fn get_total_price_string(&self) -> String {
        let mut prices: Vec<(String, f64)> = Vec::new();
        for (currency, amount) in self.total_price.borrow().values() {
            let c = currency.clone().unwrap_or_default();
            let a = amount.unwrap_or(0.0);
            match prices.iter_mut().find(|p| p.0 == c) {
                Some(p) => p.1 += a,
                None => prices.push((c, a)),
            }
        }
        let map = self.currency_map().unwrap_or_default();
        let mut currencies: Vec<String> = prices.iter().map(|p| p.0.clone()).collect();
        pob_engine::lua::lua_sort(&mut currencies, &mut |a: &String, b: &String| match (map.get(a), map.get(b)) {
            (Some(x), Some(y)) => x > y,
            _ => a > b,
        });
        let parts: Vec<String> = currencies
            .iter()
            .map(|c| {
                let v = prices.iter().find(|p| &p.0 == c).map_or(0.0, |p| p.1);
                format!("{} {c}", pob_lua::format::tostring_num(v))
            })
            .collect();
        parts.join(", ")
    }

    /// `:UpdateRealms()` (:1380-1425).
    fn update_realms(&self, _g: &mut Gfx) {
        self.all_leagues.borrow_mut().clear();
        for (_, realm_id) in realm_pairs() {
            let m = self.me.borrow().clone();
            let rid = realm_id.clone();
            self.requests.fetch_leagues(
                &realm_id,
                Rc::new(move |g: &mut Gfx, leagues: Vec<String>, err: Option<String>| {
                    let t = m.upgrade().expect("tradeQuery");
                    if let Some(e) = err {
                        t.set_notice(&format!("Using Fallback Error while fetching league list: {e}"));
                    }
                    if !leagues.is_empty() {
                        t.all_leagues.borrow_mut().entry(rid.clone()).or_default().extend(leagues);
                    }
                    t.set_realm_drop_list(g);
                }),
            );
        }
        // :1415 a generic search to make sure the authorization is valid
        let m = self.me.borrow().clone();
        self.requests.perform_search(
            Some("pc"),
            "Standard",
            r#"{"query":{"status":{"option":"online"},"stats":[{"type":"and","filters":[]}]},"sort":{"price":"asc"}}"#.to_string(),
            Rc::new(move |_g: &mut Gfx, _r, err: Option<String>| {
                if let Some(mut e) = err {
                    if e == "Response code: 403" {
                        poe_api::reset_details();
                        e.push_str("\nPlease re-authenticate");
                    }
                    m.upgrade().expect("tradeQuery").set_notice(&format!("Error: {e}"));
                }
            }),
        );
    }

    /// `setRealmDropList` (:1381-1396).
    fn set_realm_drop_list(&self, g: &mut Gfx) {
        let mut list: Vec<String> = Vec::new();
        for (realm, _) in realm_pairs() {
            if realm == "PC" {
                list.insert(0, realm);
            } else {
                list.push(realm);
            }
        }
        *self.realm_drop_list.borrow_mut() = list.clone();
        let realm = self.realm.borrow().clone().expect("controls.realm");
        realm.set_list(g, Some(list.iter().map(|s| DropItem::str(s)).collect()));
        realm.sel_index.set(0);
        realm.set_sel(g, self.pb_realm_index.get() as i64, false);
    }

    /// What the gate compares with the same fields of `trade-runner.lua`.
    pub fn debug_dump(&self, _g: &Gfx) -> Json {
        let g17 = |v: f64| pob_lua::format::format("%.17g", &[pob_lua::format::Arg::Num(v)]).expect("format");
        let rows: BTreeMap<String, Json> = self
            .result_tbl
            .borrow()
            .iter()
            .map(|(k, v)| {
                let items: Vec<Json> = v
                    .iter()
                    .map(|e| {
                        json!({"item": e.item.item_string, "amount": e.item.amount, "currency": e.item.currency, "weight": e.item.weight,
                               "evaluation": e.evaluation.as_ref().map(|l| l.iter().map(|x| g17(x.weight)).collect::<Vec<_>>())})
                    })
                    .collect();
                (k.to_string(), Json::Array(items))
            })
            .collect();
        let sorted: BTreeMap<String, Json> = self
            .sorted_result_tbl
            .borrow()
            .iter()
            .map(|(k, v)| (k.to_string(), json!(v.iter().map(|s| s.index).collect::<Vec<_>>())))
            .collect();
        let index: BTreeMap<String, usize> = self.item_index_tbl.borrow().iter().map(|(k, v)| (k.to_string(), *v)).collect();
        let weights = self.stat_list().map(|l| {
            l.iter()
                .map(|s| format!("{}|{}|{}", s.stat, s.label, s.weight_mult.map_or("nil".to_string(), g17)))
                .collect::<Vec<_>>()
        });
        let mod_weights: Vec<String> = self
            .generator
            .borrow()
            .as_ref()
            .map(|gen_| {
                gen_.mod_weights
                    .borrow()
                    .iter()
                    .map(|w| format!("{}|{}|{}|{}", w.trade_mod_id, g17(w.weight), g17(w.mean_stat_diff), w.invert))
                    .collect()
            })
            .unwrap_or_default();
        json!({
            "pbRealm": *self.pb_realm.borrow(),
            "pbLeague": *self.pb_league.borrow(),
            "realmList": *self.realm_drop_list.borrow(),
            "leagueList": *self.league_drop_list.borrow(),
            "results": rows,
            "sorted": sorted,
            "itemIndex": index,
            "total": self.get_total_price_string(),
            "weights": weights,
            "modWeights": mod_weights,
            "lastQueries": self.last_queries.borrow().iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<BTreeMap<_, _>>(),
            "rowLists": self.rows.borrow().iter().map(|(k, r)| (k.to_string(), r.result.list.borrow().iter().map(|i| i.label.clone().unwrap_or_default()).collect::<Vec<_>>())).collect::<BTreeMap<_, _>>(),
            "rates": self.currency.borrow().iter().map(|(k, v)| (k.clone(), json!(v.leagues))).collect::<BTreeMap<_, _>>(),
        })
    }
}

/// The state of the real window (`tools/sg-oracle/trade-real-seq.ps1`; the same fields as `trade-runner.lua`).
pub fn debug_input_state(g: &Gfx) -> Json {
    let m = main();
    let b = m.build_mode.clone();
    let Some(tab) = b.items_tab.borrow().clone() else {
        return Json::Null;
    };
    let pos = |c: &dyn crate::control::Control| {
        let (x, y) = c.get_pos(g);
        let (w, h) = c.get_size(g);
        json!({"x": (x + w / 2.0).floor(), "y": (y + h / 2.0).floor()})
    };
    let (Some(mode), Some(price)) = (b.host.get("modeItems"), tab.debug_control("priceDisplayItem")) else {
        return Json::Null;
    };
    let mut t = serde_json::Map::new();
    t.insert("viewMode".into(), json!(*b.view_mode.borrow()));
    t.insert("modeItems".into(), pos(&*mode));
    t.insert("price".into(), pos(&*price));
    t.insert("popups".into(), json!(m.popups.borrow().len()));
    if let Some(p) = m.popups.borrow().first() {
        t.insert("title".into(), json!(p.title.get(g)));
        for (name, field) in [
            ("fetchCountEdit", "fetch"),
            ("StatWeightMultipliersButton", "weights"),
            ("close", "close"),
            ("ListControl", "list"),
            ("Slider", "slider"),
            ("cancel", "cancel"),
        ] {
            if let Some(c) = p.host.get(name) {
                t.insert(field.into(), pos(&*c));
            }
        }
        if let Some(c) = p.host.get("fetchCountEdit") {
            t.insert("fetchBuf".into(), c.debug_value(g).unwrap_or(Json::Null));
        }
        if let Some(c) = p.host.get("SliderValue") {
            t.insert("sliderValue".into(), c.debug_value(g).unwrap_or(Json::Null));
        }
    }
    Json::Object(t)
}

fn is_jewel_unique(name: &str) -> bool {
    name == "Megalomaniac" || name == "Watcher's Eye"
}

fn notice_text(msg: &str) -> String {
    if msg.contains("No Matching Results") {
        format!("{}{msg}", cc("WARNING"))
    } else if msg.contains("Error") {
        format!("{}{msg}", cc("NEGATIVE"))
    } else {
        msg.to_string()
    }
}

fn fmt2(pat: &str, v: f64) -> String {
    pob_lua::format::format(pat, &[pob_lua::format::Arg::Num(v)]).expect("format")
}

const SORT_STAT_VALUE: &str = "(Highest) Stat Value";
const SORT_STAT_VALUE_PRICE: &str = "Stat Value / Price";
const SORT_PRICE: &str = "(Lowest) Price";
const SORT_WEIGHT: &str = "(Highest) Weighted Sum";
const SORT_MODES: [&str; 4] = [SORT_STAT_VALUE, SORT_STAT_VALUE_PRICE, SORT_PRICE, SORT_WEIGHT];

const AUTH_TOOLTIP: &str = "The Trader feature supports two modes of operation depending on the authorization availability.\nYou can click this button to authorize PoB by logging in.\n\n^2Session Mode^7\n- Requires authorization on pathofexile.com.\n- You can search, compare, and quickly import items without leaving Path of Building.\n- You can select an item and search it directly.\n- You can generate and perform searches for the private leagues you are participating.\n\n^xFF9922No Session Mode^7\n- Doesn't require authorization.\n- You cannot search and compare items in Path of Building.\n- You can generate weighted search URLs but have to visit the trade site and manually import items.\n- You can only generate weighted searches for public leagues. (Generated searches can be modified\non trade site to work on other leagues and realms)";

/// `statList[i]`.
struct WeightRow {
    label: String,
    stat: TradeStat,
}

/// `TradeStatWeightMultiplierListControl.lua`.
struct WeightList {
    rows: Rc<RefCell<Vec<WeightRow>>>,
    index: Rc<Cell<usize>>,
    slider: Rc<SliderControl>,
    slider_label: Rc<LabelControl>,
}

impl ListHooks<usize> for WeightList {
    fn get_row_value(&self, _list: &ListControl<usize>, _g: &Gfx, column: usize, _index: usize, value: &usize) -> String {
        if column == 1 {
            self.rows.borrow()[*value].label.clone()
        } else {
            String::new()
        }
    }

    fn has_add_value_tooltip(&self) -> bool {
        true
    }

    fn add_value_tooltip(&self, _list: &ListControl<usize>, g: &Gfx, tooltip: &mut Tooltip, _index: usize, _value: &usize) {
        tooltip.clear(false);
        tooltip.add_line(g, 16.0, Some("^7Click to modify this stats weight multiplier."));
    }

    fn on_sel_click(&self, _list: &ListControl<usize>, g: &mut Gfx, index: usize, _value: &usize, _double_click: bool) -> Option<bool> {
        if self.index.get() != index {
            self.index.set(index);
            let (label, mult) = {
                let r = self.rows.borrow();
                (r[index - 1].stat.label.clone(), r[index - 1].stat.weight_mult.unwrap_or(0.0))
            };
            self.slider_label.label.set(label);
            self.slider.set_val(g, if mult == 1.0 { 1.0 } else { mult - 0.01 });
        }
        None
    }
}

/// The `pcall` of `PullCXData` (:170-247).
fn currency_exchange(stat: &Json, json: &Json) -> Result<BTreeMap<String, BTreeMap<String, f64>>, String> {
    let root = launch().cfg.script_path.clone();
    let path = format!("{root}Data/CurrencyNames.lua");
    let src = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let names = crate::lua_data::parse_return(&src)?;
    let mut id_map: HashMap<String, String> = HashMap::new();
    for (k, v) in &names.as_tab().ok_or("CurrencyNames")?.fields {
        if let (crate::lua_data::LuaKey::Str(k), Some(v)) = (k, v.as_str()) {
            id_map.insert(v.to_string(), k.to_string());
        }
    }
    let mut currency_names: HashMap<String, String> = HashMap::new();
    let entries = stat["result"][0]["entries"].as_array().ok_or("static.result[1].entries")?;
    for e in entries {
        if e["id"] != "sep"
            && let Some(id) = e["text"].as_str().and_then(|t| id_map.get(t))
        {
            currency_names.insert(id.clone(), e["id"].as_str().unwrap_or("").to_string());
        }
    }
    const CHAOS: &str = "Metadata/Items/Currency/CurrencyRerollRare";
    const DIVINE: &str = "Metadata/Items/Currency/CurrencyModValues";
    let general = |s: &str| s == CHAOS || s == DIVINE;
    struct E {
        currency: String,
        price: f64,
        stock: f64,
    }
    let mut out: BTreeMap<String, BTreeMap<String, E>> = BTreeMap::new();
    for entry in json["markets"].as_array().ok_or("json.markets")? {
        let league = entry["league"].as_str().ok_or("league")?.to_string();
        let league_out = out.entry(league).or_default();
        let mut from = entry["market_pair"][0].as_str().unwrap_or("").to_string();
        let mut to = entry["market_pair"][1].as_str().unwrap_or("").to_string();
        if general(&from) && to != DIVINE {
            std::mem::swap(&mut from, &mut to);
        }
        let from_short = currency_names.get(&from);
        let to_short = currency_names.get(&to);
        let low_from = entry["lowest_ratio"][&from].as_f64();
        let (Some(from_short), true) = (from_short, general(&to)) else {
            continue;
        };
        if low_from == Some(0.0) {
            continue;
        }
        let new = E {
            currency: to_short.cloned().ok_or("toShort")?,
            price: entry["lowest_ratio"][&to].as_f64().ok_or("lowest_ratio")? / low_from.ok_or("lowest_ratio")?,
            stock: entry["highest_stock"][&from].as_f64().ok_or("highest_stock")?,
        };
        if league_out.get(from_short).is_none_or(|o| o.stock < new.stock) {
            league_out.insert(from_short.clone(), new);
        }
    }
    let mut res: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for (league, entries) in out {
        let mut done: BTreeMap<String, f64> = BTreeMap::new();
        for (from, to) in &entries {
            if to.currency == "divine" {
                done.insert(from.clone(), to.price);
            } else if let Some(div) = entries.get(&to.currency) {
                done.insert(from.clone(), div.price * to.price);
            }
        }
        if !done.is_empty() {
            done.insert("divine".to_string(), 1.0);
            res.insert(league, done);
        }
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_list() {
        assert!(is_same_as_default(&default_stat_list()));
        let mut l = default_stat_list();
        l[1].weight_mult = Some(0.25);
        assert!(!is_same_as_default(&l));
    }
}
