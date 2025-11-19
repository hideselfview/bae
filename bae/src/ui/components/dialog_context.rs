use dioxus::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

type ConfirmCallback = Box<dyn Fn()>;

#[derive(Clone)]
pub struct DialogContext {
    pub is_open: Signal<bool>,
    title: Rc<RefCell<String>>,
    message: Rc<RefCell<String>>,
    confirm_label: Rc<RefCell<String>>,
    cancel_label: Rc<RefCell<String>>,
    on_confirm: Rc<RefCell<Option<Rc<ConfirmCallback>>>>,
}

impl Default for DialogContext {
    fn default() -> Self {
        Self::new()
    }
}

impl DialogContext {
    pub fn new() -> Self {
        Self {
            is_open: Signal::new(false),
            title: Rc::new(RefCell::new(String::new())),
            message: Rc::new(RefCell::new(String::new())),
            confirm_label: Rc::new(RefCell::new("Confirm".to_string())),
            cancel_label: Rc::new(RefCell::new("Cancel".to_string())),
            on_confirm: Rc::new(RefCell::new(None)),
        }
    }

    pub fn title(&self) -> String {
        self.title.borrow().clone()
    }

    pub fn message(&self) -> String {
        self.message.borrow().clone()
    }

    pub fn confirm_label(&self) -> String {
        self.confirm_label.borrow().clone()
    }

    pub fn cancel_label(&self) -> String {
        self.cancel_label.borrow().clone()
    }

    pub fn on_confirm(&self) -> Option<Rc<ConfirmCallback>> {
        self.on_confirm.borrow().clone()
    }

    pub fn show_with_callback(
        &self,
        title: String,
        message: String,
        confirm_label: String,
        cancel_label: String,
        on_confirm: impl Fn() + 'static,
    ) {
        *self.title.borrow_mut() = title;
        *self.message.borrow_mut() = message;
        *self.confirm_label.borrow_mut() = confirm_label;
        *self.cancel_label.borrow_mut() = cancel_label;
        *self.on_confirm.borrow_mut() = Some(Rc::new(Box::new(on_confirm)));
        let mut is_open = self.is_open;
        is_open.set(true);
    }

    pub fn hide(&self) {
        let mut is_open = self.is_open;
        is_open.set(false);
        *self.on_confirm.borrow_mut() = None;
    }
}
