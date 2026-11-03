//! The panel of the display item (ItemsTab.lua:334-1213) and what it calls: `SetDisplayItem` (:2087-2171), `Update*Controls` (:2029-2084, :2195-2548), `CreateDisplayItemFromRaw` (:2001-2011), `CopyAnointsAndEldritchImplicits` (:1959-1998), `CraftItem` (:2679-2763), `EditDisplayItemText` (:2766-2818), `EnchantDisplayItem` (:2821-3030), `AnointDisplayItem` (:3138-3175), `CorruptDisplayItem` (:3178-3571), `AddCustomModifierToDisplayItem` (:3576-3927), `AddCrucibleModifierToDisplayItem` (:3930-4039).
//! `not_ported` (a note, no crash): `Buy similar` (U8), `AddImplicitPopup`, the `Sort by` lists of the popups other than `Default` (`calcFunc` per entry).

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_gfx::{Frame, Gfx};
use pob_items::craft::AffixMod;
use pob_items::item::{AffixRange, INFLUENCE_INFO, ModLine, Socket};
use pob_items::{Affix, Item};

use super::items::{ItemsTab, cc};
use crate::control::{Control, ControlRef, Rect, anchor, rect};
use crate::controls::basic::{ButtonControl, CheckBoxControl, LabelControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::scroll_bar::SliderControl;
use crate::host::InputEvents;
use crate::item_tooltip::format_mod_line;
use crate::modules::main::{BareControl, main, not_ported};
use crate::sg;
use crate::tooltip::Tooltip;

const MAX_MOD_COUNT: usize = 9;

/// An entry of an affix dropdown (`lastSeries`).
#[derive(Clone)]
struct AffixSeries {
    mod_list: Vec<String>,
    have_range: bool,
}

/// `drop.outputTable`, `drop.outputIndex`.
#[derive(Clone, Copy, Default)]
struct AffixOutput {
    suffix: bool,
    index: usize,
}

pub struct AffixRow {
    pub drop: Rc<DropDownControl>,
    pub slider: Rc<SliderControl>,
    output: Cell<AffixOutput>,
    slider_shown: Cell<bool>,
}

pub struct RangeRow {
    pub check: Rc<CheckBoxControl>,
    pub slider: Rc<SliderControl>,
    pub label: Rc<LabelControl>,
    line_count: Cell<f64>,
}

pub struct Panel {
    pub add: Rc<ButtonControl>,
    pub section_variant: Rc<BareControl>,
    pub version: Rc<DropDownControl>,
    pub variants: [Rc<DropDownControl>; 6],
    variant_state: [Cell<(bool, bool, f64)>; 6],
    pub sockets: Vec<Rc<DropDownControl>>,
    pub links: Vec<Rc<CheckBoxControl>>,
    pub influence: [Rc<DropDownControl>; 2],
    pub quality_edit: Rc<EditControl>,
    pub catalyst: Rc<DropDownControl>,
    pub catalyst_edit: Rc<EditControl>,
    pub cluster_skill: Rc<DropDownControl>,
    pub cluster_count: Rc<SliderControl>,
    pub sorting: Rc<DropDownControl>,
    pub sorting_label: Rc<LabelControl>,
    pub affixes: Vec<Rc<AffixRow>>,
    pub section_custom: Rc<BareControl>,
    pub add_custom: Rc<ButtonControl>,
    pub add_crucible: Rc<ButtonControl>,
    custom: RefCell<Vec<(Rc<ButtonControl>, Rc<LabelControl>, Rc<LabelControl>)>>,
    custom_count: Cell<f64>,
    pub positions_label: Rc<LabelControl>,
    positions: Rc<RefCell<Vec<(String, String)>>>,
    pub range_line: Rc<DropDownControl>,
    pub range_slider: Rc<SliderControl>,
    pub mutated: Rc<CheckBoxControl>,
    pub stacked: Vec<Rc<RangeRow>>,
    range_lines: RefCell<Vec<(u8, usize)>>,
    names: RefCell<IndexMap<String, ControlRef>>,
}

impl Panel {
    pub fn host_control(&self, name: &str) -> Option<ControlRef> {
        self.names.borrow().get(name).cloned()
    }

    /// `ItemsTab` panel controls (:334-1213).
    #[allow(clippy::too_many_lines)]
    pub fn new(g: &mut Gfx, tab: &Rc<ItemsTab>, ad: &Rc<BareControl>) -> Rc<Panel> {
        let names: RefCell<IndexMap<String, ControlRef>> = RefCell::new(IndexMap::new());
        let put = |name: &str, c: ControlRef| {
            tab.host.add(name, c.clone());
            names.borrow_mut().insert(name.to_string(), c);
        };
        let me = Rc::downgrade(tab);
        let t = {
            let me = me.clone();
            move || me.upgrade().expect("itemsTab")
        };
        let disp = {
            let t = t.clone();
            move || t().display_item.borrow().clone()
        };
        let adr: ControlRef = ad.clone();
        let t1 = t.clone();
        let add = ButtonControl::new(
            anchor("TOPLEFT", &adr, "TOPLEFT"),
            rect(0.0, 0.0, 100.0, 20.0),
            String::new(),
            move |g| {
                t1().add_display_item(g, false);
            },
        );
        let (t1, d1) = (t.clone(), disp.clone());
        add.label.set_fn(move |_| {
            let id = d1().and_then(|i| i.id);
            if id.is_some_and(|i| t1().read(|e| e.item_by_id(i).is_some())) {
                "Save".to_string()
            } else {
                "Add to build".to_string()
            }
        });
        let addr: ControlRef = add.clone();
        put("addDisplayItem", add.clone());
        let t1 = t.clone();
        let edit = ButtonControl::new(
            anchor("LEFT", &addr, "RIGHT"),
            rect(8.0, 0.0, 60.0, 20.0),
            "Edit...".to_string(),
            move |g| {
                t1().edit_display_item_text(g, false);
            },
        );
        let er: ControlRef = edit.clone();
        put("editDisplayItem", edit);
        let t1 = t.clone();
        let remove = ButtonControl::new(
            anchor("LEFT", &er, "RIGHT"),
            rect(8.0, 0.0, 60.0, 20.0),
            "Cancel".to_string(),
            move |g| {
                t1().set_display_item(g, None);
            },
        );
        let rr: ControlRef = remove.clone();
        put("removeDisplayItem", remove);
        let buy = ButtonControl::new(
            anchor("LEFT", &rr, "RIGHT"),
            rect(8.0, 0.0, 100.0, 20.0),
            "Buy similar".to_string(),
            |g| {
                not_ported(g, "Classes/ItemsTab.lua:356-357 buySimilar.openPopup (the network, U8)");
            },
        );
        let d1 = disp.clone();
        buy.base.shown.set_fn(move |_| d1().is_some());
        put("displayItemBuySimilar", buy);

        // :364-463 variants
        let section_variant = BareControl::new(anchor("TOPLEFT", &addr, "BOTTOMLEFT"), rect(0.0, 8.0, 0.0, 0.0));
        let svr: ControlRef = section_variant.clone();
        put("displayItemSectionVariant", section_variant.clone());
        let t1 = t.clone();
        let version = DropDownControl::new(
            g,
            anchor("TOPLEFT", &svr, "TOPLEFT"),
            rect(0.0, 0.0, 300.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                let s = t1();
                s.edit_display(|it| {
                    it.selected_version = Some(index as f64);
                    it.normalise_variant_selections();
                    it.build_and_parse_raw();
                });
                s.update_display_item_variant_controls(g);
                s.update_display_item_tooltip(g);
                s.update_display_item_range_lines(g);
            })),
            None,
            false,
        );
        version.max_dropped_width.set(1000.0);
        let d1 = disp.clone();
        version
            .base
            .shown
            .set_fn(move |_| d1().is_some_and(|i| i.uses_variant_groups && i.version_list.as_ref().is_some_and(|v| v.len() > 1)));
        put("displayItemVersion", version.clone());
        let mut made: Vec<Rc<DropDownControl>> = Vec::new();
        for i in 0..6 {
            let t1 = t.clone();
            let (a, r) = if i == 0 {
                (anchor("TOPLEFT", &svr, "TOPLEFT"), rect(0.0, 0.0, 300.0, 20.0))
            } else {
                let prev: ControlRef = made[i - 1].clone();
                (anchor("TOPLEFT", &prev, "BOTTOMLEFT"), rect(0.0, 4.0, 300.0, 20.0))
            };
            let dd = DropDownControl::new(
                g,
                a,
                r,
                Vec::new(),
                Some(Rc::new(move |g: &mut Gfx, index: usize, value: &DropItem| {
                    t1().select_variant(g, index, value, i)
                })),
                None,
                false,
            );
            dd.max_dropped_width.set(1000.0);
            let (d1, t1) = (disp.clone(), t.clone());
            dd.base.shown.set_fn(move |_| {
                let Some(it) = d1() else { return false };
                if it.uses_variant_groups {
                    t1().panel_ref().variant_state[i].get().0
                } else if i == 0 {
                    it.variant_list.as_ref().is_some_and(|l| l.len() > 1)
                } else {
                    it.has_alt_variant[i - 1]
                }
            });
            let (d1, t1) = (disp.clone(), t.clone());
            dd.base
                .enabled
                .set_fn(move |_| d1().is_none_or(|it| !it.uses_variant_groups) || t1().panel_ref().variant_state[i].get().1);
            let name = if i == 0 {
                "displayItemVariant".to_string()
            } else if i == 1 {
                "displayItemAltVariant".to_string()
            } else {
                format!("displayItemAltVariant{i}")
            };
            put(&name, dd.clone());
            made.push(dd);
        }
        let variants: [Rc<DropDownControl>; 6] = made.try_into().unwrap_or_else(|_| unreachable!());
        let vw = Rc::downgrade(&version);
        variants[0]
            .base
            .y
            .set_fn(move |g| if vw.upgrade().expect("version").is_shown(g) { 24.0 } else { 0.0 });
        let (d1, v0, vr) = (disp.clone(), Rc::downgrade(&variants[0]), Rc::downgrade(&version));
        section_variant.base.height.set_fn(move |g| {
            let Some(it) = d1() else { return 0.0 };
            if it.uses_variant_groups {
                let mut rows = if it.version_list.as_ref().is_some_and(|v| v.len() > 1) {
                    1
                } else {
                    0
                };
                let mut groups = 0;
                for gid in it.variant_groups.keys() {
                    if groups < 6 && !it.get_variant_group_options(gid.0, false).is_empty() {
                        rows += 1;
                        groups += 1;
                    }
                }
                let _ = &vr;
                return if rows > 0 { f64::from(rows) * 24.0 + 4.0 } else { 0.0 };
            }
            if !v0.upgrade().expect("variant").is_shown(g) {
                return 0.0;
            }
            28.0 + it.has_alt_variant.iter().filter(|x| **x).count() as f64 * 24.0
        });

        // :466-514 sockets
        let d1 = disp.clone();
        let section_sockets = BareControl::new(
            anchor("TOPLEFT", &svr, "BOTTOMLEFT"),
            [0.0.into(), 0.0.into(), 0.0.into(), 0.0.into()],
        );
        section_sockets.base.height.set_fn(move |_| {
            if d1().is_some_and(|i| i.selectable_socket_count.unwrap_or(0.0) > 0.0) {
                28.0
            } else {
                0.0
            }
        });
        let ssr: ControlRef = section_sockets.clone();
        put("displayItemSectionSockets", section_sockets);
        let socket_list = || -> Vec<DropItem> {
            [("STRENGTH", "R"), ("DEXTERITY", "G"), ("INTELLIGENCE", "B"), ("SCION", "W")]
                .iter()
                .map(|(c, l)| {
                    let mut it = DropItem::table(&format!("{}{l}", cc(c)));
                    it.data = Some(Rc::new((*l).to_string()));
                    it
                })
                .collect()
        };
        let (mut sockets, mut links) = (Vec::new(), Vec::new());
        for i in 1..=6usize {
            let t1 = t.clone();
            let drop = DropDownControl::new(
                g,
                anchor("TOPLEFT", &ssr, "TOPLEFT"),
                rect((i - 1) as f64 * 64.0, 0.0, 36.0, 20.0),
                socket_list(),
                Some(Rc::new(move |g: &mut Gfx, _index: usize, value: &DropItem| {
                    let color = value
                        .data
                        .as_ref()
                        .and_then(|d| d.downcast_ref::<String>())
                        .cloned()
                        .unwrap_or_default();
                    let s = t1();
                    s.edit_display(|it| {
                        it.sockets[i - 1].color = color;
                        it.build_and_parse_raw();
                    });
                    s.update_display_item_tooltip(g);
                })),
                None,
                false,
            );
            let d1 = disp.clone();
            drop.base.shown.set_fn(move |_| {
                d1().is_some_and(|it| {
                    it.selectable_socket_count.unwrap_or(0.0) >= i as f64 && it.sockets.get(i - 1).is_some_and(|s| s.color != "A")
                })
            });
            put(&format!("displayItemSocket{i}"), drop.clone());
            if i < 6 {
                let dr: ControlRef = drop.clone();
                let t1 = t.clone();
                let link = CheckBoxControl::new(
                    g,
                    anchor("LEFT", &dr, "RIGHT"),
                    [4.0, 0.0, 20.0],
                    None,
                    Some(Rc::new(move |g: &mut Gfx, state: bool| {
                        let s = t1();
                        s.edit_display(|it| {
                            let same = it.sockets[i - 1].group == it.sockets[i].group;
                            let delta = if state && !same {
                                -1.0
                            } else if !state && same {
                                1.0
                            } else {
                                0.0
                            };
                            for x in &mut it.sockets[i..] {
                                x.group += delta;
                            }
                            it.build_and_parse_raw();
                        });
                        s.update_display_item_tooltip(g);
                    })),
                    None,
                    false,
                );
                let d1 = disp.clone();
                link.base.shown.set_fn(move |_| {
                    d1().is_some_and(|it| {
                        it.selectable_socket_count.unwrap_or(0.0) > i as f64 && it.sockets.get(i).is_some_and(|s| s.color != "A")
                    })
                });
                put(&format!("displayItemLink{i}"), link.clone());
                links.push(link);
            }
            sockets.push(drop);
        }
        let t1 = t.clone();
        let add_socket = ButtonControl::new(
            anchor("TOPLEFT", &ssr, "TOPLEFT"),
            rect(0.0, 0.0, 20.0, 20.0),
            "+".to_string(),
            move |g| {
                let s = t1();
                s.edit_display(|it| {
                    let at = it.sockets.len() - it.abyssal_socket_count as usize;
                    let group = it.sockets[at - 1].group + 1.0;
                    it.sockets.insert(
                        at,
                        Socket {
                            color: it.default_socket_color.clone().unwrap_or_default(),
                            group,
                        },
                    );
                    for x in &mut it.sockets[at + 1..] {
                        x.group += 1.0;
                    }
                    it.build_and_parse_raw();
                });
                s.update_socket_controls();
                s.update_display_item_tooltip(g);
            },
        );
        let d1 = disp.clone();
        add_socket
            .base
            .x
            .set_fn(move |_| d1().map_or(0.0, |it| (it.sockets.len() as f64 - it.abyssal_socket_count) * 64.0 - 12.0));
        let d1 = disp.clone();
        add_socket.base.shown.set_fn(move |_| {
            d1().is_some_and(|it| (it.sockets.len() as f64) < it.selectable_socket_count.unwrap_or(0.0) + it.abyssal_socket_count)
        });
        put("displayItemAddSocket", add_socket);

        // :517-579 enchant / anoint / corrupt
        let section_enchant = BareControl::new(anchor("TOPLEFT", &ssr, "BOTTOMLEFT"), rect(0.0, 0.0, 0.0, 0.0));
        let ser: ControlRef = section_enchant.clone();
        put("displayItemSectionEnchant", section_enchant.clone());
        let enchantable = |it: &Item| enchantments_of(it).is_some();
        let anointable = |it: &Item| {
            it.base.is_some_and(|b| {
                b.cannot_be_anointed != Some(true) && b.sub_type.as_deref() != Some("Talisman") && (it.can_be_anointed || b.ty == "Amulet")
            })
        };
        let t1 = t.clone();
        let enchant = ButtonControl::new(
            anchor("TOPLEFT", &ser, "TOPLEFT"),
            rect(0.0, 0.0, 160.0, 20.0),
            "Change Enchantment...".to_string(),
            move |g| {
                t1().enchant_display_item(g, 1);
            },
        );
        let d1 = disp.clone();
        enchant.base.shown.set_fn(move |_| d1().is_some_and(|i| enchantable(&i)));
        let enr: ControlRef = enchant.clone();
        put("displayItemEnchant", enchant.clone());
        let chain = |name: &str,
                     label: &str,
                     prev: &ControlRef,
                     w: f64,
                     y: f64,
                     f: Rc<dyn Fn(&mut Gfx)>,
                     shown: Rc<dyn Fn(&Item) -> bool>|
         -> Rc<ButtonControl> {
            let a = crate::control::anchor_collapse("TOPLEFT", prev, "TOPRIGHT");
            let b = ButtonControl::new(a, rect(8.0, y, w, 20.0), label.to_string(), move |g| f(g));
            let d1 = disp.clone();
            b.base.shown.set_fn(move |_| d1().is_some_and(|i| shown(&i)));
            put(name, b.clone());
            b
        };
        let t1 = t.clone();
        let enchant2 = chain(
            "displayItemEnchant2",
            "Change Enchantment 2...",
            &enr,
            160.0,
            0.0,
            Rc::new(move |g| t1().enchant_display_item(g, 2)),
            Rc::new(move |i| enchantable(i) && i.can_have_two_enchants && !i.enchant_mod_lines.is_empty()),
        );
        let mut prev: ControlRef = enchant2.clone();
        let mut anoints: Vec<Rc<ButtonControl>> = Vec::new();
        for n in 1..=4usize {
            let t1 = t.clone();
            let b = chain(
                &if n == 1 {
                    "displayItemAnoint".to_string()
                } else {
                    format!("displayItemAnoint{n}")
                },
                &if n == 1 {
                    "Anoint...".to_string()
                } else {
                    format!("Anoint {n}...")
                },
                &prev,
                100.0,
                0.0,
                Rc::new(move |g| t1().anoint_display_item(g, n)),
                Rc::new(move |i| {
                    anointable(i)
                        && match n {
                            1 => true,
                            2 => i.can_have_two_enchants && !i.enchant_mod_lines.is_empty(),
                            3 => i.can_have_three_enchants && i.enchant_mod_lines.len() > 1,
                            _ => i.can_have_four_enchants && i.enchant_mod_lines.len() > 2,
                        }
                }),
            );
            prev = b.clone();
            anoints.push(b);
        }
        let t1 = t.clone();
        let corrupt = chain(
            "displayItemCorrupt",
            "Corrupt...",
            &prev,
            100.0,
            10.0,
            Rc::new(move |g| t1().corrupt_display_item(g)),
            Rc::new(|i| i.corruptible),
        );
        let cr: ControlRef = corrupt.clone();
        chain(
            "displayItemAddImplicit",
            "Add Implicit...",
            &cr,
            120.0,
            0.0,
            Rc::new(|g| {
                not_ported(
                    g,
                    "Classes/ItemsTab.lua:572 addImplicit.AddImplicitToDisplayItem (Modules/AddImplicitPopup.lua)",
                )
            }),
            Rc::new(|i| {
                let ty = i.ty.as_deref().unwrap_or("");
                ty != "Tincture"
                    && ty != "Graft"
                    && (i.corruptible || (ty != "Flask" && ty != "Jewel" && matches!(i.rarity.as_str(), "NORMAL" | "MAGIC" | "RARE")))
                    && !i.implicits_cannot_be_changed
            }),
        );
        {
            let (e, a1, a2, c) = (enchant.clone(), anoints[0].clone(), anoints[1].clone(), corrupt.clone());
            let e2 = enchant2.clone();
            section_enchant.base.height.set_fn(move |g| {
                if e.is_shown(g) || e2.is_shown(g) || a1.is_shown(g) || a2.is_shown(g) || c.is_shown(g) {
                    28.0
                } else {
                    0.0
                }
            });
        }

        // :582-629 influence
        let d1 = disp.clone();
        let section_influence = BareControl::new(anchor("TOPLEFT", &ser, "BOTTOMLEFT"), rect(0.0, 0.0, 0.0, 0.0));
        section_influence
            .base
            .height
            .set_fn(move |_| if d1().is_some_and(|i| i.can_be_influenced) { 28.0 } else { 0.0 });
        let sir: ControlRef = section_influence.clone();
        put("displayItemSectionInfluence", section_influence);
        let tip_text = crate::common::wrap_string(
            g,
            "Selecting an influence here will also allow the modifier dropdowns to contain influenced mods.",
            16.0,
            140.0,
        )
        .join("\n");
        let influence_list = || -> Vec<DropItem> {
            let mut v = vec![DropItem::str("Influence")];
            v.extend(INFLUENCE_INFO.iter().map(|(_, d)| DropItem::str(d)));
            v
        };
        let t1 = t.clone();
        let inf1 = DropDownControl::new(
            g,
            anchor("TOPLEFT", &sir, "TOPRIGHT"),
            rect(0.0, 0.0, 100.0, 20.0),
            influence_list(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                let s = t1();
                let other = s.panel_ref().influence[1].sel_index.get();
                s.set_display_item_influence(g, [index - 1, other - 1]);
            })),
            Some(&tip_text),
            false,
        );
        let d1 = disp.clone();
        inf1.base.shown.set_fn(move |_| d1().is_some_and(|i| i.can_be_influenced));
        let inr: ControlRef = inf1.clone();
        put("displayItemInfluence", inf1.clone());
        let t1 = t.clone();
        let inf2 = DropDownControl::new(
            g,
            crate::control::anchor_collapse("TOPLEFT", &inr, "TOPRIGHT"),
            rect(8.0, 0.0, 100.0, 20.0),
            influence_list(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                let s = t1();
                let other = s.panel_ref().influence[0].sel_index.get();
                s.set_display_item_influence(g, [index - 1, other - 1]);
            })),
            Some(&tip_text),
            false,
        );
        let d1 = disp.clone();
        inf2.base.shown.set_fn(move |_| d1().is_some_and(|i| i.can_be_influenced));
        put("displayItemInfluence2", inf2.clone());

        // :632-647 quality
        let section_quality = BareControl::new(anchor("TOPLEFT", &sir, "BOTTOMLEFT"), rect(0.0, 0.0, 0.0, 0.0));
        let sqr: ControlRef = section_quality.clone();
        put("displayItemSectionQuality", section_quality.clone());
        let quality = LabelControl::new(
            anchor("TOPLEFT", &sqr, "TOPRIGHT"),
            rect(0.0, 0.0, 0.0, 16.0),
            "^7Quality:".to_string(),
        );
        let d1 = disp.clone();
        quality.base.shown.set_fn(move |_| d1().is_some_and(|i| i.quality.is_some()));
        let qr: ControlRef = quality.clone();
        put("displayItemQuality", quality);
        let t1 = t.clone();
        let quality_edit = EditControl::new(
            g,
            anchor("LEFT", &qr, "RIGHT"),
            rect(2.0, 0.0, 60.0, 20.0),
            EditArgs {
                filter: Some("%D".to_string()),
                limit: Some(2),
                change_func: Some(Rc::new(move |g: &mut Gfx, buf: &str, _: bool| {
                    let s = t1();
                    s.edit_display(|it| {
                        it.quality = pob_lua::tonumber(buf);
                        it.build_and_parse_raw();
                    });
                    s.update_display_item_tooltip(g);
                })),
                ..EditArgs::default()
            },
        );
        let d1 = disp.clone();
        quality_edit.base.shown.set_fn(move |_| d1().is_some_and(|i| i.quality.is_some()));
        put("displayItemQualityEdit", quality_edit.clone());
        {
            let (d1, q) = (disp.clone(), Rc::downgrade(&quality_edit));
            section_quality.base.height.set_fn(move |g| {
                if d1().is_some_and(|i| i.quality.is_some()) && q.upgrade().expect("edit").is_shown(g) {
                    28.0
                } else {
                    0.0
                }
            });
        }

        // :658-702 catalysts
        let section_catalyst = BareControl::new(anchor("TOPLEFT", &sqr, "BOTTOMLEFT"), rect(0.0, 0.0, 0.0, 0.0));
        let scr: ControlRef = section_catalyst.clone();
        put("displayItemSectionCatalyst", section_catalyst.clone());
        let t1 = t.clone();
        let catalyst = DropDownControl::new(
            g,
            anchor("TOPLEFT", &scr, "TOPRIGHT"),
            rect(0.0, 0.0, 250.0, 20.0),
            [
                "Catalyst",
                "Abrasive (Attack)",
                "Accelerating (Speed)",
                "Dextral (Suffix)",
                "Fertile (Life & Mana)",
                "Imbued (Caster)",
                "Intrinsic (Attribute)",
                "Noxious (Physical & Chaos Damage)",
                "Prismatic (Resistance)",
                "Sinistral (Prefix)",
                "Tempering (Defense)",
                "Turbulent (Elemental)",
                "Unstable (Critical)",
            ]
            .iter()
            .map(|s| DropItem::str(s))
            .collect(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                let s = t1();
                let p = s.panel_ref();
                let mut set_text = None;
                s.edit_display(|it| {
                    it.catalyst = Some((index - 1) as f64);
                    if index > 1 {
                        if it.catalyst_quality.is_none() {
                            it.catalyst_quality = Some(20.0);
                            set_text = Some("20");
                        }
                    } else {
                        it.catalyst_quality = None;
                        set_text = Some("0");
                    }
                });
                if let Some(x) = set_text {
                    p.catalyst_edit.set_text(g, x, false);
                }
                s.force_affix_selectors(g);
                s.edit_display(Item::build_and_parse_raw);
                s.update_display_item_tooltip(g);
            })),
            None,
            false,
        );
        let d1 = disp.clone();
        catalyst.base.shown.set_fn(move |_| {
            d1().is_some_and(|i| {
                (i.crafted || i.has_mod_tags) && i.base.is_some_and(|b| matches!(b.ty.as_str(), "Amulet" | "Ring" | "Belt"))
            })
        });
        let car: ControlRef = catalyst.clone();
        put("displayItemCatalyst", catalyst.clone());
        let t1 = t.clone();
        let catalyst_edit = EditControl::new(
            g,
            anchor("LEFT", &car, "RIGHT"),
            rect(2.0, 0.0, 60.0, 20.0),
            EditArgs {
                filter: Some("%D".to_string()),
                limit: Some(2),
                change_func: Some(Rc::new(move |g: &mut Gfx, buf: &str, _: bool| {
                    let s = t1();
                    s.edit_display(|it| it.catalyst_quality = pob_lua::tonumber(buf));
                    s.force_affix_selectors(g);
                    s.edit_display(Item::build_and_parse_raw);
                    s.update_display_item_tooltip(g);
                })),
                ..EditArgs::default()
            },
        );
        let d1 = disp.clone();
        catalyst_edit
            .base
            .shown
            .set_fn(move |_| d1().is_some_and(|i| (i.crafted || i.has_mod_tags) && i.catalyst.is_some_and(|c| c > 0.0)));
        put("displayItemCatalystQualityEdit", catalyst_edit.clone());
        {
            let (c, e) = (catalyst.clone(), catalyst_edit.clone());
            section_catalyst
                .base
                .height
                .set_fn(move |g| if c.is_shown(g) || e.is_shown(g) { 28.0 } else { 0.0 });
        }

        // :705-733 cluster jewels and the sorting of the affixes
        let section_cluster = BareControl::new(anchor("TOPLEFT", &scr, "BOTTOMLEFT"), rect(0.0, 0.0, 0.0, 0.0));
        let sclr: ControlRef = section_cluster.clone();
        put("displayItemSectionClusterJewel", section_cluster.clone());
        let t1 = t.clone();
        let cluster_skill = DropDownControl::new(
            g,
            anchor("TOPLEFT", &sclr, "TOPLEFT"),
            rect(0.0, 0.0, 300.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, _i: usize, value: &DropItem| {
                let id = value.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned();
                let s = t1();
                s.edit_display(|it| it.cluster_jewel_skill = id);
                s.craft_cluster_jewel(g);
            })),
            None,
            false,
        );
        let d1 = disp.clone();
        cluster_skill
            .base
            .shown
            .set_fn(move |_| d1().is_some_and(|i| i.crafted && i.cluster_jewel.is_some()));
        let cskr: ControlRef = cluster_skill.clone();
        put("displayItemClusterJewelSkill", cluster_skill.clone());
        let cw = Rc::downgrade(&cluster_skill);
        section_cluster
            .base
            .height
            .set_fn(move |g| if cw.upgrade().expect("skill").is_shown(g) { 52.0 } else { 0.0 });
        let count_label = LabelControl::new(
            anchor("TOPLEFT", &cskr, "BOTTOMLEFT"),
            rect(0.0, 7.0, 0.0, 14.0),
            "^7Added Passives:".to_string(),
        );
        let clr: ControlRef = count_label.clone();
        put("displayItemClusterJewelNodeCountLabel", count_label);
        let t1 = t.clone();
        let cluster_count = SliderControl::new(
            anchor("LEFT", &clr, "RIGHT"),
            rect(2.0, 0.0, 150.0, 20.0),
            Some(Rc::new(move |g: &mut Gfx, val: f64| {
                let s = t1();
                s.edit_display(|it| {
                    let c = it.cluster_jewel.expect("clusterJewel");
                    it.cluster_jewel_node_count = Some(pob_lua::number::round(val * (c.max_nodes - c.min_nodes) + c.min_nodes, None));
                });
                s.craft_cluster_jewel(g);
            })),
            None,
        );
        put("displayItemClusterJewelNodeCount", cluster_count.clone());
        let sorting_label = LabelControl::new(
            anchor("TOPLEFT", &sclr, "BOTTOMLEFT"),
            rect(0.0, 0.0, 0.0, 16.0),
            "^7Modifier sorting:".to_string(),
        );
        let d1 = disp.clone();
        let sorting_shown = move || {
            d1().is_some_and(|i| i.crafted && !(i.base.is_some_and(|b| b.ty == "Jewel" && b.sub_type.as_deref() == Some("Cluster"))))
        };
        let ss1 = sorting_shown.clone();
        sorting_label.base.shown.set_fn(move |_| ss1());
        let slr: ControlRef = sorting_label.clone();
        put("craftingSortingLabel", sorting_label.clone());
        let t1 = t.clone();
        let mut sort_opts = vec![DropItem::table("Default")];
        for o in pob_data::rules::rules().power_stat_list.as_array().into_iter().flatten() {
            let label = o.get("label").and_then(serde_json::Value::as_str).unwrap_or("");
            if !o.get("ignoreForItems").and_then(serde_json::Value::as_bool).unwrap_or(false) && label != "Name" {
                sort_opts.push(DropItem::table(label));
            }
        }
        let sorting = DropDownControl::new(
            g,
            anchor("LEFT", &slr, "RIGHT"),
            rect(4.0, 0.0, 200.0, 20.0),
            sort_opts,
            Some(Rc::new(move |g: &mut Gfx, _i: usize, _v: &DropItem| t1().update_affix_controls(g))),
            None,
            false,
        );
        put("craftingSorting", sorting.clone());

        // :736-989 the affixes
        let section_affix = BareControl::new(
            crate::control::anchor_collapse("TOPLEFT", &slr, "BOTTOMLEFT"),
            rect(0.0, 0.0, 0.0, 0.0),
        );
        let ss2 = sorting_shown.clone();
        section_affix.base.y.set_fn(move |_| if ss2() { 8.0 } else { -16.0 });
        let d1 = disp.clone();
        section_affix.base.shown.set_fn(move |_| d1().is_some_and(|i| i.crafted));
        let sar: ControlRef = section_affix.clone();
        put("displayItemSectionAffix", section_affix.clone());
        let mut affixes: Vec<Rc<AffixRow>> = Vec::new();
        for i in 1..=MAX_MOD_COUNT {
            let prev: ControlRef = affixes.last().map_or_else(|| sar.clone(), |r| r.drop.clone() as ControlRef);
            let t1 = t.clone();
            let drop = DropDownControl::new(
                g,
                anchor("TOPLEFT", &prev, "TOPLEFT"),
                rect(if i == 1 { 40.0 } else { 0.0 }, 0.0, 418.0, 20.0),
                Vec::new(),
                Some(Rc::new(move |g: &mut Gfx, index: usize, value: &DropItem| {
                    t1().on_affix_select(g, i - 1, index, value);
                })),
                None,
                false,
            );
            let d1 = disp.clone();
            drop.base
                .shown
                .set_fn(move |_| d1().is_some_and(|it| it.crafted && i as f64 <= it.affix_limit));
            let dr: ControlRef = drop.clone();
            let t1 = t.clone();
            let slider = SliderControl::new(
                anchor("TOPLEFT", &dr, "BOTTOMLEFT"),
                rect(0.0, 2.0, 300.0, 16.0),
                Some(Rc::new(move |g: &mut Gfx, _val: f64| t1().on_affix_slider(g, i - 1))),
                None,
            );
            let sw = Rc::downgrade(&slider);
            slider.base.width.set_fn(move |_| {
                if sw.upgrade().expect("slider").div_count.get().is_some() {
                    300.0
                } else {
                    100.0
                }
            });
            let t1 = t.clone();
            slider
                .base
                .shown
                .set_fn(move |_| t1().panel_ref().affixes[i - 1].slider_shown.get());
            if let Some(p) = affixes.last() {
                let ps = p.slider.clone();
                drop.base.y.set_fn(move |g| 24.0 + if ps.is_shown(g) { 18.0 } else { 0.0 });
            }
            put(&format!("displayItemAffix{i}"), drop.clone());
            put(&format!("displayItemAffixRange{i}"), slider.clone());
            let t1 = t.clone();
            put(
                &format!("displayItemAffixLabel{i}"),
                LabelControl::new(
                    anchor("RIGHT", &dr, "LEFT"),
                    rect(-4.0, 0.0, 0.0, 14.0),
                    crate::control::Prop::func(move |_| {
                        let s = t1();
                        let d = s.display_item.borrow().clone();
                        if d.is_some_and(|x| x.ignore_mod_type()) {
                            "^7Explicit:".to_string()
                        } else if s.panel_ref().affixes[i - 1].output.get().suffix {
                            "^7Suffix:".to_string()
                        } else {
                            "^7Prefix:".to_string()
                        }
                    }),
                ),
            );
            let sr: ControlRef = slider.clone();
            let w = Rc::downgrade(&drop);
            put(
                &format!("displayItemAffixRangeLabel{i}"),
                LabelControl::new(
                    anchor("RIGHT", &sr, "LEFT"),
                    rect(-4.0, 0.0, 0.0, 14.0),
                    crate::control::Prop::func(move |_| {
                        if w.upgrade().expect("drop").sel_index.get() > 1 {
                            "^7Roll:".to_string()
                        } else {
                            "^x7F7F7FRoll:".to_string()
                        }
                    }),
                ),
            );
            affixes.push(Rc::new(AffixRow {
                drop,
                slider,
                output: Cell::new(AffixOutput::default()),
                slider_shown: Cell::new(false),
            }));
        }
        {
            let (rows, d1) = (affixes.clone(), disp.clone());
            section_affix.base.height.set_fn(move |g| {
                if !d1().is_some_and(|i| i.crafted) {
                    return 0.0;
                }
                let mut h = 6.0;
                for r in &rows {
                    if r.drop.is_shown(g) {
                        h += 24.0;
                        if r.slider.is_shown(g) {
                            h += 18.0;
                        }
                    }
                }
                h
            });
        }

        // :993-1015 custom and crucible modifiers
        let section_custom = BareControl::new(
            crate::control::anchor_collapse("TOPLEFT", &sar, "BOTTOMLEFT"),
            rect(0.0, 0.0, 0.0, 0.0),
        );
        let d1 = disp.clone();
        section_custom.base.shown.set_fn(move |_| d1().is_some());
        let scur: ControlRef = section_custom.clone();
        put("displayItemSectionCustom", section_custom.clone());
        let t1 = t.clone();
        let add_custom = ButtonControl::new(
            anchor("TOPLEFT", &scur, "TOPLEFT"),
            rect(0.0, 0.0, 120.0, 20.0),
            "Add modifier...".to_string(),
            move |g| {
                t1().add_custom_modifier(g);
            },
        );
        let d1 = disp.clone();
        add_custom.base.shown.set_fn(move |_| {
            d1().is_some_and(|i| {
                i.rarity == "MAGIC" || i.rarity == "RARE" || i.rare_like_unique.is_some_and(|r| r.supports_custom_modifiers.is_some())
            })
        });
        put("displayItemAddCustom", add_custom.clone());
        let t1 = t.clone();
        let add_crucible = ButtonControl::new(
            anchor("TOPLEFT", &scur, "TOPLEFT"),
            rect(0.0, 0.0, 150.0, 20.0),
            "Add Crucible mod...".to_string(),
            move |g| {
                t1().add_crucible_modifier(g);
            },
        );
        let ac = add_custom.clone();
        add_crucible.base.x.set_fn(move |g| if ac.is_shown(g) { 128.0 } else { 0.0 });
        let d1 = disp.clone();
        add_crucible.base.shown.set_fn(move |_| {
            d1().is_some_and(|i| i.get_primary_slot() == "Weapon 1" || i.ty.as_deref() == Some("Shield") || i.can_have_shield_crucible_tree)
        });
        put("displayItemAddCrucible", add_crucible.clone());
        {
            let (a, c, t1) = (add_custom.clone(), add_crucible.clone(), t.clone());
            section_custom.base.height.set_fn(move |g| {
                if a.is_shown(g) || c.is_shown(g) {
                    28.0 + t1().panel_ref().custom_count.get() * 22.0
                } else {
                    0.0
                }
            });
        }

        // :1018-1042 notable positions
        let positions: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
        let section_pos = BareControl::new(anchor("TOPLEFT", &scur, "BOTTOMLEFT"), rect(0.0, 0.0, 0.0, 0.0));
        let spr: ControlRef = section_pos.clone();
        put("displayItemSectionClusterJewelPositions", section_pos.clone());
        let positions_label = LabelControl::new(
            anchor("TOPLEFT", &spr, "TOPLEFT"),
            rect(0.0, 0.0, 0.0, 14.0),
            "^7Notable positions (relative to entrance):".to_string(),
        );
        let (d1, p1) = (disp.clone(), positions.clone());
        positions_label.base.shown.set_fn(move |_| {
            d1().is_some_and(|i| {
                i.cluster_jewel
                    .is_some_and(|c| (c.size == "Large" && i.crafted) || i.title.as_deref() == Some("Megalomaniac"))
                    && !p1.borrow().is_empty()
            })
        });
        let plr: ControlRef = positions_label.clone();
        put("displayItemClusterJewelPositions", positions_label.clone());
        for i in 1..=4usize {
            let p1 = positions.clone();
            let l = LabelControl::new(
                anchor("TOPLEFT", &plr, "TOPLEFT"),
                rect(0.0, i as f64 * 18.0, 0.0, 14.0),
                crate::control::Prop::func(move |_| {
                    p1.borrow().get(i - 1).map_or_else(String::new, |(pos, name)| {
                        let color = if pos == "Front" { cc("POSITIVE") } else { cc("NORMAL") };
                        format!("{color}{pos}: ^7{name}")
                    })
                }),
            );
            let (p1, pl) = (positions.clone(), positions_label.clone());
            l.base.shown.set_fn(move |g| pl.is_shown(g) && p1.borrow().len() >= i);
            put(&format!("displayItemClusterJewelPosition{i}"), l);
        }
        {
            let (pl, p1) = (positions_label.clone(), positions.clone());
            section_pos.base.height.set_fn(move |g| {
                if pl.is_shown(g) {
                    24.0 + p1.borrow().len() as f64 * 18.0
                } else {
                    0.0
                }
            });
        }

        // :1045-1211 modifier range
        let section_range = BareControl::new(anchor("TOPLEFT", &spr, "BOTTOMLEFT"), rect(0.0, 0.0, 0.0, 0.0));
        let srr: ControlRef = section_range.clone();
        put("displayItemSectionRange", section_range.clone());
        let t1 = t.clone();
        let range_line = DropDownControl::new(
            g,
            anchor("TOPLEFT", &srr, "TOPLEFT"),
            rect(0.0, 0.0, 350.0, 18.0),
            Vec::new(),
            Some(Rc::new(move |_g: &mut Gfx, index: usize, _v: &DropItem| {
                let s = t1();
                let p = s.panel_ref();
                let id = p.range_lines.borrow().get(index - 1).copied();
                if let (Some(id), Some(it)) = (id, s.display_item.borrow().as_ref()) {
                    p.range_slider.val.set(mod_line(it, id).and_then(|m| m.range).unwrap_or(0.0));
                }
            })),
            None,
            false,
        );
        let show_all = || main().s.borrow().show_all_item_affixes;
        let (d1, t1) = (disp.clone(), t.clone());
        range_line.base.shown.set_fn(move |_| {
            d1().is_some_and(|i| !(show_all() && i.rarity == "UNIQUE") && !t1().panel_ref().range_lines.borrow().is_empty())
        });
        let rlr: ControlRef = range_line.clone();
        put("displayItemRangeLine", range_line.clone());
        let t1 = t.clone();
        let mutated = CheckBoxControl::new(
            g,
            crate::control::anchor_collapse("LEFT", &rlr, "RIGHT"),
            [0.0, 0.0, 18.0],
            None,
            Some(Rc::new(move |g: &mut Gfx, _v: bool| {
                let s = t1();
                let p = s.panel_ref();
                let id = p.range_lines.borrow().get(p.range_line.sel_index.get() - 1).copied();
                s.toggle_mutation(g, id);
                s.update_display_item_range_lines(g);
            })),
            None,
            false,
        );
        let t1 = t.clone();
        mutated.base.shown.set_fn(move |_| {
            let s = t1();
            let p = s.panel_ref();
            let id = p.range_lines.borrow().get(p.range_line.sel_index.get().wrapping_sub(1)).copied();
            let line = id.and_then(|i| s.display_item.borrow().as_ref().and_then(|it| mod_line(it, i).cloned()));
            p.mutated.state.set(line.as_ref().is_some_and(|l| l.flag("mutated")));
            !show_all() && line.is_some_and(|l| l.mod_id.is_some() && l.new_mod_id.is_some())
        });
        // :1051-1056
        let icon = tab.foulborn_icon.clone();
        *mutated.before_draw.borrow_mut() = Some(Rc::new(move |f, x, y| {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            sg::draw_image(f, Some(&icon), x - 24.0, y - 1.0, 20.0, 20.0, None);
        }));
        let mw = Rc::downgrade(&mutated);
        mutated
            .base
            .x
            .set_fn(move |g| if mw.upgrade().expect("box").is_shown(g) { 25.0 } else { 8.0 });
        let mr: ControlRef = mutated.clone();
        put("displayItemMutatedCheckbox", mutated.clone());
        let t1 = t.clone();
        let range_slider = SliderControl::new(
            crate::control::anchor_collapse("LEFT", &mr, "RIGHT"),
            rect(4.0, 0.0, 100.0, 18.0),
            Some(Rc::new(move |g: &mut Gfx, val: f64| {
                let s = t1();
                let p = s.panel_ref();
                let id = p.range_lines.borrow().get(p.range_line.sel_index.get().wrapping_sub(1)).copied();
                if let Some(id) = id {
                    s.edit_display(|it| {
                        if let Some(m) = it.mod_line_by_id(id) {
                            m.range = Some(val);
                        }
                        it.build_and_parse_raw();
                    });
                }
                s.update_display_item_tooltip(g);
                s.update_custom_controls(g);
            })),
            None,
        );
        let t1 = t.clone();
        range_slider.base.shown.set_fn(move |_| {
            let s = t1();
            let p = s.panel_ref();
            let d = s.display_item.borrow();
            let Some(it) = d.as_ref().filter(|i| !(show_all() && i.rarity == "UNIQUE")) else {
                return false;
            };
            let id = p.range_lines.borrow().get(p.range_line.sel_index.get().wrapping_sub(1)).copied();
            let Some(m) = id.and_then(|i| mod_line(it, i)) else { return false };
            if let Some(r) = m.range {
                p.range_slider.val.set(r);
            }
            m.show_slider == Some(true)
        });
        put("displayItemRangeSlider", range_slider.clone());
        let mut stacked: Vec<Rc<RangeRow>> = Vec::new();
        for i in 1..=20usize {
            let t1 = t.clone();
            let check = CheckBoxControl::new(
                g,
                crate::control::anchor_collapse("TOPLEFT", &srr, "TOPLEFT"),
                [0.0, 0.0, 18.0],
                None,
                Some(Rc::new(move |g: &mut Gfx, _v: bool| {
                    let s = t1();
                    let id = s.panel_ref().range_lines.borrow().get(i - 1).copied();
                    s.toggle_mutation(g, id);
                })),
                None,
                false,
            );
            // :1134-1139
            let icon = tab.foulborn_icon.clone();
            *check.before_draw.borrow_mut() = Some(Rc::new(move |f, x, y| {
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
                sg::draw_image(f, Some(&icon), x - 24.0, y, 20.0, 20.0, None);
            }));
            let cw = Rc::downgrade(&check);
            check
                .base
                .x
                .set_fn(move |g| if cw.upgrade().expect("box").is_shown(g) { 24.0 } else { 0.0 });
            let pv = stacked.last().cloned();
            check.base.y.set_fn(move |g| match &pv {
                None => 2.0,
                Some(p) => p.check.base.y.get(g) + f64::max(22.0, p.line_count.get() * 14.0),
            });
            let t1 = t.clone();
            check.base.shown.set_fn(move |_| {
                let s = t1();
                let p = s.panel_ref();
                let id = p.range_lines.borrow().get(i - 1).copied();
                let line = id.and_then(|x| s.display_item.borrow().as_ref().and_then(|it| mod_line(it, x).cloned()));
                p.stacked[i - 1].check.state.set(line.as_ref().is_some_and(|l| l.flag("mutated")));
                show_all() && line.is_some_and(|l| l.mod_id.is_some() && l.new_mod_id.is_some())
            });
            let cr: ControlRef = check.clone();
            put(&format!("displayItemStackedMutatedCheckbox{i}"), check.clone());
            let t1 = t.clone();
            let slider = SliderControl::new(
                crate::control::anchor_collapse("LEFT", &cr, "RIGHT"),
                rect(4.0, 0.0, 100.0, 18.0),
                Some(Rc::new(move |g: &mut Gfx, val: f64| {
                    let s = t1();
                    let id = s.panel_ref().range_lines.borrow().get(i - 1).copied();
                    if let Some(id) = id {
                        s.edit_display(|it| {
                            if let Some(m) = it.mod_line_by_id(id) {
                                m.range = Some(val);
                            }
                            it.build_and_parse_raw();
                        });
                        s.update_display_item_tooltip(g);
                        s.update_custom_controls(g);
                    }
                })),
                None,
            );
            let t1 = t.clone();
            slider.base.shown.set_fn(move |_| {
                let s = t1();
                let p = s.panel_ref();
                let d = s.display_item.borrow();
                let Some(it) = d.as_ref().filter(|x| show_all() && x.rarity == "UNIQUE") else {
                    return false;
                };
                let id = p.range_lines.borrow().get(i - 1).copied();
                let Some(m) = id.and_then(|x| mod_line(it, x)) else { return false };
                if let Some(r) = m.range {
                    p.stacked[i - 1].slider.val.set(r);
                }
                m.show_slider == Some(true)
            });
            let slr2: ControlRef = slider.clone();
            put(&format!("displayItemStackedRangeSlider{i}"), slider.clone());
            let t1 = t.clone();
            let label = LabelControl::new(
                crate::control::anchor_collapse("LEFT", &slr2, "RIGHT"),
                rect(4.0, -2.0, 350.0, 14.0),
                crate::control::Prop::func(move |g| {
                    let s = t1();
                    let p = s.panel_ref();
                    let id = p.range_lines.borrow().get(i - 1).copied();
                    let d = s.display_item.borrow();
                    let line = id.and_then(|x| d.as_ref().and_then(|it| mod_line(it, x)));
                    let Some(m) = line else { return String::new() };
                    let colour = if m.flag("mutated") { cc("MUTATED") } else { "^7".to_string() };
                    let text = crate::common::wrap_string(g, &m.line, 14.0, 370.0).join("\n");
                    p.stacked[i - 1].line_count.set(text.matches('\n').count() as f64 + 1.0);
                    format!("{colour}{text}")
                }),
            );
            let (sl, ch) = (slider.clone(), check.clone());
            label.base.shown.set_fn(move |g| sl.is_shown(g) || ch.is_shown(g));
            put(&format!("displayItemStackedRangeLine{i}"), label.clone());
            stacked.push(Rc::new(RangeRow {
                check,
                slider,
                label,
                line_count: Cell::new(0.0),
            }));
        }
        {
            let (d1, t1, rows) = (disp.clone(), t.clone(), stacked.clone());
            section_range.base.height.set_fn(move |_| {
                let Some(it) = d1() else { return 0.0 };
                let n = t1().panel_ref().range_lines.borrow().len();
                if n == 0 {
                    return 0.0;
                }
                if show_all() && it.rarity == "UNIQUE" {
                    let mut h = 0.0;
                    for r in rows.iter().take(n) {
                        h += f64::max(22.0, r.line_count.get() * 14.0);
                    }
                    h + 4.0
                } else {
                    28.0
                }
            });
        }
        let tooltip_anchor = BareControl::new(anchor("TOPLEFT", &srr, "BOTTOMLEFT"), rect(0.0, 0.0, 0.0, 0.0));
        put("displayItemTooltipAnchor", tooltip_anchor.clone());
        *tab.tooltip_anchor.borrow_mut() = Some(tooltip_anchor);
        Rc::new(Panel {
            add,
            section_variant,
            version,
            variants,
            variant_state: std::array::from_fn(|_| Cell::new((false, false, 0.0))),
            sockets,
            links,
            influence: [inf1, inf2],
            quality_edit,
            catalyst,
            catalyst_edit,
            cluster_skill,
            cluster_count,
            sorting,
            sorting_label,
            affixes,
            section_custom,
            add_custom,
            add_crucible,
            custom: RefCell::new(Vec::new()),
            custom_count: Cell::new(0.0),
            positions_label,
            positions,
            range_line,
            range_slider,
            mutated,
            stacked,
            range_lines: RefCell::new(Vec::new()),
            names: RefCell::new(names.into_inner()),
        })
    }

    /// The display part of the gate's dump.
    pub fn debug_dump(&self, g: &Gfx, tab: &ItemsTab) -> serde_json::Value {
        use serde_json::json;
        let Some(item) = tab.display_item.borrow().clone() else {
            return serde_json::Value::Null;
        };
        let drop = |x: &DropDownControl| {
            let list: Vec<String> = x.list.borrow().iter().map(|v| v.label.clone().unwrap_or_default()).collect();
            let shown = x.is_shown(g);
            json!({ "shown": shown, "enabled": shown && x.is_enabled(g), "sel": x.sel_index.get(), "list": list })
        };
        let lines: Vec<String> = tab
            .display_tooltip
            .borrow()
            .lines
            .iter()
            .map(|l| l.text.clone().unwrap_or_else(|| "<sep>".to_string()))
            .collect();
        let rect_of = |n: &str| {
            let c = self.host_control(n).expect("control");
            let (x, y) = c.get_pos(g);
            let (w, h) = c.get_size(g);
            if c.is_shown(g) {
                json!(format!("{x},{y},{w},{h}"))
            } else {
                json!(false)
            }
        };
        let mut rects = serde_json::Map::new();
        for n in [
            "addDisplayItem",
            "displayItemEnchant",
            "displayItemAnoint",
            "displayItemCorrupt",
            "displayItemAddImplicit",
            "displayItemInfluence",
            "displayItemQualityEdit",
            "displayItemCatalyst",
            "displayItemClusterJewelSkill",
            "craftingSorting",
            "displayItemAddCustom",
            "displayItemAddCrucible",
            "displayItemRangeLine",
            "displayItemTooltipAnchor",
            "displayItemAffix1",
            "displayItemSocket1",
        ] {
            rects.insert(n.to_string(), rect_of(n));
        }
        let affix: Vec<_> = self
            .affixes
            .iter()
            .filter(|a| a.drop.is_shown(g))
            .map(|a| json!({ "drop": drop(&a.drop), "slider": a.slider.is_shown(g), "val": a.slider.val.get() }))
            .collect();
        json!({
            "raw": item.build_raw(), "id": item.id, "tooltip": lines, "rects": rects,
            "variant": drop(&self.variants[0]), "version": drop(&self.version),
            "rangeLine": drop(&self.range_line), "rangeVal": self.range_slider.val.get(),
            "quality": self.quality_edit.buf.borrow().clone(), "catalyst": drop(&self.catalyst),
            "influence": [drop(&self.influence[0]), drop(&self.influence[1])],
            "affixes": affix, "custom": self.custom_count.get(),
        })
    }
}

