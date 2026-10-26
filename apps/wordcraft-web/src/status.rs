pub(crate) fn combine(messages: Vec<String>) -> Option<String> {
    (!messages.is_empty()).then(|| messages.join("\n"))
}

pub(crate) fn post(app: &mut wordcraft_ui_egui::WordApp, messages: Vec<String>) -> bool {
    if let Some(message) = combine(messages) {
        app.status(message);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{combine, post};

    #[test]
    fn keeps_all_file_read_errors_in_one_status_message() {
        assert_eq!(combine(vec!["first failed".into(), "second failed".into()]).as_deref(), Some("first failed\nsecond failed"));
        assert!(combine(Vec::new()).is_none());
    }

    #[test]
    fn picker_read_errors_survive_document_switching_in_the_same_frame() {
        let mut app =
            wordcraft_ui_egui::WordApp::new(wordcraft_engine::Session::new(wordcraft_doc::Document::new()), wordcraft_ui_egui::Services::default());
        app.status("file read failed");
        app.run("file.new", serde_json::json!({})).unwrap();
        assert!(app.status_msg.is_none());
        assert!(post(&mut app, vec!["file read failed".into()]));
        assert_eq!(app.status_msg.as_ref().map(|(message, _)| message.as_str()), Some("file read failed"));
        assert!(!post(&mut app, Vec::new()));
    }
}
