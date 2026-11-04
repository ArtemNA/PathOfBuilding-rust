//! `Classes/Tooltip.lua` and `Classes/TooltipHost.lua`.
//!
//! Item/passive headers (`tooltipHeader`, Tooltip.lua:386-392, :428-432, :480-574), header separators (:131-152), oil
//! recipes (:116-118, :175-189, :236-282) and line background images (:609-636) need `Assets/`/`TreeData/` images and
//! are only used by item, gem and tree tooltips (U3/U5/U6): those branches are `unimplemented!` with their lines.

use crate::common::{not_supported_mod_tooltips, show_flavour_text, wrap_string};
use crate::control::{PropCell, Rect};
use crate::sg;
use pob_gfx::{Frame, Gfx, ImageHandle};
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const BORDER_WIDTH: f64 = 1.0;
const H_PAD: f64 = 12.0;
const V_PAD: f64 = 10.0;

/// `self.color`: `{ r, g, b }` or an escape string.
#[derive(Clone, Debug, PartialEq)]
pub enum TooltipColor {
    Rgb(f64, f64, f64),
    Escape(String),
}

#[derive(Clone, Debug, Default)]
pub struct TooltipLine {
    pub size: f64,
    pub text: Option<String>,
    pub block: usize,
    pub font: &'static str,
    /// `center`: `nil` = follow `tooltip.center`.
    pub center: Option<bool>,
    /// `modLine and modLine.disabled`.
    pub mod_line_disabled: bool,
    /// `background` (a path).
    pub background: Option<String>,
    /// `line.modLine`: which line of the item (the list, the index), for the click that toggles it (ItemsTab.lua:1584-1600).
    pub mod_line: Option<(u8, usize)>,
    /// `bounds = { x, y, width, height }` (set by `CalculateColumns`).
    pub bounds: Option<Rect>,
    /// `separatorImage`.
    pub separator_image: Option<ImageHandle>,
}

/// One `drawStack` entry (Tooltip.lua:226-370): text lines have 6 fields, rectangles 5.
#[derive(Clone, Debug)]
enum DrawEntry {
    /// `{ { handle = ..., isSeparator = ... }, x, y, w, h }`.
    Image {
        handle: ImageHandle,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        is_separator: bool,
    },
    Text {
        x: f64,
        y: f64,
        align: &'static str,
        size: f64,
        font: &'static str,
        text: String,
        strikethrough: bool,
        background: Option<String>,
    },
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
}

pub struct Tooltip {
    pub lines: Vec<TooltipLine>,
    /// `blocks[i].height`.
    pub blocks: Vec<f64>,
    update_params: Option<(Box<dyn Any>, bool)>,
    pub tooltip_header: Option<String>,
    pub title_y_offset: f64,
    pub center: bool,
    pub max_width: Option<f64>,
    pub color: TooltipColor,
    pub child_tooltips: Option<Vec<Tooltip>>,
    /// `self.recipe`: `SetRecipe` (a notable's oils).
    pub recipe: Option<Vec<String>>,
    /// `self.foilType` (ItemsTab.lua:4281).
    pub foil_type: Option<String>,
    /// `self.influenceHeader1`, `self.influenceHeader2` (ItemsTab.lua:4053-4122).
    pub influence_header: [Option<String>; 2],
}

/// `headerInfluence` (Tooltip.lua:60-76).
fn header_influence(name: &str) -> String {
    format!(
        "Assets/{}icon.png",
        if name == "Foulborn" {
            "breach".to_string()
        } else {
            name.to_lowercase()
        }
    )
}

/// `foilTypes` (Tooltip.lua:502-516).
fn foil_color(name: Option<&str>) -> (f64, f64, f64) {
    match name {
        Some("Amethyst") => (0.9, 0.6, 1.0),
        Some("Verdant") => (0.5, 1.0, 0.5),
        Some("Ruby") => (1.0, 0.5, 0.6),
        Some("Cobalt") => (0.6, 0.7, 1.0),
        Some("Sunset") => (1.0, 1.0, 0.6),
        Some("Aureate") => (1.0, 0.85, 0.2),
        Some("Celestial Quartz") => (1.0, 0.7, 0.85),
        Some("Celestial Ruby") => (0.8, 0.3, 0.2),
        Some("Celestial Emerald") => (0.2, 0.6, 0.3),
        Some("Celestial Aureate") => (0.8, 0.7, 0.2),
        Some("Celestial Pearl") => (1.0, 0.85, 0.9),
        Some("Celestial Amethyst") => (0.5, 0.6, 1.0),
        _ => (0.6, 1.0, 0.5),
    }
}