/// `self.displayItem.rangeLineList[i]`.
fn mod_line(it: &Item, id: (u8, usize)) -> Option<&ModLine> {
    match id.0 {
        1 => it.enchant_mod_lines.get(id.1),
        2 => it.scourge_mod_lines.get(id.1),
        3 => it.implicit_mod_lines.get(id.1),
        4 => it.explicit_mod_lines.get(id.1),
        5 => it.crucible_mod_lines.get(id.1),
        _ => None,
    }
}

/// `isAnointable(item)` (:70-74).
pub fn is_anointable(it: &Item) -> bool {
    it.base.is_some_and(|b| {
        b.cannot_be_anointed != Some(true) && b.sub_type.as_deref() != Some("Talisman") && (it.can_be_anointed || b.ty == "Amulet")
    })
}

impl ItemsTab {
    pub(crate) fn panel_ref(&self) -> Rc<Panel> {
        self.panel.borrow().clone().expect("panel")
    }

    /// An edit of `self.displayItem` in place.
    pub(crate) fn edit_display<R>(&self, f: impl FnOnce(&mut Item) -> R) -> R {
        let mut d = self.display_item.borrow_mut();
        let rc = d.as_mut().expect("displayItem");
        f(Rc::make_mut(rc))
    }

    /// `ItemsTab:CopyAnointsAndEldritchImplicits(newItem, copyEldritchImplicits, overwrite, sourceSlotName)` (:1959-1998).
    fn copy_anoints(&self, it: &mut Item, copy_eldritch: bool) {
        let base = it.base.expect("base");
        let ty = if base.weapon.is_some() {
            "Weapon 1".to_string()
        } else {
            base.ty.clone()
        };
        let (exists, sel) = self.read(|e| {
            (
                e.active_item_set().slots.contains_key(&ty) || self.slots.borrow().contains_key(&ty),
                e.slot_sel_item(&ty),
            )
        });
        if !exists {
            return;
        }
        let Some(cur) = self.read(|e| e.item_by_id(sel)) else { return };
        if is_anointable(it) && it.enchant_mod_lines.is_empty() && sel > 0.0 && cur.enchant_mod_lines.len() == 1 {
            it.enchant_mod_lines.clone_from(&cur.enchant_mod_lines);
        }
        for (k, _) in &INFLUENCE_INFO[..6] {
            if it.influence(k) {
                return;
            }
        }
        let modifiable = !(it.corrupted || it.mirrored);
        if copy_eldritch
            && ["Helmet", "Body Armour", "Gloves", "Boots"].contains(&base.ty.as_str())
            && ["NORMAL", "MAGIC", "RARE"].contains(&it.rarity.as_str())
            && it.implicit_mod_lines.is_empty()
            && modifiable
            && (cur.influence("cleansing") || cur.influence("tangle"))
        {
            it.implicit_mod_lines.clone_from(&cur.implicit_mod_lines);
            it.set_influence("tangle", cur.influence("tangle"));
            it.set_influence("cleansing", cur.influence("cleansing"));
        }
        if (base.weapon.is_some() || base.ty == "Body Armour") && it.enchant_mod_lines.is_empty() && sel > 0.0 && modifiable {
            it.enchant_mod_lines.clone_from(&cur.enchant_mod_lines);
        }
        it.build_and_parse_raw();
    }

