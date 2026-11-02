//! `Classes/UndoHandler.lua`: a mixin; the host class defines `CreateUndoState` / `RestoreUndoState` ([`UndoHost`]).

use std::cell::{Cell, RefCell};

/// Fields of the mixin (`self.undo`, `self.redo`, `self.modFlag`).
pub struct UndoHandler<S> {
    pub undo: RefCell<Vec<S>>,
    pub redo: RefCell<Vec<S>>,
    pub mod_flag: Cell<bool>,
}

impl<S> Default for UndoHandler<S> {
    fn default() -> Self {
        Self {
            undo: RefCell::new(Vec::new()),
            redo: RefCell::new(Vec::new()),
            mod_flag: Cell::new(false),
        }
    }
}

/// The methods of `UndoHandler` (UndoHandler.lua:22-58) over the host's two required methods.
pub trait UndoHost {
    type State;
    fn undo_handler(&self) -> &UndoHandler<Self::State>;
    fn create_undo_state(&self) -> Self::State;
    fn restore_undo_state(&self, state: Self::State);

    /// UndoHandler.lua:22-26
    fn reset_undo(&self) {
        let s = self.create_undo_state();
        let u = self.undo_handler();
        u.undo.borrow_mut().clear();
        u.redo.borrow_mut().clear();
        u.undo.borrow_mut().push(s);
    }

    /// UndoHandler.lua:31-38 (`self.undo[102] = nil`: at most 101 states).
    fn add_undo_state(&self, no_clear_redo: bool) {
        let s = self.create_undo_state();
        let u = self.undo_handler();
        {
            let mut undo = u.undo.borrow_mut();
            undo.insert(0, s);
            undo.truncate(101);
        }
        u.mod_flag.set(true);
        if !no_clear_redo {
            u.redo.borrow_mut().clear();
        }
    }

    /// UndoHandler.lua:41-47
    fn undo(&self) {
        let u = self.undo_handler();
        if u.undo.borrow().len() >= 2 {
            let top = u.undo.borrow_mut().remove(0);
            u.redo.borrow_mut().insert(0, top);
            let s = u.undo.borrow_mut().remove(0);
            self.restore_undo_state(s);
            self.add_undo_state(true);
        }
    }

    /// UndoHandler.lua:50-55
    fn redo(&self) {
        let u = self.undo_handler();
        if !u.redo.borrow().is_empty() {
            let s = u.redo.borrow_mut().remove(0);
            self.restore_undo_state(s);
            self.add_undo_state(true);
        }
    }
}