/// `headerConfigs[rarity]` (Tooltip.lua:45-59).
struct HeaderConfig {
    left: &'static str,
    middle: &'static str,
    right: &'static str,
    height: f64,
    side_width: f64,
    middle_width: f64,
    text_y_offset: f64,
}

fn header_config_of(rarity: &str) -> HeaderConfig {
    let c = |left, middle, right, height, side_width, middle_width, text_y_offset| HeaderConfig {
        left,
        middle,
        right,
        height,
        side_width,
        middle_width,
        text_y_offset,
    };
    match rarity {
        "RELIC" => c(
            "Assets/itemsheaderfoilleft.png",
            "Assets/itemsheaderfoilmiddle.png",
            "Assets/itemsheaderfoilright.png",
            54.0,
            47.0,
            52.0,
            3.0,
        ),
        "UNIQUE" => c(
            "Assets/itemsheaderuniqueleft.png",
            "Assets/itemsheaderuniquemiddle.png",
            "Assets/itemsheaderuniqueright.png",
            54.0,
            47.0,
            52.0,
            3.0,
        ),
        "RARE" => c(
            "Assets/itemsheaderrareleft.png",
            "Assets/itemsheaderraremiddle.png",
            "Assets/itemsheaderrareright.png",
            54.0,
            47.0,
            52.0,
            3.0,
        ),
        "MAGIC" => c(
            "Assets/itemsheadermagicleft.png",
            "Assets/itemsheadermagicmiddle.png",
            "Assets/itemsheadermagicright.png",
            38.0,
            32.0,
            32.0,
            6.0,
        ),
        "GEM" => c(
            "Assets/itemsheadergemleft.png",
            "Assets/itemsheadergemmiddle.png",
            "Assets/itemsheadergemright.png",
            38.0,
            33.0,
            38.0,
            5.0,
        ),
        "JEWEL" => c(
            "Assets/jewelpassiveheaderleft.png",
            "Assets/jewelpassiveheadermiddle.png",
            "Assets/jewelpassiveheaderright.png",
            38.0,
            33.0,
            38.0,
            6.0,
        ),
        "NOTABLE" => c(
            "Assets/notablepassiveheaderleft.png",
            "Assets/notablepassiveheadermiddle.png",
            "Assets/notablepassiveheaderright.png",
            38.0,
            38.0,
            38.0,
            6.0,
        ),
        "PASSIVE" => c(
            "Assets/normalpassiveheaderleft.png",
            "Assets/normalpassiveheadermiddle.png",
            "Assets/normalpassiveheaderright.png",
            38.0,
            33.0,
            38.0,
            6.0,
        ),
        "KEYSTONE" => c(
            "Assets/keystonepassiveheaderleft.png",
            "Assets/keystonepassiveheadermiddle.png",
            "Assets/keystonepassiveheaderright.png",
            38.0,
            33.0,
            38.0,
            6.0,
        ),
        "ASCENDANCY" => c(
            "Assets/ascendancypassiveheaderleft.png",
            "Assets/ascendancypassiveheadermiddle.png",
            "Assets/ascendancypassiveheaderright.png",
            38.0,
            33.0,
            38.0,
            6.0,
        ),
        "MASTERY" => c(
            "Assets/masteryheaderunallocatedleft.png",
            "Assets/masteryheaderunallocatedmiddle.png",
            "Assets/masteryheaderunallocatedright.png",
            38.0,
            33.0,
            38.0,
            6.0,
        ),
        "MASTERYALLOC" => c(
            "Assets/masteryheaderallocatedleft.png",
            "Assets/masteryheaderallocatedmiddle.png",
            "Assets/masteryheaderallocatedright.png",
            38.0,
            33.0,
            38.0,
            6.0,
        ),
        _ => c(
            "Assets/itemsheaderwhiteleft.png",
            "Assets/itemsheaderwhitemiddle.png",
            "Assets/itemsheaderwhiteright.png",
            38.0,
            32.0,
            32.0,
            6.0,
        ),
    }
}

thread_local! {
    /// `tooltipAssetCache` (Tooltip.lua:88-100) + `recipeImages` (:37-41).
    static IMAGES: RefCell<HashMap<String, ImageHandle>> = RefCell::new(HashMap::new());
}