    /// `ItemsTab:CreateDisplayItemFromRaw(itemRaw, normalise)` (:2001-2011).
    pub fn create_display_item_from_raw(&self, g: &mut Gfx, raw: &str, normalise: bool) {
        let mut it = Item::parse_with(raw, None, false);
        if it.base.is_some() {
            if normalise {
                let copy = main().s.borrow().migrate_eldritch_implicits;
                self.copy_anoints(&mut it, copy);
                it.normalise_quality();
                it.build_mod_list();
            }
            self.set_display_item(g, Some(it));
        }
    }

    /// `SelectDisplayItemVariant` (:2013-2027).
    fn select_variant(&self, g: &mut Gfx, index: usize, value: &DropItem, which: usize) {
        let uses = self.display_item.borrow().as_ref().is_some_and(|i| i.uses_variant_groups);
        if uses {
            let Some(vid) = value.data.as_ref().and_then(|d| d.downcast_ref::<f64>()).copied() else {
                return;
            };
            let gid = self.panel_ref().variant_state[which].get().2;
            self.edit_display(|it| {
                it.variant_group_selections.insert(pob_items::NumKey(gid), vid);
                it.normalise_variant_selections();
            });
        } else {
            self.edit_display(|it| {
                if which == 0 {
                    it.variant = Some(index as f64);
                } else {
                    it.variant_alt[which - 1] = Some(index as f64);
                }
            });
        }
        self.edit_display(Item::build_and_parse_raw);
        self.update_display_item_variant_controls(g);
        self.update_display_item_tooltip(g);
        self.update_display_item_range_lines(g);
    }

