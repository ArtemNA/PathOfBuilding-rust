//! The name popups of the 2.69.0 set lists (`CreateItemSet` / `CopyItemSet` / `RenameItemSet` of ItemSetListControl.lua:39-91, the same of
//! SkillSetListControl.lua:46-99 and ConfigSetListControl.lua:38-90) and `BuildSetListControl:BasicLoadoutPopup` (BuildSetListControl.lua:218-242):
//! `controls.label`, `controls.edit` (whose `changeFunc` sets `controls.save.enabled`), `controls.save` (disabled until then) and `controls.cancel`,
//! `main:OpenPopup(370, height, title, controls, "save", "edit", "cancel")`.
use crate::control::{ControlRef, rect};
use crate::controls::{ButtonControl, EditArgs, EditControl, LabelControl};
use crate::modules::main::main;
use indexmap::IndexMap;
use pob_gfx::Gfx;
use std::cell::RefCell;
use std::rc::Rc;

/// `buf:match("%S")`.
pub fn has_non_space(buf: &str) -> bool {
    crate::lua::lmatch(buf, "%S").is_some()
}

/// One name popup: `valid(buf)` is what the edit's `changeFunc` gives `controls.save.enabled`; `on_save(buf)` runs before `main:ClosePopup()`.
#[allow(clippy::too_many_arguments)]
pub fn open_name_popup(
    g: &mut Gfx,
    height: f64,
    title: &str,
    label: &str,
    init: &str,
    valid: Rc<dyn Fn(&str) -> bool>,
    on_save: Rc<dyn Fn(&mut Gfx, String)>,
) {
    let save_cell: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
    let sc = save_cell.clone();
    let edit = EditControl::new(
        g,
        None,
        rect(0.0, 40.0, 350.0, 20.0),
        EditArgs {
            init: Some(init.to_string()),
            limit: Some(100),
            change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _: bool| {
                if let Some(b) = sc.borrow().as_ref() {
                    b.base.enabled.set(valid(buf));
                }
            })),
            ..EditArgs::default()
        },
    );
    let e2 = edit.clone();
    let save = ButtonControl::new(None, rect(-45.0, 70.0, 80.0, 20.0), "Save".to_string(), move |g| {
        let buf = e2.buf.borrow().clone();
        on_save(g, buf);
        main().close_popup();
    });
    save.base.enabled.set(false);
    *save_cell.borrow_mut() = Some(save.clone());
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert(
        "label".into(),
        LabelControl::new(None, rect(0.0, 20.0, 0.0, 16.0), label.to_string()),
    );
    controls.insert("edit".into(), edit);
    controls.insert("save".into(), save);
    controls.insert(
        "cancel".into(),
        ButtonControl::new(None, rect(45.0, 70.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup()),
    );
    main().open_popup(g, 370.0, height, title, controls, Some("save"), Some("edit"), Some("cancel"), None);
}