fn image_of(path: &str, flags: &[pob_gfx::LoadFlag]) -> ImageHandle {
    IMAGES.with_borrow_mut(|m| {
        m.entry(path.to_string())
            .or_insert_with(|| {
                let h = ImageHandle::new();
                let root = crate::launch::launch().cfg.script_path.clone();
                let _ = h.load(crate::src_cache::resolve(&root, path), flags);
                h
            })
            .clone()
    })
}

/// `getCachedImage(path)` (`image:Load(path)`).
fn cached_image(path: &str) -> ImageHandle {
    image_of(path, &[])
}

fn cached_clamp_image(path: &str) -> ImageHandle {
    image_of(path, &[pob_gfx::LoadFlag::Clamp])
}

/// A recipe name without its `"Oil"`.
fn recipe_short(name: &str) -> &str {
    if name.len() > 3 && name.ends_with("Oil") {
        &name[..name.len() - 3]
    } else {
        name
    }
}

impl Default for Tooltip {
    fn default() -> Self {
        Self::new()
    }
}

impl Tooltip {
    /// `Tooltip:Tooltip()` (Tooltip.lua:46-52).
    pub fn new() -> Self {
        let mut t = Self {
            lines: Vec::new(),
            blocks: Vec::new(),
            update_params: None,
            tooltip_header: None,
            title_y_offset: 0.0,
            center: false,
            max_width: None,
            color: TooltipColor::Rgb(0.5, 0.3, 0.0),
            child_tooltips: None,
            recipe: None,
            foil_type: None,
            influence_header: [None, None],
        };
        t.clear(false);
        t
    }

    /// `Tooltip:Clear(clearUpdateParams)` (Tooltip.lua:54-69).
    pub fn clear(&mut self, clear_update_params: bool) {
        self.lines.clear();
        self.blocks.clear();
        if clear_update_params {
            self.update_params = None;
        }
        self.tooltip_header = None;
        self.title_y_offset = 0.0;
        self.recipe = None;
        self.center = false;
        self.max_width = None;
        self.color = TooltipColor::Rgb(0.5, 0.3, 0.0);
        self.blocks.push(0.0);
    }