    /// `UpdateDisplayItemVariantControls` (:2029-2084).
    pub(crate) fn update_display_item_variant_controls(&self, g: &Gfx) {
        let p = self.panel_ref();
        for s in &p.variant_state {
            s.set((false, false, s.get().2));
        }
        let Some(item) = self.display_item.borrow().clone().filter(|i| i.uses_variant_groups) else {
            return;
        };
        *p.version.list.borrow_mut() = item.version_list.iter().flatten().map(|s| DropItem::str(s)).collect();
        p.version.sel_index.set(item.selected_version.unwrap_or(1.0) as usize);
        p.version.check_dropped_width(g, true);
        let mut at = 0;
        for gid in item.variant_groups.keys() {
            if item.get_variant_group_options(gid.0, false).is_empty() {
                continue;
            }
            let Some(c) = p.variants.get(at) else { break };
            let available = item.get_variant_group_options(gid.0, true);
            let mut list = Vec::new();
            let mut selected = None;
            for v in &available {
                let mut e = DropItem::table(&item.variant_list.as_ref().expect("variantList")[*v as usize - 1]);
                e.data = Some(Rc::new(*v));
                list.push(e);
                if item.variant_group_selections.get(gid) == Some(v) {
                    selected = Some(list.len());
                }
            }
            if list.is_empty() {
                list.push(DropItem::table("No available variants"));
            }
            *c.list.borrow_mut() = list;
            c.sel_index.set(selected.unwrap_or(1));
            p.variant_state[at].set((true, available.len() > 1, gid.0));
            c.check_dropped_width(g, true);
            at += 1;
        }
    }

