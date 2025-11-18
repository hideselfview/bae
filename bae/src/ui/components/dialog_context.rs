use dioxus::prelude::*;

#[derive(Clone)]
pub struct DialogContext {
    pub is_open: Signal<bool>,
    pub title: Signal<String>,
    pub message: Signal<String>,
    pub confirm_label: Signal<String>,
    pub cancel_label: Signal<String>,
    pub confirm_action_id: Signal<Option<String>>, // Simple identifier to trigger actions
}

impl DialogContext {
    pub fn new() -> Self {
        Self {
            is_open: Signal::new(false),
            title: Signal::new(String::new()),
            message: Signal::new(String::new()),
            confirm_label: Signal::new("Confirm".to_string()),
            cancel_label: Signal::new("Cancel".to_string()),
            confirm_action_id: Signal::new(None),
        }
    }

    pub fn show(
        &self,
        title: String,
        message: String,
        confirm_label: String,
        cancel_label: String,
        action_id: String,
    ) {
        let mut title_signal = self.title;
        let mut message_signal = self.message;
        let mut confirm_label_signal = self.confirm_label;
        let mut cancel_label_signal = self.cancel_label;
        let mut action_id_signal = self.confirm_action_id;
        let mut is_open_signal = self.is_open;

        title_signal.set(title);
        message_signal.set(message);
        confirm_label_signal.set(confirm_label);
        cancel_label_signal.set(cancel_label);
        action_id_signal.set(Some(action_id));
        is_open_signal.set(true);
    }

    pub fn hide(&self) {
        let mut is_open_signal = self.is_open;
        let mut action_id_signal = self.confirm_action_id;
        is_open_signal.set(false);
        action_id_signal.set(None);
    }

    pub fn confirm(&self) {
        // Hide dialog but keep action_id set so components can react
        let mut is_open_signal = self.is_open;
        is_open_signal.set(false);
    }

    pub fn clear_action(&self) {
        let mut action_id_signal = self.confirm_action_id;
        action_id_signal.set(None);
    }
}