    /// `Tooltip:CheckForUpdate(...)` (Tooltip.lua:71-89): `params` is the tuple of arguments (compared with `==`).
    pub fn check_for_update<P: PartialEq + 'static>(&mut self, params: P) -> bool {
        let flag = not_supported_mod_tooltips();
        let same = self
            .update_params
            .as_ref()
            .is_some_and(|(p, f)| *f == flag && p.downcast_ref::<P>() == Some(&params));
        if same {
            return false;
        }
        self.update_params = Some((Box::new(params), flag));
        self.clear(false);
        true
    }

    /// `Tooltip:AddLine(size, text, font, modLine, background)` (Tooltip.lua:91-114).
    pub fn add_line(&mut self, g: &Gfx, size: f64, text: Option<&str>) {
        self.add_line_ex(g, size, text, None, false, None);
    }

    /// `AddLine(size, text, font, modLine)`: the lines it adds know their `modLine`.
    pub fn add_mod_line(&mut self, g: &Gfx, size: f64, text: &str, font: Option<&'static str>, disabled: bool, id: (u8, usize)) {
        let from = self.lines.len();
        self.add_line_ex(g, size, Some(text), font, disabled, None);
        for l in &mut self.lines[from..] {
            l.mod_line = Some(id);
        }
    }

    pub fn add_line_ex(
        &mut self,
        g: &Gfx,
        size: f64,
        text: Option<&str>,
        font: Option<&'static str>,
        mod_line_disabled: bool,
        background: Option<&str>,
    ) {
        let Some(text) = text else { return };
        let font_to_use = if show_flavour_text() { font.unwrap_or("VAR") } else { "VAR" };
        // `for line in s_gmatch(text .. "\n", "([^\n]*)\n")`
        for line in text.split('\n') {
            if line_matches_tail(line, "Equipping") || line_matches_tail(line, "Removing") {
                self.blocks.push(size + 2.0);
            } else {
                *self.blocks.last_mut().expect("blocks") += size + 2.0;
            }
            let mk = |t: String| TooltipLine {
                size,
                text: Some(t),
                block: self.blocks.len(),
                font: font_to_use,
                center: Some(self.center),
                mod_line_disabled,
                background: background.map(str::to_string),
                bounds: None,
                separator_image: None,
                mod_line: None,
            };
            if let Some(max_width) = self.max_width {
                for wrapped in wrap_string(g, line, size, max_width - H_PAD) {
                    let l = mk(wrapped);
                    self.lines.push(l);
                }
            } else {
                let l = mk(line.to_string());
                self.lines.push(l);
            }
        }
    }

    /// `Tooltip:AddSeparator(size)` (Tooltip.lua:120-160).
    pub fn add_separator(&mut self, size: Option<f64>) {
        let size = size.unwrap_or(10.0);
        // :183-186 no back-to-back separator images
        if self.lines.last().is_some_and(|l| l.separator_image.is_some()) {
            return;
        }
        let separator_image = self.tooltip_header.as_ref().map(|h| {
            let path = match h.to_uppercase().as_str() {
                "RELIC" => "Assets/itemsseparatorfoil.png",
                "UNIQUE" => "Assets/itemsseparatorunique.png",
                "RARE" => "Assets/itemsseparatorrare.png",
                "MAGIC" => "Assets/itemsseparatormagic.png",
                "GEM" => "Assets/itemsseparatorgem.png",
                _ => "Assets/itemsseparatorwhite.png",
            };
            cached_image(path)
        });
        let last_block = self.lines.last().map_or(1, |l| l.block);
        self.lines.push(TooltipLine {
            size,
            text: None,
            block: last_block,
            font: "VAR",
            separator_image,
            ..TooltipLine::default()
        });
    }

    /// `Tooltip:GetSize()` (Tooltip.lua:163-192).
    pub fn get_size(&self, g: &Gfx) -> (f64, f64) {
        let (mut tt_w, mut tt_h) = (0.0f64, 0.0);
        for (i, data) in self.lines.iter().enumerate() {
            let prev = i.checked_sub(1).and_then(|k| self.lines.get(k));
            let next = self.lines.get(i + 1);
            if data.text.is_some() || (prev.is_some() && next.is_some_and(|n| n.text.is_some())) {
                tt_h += data.size + 2.0;
            }
            if let Some(t) = &data.text {
                tt_w = tt_w.max(sg::draw_string_width(g, data.size, data.font, t));
            }
        }
        // :216-230 recipe display
        if let (Some(recipe), Some(title)) = (&self.recipe, self.lines.first()) {
            let font = if show_flavour_text() { "FONTIN SC" } else { "VAR" };
            let mut image_x = sg::draw_string_width(g, title.size, font, title.text.as_deref().unwrap_or("")) + title.size;
            let recipe_text_size = (title.size * 3.0) / 4.0;
            for name in recipe {
                let short = recipe_short(name);
                image_x += sg::draw_string_width(g, recipe_text_size, font, short) + title.size * 1.25;
            }
            tt_w = tt_w.max(image_x);
        }
        (tt_w + H_PAD, tt_h + V_PAD)
    }

    /// `Tooltip:GetDynamicSize(viewPort)` (Tooltip.lua:194-203).
    pub fn get_dynamic_size(&mut self, g: &Gfx, view_port: &Rect) -> (f64, f64) {
        let (static_w, static_h) = self.get_size(g);
        let (columns, tt_h, _, extra) = self.calculate_columns(g, 0.0, 0.0, static_h, static_w, view_port);
        let extra = if columns > 1 && extra > 0.0 { extra } else { static_w };
        let tt_w = static_w + (columns as f64 - 1.0).max(0.0) * extra;
        (tt_w + H_PAD, tt_h + V_PAD)
    }

    /// `Tooltip:CalculateColumns(ttY, ttX, ttH, ttW, viewPort)` (Tooltip.lua:216-372).
    fn calculate_columns(
        &mut self,
        g: &Gfx,
        tt_y: f64,
        tt_x: f64,
        tt_h: f64,
        tt_w: f64,
        view_port: &Rect,
    ) -> (usize, f64, Vec<DrawEntry>, f64) {
        let mut y = tt_y + 2.0 * BORDER_WIDTH + self.title_y_offset;
        let mut x = tt_x;
        let mut columns = 1usize;
        let mut current_block = 1usize;
        let mut extra_column_width = 0.0f64;
        let mut max_column_height = 0.0f64;
        let mut draw_stack = Vec::new();
        let flavour = show_flavour_text();
        for i in 0..self.lines.len() {
            let font = if flavour { self.lines[i].font } else { "VAR" };
            // :277-323 the first line with its oils
            let mut handled = false;
            if let (Some(recipe), 0, Some(title_text)) = (&self.recipe, i, self.lines[i].text.clone()) {
                let title_size = self.lines[i].size;
                let recipe_text_size = (title_size * 3.0 / 4.0).floor();
                let padding = 4.0;
                let mut total_width = sg::draw_string_width(g, title_size, font, &title_text);
                let mut oils = Vec::new();
                for r in recipe {
                    let rn = recipe_short(r);
                    let text_w = sg::draw_string_width(g, recipe_text_size, font, rn);
                    oils.push((rn.to_string(), r.clone(), text_w, title_size));
                    total_width += text_w + title_size + padding;
                }
                let mut cur_x = tt_x + tt_w / 2.0 - total_width / 2.0;
                draw_stack.push(DrawEntry::Text {
                    x: cur_x,
                    y,
                    align: "LEFT",
                    size: title_size,
                    font,
                    text: title_text.clone(),
                    strikethrough: false,
                    background: None,
                });
                cur_x += sg::draw_string_width(g, title_size, font, &title_text) + H_PAD / 2.0;
                for (rn, name, text_w, icon_w) in oils {
                    let rn = if flavour { format!("^xF8E6CA{rn}") } else { rn };
                    draw_stack.push(DrawEntry::Text {
                        x: cur_x,
                        y: y + (title_size - recipe_text_size) / 2.0,
                        align: "LEFT",
                        size: recipe_text_size,
                        font,
                        text: rn,
                        strikethrough: false,
                        background: None,
                    });
                    cur_x += text_w;
                    draw_stack.push(DrawEntry::Image {
                        handle: cached_clamp_image(&format!("TreeData/{name}.png")),
                        x: cur_x,
                        y,
                        w: icon_w,
                        h: icon_w,
                        is_separator: false,
                    });
                    cur_x += icon_w + padding;
                }
                y += title_size.max(0.0) + 2.0;
                handled = true;
            }
            let data = &self.lines[i];
            if handled {
                // the line is printed above
            } else if let Some(text) = data.text.clone() {
                let size = data.size;
                if current_block != data.block && self.blocks[data.block - 1] + y > tt_y + tt_h.min(view_port.height) {
                    y = tt_y + 2.0 * BORDER_WIDTH;
                    x = tt_x + tt_w * columns as f64;
                    columns += 1;
                }
                current_block = data.block;
                let line_centered = data.center.unwrap_or(self.center);
                let line_x = if line_centered { x + tt_w / 2.0 } else { x + H_PAD / 2.0 };
                let line_align = if line_centered { "CENTER_X" } else { "LEFT" };
                draw_stack.push(DrawEntry::Text {
                    x: line_x,
                    y,
                    align: line_align,
                    size,
                    font,
                    text: text.clone(),
                    strikethrough: data.mod_line_disabled,
                    background: data.background.clone(),
                });
                self.lines[i].bounds = Some(Rect::new(x + H_PAD / 2.0, y, tt_w - H_PAD, size + 2.0));
                y += size + 2.0;
                if columns > 1 {
                    extra_column_width = extra_column_width.max(sg::draw_string_width(g, size, font, &text) + H_PAD);
                }
            } else if let (Some(img), true) = (&data.separator_image, flavour) {
                // :356-365
                let sep_size = data.size;
                if current_block != data.block && y + sep_size > tt_y + tt_h.min(view_port.height) {
                    y = tt_y + 2.0 * BORDER_WIDTH;
                    x = tt_x + tt_w * columns as f64;
                    columns += 1;
                }
                current_block = data.block;
                draw_stack.push(DrawEntry::Image {
                    handle: img.clone(),
                    x: x + H_PAD / 2.0,
                    y,
                    w: tt_w - H_PAD,
                    h: sep_size,
                    is_separator: true,
                });
                y += sep_size + 2.0;
            } else if i >= 1 && self.lines.get(i + 1).is_some_and(|n| n.text.is_some()) {
                draw_stack.push(DrawEntry::Rect {
                    x,
                    y: y - 1.0 + data.size / 2.0,
                    w: tt_w - BORDER_WIDTH,
                    h: 2.0,
                });
                y += data.size + 2.0;
            }
            max_column_height = max_column_height.max(y - tt_y + 2.0 * BORDER_WIDTH);
        }
        if columns > 1 && extra_column_width > 0.0 {
            for line in &mut draw_stack {
                let orig_x = match line {
                    DrawEntry::Text { x, .. } | DrawEntry::Rect { x, .. } | DrawEntry::Image { x, .. } => *x,
                };
                let col_index = ((orig_x - tt_x) / tt_w).floor() + 1.0;
                if col_index > 1.0 {
                    let old_base_x = tt_x + tt_w * (col_index - 1.0);
                    let new_base_x = tt_x + tt_w + extra_column_width * (col_index - 2.0);
                    match line {
                        DrawEntry::Text { x, align, .. } => {
                            *x = if *align == "CENTER_X" {
                                new_base_x + extra_column_width / 2.0
                            } else {
                                orig_x - old_base_x + new_base_x
                            };
                        }
                        DrawEntry::Rect { x, w, .. } => {
                            *x = orig_x - old_base_x + new_base_x;
                            *w = extra_column_width - BORDER_WIDTH;
                        }
                        DrawEntry::Image { x, w, is_separator, .. } => {
                            *x = orig_x - old_base_x + new_base_x;
                            *w = extra_column_width - if *is_separator { H_PAD } else { BORDER_WIDTH };
                        }
                    }
                }
            }
        }
        (columns, max_column_height, draw_stack, extra_column_width)
    }

    fn set_color(&self, f: &mut Frame<'_>) {
        match &self.color {
            TooltipColor::Escape(s) => sg::set_draw_color_str(f, s),
            TooltipColor::Rgb(r, g, b) => sg::set_draw_color(f, *r, *g, *b),
        }
    }

    /// `Tooltip:Draw(x, y, w, h, viewPort)` (Tooltip.lua:379-698).
    pub fn draw(&mut self, f: &mut Frame<'_>, x: f64, y: f64, wh: Option<(f64, f64)>, view_port: &Rect) -> Option<(f64, f64)> {
        if self.lines.is_empty() {
            return None;
        }
        let (mut tt_w, tt_h) = self.get_size(f);
        // :427-439 the header: a minimum width, the title's offset
        let flavour = show_flavour_text();
        let mut header_config = None;
        if let Some(h) = self.tooltip_header.clone().filter(|_| self.lines[0].text.is_some()) {
            let font = if flavour { "FONTIN SC" } else { "VAR" };
            let title_w = sg::draw_string_width(f, self.lines[0].size, font, self.lines[0].text.as_deref().unwrap_or(""));
            if title_w + 50.0 > tt_w {
                tt_w = title_w + 50.0;
            }
            if flavour {
                let c = header_config_of(&h.to_uppercase());
                self.title_y_offset = c.text_y_offset;
                header_config = Some(c);
            }
        }
        let mut tt_x = x;
        let mut tt_y = y;
        if let Some((w, h)) = wh {
            tt_x = tt_x + w + 5.0;
            if tt_x + tt_w > view_port.x + view_port.width {
                tt_x = view_port.x.max(x - 5.0 - tt_w);
                if tt_x + tt_w > x {
                    tt_y += h;
                }
            }
            if tt_y + tt_h > view_port.y + view_port.height {
                tt_y = view_port.y.max(y + h - tt_h);
            }
        }
        let (columns, max_column_height, mut draw_stack, extra) = self.calculate_columns(f, tt_y, tt_x, tt_h, tt_w, view_port);
        let extra_column_width = if columns > 1 && extra > 0.0 { extra } else { tt_w };
        let total_draw_width = tt_w + (columns as f64 - 1.0).max(0.0) * extra_column_width;
        if columns > 1 && wh.is_some() && total_draw_width + tt_x >= view_port.x + view_port.width {
            let new_x = view_port.x.max(view_port.x + view_port.width - total_draw_width);
            let offset_x = new_x - tt_x;
            tt_x = new_x;
            // Tooltip.lua:461-469: index [2] of 5-field entries `{nil, x, y, w, h}` and [1] of text entries — both `x`.
            for line in &mut draw_stack {
                match line {
                    DrawEntry::Rect { x, .. } | DrawEntry::Text { x, .. } | DrawEntry::Image { x, .. } => *x += offset_x,
                }
            }
        }
        sg::set_draw_color_a(f, 0.0, 0.0, 0.0, 0.85);
        sg::draw_rect(
            f,
            tt_x,
            tt_y + BORDER_WIDTH,
            total_draw_width - BORDER_WIDTH,
            max_column_height - 2.0 * BORDER_WIDTH,
        );
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        // :487-562 the header (RELIC foil colours and influence icons: U6)
        if let Some(c) = &header_config {
            let header_x = tt_x + BORDER_WIDTH;
            let header_y = tt_y + BORDER_WIDTH;
            let header_total_width = tt_w - 2.0 * BORDER_WIDTH;
            let middle_area = (header_total_width - 2.0 * c.side_width).max(0.0);
            let relic = self.tooltip_header.as_deref() == Some("RELIC");
            let foil = foil_color(self.foil_type.as_deref());
            let allow_icon = c.left.starts_with("Assets/itemsheader") && !c.left.contains("gem");
            if relic {
                sg::set_draw_color(f, foil.0, foil.1, foil.2);
            }
            sg::draw_image(f, Some(&cached_image(c.left)), header_x, header_y, c.side_width, c.height, None);
            if let (Some(h), true) = (&self.influence_header[0], allow_icon) {
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
                sg::draw_image(
                    f,
                    Some(&cached_image(&header_influence(h))),
                    header_x + 2.0,
                    header_y + (c.height - c.height / 2.0) / 2.0,
                    c.height / 2.0,
                    c.height / 2.0,
                    None,
                );
            }
            if relic {
                sg::set_draw_color(f, foil.0, foil.1, foil.2);
            }
            if middle_area > 0.0 {
                let mut draw_x = header_x + c.side_width;
                let end_x = header_x + header_total_width - c.side_width;
                let middle = cached_image(c.middle);
                while draw_x + c.middle_width <= end_x {
                    sg::draw_image(f, Some(&middle), draw_x, header_y, c.middle_width, c.height, None);
                    draw_x += c.middle_width;
                }
                let remaining = end_x - draw_x;
                if remaining > 0.0 {
                    sg::draw_image(f, Some(&middle), draw_x, header_y, remaining, c.height, None);
                }
            }
            sg::draw_image(
                f,
                Some(&cached_image(c.right)),
                header_x + header_total_width - c.side_width,
                header_y,
                c.side_width,
                c.height,
                None,
            );
            if let (Some(h), true) = (&self.influence_header[1], allow_icon) {
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
                sg::draw_image(
                    f,
                    Some(&cached_image(&header_influence(h))),
                    header_x + header_total_width - c.height / 2.0 - 2.0,
                    header_y + (c.height - c.height / 2.0) / 2.0,
                    c.height / 2.0,
                    c.height / 2.0,
                    None,
                );
            }
        }
        let mut first_separator_skipped = false;
        let header_type = self.tooltip_header.as_deref().map(str::to_uppercase).unwrap_or_default();
        for line in &draw_stack {
            match line {
                DrawEntry::Rect { x, y, w, h } => {
                    self.set_color(f);
                    sg::draw_rect(f, *x, *y, *w, *h);
                }
                DrawEntry::Image {
                    handle,
                    x,
                    y,
                    w,
                    h,
                    is_separator,
                } => {
                    let skip = *is_separator
                        && flavour
                        && !first_separator_skipped
                        && matches!(header_type.as_str(), "RELIC" | "UNIQUE" | "RARE" | "MAGIC" | "GEM");
                    if skip {
                        first_separator_skipped = true;
                    } else {
                        sg::set_draw_color(f, 1.0, 1.0, 1.0);
                        sg::draw_image(f, Some(handle), *x, *y, *w, *h, None);
                    }
                }
                DrawEntry::Text {
                    x,
                    y,
                    align,
                    size,
                    font,
                    text,
                    strikethrough,
                    background,
                } => {
                    if let (Some(bg), true) = (background, flavour) {
                        // :598-624 the colour survives the image
                        let prev = f.get_draw_color();
                        sg::set_draw_color_a(f, 1.0, 1.0, 1.0, 1.0);
                        sg::draw_image(f, Some(&cached_image(bg)), tt_x + 4.0, *y - 5.0, tt_w - 8.0, *size + 10.0, None);
                        sg::set_draw_color_a(f, f64::from(prev[0]), f64::from(prev[1]), f64::from(prev[2]), f64::from(prev[3]));
                    }
                    sg::draw_string(f, *x, *y, Some(align), *size, font, text);
                    if *strikethrough {
                        let text_w = sg::draw_string_width(f, *size, font, text);
                        let strike_x = if *align == "CENTER_X" { x - text_w / 2.0 } else { *x };
                        let strike_y = y + size / 2.0;
                        sg::set_draw_color_a(f, 0.75, 0.75, 0.75, 0.35);
                        sg::draw_rect(f, strike_x, strike_y, text_w, 1.0);
                    }
                }
            }
        }
        self.set_color(f);
        for i in 0..=columns {
            let fi = i as f64;
            let extra_col_x_offset = if i > 0 { tt_w + (fi - 1.0) * extra_column_width } else { 0.0 };
            let current_x = tt_x + extra_col_x_offset;
            sg::draw_rect(
                f,
                current_x - BORDER_WIDTH * (fi * fi / (fi * fi + 1.0)).ceil(),
                tt_y,
                BORDER_WIDTH,
                max_column_height,
            );
        }
        sg::draw_rect(f, tt_x, tt_y, total_draw_width, BORDER_WIDTH);
        sg::draw_rect(f, tt_x, tt_y + max_column_height - BORDER_WIDTH, total_draw_width, BORDER_WIDTH);
        if let Some(mut children) = self.child_tooltips.take() {
            let total_h: f64 = children.iter_mut().map(|tt| tt.get_dynamic_size(f, view_port).1).sum();
            let y_offset = (view_port.height - total_h - tt_y).min(0.0);
            let mut y_pos = (tt_y + y_offset).max(view_port.y);
            for tt in &mut children {
                let (child_w, child_h) = tt.get_size(f);
                let furthest_allowed_x = view_port.width + view_port.x - child_w;
                tt.draw(f, (tt_x + tt_w + 4.0).min(furthest_allowed_x), y_pos, None, view_port);
                y_pos += child_h + 6.0;
            }
            self.child_tooltips = Some(children);
        }
        Some((tt_w, tt_h))
    }
}