    /// `ItemsTab:SetDisplayItem(item)` (:2087-2171).
    pub fn set_display_item(&self, g: &mut Gfx, item: Option<Item>) {
        let mut item = item;
        if let Some(it) = &mut item
            && !it.catalyst.is_some_and(|c| c > 0.0)
        {
            it.catalyst_quality = None;
        }
        *self.display_item.borrow_mut() = item.map(Rc::new);
        let Some(item) = self.display_item.borrow().clone() else {
            self.snap(Some("LEFT"));
            return;
        };
        let p = self.panel_ref();
        self.update_display_item_tooltip(g);
        self.snap(Some("RIGHT"));
        if item.uses_variant_groups {
            self.update_display_item_variant_controls(g);
        } else {
            let list: Vec<DropItem> = item.variant_list.iter().flatten().map(|s| DropItem::str(s)).collect();
            *p.variants[0].list.borrow_mut() = list.clone();
            p.variants[0].sel_index.set(item.variant.unwrap_or(0.0) as usize);
            p.variants[0].check_dropped_width(g, true);
            for i in 0..5 {
                if item.has_alt_variant[i] {
                    *p.variants[i + 1].list.borrow_mut() = list.clone();
                    p.variants[i + 1].sel_index.set(item.variant_alt[i].unwrap_or(1.0) as usize);
                    p.variants[i + 1].check_dropped_width(g, true);
                }
            }
            self.update_display_item_variant_controls(g);
        }
        self.update_socket_controls();
        if item.crafted {
            self.update_affix_controls(g);
        }
        // :2134-2155
        let all = item.can_have_eldritch_influence || matches!(item.ty.as_deref(), Some("Helmet" | "Body Armour" | "Gloves" | "Boots"));
        let mut list = vec![DropItem::str("Influence")];
        list.extend(INFLUENCE_INFO[..if all { 8 } else { 6 }].iter().map(|(_, d)| DropItem::str(d)));
        *p.influence[0].list.borrow_mut() = list.clone();
        *p.influence[1].list.borrow_mut() = list;
        let (mut i1, mut i2) = (1, 1);
        for (i, (k, _)) in INFLUENCE_INFO.iter().enumerate() {
            if item.influence(k) {
                if i1 == 1 {
                    i1 = i + 2;
                } else if i2 == 1 {
                    i2 = i + 2;
                    break;
                }
            }
        }
        p.influence[0].set_sel(g, i1 as i64, true);
        p.influence[1].set_sel(g, i2 as i64, true);
        // `SetText(item.quality)`: `tostring(nil)`
        p.quality_edit
            .set_text(g, &item.quality.map_or_else(|| "nil".to_string(), pob_lua::tostring_num), false);
        p.catalyst.set_sel(g, item.catalyst.unwrap_or(0.0) as i64 + 1, true);
        p.catalyst_edit
            .set_text(g, &pob_lua::tostring_num(item.catalyst_quality.map_or(0.0, |q| q.max(0.0))), false);
        self.update_custom_controls(g);
        self.update_display_item_range_lines(g);
        if item.cluster_jewel.is_some() && item.crafted {
            self.update_cluster_jewel_controls(g);
        }
    }

    /// `UpdateDisplayItemTooltip` (:2173-2178).
    pub(crate) fn update_display_item_tooltip(&self, g: &Gfx) {
        let Some(item) = self.display_item.borrow().clone() else { return };
        *self.panel_ref().positions.borrow_mut() = notable_positions(&item, self);
        let mut t = self.display_tooltip.borrow_mut();
        t.clear(false);
        self.add_tooltip(&mut t, g, &item, None, false);
        t.center = true;
    }

    /// `ToggleDisplayItemModLine` (:2180-2193).
    pub(crate) fn toggle_display_mod_line(&self, g: &mut Gfx, id: (u8, usize)) {
        self.edit_display(|it| {
            if let Some(m) = it.mod_line_by_id(id) {
                let v = m.flag("disabled");
                m.set_flag_to("disabled", !v);
            }
            it.build_and_parse_raw();
        });
        self.update_display_item_tooltip(g);
        self.update_display_item_range_lines(g);
        self.update_custom_controls(g);
        if self.display_item.borrow().as_ref().is_some_and(|i| i.crafted) {
            self.update_affix_controls(g);
        }
        self.build().build_flag.set(true);
    }

    /// `UpdateSocketControls` (:2195-2203).
    pub(crate) fn update_socket_controls(&self) {
        let p = self.panel_ref();
        let Some(it) = self.display_item.borrow().clone() else { return };
        let n = it.sockets.len() - it.abyssal_socket_count as usize;
        for i in 0..n {
            crate::modules::main::sel_by_value(&p.sockets[i], &it.sockets[i].color);
            if i > 0 {
                p.links[i - 1].state.set(it.sockets[i].group == it.sockets[i - 1].group);
            }
        }
    }

