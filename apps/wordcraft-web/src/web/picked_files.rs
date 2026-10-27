//! Read a complete picker batch; unreadable files never become valid empty documents.

pub(super) const MAX_PICKED_FILES: usize = 2000;
const MAX_PENDING_PICKED_BYTES: usize = 256 << 20;

fn exceeds_byte_limit(size: usize, held: usize, many: bool) -> bool {
    size > MAX_PENDING_PICKED_BYTES || (many && held.saturating_add(size) > MAX_PENDING_PICKED_BYTES)
}

fn too_large_message(name: &str) -> String {
    wordcraft_ui_egui::i18n::fmt(wordcraft_ui_egui::i18n::t("Couldn't open {name}. The file exceeds the 256 MB browser limit."), &[("name", name)])
}

pub(super) async fn read(files: Vec<web_sys::File>, many: bool) -> (wordcraft_ui_egui::PickedFiles, Vec<String>) {
    let mut pending: wordcraft_ui_egui::PickedFiles = Vec::new();
    let mut messages = Vec::new();
    let mut held = 0usize;
    for file in files.into_iter().take(MAX_PICKED_FILES) {
        let name = file.name();
        let size = file.size().clamp(0.0, usize::MAX as f64) as usize;
        if exceeds_byte_limit(size, held, many) {
            messages.push(too_large_message(&name));
            continue;
        }
        let bytes = match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
            Ok(buffer) => js_sys::Uint8Array::new(&buffer).to_vec(),
            Err(error) => {
                log::error!("couldn't read the picked file: {error:?}");
                messages.push(wordcraft_ui_egui::i18n::fmt(
                    wordcraft_ui_egui::i18n::t("Couldn't read {name}. The file was not opened."),
                    &[("name", &name)],
                ));
                continue;
            }
        };
        if exceeds_byte_limit(bytes.len(), held, many) {
            messages.push(too_large_message(&name));
            continue;
        }
        held = held.saturating_add(bytes.len());
        pending.push((name, bytes));
    }
    (pending, messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picked_files_are_bounded_before_reading() {
        assert!(!exceeds_byte_limit(MAX_PENDING_PICKED_BYTES, 0, false));
        assert!(exceeds_byte_limit(MAX_PENDING_PICKED_BYTES + 1, 0, false));
        assert!(!exceeds_byte_limit(1, MAX_PENDING_PICKED_BYTES - 1, true));
        assert!(exceeds_byte_limit(1, MAX_PENDING_PICKED_BYTES, true));
    }
}