/// `line:match("^.*(Equipping)") == "Equipping"`: the line contains the word.
fn line_matches_tail(line: &str, word: &str) -> bool {
    line.contains(word)
}

/// `Classes/TooltipHost.lua`: `A` = the extra arguments the host passes to `tooltipFunc` (`...`).
pub struct TooltipHost<A = ()> {
    pub tooltip: RefCell<Tooltip>,
    pub tooltip_text: PropCell<Option<String>>,
    #[allow(clippy::type_complexity)]
    pub tooltip_func: RefCell<Option<Rc<dyn Fn(&mut Tooltip, &Gfx, A)>>>,
    #[allow(clippy::type_complexity)]
    pub draw_hook: RefCell<Option<Rc<dyn Fn(&Tooltip, &Gfx, f64, f64) -> (f64, f64)>>>,
}

impl<A> TooltipHost<A> {
    /// `TooltipHost:TooltipHost(tooltipText)` (TooltipHost.lua:9-13).
    pub fn new(tooltip_text: Option<String>) -> Self {
        Self {
            tooltip: RefCell::new(Tooltip::new()),
            tooltip_text: PropCell::new(tooltip_text),
            tooltip_func: RefCell::new(None),
            draw_hook: RefCell::new(None),
        }
    }

    /// `tooltip.Draw = function(self, x, y, …)` (TreeTab.lua:1748): the position the tooltip's real `Draw` gets.
    pub fn set_draw_hook(&self, f: impl Fn(&Tooltip, &Gfx, f64, f64) -> (f64, f64) + 'static) {
        *self.draw_hook.borrow_mut() = Some(Rc::new(f));
    }

    /// `self.tooltipFunc = function(tooltip, …) … end`.
    pub fn set_tooltip_func(&self, f: impl Fn(&mut Tooltip, &Gfx, A) + 'static) {
        *self.tooltip_func.borrow_mut() = Some(Rc::new(f));
    }

    /// `TooltipHost:DrawTooltip(x, y, width, height, viewPort, ...)` (TooltipHost.lua:15-27).
    pub fn draw_tooltip(&self, f: &mut Frame<'_>, x: f64, y: f64, width: f64, height: f64, view_port: &Rect, args: A) {
        let func = self.tooltip_func.borrow().clone();
        if let Some(func) = func {
            func(&mut self.tooltip.borrow_mut(), f, args);
            let hook = self.draw_hook.borrow().clone();
            let (x, y) = hook.map_or((x, y), |h| h(&self.tooltip.borrow(), f, x, y));
            self.tooltip.borrow_mut().draw(f, x, y, Some((width, height)), view_port);
        } else if let Some(text) = self.tooltip_text.get(f) {
            let mut tt = self.tooltip.borrow_mut();
            tt.clear(false);
            tt.add_line(f, 14.0, Some(&text));
            tt.draw(f, x, y, Some((width, height)), view_port);
        }
    }
}