    /// `setDisplayItemInfluence` (:586-609).
    fn set_display_item_influence(&self, g: &mut Gfx, list: [usize; 2]) {
        self.edit_display(|it| {
            it.reset_influence();
            if it.has_elder_shaper_and_all_conqueror_influences {
                for i in 0..6 {
                    it.influence[i] = true;
                }
            } else {
                for i in list {
                    if i > 0 {
                        it.influence[i - 1] = true;
                    }
                }
            }
        });
        self.force_affix_selectors(g);
        self.edit_display(Item::build_and_parse_raw);
        self.update_display_item_tooltip(g);
    }

    /// "Force affix selectors to update": `drop.selFunc(drop.selIndex, drop.list[drop.selIndex])`.
    fn force_affix_selectors(&self, g: &mut Gfx) {
        let Some(it) = self.display_item.borrow().clone().filter(|i| i.crafted) else {
            return;
        };
        let p = self.panel_ref();
        for i in 0..it.affix_limit as usize {
            let d = p.affixes[i].drop.clone();
            let sel = d.sel_index.get();
            let v = d.list.borrow()[sel - 1].clone();
            let f = d.sel_func.borrow().clone();
            if let Some(f) = f {
                f(g, sel, &v);
            }
        }
    }

    /// `UpdateClusterJewelControls` (:2205-2230).
    fn update_cluster_jewel_controls(&self, g: &mut Gfx) {
        let p = self.panel_ref();
        let item = self.display_item.borrow().clone().expect("displayItem");
        let c = item.cluster_jewel.expect("clusterJewel");
        let mut list: Vec<DropItem> = c
            .skills
            .iter()
            .filter(|(id, _)| !["affliction_strength", "affliction_dexterity", "affliction_intelligence"].contains(&id.as_str()))
            .map(|(id, s)| {
                let mut e = DropItem::table(&s.name);
                e.data = Some(Rc::new(id.clone()));
                e
            })
            .collect();
        list.sort_by(|a, b| a.label.cmp(&b.label));
        let first = list[0].data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned();
        *p.cluster_skill.list.borrow_mut() = list;
        self.edit_display(|it| {
            if it.cluster_jewel_skill.as_ref().is_none_or(|s| !c.skills.contains_key(s)) {
                it.cluster_jewel_skill = first;
            }
            it.cluster_jewel_node_count = Some(it.cluster_jewel_node_count.unwrap_or(c.min_nodes).max(c.min_nodes).min(c.max_nodes));
        });
        let it = self.display_item.borrow().clone().expect("displayItem");
        crate::modules::main::sel_by_value(&p.cluster_skill, it.cluster_jewel_skill.as_ref().expect("skill"));
        p.cluster_count.div_count.set(Some(c.max_nodes - c.min_nodes));
        p.cluster_count
            .val
            .set((it.cluster_jewel_node_count.unwrap_or(0.0) - c.min_nodes) / (c.max_nodes - c.min_nodes));
        self.craft_cluster_jewel(g);
    }

    /// `CraftClusterJewel` (:2232-2251).
    fn craft_cluster_jewel(&self, g: &mut Gfx) {
        self.edit_display(|it| {
            let c = it.cluster_jewel.expect("clusterJewel");
            let mut lines = vec![format!(
                "Adds {} Passive Skills",
                pob_lua::tostring_num(it.cluster_jewel_node_count.unwrap_or(c.min_nodes))
            )];
            if c.size == "Large" {
                lines.push("2 Added Passive Skills are Jewel Sockets".to_string());
            } else if c.size == "Medium" {
                lines.push("1 Added Passive Skill is a Jewel Socket".to_string());
            }
            let skill = c.skills.get(it.cluster_jewel_skill.as_deref().expect("skill")).expect("skill");
            lines.push(skill.enchant.clone().unwrap_or_default().join("\n"));
            it.enchant_mod_lines = lines
                .into_iter()
                .map(|line| {
                    let mut m = ModLine {
                        line,
                        ..ModLine::default()
                    };
                    m.set_flag_pub("crafted");
                    m
                })
                .collect();
            it.build_and_parse_raw();
        });
        self.update_affix_controls(g);
        self.force_affix_selectors(g);
    }

    /// `UpdateAffixControls` (:2254-2273).
    pub(crate) fn update_affix_controls(&self, g: &mut Gfx) {
        let item = self.display_item.borrow().clone().expect("displayItem");
        let half = item.affix_limit / 2.0;
        let prefix_limit = item.prefixes_limit.unwrap_or(half) as usize;
        for i in 1..=item.affix_limit as usize {
            if i <= prefix_limit {
                let ty = if item.ignore_mod_type() { None } else { Some("Prefix") };
                self.update_affix_control(g, i - 1, ty, false, i);
            } else {
                self.update_affix_control(g, i - 1, Some("Suffix"), true, i - prefix_limit);
            }
        }
        self.update_custom_controls(g);
    }

    /// `UpdateAffixControl` (:2275-2484).
    fn update_affix_control(&self, g: &Gfx, row: usize, affix_type: Option<&str>, suffix: bool, output_index: usize) {
        let _ = g;
        let p = self.panel_ref();
        let item = self.display_item.borrow().clone().expect("displayItem");
        let Some(mods) = item.affix_mods() else { return };
        let half = item.affix_limit / 2.0;
        let mut extra: HashSet<String> = HashSet::new();
        let mut exclude: HashSet<String> = HashSet::new();
        for (is_suffix, list, limit) in [
            (false, &item.prefixes, item.prefixes_limit.unwrap_or(half)),
            (true, &item.suffixes, item.suffixes_limit.unwrap_or(half)),
        ] {
            for index in 1..=limit as usize {
                if index != output_index || is_suffix != suffix {
                    let m = list.get(index - 1).and_then(|a| mods.get(&a.mod_id));
                    if let Some(m) = m {
                        if let Some(gr) = &m.group {
                            exclude.insert(gr.clone());
                        }
                        extra.extend(m.tags.iter().cloned());
                    }
                }
            }
        }
        if let (Some(c), Some(s)) = (item.cluster_jewel, &item.cluster_jewel_skill)
            && let Some(sk) = c.skills.get(s)
        {
            extra.insert(sk.tag.clone());
        }
        let list_of = if suffix { &item.suffixes } else { &item.prefixes };
        let sel_affix = list_of.get(output_index - 1).map(|a| a.mod_id.clone());
        let mut affix_list: Vec<&AffixMod> = Vec::new();
        let mut retained: HashSet<String> = HashSet::new();
        for (id, m) in mods {
            if affix_type.is_none_or(|t| m.ty.as_deref() == Some(t))
                && !m.group.as_ref().is_some_and(|gr| exclude.contains(gr))
                && !item.check_if_mod_is_delve(m)
            {
                if item.can_have_mod(m, &extra) {
                    affix_list.push(m);
                } else if Some(id) == sel_affix.as_ref() {
                    affix_list.push(m);
                    retained.insert(id.clone());
                }
            }
        }
        // :2315-2331
        pob_engine::lua::lua_sort(&mut affix_list, &mut |a, b| {
            for i in 0..a.lines.len().max(b.lines.len()) {
                if i >= a.lines.len() {
                    return true;
                } else if i >= b.lines.len() {
                    return false;
                } else if a.stat_order[i] != b.stat_order[i] {
                    return a.stat_order[i] < b.stat_order[i];
                }
            }
            if a.level != b.level {
                return a.level < b.level;
            }
            a.id.as_bytes() < b.id.as_bytes()
        });
        let row_c = &p.affixes[row];
        let default_quality = main().s.borrow().default_item_affix_quality;
        row_c.output.set(AffixOutput {
            suffix,
            index: output_index,
        });
        row_c.slider_shown.set(false);
        row_c.slider.val.set(default_quality);
        let mut entries: Vec<DropItem> = vec![DropItem::str("None")];
        let mut last: Option<(Vec<f64>, usize)> = None;
        let mut series: Vec<AffixSeries> = vec![AffixSeries {
            mod_list: Vec::new(),
            have_range: false,
        }];
        let has_range = |s: &str| {
            pob_lua::find(s.as_bytes(), b"%(%-?[%d%.]+%-%-?[%d%.]+%)", None, false)
                .ok()
                .flatten()
                .is_some()
        };
        for m in affix_list {
            if last.as_ref().is_none_or(|(o, _)| *o != m.stat_order) {
                let text = m.lines.join("/");
                series.push(AffixSeries {
                    mod_list: Vec::new(),
                    have_range: has_range(&text),
                });
                entries.push(DropItem::table(&text));
                last = Some((m.stat_order.clone(), entries.len() - 1));
            }
            let at = last.as_ref().expect("series").1;
            if retained.contains(&m.id) && item.cluster_jewel.is_some() {
                let l = entries[at].label.clone().unwrap_or_default();
                entries[at].label = Some(format!("^8[Retained] {l}"));
            }
            series[at].mod_list.push(m.id.clone());
            if series[at].mod_list.len() == 2 {
                let l = entries[at].label.clone().unwrap_or_default();
                let l = pob_items::lua::gsub(&l, "%(%-?[%d%.]+%-%-?[%d%.]+%)", "#", None);
                let l = pob_items::lua::gsub(&l, "%-?%d+%.?%d*", "#", None);
                entries[at].label = Some(l);
                series[at].have_range = true;
            }
        }
        for (e, s) in entries.iter_mut().zip(&series) {
            e.data = Some(Rc::new(s.clone()));
        }
        // :2365-2449 sort the groups by power
        let sort_label = p
            .sorting
            .list
            .borrow()
            .get(p.sorting.sel_index.get().wrapping_sub(1))
            .and_then(|v| v.label.clone());
        let stat = sort_label.and_then(|l| {
            pob_calc::power::power_stat_list()
                .into_iter()
                .find(|s| s.label == l && s.stat.is_some())
        });
        if let (Some(stat), true) = (stat, p.sorting_label.base.shown.get(g)) {
            let quality = main().s.borrow().default_item_affix_quality;
            let slot = item.get_primary_slot();
            let mut subject = Item::parse_with(&item.build_raw(), None, false);
            if sel_affix.as_deref().is_some_and(|a| a != "None") {
                let list = if suffix { &mut subject.suffixes } else { &mut subject.prefixes };
                if let Some(a) = list.get_mut(output_index - 1) {
                    a.mod_id = "None".to_string();
                    a.range = None;
                }
                subject.craft();
            }
            let scaled = !subject.mod_magnitude_empty() || subject.catalyst.is_some_and(|c| c > 0.0);
            let power_of = |id: &str| -> f64 {
                let m = &mods[id];
                let mut it = subject.clone();
                for sub in &m.lines {
                    let line = if scaled {
                        sub.clone()
                    } else {
                        pob_items::apply_range::apply_range(sub, quality, Some(1.0), Some(1.0), pob_calc::ParseMode::Full)
                    };
                    let mut ml = ModLine {
                        line,
                        mod_tags: m.mod_tags.clone(),
                        ..ModLine::default()
                    };
                    match m.ty.as_deref() {
                        Some("Prefix") => ml.set_flag_pub("prefix"),
                        Some("Suffix") => ml.set_flag_pub("suffix"),
                        _ => {}
                    }
                    it.explicit_mod_lines.push(ml);
                }
                it.build_and_parse_raw();
                let ov = pob_calc::env::CalcOverride {
                    rep_slot_name: Some(slot.clone()),
                    rep_item: Some(Rc::new(it)),
                    ..Default::default()
                };
                self.read(|e| e.item_override_power(ov, &stat)).unwrap_or_else(|er| panic!("{er}"))
            };
            let pick = |l: &[String]| -> String {
                if l.len() == 1 {
                    l[0].clone()
                } else {
                    l[pob_lua::number::round((l.len() - 1) as f64 * quality, None) as usize].clone()
                }
            };
            let mut keyed: Vec<(Option<f64>, DropItem, AffixSeries)> = entries
                .iter()
                .cloned()
                .zip(series.iter().cloned())
                .map(|(e, s)| {
                    (
                        if s.mod_list.is_empty() {
                            None
                        } else {
                            Some(power_of(&pick(&s.mod_list)))
                        },
                        e,
                        s,
                    )
                })
                .collect();
            pob_engine::lua::lua_sort(&mut keyed, &mut |a, b| match (a.0, b.0) {
                (None, _) => true,
                (_, None) => false,
                (Some(x), Some(y)) => x > y,
            });
            entries = keyed.iter().map(|x| x.1.clone()).collect();
            series = keyed.into_iter().map(|x| x.2).collect();
        }
        if row_c.drop.list.borrow().len() != entries.len() {
            row_c.drop.dropped.set(false);
        }
        *row_c.drop.list.borrow_mut() = entries;
        let sel_index = series
            .iter()
            .position(|s| sel_affix.as_ref().is_some_and(|a| s.mod_list.contains(a)))
            .map_or(1, |i| i + 1);
        row_c.drop.sel_index.set(sel_index);
        let s = &series[sel_index - 1];
        if s.have_range {
            let n = s.mod_list.len() as f64;
            let index = sel_affix
                .as_ref()
                .and_then(|a| s.mod_list.iter().position(|x| x == a))
                .map_or(1.0, |i| (i + 1) as f64);
            let range = match list_of.get(output_index - 1).and_then(|a| a.range.as_ref()) {
                Some(AffixRange::Num(x)) => *x,
                Some(AffixRange::List(l)) => l.first().copied().unwrap_or(0.5),
                None => 0.5,
            };
            let mut range = range.clamp(0.0, 1.0);
            if range == 0.0 && index > 1.0 {
                range = 1e-4;
            }
            row_c.slider.val.set((index - 1.0 + range) / n);
            row_c.slider.div_count.set(if n == 1.0 { None } else { Some(n) });
            row_c.slider_shown.set(true);
        }
    }

