//! Read a complete picker batch; unreadable files never become valid empty documents.

pub(super) const MAX_PICKED_FILES: usize = 2000;
const MAX_PENDING_PICKED_BYTES: usize = 256 << 20;

pub(super) async fn read(files: Vec<web_sys::File>, many: bool) -> (wordcraft_ui_egui::PickedFiles, Vec<String>) {
    let mut pending: wordcraft_ui_egui::PickedFiles = Vec::new();
    let mut messages = Vec::new();
    for file in files.into_iter().take(MAX_PICKED_FILES) {
        let mut bytes = match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
            Ok(buffer) => js_sys::Uint8Array::new(&buffer).to_vec(),
            Err(error) => {
                log::error!("couldn't read the picked file: {error:?}");
                messages.push(wordcraft_ui_egui::i18n::fmt(
                    wordcraft_ui_egui::i18n::t("Couldn't read {name}. The file was not opened."),
                    &[("name", &file.name())],
                ));
                continue;
            }
        };
        // Over-cap fonts stay empty so Add Font reports the rejected file individually.
        let held = pending.iter().map(|(_, b)| b.len()).fold(0, usize::saturating_add);
        if many && held.saturating_add(bytes.len()) > MAX_PENDING_PICKED_BYTES {
            bytes = Vec::new();
        }
        pending.push((file.name(), bytes));
    }
    (pending, messages)
}