    /// `verifyRange` (:764-803).
    fn verify_range(&self, range: f64, index: usize, modlist: &[String]) -> f64 {
        let item = self.display_item.borrow().clone().expect("displayItem");
        let Some(mods) = item.affix_mods() else { return range };
        let min_max = |m: &AffixMod| -> Option<(f64, f64)> {
            for l in &m.lines {
                if let Some(c) = pob_items::lua::match_(l, "%((%d[%d%.]*)%-(%d[%d%.]*)%)", "ItemsTab.lua:770") {
                    return Some((c[0].parse().ok()?, c[1].parse().ok()?));
                }
            }
            None
        };
        let flip = |a: &AffixMod, b: &AffixMod| -> bool {
            let (Some((min_a, max_a)), Some((min_b, max_b))) = (min_max(a), min_max(b)) else {
                return false;
            };
            if min_a == min_a.floor() && max_a == max_a.floor() && min_b == min_b.floor() && max_b == max_b.floor() {
                if min_a < min_b {
                    min_a + 1.0 == max_b
                } else {
                    min_a - 1.0 == max_b
                }
            } else {
                false
            }
        };
        let cur = mods.get(&modlist[index - 1]);
        let prior = index.checked_sub(2).and_then(|i| modlist.get(i)).and_then(|id| mods.get(id));
        // `index + 1 < #list`
        let next = if index + 1 < modlist.len() {
            mods.get(&modlist[index])
        } else {
            None
        };
        if let (Some(p), Some(c)) = (prior, cur) {
            if flip(p, c) {
                return 1.0 - range;
            }
        } else if let (Some(n), Some(c)) = (next, cur)
            && flip(c, n)
        {
            return 1.0 - range;
        }
        range
    }

    /// The `selFunc` of an affix dropdown (:804-819).
    fn on_affix_select(&self, g: &mut Gfx, row: usize, _index: usize, value: &DropItem) {
        let p = self.panel_ref();
        let r = p.affixes[row].clone();
        let out = r.output.get();
        let series = value.data.as_ref().and_then(|d| d.downcast_ref::<AffixSeries>()).cloned();
        let mut affix = Affix {
            mod_id: "None".to_string(),
            range: None,
            fractured: self
                .display_item
                .borrow()
                .as_ref()
                .and_then(|it| {
                    if out.suffix {
                        it.suffixes.get(out.index - 1)
                    } else {
                        it.prefixes.get(out.index - 1)
                    }
                    .map(|a| a.fractured)
                })
                .unwrap_or(false),
        };
        if let Some(s) = series.filter(|s| !s.mod_list.is_empty()) {
            r.slider.div_count.set(Some(s.mod_list.len() as f64));
            let (i, range) = r.slider.get_div_val(None);
            affix.mod_id.clone_from(&s.mod_list[i as usize - 1]);
            affix.range = Some(AffixRange::Num(self.verify_range(range, i as usize, &s.mod_list)));
        }
        self.edit_display(|it| {
            let list = if out.suffix { &mut it.suffixes } else { &mut it.prefixes };
            while list.len() < out.index {
                list.push(Affix {
                    mod_id: "None".to_string(),
                    range: None,
                    fractured: false,
                });
            }
            list[out.index - 1] = affix;
            it.craft();
        });
        self.update_display_item_tooltip(g);
        self.update_affix_controls(g);
    }

    /// The `changeFunc` of an affix slider (:939-947).
    fn on_affix_slider(&self, g: &mut Gfx, row: usize) {
        let p = self.panel_ref();
        let r = p.affixes[row].clone();
        let out = r.output.get();
        let sel = r.drop.sel_index.get();
        let Some(s) = r.drop.list.borrow()[sel - 1]
            .data
            .as_ref()
            .and_then(|d| d.downcast_ref::<AffixSeries>())
            .cloned()
        else {
            return;
        };
        let (i, range) = r.slider.get_div_val(None);
        let range = self.verify_range(range, i as usize, &s.mod_list);
        self.edit_display(|it| {
            let list = if out.suffix { &mut it.suffixes } else { &mut it.prefixes };
            list[out.index - 1].mod_id.clone_from(&s.mod_list[i as usize - 1]);
            list[out.index - 1].range = Some(AffixRange::Num(range));
            it.craft();
        });
        self.update_display_item_tooltip(g);
    }

    /// `UpdateCustomControls` (:2487-2534).
    pub(crate) fn update_custom_controls(&self, g: &Gfx) {
        let p = self.panel_ref();
        let Some(item) = self.display_item.borrow().clone() else { return };
        let mut i = 0usize;
        let explicit_n = item.explicit_mod_lines.len();
        let all: Vec<&ModLine> = item.explicit_mod_lines.iter().chain(item.crucible_mod_lines.iter()).collect();
        if item.rare_like_unique.is_some() || item.rarity == "MAGIC" || item.rarity == "RARE" || !item.crucible_mod_lines.is_empty() {
            for (index, ml) in all.iter().enumerate() {
                if ml.flag("custom") || ml.flag("crafted") || ml.flag("crucible") {
                    let Some(line) = format_mod_line(ml, false) else { continue };
                    i += 1;
                    if p.custom.borrow().len() < i {
                        let srr: ControlRef = p.section_custom.clone();
                        let rm = ButtonControl::new(
                            anchor("TOPLEFT", &srr, "TOPLEFT"),
                            rect(0.0, i as f64 * 22.0 + 4.0, 70.0, 20.0),
                            "^7Remove".to_string(),
                            |_| {},
                        );
                        let rr: ControlRef = rm.clone();
                        let text = LabelControl::new(anchor("LEFT", &rr, "RIGHT"), rect(65.0, 0.0, 0.0, 16.0), String::new());
                        let label = LabelControl::new(anchor("LEFT", &rr, "RIGHT"), rect(5.0, 0.0, 0.0, 16.0), String::new());
                        self.host.add(&format!("displayItemCustomModifierRemove{i}"), rm.clone());
                        self.host.add(&format!("displayItemCustomModifier{i}"), text.clone());
                        self.host.add(&format!("displayItemCustomModifierLabel{i}"), label.clone());
                        p.custom.borrow_mut().push((rm, text, label));
                    }
                    let (rm, text, label) = p.custom.borrow()[i - 1].clone();
                    rm.base.shown.set(true);
                    let mut shown = line;
                    if sg::draw_string_cursor_index(g, 16.0, "VAR", &shown, 330.0, 10.0) < shown.len() {
                        let at = sg::draw_string_cursor_index(g, 16.0, "VAR", &shown, 310.0, 10.0);
                        shown = format!("{}...", &shown[..at.min(shown.len())]);
                    }
                    text.label.set(shown);
                    label.label.set(
                        if ml.flag("crafted") {
                            " ^7Crafted:"
                        } else if ml.flag("crucible") {
                            "^7Crucible:"
                        } else {
                            " ^7Custom:"
                        }
                        .to_string(),
                    );
                    let me = self.me.clone();
                    *rm.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| {
                        let t = me.upgrade().expect("itemsTab");
                        t.edit_display(|it| {
                            if index >= explicit_n {
                                it.crucible_mod_lines.remove(index - explicit_n);
                            } else {
                                it.explicit_mod_lines.remove(index);
                            }
                            it.build_and_parse_raw();
                        });
                        let (id, raw) = {
                            let d = t.display_item.borrow();
                            let it = d.as_ref().expect("displayItem");
                            (it.id, it.build_raw())
                        };
                        t.create_display_item_from_raw(g, &raw, false);
                        t.edit_display(|it| it.id = id);
                    });
                }
            }
        }
        p.custom_count.set(i as f64);
        for (rm, _, _) in p.custom.borrow().iter().skip(i) {
            rm.base.shown.set(false);
        }
    }

    /// `UpdateDisplayItemRangeLines` (:2537-2548).
    pub(crate) fn update_display_item_range_lines(&self, _g: &Gfx) {
        let p = self.panel_ref();
        let Some(item) = self.display_item.borrow().clone() else {
            p.range_lines.borrow_mut().clear();
            return;
        };
        let lines = item.range_line_list();
        if let Some(first) = lines.first() {
            let list: Vec<DropItem> = lines
                .iter()
                .filter_map(|id| mod_line(&item, *id))
                .map(|m| DropItem::table(&m.line))
                .collect();
            *p.range_line.list.borrow_mut() = list;
            p.range_line.sel_index.set(1);
            p.range_slider.val.set(mod_line(&item, *first).and_then(|m| m.range).unwrap_or(0.0));
        }
        *p.range_lines.borrow_mut() = lines;
    }

    /// The checkboxes of a mutated line (:1073-1081, :1134-1141).
    fn toggle_mutation(&self, g: &mut Gfx, id: Option<(u8, usize)>) {
        let Some(id) = id else { return };
        let line = self.display_item.borrow().as_ref().and_then(|it| mod_line(it, id).cloned());
        if let Some(l) = line
            && let (Some(a), Some(b)) = (&l.mod_id, &l.new_mod_id)
        {
            let mutated = !l.flag("mutated");
            self.edit_display(|it| it.mutate_mod(a, b, mutated));
            self.update_display_item_tooltip(g);
            self.update_custom_controls(g);
        }
    }

    // ------------------------------------------------------------------------------------------ popups

    /// `ItemsTab:CraftItem()` (:2679-2763).
    pub(crate) fn craft_item_popup(&self, g: &mut Gfx) {
        let data = pob_data::item_bases::item_bases();
        let types: Vec<String> = data.type_list.clone();
        let (rar, ty_sel, base_sel) = self.last_craft.get();
        let rarity_list = rarity_drop_list();
        let rarity = DropDownControl::new(g, None, rect(-80.0, 20.0, 100.0, 18.0), rarity_list, None, None, false);
        rarity.sel_index.set(rar);
        let title = EditControl::new(
            g,
            None,
            rect(70.0, 20.0, 190.0, 18.0),
            EditArgs {
                init: Some(String::new()),
                prompt: Some("Name".to_string()),
                ..EditArgs::default()
            },
        );
        let w = Rc::downgrade(&rarity);
        title.base.shown.set_fn(move |_| w.upgrade().expect("rarity").sel_index.get() >= 3);
        let base_items = |t: &str| -> Vec<DropItem> {
            data.lists
                .get(t)
                .into_iter()
                .flatten()
                .map(|e| {
                    let mut d = DropItem::table(&e.label);
                    d.data = Some(Rc::new(e.name.clone()));
                    d
                })
                .collect()
        };
        let base = DropDownControl::new(
            g,
            crate::control::anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(55.0, 70.0, 200.0, 18.0),
            base_items(&types[ty_sel - 1]),
            None,
            None,
            false,
        );
        base.sel_index.set(base_sel);
        let b1 = Rc::downgrade(&base);
        let ty_names = types.clone();
        let ty = DropDownControl::new(
            g,
            crate::control::anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(55.0, 45.0, 295.0, 18.0),
            types.iter().map(|s| DropItem::str(s)).collect(),
            Some(Rc::new(move |_g: &mut Gfx, index: usize, _v: &DropItem| {
                let b = b1.upgrade().expect("base");
                *b.list.borrow_mut() = pob_data::item_bases::item_bases()
                    .lists
                    .get(&ty_names[index - 1])
                    .into_iter()
                    .flatten()
                    .map(|e| {
                        let mut d = DropItem::table(&e.label);
                        d.data = Some(Rc::new(e.name.clone()));
                        d
                    })
                    .collect();
                b.sel_index.set(1);
            })),
            None,
            false,
        );
        ty.sel_index.set(ty_sel);
        let (r1, tt1) = (Rc::downgrade(&rarity), Rc::downgrade(&title));
        let make: Rc<dyn Fn(&str) -> Item> = Rc::new(move |name: &str| {
            let r = r1.upgrade().expect("rarity");
            let t = tt1.upgrade().expect("title");
            make_craft_item(name, r.sel_index.get(), &t.buf.borrow())
        });
        let (mk, me) = (make.clone(), self.me.clone());
        base.tip.set_tooltip_func(move |tooltip, g, (mode, _i, value)| {
            tooltip.clear(false);
            if mode != "OUT"
                && let Some(n) = value.and_then(|v| v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned())
            {
                let t = me.upgrade().expect("itemsTab");
                t.add_tooltip(tooltip, g, &Rc::new(mk(&n)), None, true);
            }
        });
        let (mk, me, b2, r2, ty2) = (make, self.me.clone(), base.clone(), rarity.clone(), ty.clone());
        let save = ButtonControl::new(None, rect(-45.0, 100.0, 80.0, 20.0), "Create".to_string(), move |g| {
            main().close_popup();
            let t = me.upgrade().expect("itemsTab");
            let sel = b2.sel_index.get();
            let name = b2.list.borrow()[sel - 1]
                .data
                .as_ref()
                .and_then(|d| d.downcast_ref::<String>())
                .cloned()
                .expect("base");
            let item = mk(&name);
            let open = !item.crafted && item.rarity != "NORMAL";
            t.set_display_item(g, Some(item));
            if open {
                t.edit_display_item_text(g, false);
            }
            t.last_craft.set((r2.sel_index.get(), ty2.sel_index.get(), sel));
        });
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert(
            "rarityLabel".into(),
            LabelControl::new(
                crate::control::anchor_nil("TOPRIGHT", "TOPLEFT"),
                rect(50.0, 20.0, 0.0, 16.0),
                "Rarity:".to_string(),
            ),
        );
        controls.insert("rarity".into(), rarity);
        controls.insert("title".into(), title);
        controls.insert(
            "typeLabel".into(),
            LabelControl::new(
                crate::control::anchor_nil("TOPRIGHT", "TOPLEFT"),
                rect(50.0, 45.0, 0.0, 16.0),
                "Type:".to_string(),
            ),
        );
        controls.insert("type".into(), ty);
        controls.insert(
            "baseLabel".into(),
            LabelControl::new(
                crate::control::anchor_nil("TOPRIGHT", "TOPLEFT"),
                rect(50.0, 70.0, 0.0, 16.0),
                "Base:".to_string(),
            ),
        );
        controls.insert("base".into(), base);
        controls.insert("save".into(), save);
        controls.insert(
            "cancel".into(),
            ButtonControl::new(None, rect(45.0, 100.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup()),
        );
        main().open_popup(g, 370.0, 130.0, "Craft Item", controls, None, None, None, None);
    }

    /// `ItemsTab:EditDisplayItemText(alsoAddItem)` (:2766-2818).
    pub(crate) fn edit_display_item_text(&self, g: &mut Gfx, also_add: bool) {
        let rarity = DropDownControl::new(g, None, rect(-190.0, 10.0, 100.0, 18.0), rarity_drop_list(), None, None, false);
        let edit = EditControl::new(
            g,
            None,
            rect(0.0, 40.0, 480.0, 420.0),
            EditArgs {
                init: Some(String::new()),
                filter: Some("^%C\t\n".to_string()),
                line_height: Some(14.0),
                ..EditArgs::default()
            },
        );
        let display = self.display_item.borrow().clone();
        if let Some(d) = &display {
            let text = pob_items::lua::gsub(&d.build_raw(), "Rarity: %w+\n", "", None);
            edit.set_text(g, &text, false);
            crate::modules::main::sel_by_value(&rarity, &d.rarity);
        } else {
            rarity.sel_index.set(3);
        }
        edit.font.set("FIXED");
        *edit.paste_filter.borrow_mut() = Some(Rc::new(|s: String| crate::common::sanitise_text(&s)));
        let (e1, r1) = (Rc::downgrade(&edit), Rc::downgrade(&rarity));
        let build_raw: Rc<dyn Fn() -> String> = Rc::new(move || {
            let e = e1.upgrade().expect("edit");
            let buf = e.buf.borrow().clone();
            let m = |p: &str| pob_lua::find(buf.as_bytes(), p.as_bytes(), None, false).ok().flatten().is_some();
            if m("^Item Class: .*\nRarity: ") || m("^Rarity: ") {
                buf
            } else {
                let r = r1.upgrade().expect("rarity");
                let rar = r.list.borrow()[r.sel_index.get() - 1]
                    .data
                    .as_ref()
                    .and_then(|d| d.downcast_ref::<String>())
                    .cloned()
                    .unwrap_or_default();
                format!("Rarity: {rar}\n{buf}")
            }
        });
        let (br, me, has) = (build_raw.clone(), self.me.clone(), display.is_some());
        let save = ButtonControl::new(
            None,
            rect(-45.0, 470.0, 80.0, 20.0),
            if has { "Save" } else { "Create" }.to_string(),
            move |g| {
                let t = me.upgrade().expect("itemsTab");
                let id = t.display_item.borrow().as_ref().and_then(|d| d.id);
                t.create_display_item_from_raw(g, &br(), !has);
                if t.display_item.borrow().is_some() {
                    t.edit_display(|it| it.id = id);
                }
                if also_add {
                    t.add_display_item(g, false);
                }
                main().close_popup();
            },
        );
        save.force_tooltip.set(true);
        let br = build_raw.clone();
        save.base
            .enabled
            .set_fn(move |_| Item::parse_with(&br(), None, false).base.is_some());
        let (br, me) = (build_raw, self.me.clone());
        save.tip.set_tooltip_func(move |tooltip, g, ()| {
            tooltip.clear(false);
            let item = Item::parse_with(&br(), None, false);
            if item.base.is_some() {
                let t = me.upgrade().expect("itemsTab");
                t.add_tooltip(tooltip, g, &Rc::new(item), None, true);
            } else {
                for l in [
                    "The item is invalid.",
                    "Check that the item's title and base name are in the correct format.",
                    "For Rare and Unique items, the first 2 lines must be the title and base name. E.g.:",
                    "Abberath's Horn",
                    "Goat's Horn",
                    "For Normal and Magic items, the base name must be somewhere in the first line. E.g.:",
                    "Scholar's Platinum Kris of Joy",
                ] {
                    tooltip.add_line(g, 14.0, Some(l));
                }
            }
        });
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert("rarity".into(), rarity);
        controls.insert("edit".into(), edit);
        controls.insert("save".into(), save);
        controls.insert(
            "cancel".into(),
            ButtonControl::new(None, rect(45.0, 470.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup()),
        );
        main().open_popup(
            g,
            500.0,
            500.0,
            if has { "Edit Item Text" } else { "Create Custom Item from Text" },
            controls,
            None,
            Some("edit"),
            None,
            None,
        );
    }

    pub(crate) fn enchant_display_item(&self, g: &mut Gfx, slot: usize) {
        self.enchant_display_item_popup(g, slot);
    }

    pub(crate) fn anoint_display_item(&self, g: &mut Gfx, slot: usize) {
        self.anoint_display_item_popup(g, slot);
    }

    pub(crate) fn corrupt_display_item(&self, g: &mut Gfx) {
        self.corrupt_display_item_popup(g);
    }

    pub(crate) fn add_custom_modifier(&self, g: &mut Gfx) {
        self.add_custom_modifier_popup(g);
    }

    /// `ItemsTab:AddCrucibleModifierToDisplayItem()` (:3930-4039).
    pub(crate) fn add_crucible_modifier(&self, g: &mut Gfx) {
        not_ported(g, "Classes/ItemsTab.lua:3930-4039 AddCrucibleModifierToDisplayItem");
    }
}

/// `rarityDropList` (:22-28).
fn rarity_drop_list() -> Vec<DropItem> {
    [
        ("NORMAL", "Normal"),
        ("MAGIC", "Magic"),
        ("RARE", "Rare"),
        ("UNIQUE", "Unique"),
        ("RELIC", "Relic"),
    ]
    .iter()
    .map(|(r, l)| {
        let mut it = DropItem::table(&format!("{}{l}", cc(r)));
        it.data = Some(Rc::new((*r).to_string()));
        it
    })
    .collect()
}

/// `makeItem(base)` of `CraftItem` (:2681-2725).
fn make_craft_item(name: &str, rarity_sel: usize, title_buf: &str) -> Item {
    let base = pob_data::item_bases::item_bases().bases.get(name).expect("base");
    let mut sel = rarity_sel;
    if (base.flask.is_some() || (base.ty == "Jewel" && base.sub_type.as_deref() == Some("Charm")) || base.ty == "Tincture") && sel == 3 {
        sel = 2;
    }
    let rarity = ["NORMAL", "MAGIC", "RARE", "UNIQUE", "RELIC"][sel - 1];
    let mut item = Item::new_crafted(name, base);
    if matches!(base.ty.as_str(), "Amulet" | "Belt" | "Jewel" | "Quiver" | "Ring" | "Graft") {
        item.quality = None;
    } else {
        item.quality = Some(0.0);
    }
    if sel == 2 || sel == 3 {
        item.crafted = true;
    }
    item.rarity = rarity.to_string();
    if sel >= 3 {
        item.title = Some(if title_buf.chars().any(|c| !c.is_whitespace()) {
            title_buf.to_string()
        } else {
            "New Item".to_string()
        });
    }
    if let Some(imp) = &base.implicit {
        for (i, line) in imp.split('\n').filter(|l| !l.is_empty()).enumerate() {
            item.implicit_mod_lines.push(ModLine {
                line: line.to_string(),
                mod_tags: base.implicit_mod_types.get(i).cloned().unwrap_or_default(),
                ..ModLine::default()
            });
        }
    }
    item.normalise_quality();
    item.build_and_parse_raw();
    item
}

/// `self.enchantments` (Item.lua:1044-1053): the table of `data.enchantments` for the base.
pub fn enchantments_of(it: &Item) -> Option<&'static serde_json::Value> {
    static E: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let base = it.base?;
    let all = E.get_or_init(|| pob_data::read_json("enchantments.json").unwrap_or_else(|e| panic!("{e}")));
    let key = if base.weapon.is_some() {
        "Weapon"
    } else if base.flask.is_some() {
        // Item.lua:1042-1046; `data.enchantments["Flask"] = data.enchantments["UtilityFlask"]` (Data.lua:695)
        "UtilityFlask"
    } else {
        base.ty.as_str()
    };
    all.get(key)
}

/// `itemLib.getClusterJewelNotableIndices(clusterJewel, jewelData, notableCount)` (ItemTools.lua:16-56).
fn cluster_notable_indices(c: &pob_data::cluster_jewels::ClusterJewelInfo, jd: &pob_items::DataTable, notable_count: usize) -> Vec<f64> {
    let socket_count = jd
        .num("clusterJewelSocketCountOverride")
        .or_else(|| jd.num("clusterJewelSocketCount"))
        .unwrap_or(0.0);
    let node_count = jd
        .num("clusterJewelNodeCount")
        .unwrap_or(socket_count + notable_count as f64 + jd.num("clusterJewelNothingnessCount").unwrap_or(0.0));
    let mut sockets: Vec<f64> = Vec::new();
    if c.size == "Large" && socket_count == 1.0 {
        sockets.push(6.0);
    } else {
        sockets.extend(c.socket_indicies.iter().take(socket_count as usize));
    }
    let mut out: Vec<f64> = Vec::new();
    for idx in &c.notable_indicies {
        if out.len() == notable_count {
            break;
        }
        let mut n = *idx;
        if c.size == "Medium" {
            if socket_count == 0.0 && notable_count == 2 {
                if n == 6.0 {
                    n = 4.0;
                } else if n == 10.0 {
                    n = 8.0;
                }
            } else if node_count == 4.0 {
                if n == 10.0 {
                    n = 9.0;
                } else if n == 2.0 {
                    n = 3.0;
                }
            }
        }
        if !sockets.contains(&n) {
            out.push(n);
        }
    }
    out.sort_by(f64::total_cmp);
    out
}

/// `itemLib.getClusterJewelNotablePositions(item, sortOrder)` (ItemTools.lua:58-81).
fn notable_positions(item: &Item, tab: &ItemsTab) -> Vec<(String, String)> {
    let (Some(c), Some(jd)) = (item.cluster_jewel, &item.jewel_data) else {
        return Vec::new();
    };
    if jd.get("clusterJewelKeystone").truthy() {
        return Vec::new();
    }
    let names = pob_items::item::value_strings(&jd.get("clusterJewelNotables"));
    let _ = tab;
    let order = &pob_data::cluster_jewels::cluster_jewels().notable_sort_order;
    if names.iter().any(|n| order.get(n).is_none()) {
        return Vec::new();
    }
    let mut names = names;
    names.sort_by(|a, b| {
        order
            .get(a)
            .copied()
            .unwrap_or(0.0)
            .total_cmp(&order.get(b).copied().unwrap_or(0.0))
    });
    cluster_notable_indices(c, jd, names.len())
        .into_iter()
        .zip(names)
        .map(|(idx, name)| {
            let position = if c.size == "Small" {
                "End"
            } else if idx > c.total_indicies / 4.0 && idx < c.total_indicies * 3.0 / 4.0 {
                "Back"
            } else {
                "Front"
            };
            (position.to_string(), name)
        })
        .collect()
}

#[allow(dead_code)]
fn unused_marks(_: &mut Frame<'_>, _: &Rect, _: &InputEvents, _: &Tooltip) {}
