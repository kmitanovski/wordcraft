//! Document tabs adapted from PR #528 (23c107a5), by EvilDemonFluffball.
use super::*;

impl WordApp {
    pub fn empty_window(&mut self) {
        self.no_document = Some(self.session.document_id());
        self.tab_ids.clear();
        self.pinned_tabs.clear();
        self.active_tab = 0;
    }

    /// List all open documents, in display order.
    pub fn document_tabs(&self) -> Vec<DocumentTabInfo> {
        let documents = self.session.documents();
        self.tab_ids
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                if Some(*id) == self.no_document {
                    return None;
                }
                let doc = documents.iter().find(|doc| doc.id == *id)?;
                Some(DocumentTabInfo {
                    id: *id,
                    index,
                    name: documents::display_title(doc),
                    dirty: self.tab_has_unsaved_work(index),
                    active: *id == self.session.document_id(),
                    pinned: self.pinned_tabs.get(index).copied().unwrap_or(false),
                })
            })
            .collect()
    }

    pub(crate) fn sync_tab_order(&mut self) {
        let open: Vec<u64> = self.session.documents().into_iter().map(|doc| doc.id).collect();
        let mut kept = Vec::new();
        let mut pins = Vec::new();
        for (index, id) in self.tab_ids.iter().copied().enumerate() {
            if Some(id) != self.no_document && open.contains(&id) && !kept.contains(&id) {
                kept.push(id);
                pins.push(self.pinned_tabs.get(index).copied().unwrap_or(false));
            }
        }
        let active = self.session.document_id();
        if Some(active) != self.no_document && open.contains(&active) && !kept.contains(&active) {
            kept.push(active);
            pins.push(false);
        }
        self.tab_ids = kept;
        self.pinned_tabs = pins;
        self.active_tab = self.tab_ids.iter().position(|id| *id == active).unwrap_or(0);
    }

    pub(crate) fn sync_window_tabs(&mut self) {
        self.sync_shown_window_tabs();
    }

    pub fn active_document_tab(&self) -> usize {
        self.active_tab
    }

    pub fn document_tab_path(&self, index: usize) -> Option<String> {
        let id = *self.tab_ids.get(index)?;
        self.session.documents().into_iter().find(|doc| doc.id == id)?.path?.to_str().map(str::to_owned)
    }

    /// Stable ID for egui state such as a document's last scroll offset.
    pub(crate) fn active_document_id(&self) -> u64 {
        self.session.document_id()
    }

    pub fn switch_document_tab(&mut self, index: usize) -> bool {
        let Some(&id) = self.tab_ids.get(index) else {
            return false;
        };
        if self.document_modal() {
            return false;
        }
        if index == self.active_tab {
            return true;
        }
        if let Err(error) = self.session.switch_document(id) {
            self.status(error.to_string());
            return false;
        }
        // A platform composition belongs to the visible document, never an inactive tab.
        let composing = !self.canvas.ime_preedit.is_empty();
        self.canvas.ime_preedit.clear();
        self.canvas.ime_preedit_range = None;
        self.canvas.ime_join_text = None;
        self.session.clear_ime_joins();
        self.sync_tab_document();
        self.canvas.ime_interrupt_now = true;
        self.canvas.ime_interrupted_preedit |= composing;
        self.smart_art_row_focused = false;
        options::sync(self);
        self.dialog = None;
        self.colors = None;
        self.ui.backstage = false;
        self.canvas.want_focus = true;
        self.status_msg = None;
        self.applied_theme = None; // page-specific dark mode
        self.previews = previews::Previews::default(); // style previews belong to the document
        if let Some(ctx) = &self.ctx {
            ctx.request_repaint();
        }
        true
    }

    /// Close a tab by its displayed index. Unsaved work asks before the tab is removed.
    pub fn close_document_tab(&mut self, index: usize) -> Result<Value, String> {
        if !self.switch_document_tab(index) {
            return Err("unable to select that document while a save prompt is open".into());
        }
        self.run("file.close", json!({}))
    }

    /// Toggle pinning; pinned documents form an ordered group at the left edge.
    pub fn pin_document_tab(&mut self, index: usize, pinned: bool) -> bool {
        if index >= self.tab_ids.len() {
            return false;
        }
        if self.pinned_tabs[index] == pinned {
            return true;
        }
        let other_pins = self.pinned_tabs.iter().filter(|&&p| p).count();
        let destination = if pinned { other_pins } else { other_pins - 1 };
        self.pinned_tabs[index] = pinned;
        self.move_document_tab_unchecked(index, destination)
    }

    /// Queue a close operation that respects each tab's Save Changes prompt.
    pub fn close_other_document_tabs(&mut self, index: usize) -> bool {
        if index >= self.tab_ids.len() || self.document_modal() {
            return false;
        }
        self.dialog = None;
        self.colors = None;
        self.pending_keep_tab = Some(self.tab_ids[index]);
        self.pending_closes =
            self.tab_ids.iter().enumerate().filter(|(i, _)| *i != index && !self.pinned_tabs[*i]).map(|(_, id)| *id).rev().collect();
        self.continue_pending_closes();
        true
    }

    /// Close every tab, including pinned tabs, leaving an empty application window.
    pub fn close_all_document_tabs(&mut self) -> bool {
        if self.document_modal() {
            return false;
        }
        self.dialog = None;
        self.colors = None;
        self.pending_keep_tab = None;
        self.pending_closes = self.tab_ids.iter().copied().rev().collect();
        self.continue_pending_closes();
        true
    }

    pub(super) fn continue_pending_closes(&mut self) {
        if self.document_modal() || self.closing_all || self.quit_requested || self.finishing_pending_closes {
            return;
        }
        self.finishing_pending_closes = true;
        // Keep the remaining queue off self while closing, so clean tabs don't recurse.
        let mut pending = std::mem::take(&mut self.pending_closes);
        let keep = self.pending_keep_tab.take();
        while let Some(id) = pending.pop() {
            if let Some(index) = self.tab_ids.iter().position(|t| *t == id) {
                if let Err(error) = self.close_document_tab(index) {
                    self.status(error);
                    self.pending_closes.clear();
                    self.pending_keep_tab = None;
                    self.closing_window = None;
                    self.finishing_pending_closes = false;
                    return;
                }
                if self.dialog.is_some() || self.file_dialog_is_continuation() || self.awaiting_save.is_some() {
                    self.pending_closes = pending;
                    self.pending_keep_tab = keep;
                    self.finishing_pending_closes = false;
                    return;
                }
            }
        }
        if let Some(id) = keep
            && let Some(index) = self.tab_ids.iter().position(|t| *t == id)
        {
            self.switch_document_tab(index);
        }
        self.finishing_pending_closes = false;
        self.finish_marked_window_close();
    }

    /// Reopen the last closed file from disk, not discarded unsaved edits.
    pub fn reopen_closed_document_tab(&mut self) -> Result<Value, String> {
        let Some(closed) = self.recently_closed.last().cloned() else {
            return Err("there is no previously closed saved document".into());
        };
        // A failed reopen must remain on the history stack for a future retry.
        let result = if let Some(path) = closed.path.to_str() {
            self.add_document_tab("file.open", &json!({"path": path}))?
        } else if let Some(document) = self.session.find_open(&closed.path) {
            self.session.activate(document).map_err(|error| error.to_string())?;
            self.sync_tab_document();
            json!({"path": closed.path.to_string_lossy(), "id": document, "alreadyOpen": true})
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let bytes = wordcraft_engine::io::read_document(&closed.path)?;
                let name = closed.path.to_string_lossy();
                let result =
                    self.add_document_tab("file.open", &json!({"path": name, "data": wordcraft_engine::cmd::insert::base64_encode(&bytes)}))?;
                self.session.path = Some(closed.path.clone());
                result
            }
            #[cfg(target_arch = "wasm32")]
            {
                return Err("cannot reopen a file whose path is not valid Unicode in the browser".into());
            }
        };
        self.recently_closed.pop();
        if result.get("alreadyOpen").and_then(Value::as_bool) == Some(true) {
            return Ok(result);
        }
        let from = self.active_tab;
        // Pin first, then reorder within the same group so pinned tabs stay to the left.
        self.pin_document_tab(from, closed.pinned);
        let from = self.active_tab;
        let pin_count = self.pinned_tabs.iter().filter(|&&p| p).count();
        let min = if closed.pinned { 0 } else { pin_count };
        let max = if closed.pinned { pin_count.saturating_sub(1) } else { self.tab_ids.len() - 1 };
        self.reorder_document_tab(from, closed.index.clamp(min, max));
        Ok(result)
    }

    /// Move an existing tab to another position without affecting any document state.
    pub fn reorder_document_tab(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tab_ids.len() || to >= self.tab_ids.len() {
            return false;
        }
        if from == to {
            return true;
        }
        // Only move within the pinned or unpinned group; use pin_document_tab to cross it.
        if !self.can_move_document_tab(from, to) {
            return false;
        }
        self.move_document_tab_unchecked(from, to)
    }

    pub(crate) fn can_move_document_tab(&self, from: usize, to: usize) -> bool {
        from != to && self.pinned_tabs.get(from).zip(self.pinned_tabs.get(to)).is_some_and(|(from, to)| from == to)
    }

    pub(super) fn move_document_tab_unchecked(&mut self, from: usize, to: usize) -> bool {
        if from == to {
            return true;
        }
        let id = self.tab_ids.remove(from);
        self.tab_ids.insert(to, id);
        let pin = self.pinned_tabs.remove(from);
        self.pinned_tabs.insert(to, pin);
        if self.active_tab == from {
            self.active_tab = to;
        } else if from < self.active_tab && to >= self.active_tab {
            self.active_tab -= 1;
        } else if from > self.active_tab && to <= self.active_tab {
            self.active_tab += 1;
        }
        self.active_tab = self.tab_ids.iter().position(|id| *id == self.session.document_id()).unwrap_or(0);
        true
    }

    pub(super) fn add_document_tab(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        if self.document_modal() {
            return Err("finish the document dialog or operation before opening another tab".into());
        }
        let placeholder = !self.has_document();
        if id == "file.open"
            && params.get("data").is_none()
            && let Some(path) = params.get("path").and_then(Value::as_str)
            && let Some(document) = self.session.find_open(std::path::Path::new(path))
        {
            self.session.activate(document).map_err(|e| e.to_string())?;
            if self.tab_ids.contains(&document) {
                self.sync_tab_document();
            } else {
                self.sync_document();
            }
            self.recovery_seen = true;
            self.after_command(id);
            return Ok(json!({"path": path, "id": document, "alreadyOpen": true}));
        }
        if self.document_tabs().len() >= 128 && !placeholder {
            return Err("at most 128 document tabs may be open in one window".into());
        }
        if id == "file.recover" {
            let restore = params.get("restore").and_then(Value::as_str).ok_or("restore must be a recovery id")?;
            if params.get("discard").is_some() || !self.session.recovery_documents().iter().any(|d| d.id == restore) {
                return Err("choose an available recovery copy to restore".into());
            }
        }
        let separate_open = id == "file.open" && !placeholder && self.session.is_untouched_blank();
        let opening_blank = separate_open.then(|| self.session.blank_document());
        if let Some(blank) = opening_blank {
            self.session.add_document(blank);
        }
        let result = if id == "file.recover" {
            #[cfg(not(target_arch = "wasm32"))]
            if self.recovery_config.is_none() {
                return Err("local recovery requires the desktop host".into());
            }
            let blank = self.session.blank_document();
            let document = self.session.add_document(blank);
            self.sync_tab_document();
            self.pending_restores.push((document, self.session.rev(), params.clone()));
            json!({"pending": "recovery"})
        } else {
            let previous = self.session.document_id();
            let result = match self.session.run(id, params) {
                Ok(result) => result,
                Err(error) => {
                    if separate_open {
                        let added = self.session.document_id();
                        let _ = self.session.close_document(added);
                        self.sync_tab_document();
                    }
                    return Err(error.to_string());
                }
            };
            if placeholder && self.session.document_id() == previous && self.session.path.is_some() {
                self.no_document = None;
            }
            self.sync_tab_document();
            result
        };
        self.sync_tab_order();
        self.recovery_seen = true;
        self.after_command(id);
        Ok(result)
    }

    pub(super) fn close_active_tab_now(&mut self) -> Result<Value, String> {
        if !self.has_document() {
            return Ok(json!({"closed": false}));
        }
        let closed_id = self.session.document_id();
        if let Some(path) = self.session.path.as_ref() {
            // Reopening loads the saved file. Explicitly discarded edits are not recoverable.
            self.recently_closed.push(ClosedDocument { path: path.clone(), index: self.active_tab, pinned: self.pinned_tabs[self.active_tab] });
            if self.recently_closed.len() > 20 {
                self.recently_closed.remove(0);
            }
        }
        let shared_with_another_window = self.windows.native() && self.other_window_on_document().is_some();
        if shared_with_another_window {
            self.tab_ids.remove(self.active_tab);
            self.pinned_tabs.remove(self.active_tab);
            if self.tab_ids.is_empty() {
                self.no_document = Some(closed_id);
                self.active_tab = 0;
                self.canvas = Default::default();
            } else {
                let index = self.active_tab.min(self.tab_ids.len() - 1);
                self.session.activate(self.tab_ids[index]).map_err(|e| e.to_string())?;
                self.sync_tab_document();
            }
            return Ok(json!({"closed": true, "activeTab": self.active_tab}));
        }
        self.confirmed_saves.remove(&closed_id);
        // An explicit close supersedes Quit review; remaining dirty tabs need a fresh review.
        if self.closing_all {
            self.cancel_close_sequence();
        }
        self.dialog = None;
        self.colors = None;
        let previous = self.tab_ids.clone();
        self.session.close_document(closed_id).map_err(|e| e.to_string())?;
        self.tab_ids.retain(|id| *id != closed_id);
        self.pinned_tabs = previous
            .iter()
            .filter(|id| **id != closed_id)
            .filter_map(|id| previous.iter().position(|candidate| candidate == id).and_then(|i| self.pinned_tabs.get(i).copied()))
            .collect();
        if self.tab_ids.is_empty() {
            self.no_document = Some(self.session.document_id());
            self.session.release_untitled_name();
            self.canvas = Default::default();
            self.comment_drafts = Default::default();
            self.restrict = Default::default();
            self.controls = Default::default();
            self.previews = Default::default();
            self.read_aloud_at = None;
            self.read_aloud_error = None;
            self.change_picture_target = None;
            self.equation_prev_tab = None;
            self.outline_prev_tab = None;
            self.smart_art_item = None;
            self.smart_art_typing = None;
            self.smart_art_row_focused = false;
            self.info_editing = None;
            self.word_count = (0, 0);
            self.autosave_path = None;
            self.ui.backstage = false;
            self.pinned_tabs.clear();
            self.active_tab = 0;
            self.sync_tab_document();
            return Ok(json!({"closed": true}));
        }
        let index = self.active_tab.min(self.tab_ids.len() - 1);
        if let Some(&next) = self.tab_ids.get(index) {
            self.session.activate(next).map_err(|e| e.to_string())?;
        }
        self.sync_tab_document();
        self.dialog = None;
        self.status_msg = None;
        self.previews = previews::Previews::default();
        self.applied_theme = None;
        self.canvas.want_focus = true;
        if let Some(ctx) = &self.ctx {
            ctx.request_repaint();
        }
        Ok(json!({"closed": true, "activeTab": index}))
    }

    pub(super) fn any_dirty_documents(&self) -> bool {
        self.has_unsaved_documents() || !self.pending_restores.is_empty()
    }

    /// Review the dirty tabs without disposing of sessions: the full workspace must still
    /// exist when `on_exit` serializes preferences, including files saved during these prompts.
    pub(super) fn continue_close_all(&mut self) {
        if !self.closing_all {
            return;
        }
        let session = &mut self.session;
        self.quit_reviewed.retain(|(id, rev)| session.with_document(*id, |session| session.rev()).is_ok_and(|current| current == *rev));
        let unsaved = self.unsaved_documents();
        let next = self
            .tab_ids
            .iter()
            .copied()
            .find(|id| unsaved.contains(id) && !self.quit_reviewed.iter().any(|(reviewed, _)| reviewed == id))
            .or_else(|| unsaved.into_iter().find(|id| !self.quit_reviewed.iter().any(|(reviewed, _)| reviewed == id)));
        if let Some(next) = next {
            if next != self.session.document_id() && self.show_document(next).is_err() {
                return;
            }
            let name = self
                .session
                .documents()
                .into_iter()
                .find(|doc| doc.id == next)
                .map(|doc| documents::display_title(&doc))
                .unwrap_or_else(|| self.title_stem());
            self.dialog = Some(dialogs::Dialog::SaveChanges {
                name,
                then: "ui.confirmQuitTab".into(),
                params: json!({}),
                document: next,
                message: String::new(),
            });
        } else {
            for id in self.session.documents().into_iter().map(|doc| doc.id).collect::<Vec<_>>() {
                let _ = self.session.with_document(id, |session| session.dirty = false);
                if let Some(parked) = self.parked.get_mut(&id) {
                    parked.comment_drafts = Default::default();
                }
            }
            self.comment_drafts = Default::default();
            // Prompting may have selected several dirty documents. Restore the user's
            // selected document before persisting the workspace on exit.
            if let Some(id) = self.quit_original_tab {
                if let Some(index) = self.tab_ids.iter().position(|&candidate| candidate == id) {
                    self.switch_document_tab(index);
                } else if self.session.is_open(id) {
                    let _ = self.show_document(id);
                }
            }
            self.quit_requested = true;
        }
    }

    /// Capture tab order, paths and pinning. Empty tabs reopen blank (never resurrect unsaved text).
    pub fn workspace_snapshot(&self) -> WorkspaceState {
        let documents = self.session.documents();
        WorkspaceState {
            tabs: self
                .tab_ids
                .iter()
                .enumerate()
                .filter_map(|(i, id)| {
                    if Some(*id) == self.no_document {
                        return None;
                    }
                    let doc = documents.iter().find(|doc| doc.id == *id)?;
                    let path = doc.path.as_ref().map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()));
                    Some(WorkspaceTab {
                        path: path.as_ref().and_then(|p| p.to_str().map(str::to_owned)),
                        pinned: self.pinned_tabs.get(i).copied().unwrap_or(false),
                        unrestorable_path: path.as_ref().is_some_and(|p| p.to_str().is_none()),
                    })
                })
                .collect(),
            active: self.active_tab,
        }
    }

    /// Called after preferences load, only for desktop launches without explicit files or --sample.
    /// Missing or unreadable documents are skipped; the initial blank stays if nothing opens.
    pub fn restore_workspace(&mut self) -> usize {
        let saved = self.ui.workspace.clone();
        if saved.tabs.is_empty() || self.services.download.is_some() {
            return 0;
        }
        let was_placeholder = self.no_document;
        if !self.session.dirty && self.session.path.is_none() {
            self.no_document = Some(self.session.document_id());
        }
        let mut errors = Vec::new();
        if saved.tabs.len() > 128 {
            errors.push(tl!("Skipped workspace entries beyond the 128-tab limit.").to_string());
        }
        let mut count = 0;
        let mut active_id = None;
        for (i, tab) in saved.tabs.iter().take(128).enumerate() {
            let result = if tab.unrestorable_path {
                Err(tl!("Skipped a workspace entry because its file path is not valid Unicode.").to_string())
            } else {
                match &tab.path {
                    Some(path) => self.add_document_tab("file.open", &json!({"path": path})),
                    None if self.tab_ids.is_empty() && !self.has_document() => {
                        self.no_document = None;
                        self.sync_tab_order();
                        Ok(json!({"blank": true}))
                    }
                    None => self.add_document_tab("file.new", &json!({})),
                }
            };
            match result {
                Ok(value) if value.get("alreadyOpen").and_then(Value::as_bool) == Some(true) => {
                    if saved.active == i {
                        active_id = Some(self.active_document_id());
                    }
                }
                Ok(_) => {
                    count += 1;
                    let index = self.active_tab;
                    self.pin_document_tab(index, tab.pinned);
                    if saved.active == i {
                        active_id = Some(self.active_document_id());
                    }
                }
                Err(error) => {
                    log::warn!("could not restore document tab: {error}");
                    errors.push(error);
                }
            }
        }
        if count > 0 {
            let target = active_id
                .and_then(|id| self.tab_ids.iter().position(|candidate| *candidate == id))
                .unwrap_or_else(|| saved.active.min(self.tab_ids.len() - 1));
            self.switch_document_tab(target);
        } else {
            self.no_document = was_placeholder;
        }
        if !errors.is_empty() {
            self.status(format!("{}: {}", tl!("Could not restore document"), errors.join("; ")));
        }
        count
    }
}

pub(crate) fn is_tab_command(id: &str) -> bool {
    matches!(
        id,
        "ui.documentTabs"
            | "ui.switchDocument"
            | "ui.closeDocument"
            | "ui.reopenClosedDocument"
            | "ui.closeOtherDocuments"
            | "ui.closeAllDocuments"
            | "ui.pinDocument"
            | "ui.moveDocument"
            | "ui.nextDocument"
            | "ui.previousDocument"
            | "ui.newWindow"
    )
}

impl WordApp {
    pub(super) fn document_modal(&self) -> bool {
        matches!(
            self.dialog,
            Some(
                dialogs::Dialog::SaveChanges { .. }
                    | dialogs::Dialog::Password { .. }
                    | dialogs::Dialog::EncryptPassword { .. }
                    | dialogs::Dialog::DiscardRecovery { .. }
            )
        ) || self.awaiting_save.is_some()
            || self.zotero.busy.is_some()
            || self.file_dialog_is_continuation()
    }

    pub(super) fn tab_has_unsaved_work(&self, index: usize) -> bool {
        let Some(&id) = self.tab_ids.get(index) else { return false };
        self.document_has_unsaved_work(id)
    }

    pub(super) fn tab_index(&self, p: &Value, field: &str) -> Result<usize, String> {
        if field == "index"
            && let Some(id) = p.get("id")
        {
            let id = id.as_u64().ok_or("id must be an unsigned integer")?;
            return self.tab_ids.iter().position(|&t| t == id).ok_or_else(|| "unknown document tab id".into());
        }
        let n = p
            .get(field)
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| format!("{field} must be a nonnegative integer"))?;
        if n >= self.tab_ids.len() {
            return Err("document tab index is out of range".into());
        }
        Ok(n)
    }

    pub(super) fn tab_command(&mut self, id: &str, p: &Value, user: bool) -> Result<Value, String> {
        match id {
            "ui.documentTabs" => Ok(json!({"active": self.active_tab, "activeId": self.active_document_id(),
                "tabs": self.document_tabs().iter().map(|t| json!({"id": t.id, "index": t.index, "name": t.name,
                    "dirty": t.dirty, "active": t.active, "pinned": t.pinned})).collect::<Vec<_>>()})),
            "ui.switchDocument" => {
                let index = self.tab_index(p, "index")?;
                if !self.switch_document_tab(index) {
                    return Err("finish the document dialog or operation before switching tabs".into());
                }
                Ok(json!({"active": self.active_tab, "id": self.active_document_id()}))
            }
            "ui.nextDocument" | "ui.previousDocument" => {
                if self.document_tabs().len() < 2 {
                    return Ok(json!({"changed": false}));
                }
                let n = self.tab_ids.len();
                let index = (self.active_tab + if id == "ui.nextDocument" { 1 } else { n - 1 }) % n;
                self.tab_command("ui.switchDocument", &json!({"index": index}), user)
            }
            "ui.closeDocument" | "ui.closeOtherDocuments" | "ui.closeAllDocuments" => {
                let index = if id == "ui.closeAllDocuments" || (user && p.get("id").is_none() && p.get("index").is_none()) {
                    self.active_tab
                } else {
                    self.tab_index(p, "index")?
                };
                let targets: Vec<_> = (0..self.tab_ids.len())
                    .filter(|&i| match id {
                        "ui.closeDocument" => i == index,
                        "ui.closeOtherDocuments" => i != index && !self.pinned_tabs[i],
                        _ => true,
                    })
                    .collect();
                if !user && p.get("force").and_then(Value::as_bool) != Some(true) && targets.iter().any(|&i| self.tab_has_unsaved_work(i)) {
                    return Err("documents have unsaved work; save first or pass force: true".into());
                }
                if self.document_modal() {
                    return Err("finish the document dialog or operation before closing tabs".into());
                }
                if !user {
                    let ids: Vec<_> = targets.iter().filter_map(|&i| self.tab_ids.get(i).copied()).collect();
                    for id in ids {
                        if let Some(i) = self.tab_ids.iter().position(|&t| t == id) {
                            if !self.switch_document_tab(i) {
                                return Err("unable to switch document".into());
                            }
                            self.execute("file.close", p.clone())?;
                        }
                    }
                } else {
                    match id {
                        "ui.closeDocument" => return self.close_document_tab(index),
                        "ui.closeOtherDocuments" => {
                            self.close_other_document_tabs(index);
                        }
                        _ => {
                            self.close_all_document_tabs();
                        }
                    }
                }
                Ok(json!({"pending": !self.pending_closes.is_empty() || self.dialog.is_some()}))
            }
            "ui.reopenClosedDocument" => self.reopen_closed_document_tab(),
            "ui.pinDocument" => {
                let index = if user && p.get("id").is_none() && p.get("index").is_none() { self.active_tab } else { self.tab_index(p, "index")? };
                let pinned = if user && p.get("pinned").is_none() {
                    !self.pinned_tabs.get(index).copied().ok_or("there is no document tab to pin")?
                } else {
                    p.get("pinned").and_then(Value::as_bool).ok_or("pinned must be a boolean")?
                };
                if !self.pin_document_tab(index, pinned) {
                    return Err("unable to pin tab".into());
                }
                Ok(json!({"active": self.active_tab}))
            }
            "ui.moveDocument" => {
                let from = self.tab_index(p, "from")?;
                let to = self.tab_index(p, "to")?;
                if !self.reorder_document_tab(from, to) {
                    return Err("unable to reorder tabs across pinned groups".into());
                }
                Ok(json!({"active": self.active_tab}))
            }
            "ui.newWindow" => {
                if let Some(launch) = &self.services.document_window {
                    launch()?;
                } else {
                    self.new_window();
                }
                Ok(json!({"opened": true}))
            }
            _ => Err("unknown tab command".into()),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn enable_recovery(&mut self, root: std::path::PathBuf, wake: std::sync::Arc<dyn Fn() + Send + Sync>) -> Result<(), String> {
        self.session.enable_recovery(root.clone(), wake.clone())?;
        self.recovery_config = Some((root, wake));
        Ok(())
    }

    pub fn finish_recovery(&mut self) {
        self.session.finish_recovery();
    }
}

const COMMANDS: &[(&str, &str, &str)] = &[
    ("ui.documentTabs", "Document Tabs", "{}"),
    ("ui.switchDocument", "Switch Document", "{id: stable tab id} or {index: display index}"),
    ("ui.closeDocument", "Close Tab", "{id|index, force?: bool}"),
    ("ui.closeOtherDocuments", "Close Other Tabs", "{id|index, force?: bool}; pinned tabs stay open"),
    ("ui.closeAllDocuments", "Close All Tabs", "{force?: bool}"),
    ("ui.reopenClosedDocument", "Reopen Closed Tab", "{}"),
    ("ui.pinDocument", "Pin Tab", "{id|index, pinned: bool}"),
    ("ui.moveDocument", "Move Tab", "{from: display index, to: display index}"),
    ("ui.nextDocument", "Next Document", "{}"),
    ("ui.previousDocument", "Previous Document", "{}"),
    ("ui.newWindow", "New Window", "{}"),
];

pub(crate) fn commands(app: &WordApp) -> Vec<Value> {
    let tabs = app.document_tabs();
    let modal = app.document_modal();
    COMMANDS
        .iter()
        .map(|(id, label, params)| {
            let enabled = match *id {
                "ui.newWindow" => app.windows.native() || app.services.document_window.is_some(),
                "ui.reopenClosedDocument" => !app.recently_closed.is_empty() && !modal,
                "ui.switchDocument" => !tabs.is_empty() && !modal,
                "ui.moveDocument" => tabs.len() > 1 && !modal,
                "ui.nextDocument" | "ui.previousDocument" => tabs.len() > 1 && !modal,
                "ui.closeDocument" | "ui.closeAllDocuments" => !tabs.is_empty() && !modal,
                "ui.closeOtherDocuments" => tabs.iter().any(|tab| !tab.active && !tab.pinned) && !modal,
                "ui.pinDocument" => !tabs.is_empty(),
                _ => true,
            };
            json!({"id": id, "label": label, "location": "Window",
        "params": params, "enabled": enabled,
        "shortcut": shortcut(app, id)})
        })
        .collect()
}

pub(crate) fn shortcut(app: &WordApp, id: &str) -> String {
    let key = match id {
        "ui.nextDocument" => "Ctrl+Tab",
        "ui.previousDocument" => "Ctrl+Shift+Tab",
        "ui.reopenClosedDocument" => "Ctrl+Shift+T",
        _ => return String::new(),
    };
    let canonical = if cfg!(target_os = "macos") { key.to_string() } else { key.replacen("Ctrl+", "Mod+", 1) };
    if app.session.keymap.removed.contains(&canonical) || app.session.keymap.assigned.contains_key(&canonical) {
        return String::new();
    }
    let bound = app.session.keymap.resolve(&app.session.registry, &canonical);
    if bound.is_some_and(|s| !(s.id == "text.insertTab" && id != "ui.reopenClosedDocument")) {
        return String::new();
    }
    key.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> WordApp {
        WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default())
    }
    fn command_enabled(app: &WordApp, id: &str) -> bool {
        commands(app).iter().find(|command| command["id"] == id).is_some_and(|command| command["enabled"].as_bool() == Some(true))
    }
    fn edit(a: &mut WordApp, text: &str) {
        a.run("text.insert", json!({"text": text})).unwrap();
    }

    #[test]
    fn command_search_enablement_tracks_the_available_tabs() {
        let mut a = app();
        a.start_screen();
        for id in [
            "ui.closeDocument",
            "ui.closeOtherDocuments",
            "ui.closeAllDocuments",
            "ui.pinDocument",
            "ui.switchDocument",
            "ui.moveDocument",
            "ui.nextDocument",
            "ui.previousDocument",
            "ui.reopenClosedDocument",
        ] {
            assert!(!command_enabled(&a, id), "{id} must be disabled with no tabs");
        }
        assert!(a.execute("ui.pinDocument", json!({})).is_err(), "pinning an empty workspace returns an error");

        a.run("file.new", json!({})).unwrap();
        assert!(command_enabled(&a, "ui.closeDocument"));
        assert!(command_enabled(&a, "ui.closeAllDocuments"));
        assert!(command_enabled(&a, "ui.pinDocument"));
        assert!(!command_enabled(&a, "ui.closeOtherDocuments"));
        assert!(!command_enabled(&a, "ui.nextDocument"));
        assert!(!command_enabled(&a, "ui.previousDocument"));

        a.run("file.new", json!({})).unwrap();
        assert!(command_enabled(&a, "ui.closeOtherDocuments"));
        assert!(command_enabled(&a, "ui.nextDocument"));
        assert!(command_enabled(&a, "ui.previousDocument"));
        a.dialog = Some(dialogs::Dialog::SaveChanges {
            name: "Document".into(),
            then: "file.close".into(),
            params: json!({}),
            document: a.session.document_id(),
            message: String::new(),
        });
        assert!(!command_enabled(&a, "ui.closeDocument"));
        assert!(!command_enabled(&a, "ui.nextDocument"));
    }

    #[test]
    fn closing_the_last_tab_releases_the_standin_untitled_name() {
        let mut a = app();
        assert_eq!(a.document_tabs()[0].name, "Document1");
        a.close_active_tab_now().unwrap();
        a.run("file.new", json!({})).unwrap();
        assert_eq!(a.document_tabs()[0].name, "Document2");
    }

    #[test]
    #[cfg(unix)]
    fn workspace_skips_non_unicode_paths_instead_of_restoring_a_lossy_name() {
        use std::os::unix::ffi::OsStringExt;

        let mut a = app();
        a.session.path = Some(std::path::PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/wordcraft-\xff.docx".to_vec())));
        let snapshot = a.workspace_snapshot();
        assert!(snapshot.tabs[0].path.is_none());
        assert!(snapshot.tabs[0].unrestorable_path);

        let mut restored = app();
        restored.ui.workspace = serde_json::from_value(serde_json::to_value(snapshot).unwrap()).unwrap();
        assert_eq!(restored.restore_workspace(), 0);
        assert_eq!(restored.document_tabs().len(), 1, "keep the initial blank document");
        assert!(restored.status_msg.as_ref().is_some_and(|(message, _)| message.contains("not valid Unicode")));

        let old_tab: WorkspaceTab = serde_json::from_value(json!({"path": null, "pinned": true})).unwrap();
        assert!(!old_tab.unrestorable_path, "older preferences default the marker to false");
    }

    #[test]
    fn direct_close_document_refuses_an_inactive_unconfirmed_save() {
        let mut a = app();
        a.empty_window();
        a.run("file.new", json!({})).unwrap();
        let document = a.session.document_id();
        let saves = async_saves::AsyncSaves::default();
        let started = saves.clone();
        a.services.async_saves = Some(saves);
        a.services.download = Some(Box::new(move |_, _| {
            started.start();
            Ok(())
        }));
        edit(&mut a, "pending save");
        a.run("file.save", json!({"path": "pending.docx"})).unwrap();
        a.run("file.new", json!({})).unwrap();

        assert!(a.execute("file.closeDocument", json!({"id": document})).is_err());
        assert!(a.session.is_open(document));
    }

    #[test]
    fn stable_ids_survive_moves_and_malformed_indexes_never_close_work() {
        let mut a = app();
        edit(&mut a, "keep me");
        let first = a.document_tabs()[0].id;
        a.run("file.new", json!({})).unwrap();
        a.run("ui.moveDocument", json!({"from": 0, "to": 1})).unwrap();
        a.execute("ui.switchDocument", json!({"id": first})).unwrap();
        assert!(a.session.dirty);
        for value in [json!(-1), json!(1.2), json!("0"), json!(u64::MAX), Value::Null] {
            assert!(a.execute("ui.closeDocument", json!({"index": value, "force": true})).is_err());
            assert!(a.execute("ui.moveDocument", json!({"from": value, "to": 0})).is_err());
        }
        assert!(a.execute("ui.closeDocument", json!({"id": first})).is_err());
        assert!(a.execute("ui.closeAllDocuments", json!({})).is_err());
        assert!(a.execute("ui.confirmQuitTab", json!({})).is_err());
        assert_eq!(a.document_tabs().len(), 2);
    }

    #[test]
    fn move_tab_availability_respects_the_pinned_group_boundary() {
        let mut a = app();
        a.run("file.new", json!({})).unwrap();
        a.run("file.new", json!({})).unwrap();
        assert!(a.pin_document_tab(1, true));
        a.run("file.new", json!({})).unwrap();

        assert!(!a.can_move_document_tab(0, 1));
        assert!(!a.can_move_document_tab(1, 0));
        assert!(a.can_move_document_tab(1, 2));
        assert!(a.can_move_document_tab(2, 1));
        assert!(!a.can_move_document_tab(2, usize::MAX));
    }

    #[test]
    fn switching_tabs_interrupts_the_previous_input_method_target() {
        let mut a = app();
        edit(&mut a, "first");
        a.run("file.new", json!({})).unwrap();
        a.canvas.ime_preedit = "unfinished composition".into();
        a.canvas.ime_preedit_range = Some(0..2);
        assert!(a.switch_document_tab(0));
        assert!(a.canvas.ime_interrupt_now && a.canvas.ime_interrupted_preedit);
        assert!(a.canvas.ime_preedit.is_empty());
        assert!(a.switch_document_tab(1));
        assert!(a.canvas.ime_preedit.is_empty());
        assert!(a.canvas.ime_preedit_range.is_none());
        assert!(a.canvas.ime_interrupt_now);
    }

    #[test]
    fn first_open_replaces_start_placeholder_and_last_close_keeps_empty_window() {
        let mut a = app();
        a.start_screen();
        assert!(a.document_tabs().is_empty());
        assert!(a.workspace_snapshot().tabs.is_empty(), "Start's blank placeholder is not a restored tab");
        a.run("file.new", json!({})).unwrap();
        assert_eq!(a.document_tabs().len(), 1);
        a.restrict.open = true;
        a.restrict.asking = true;
        a.run("file.close", json!({})).unwrap();
        assert!(!a.has_document() && !a.quit_requested);
        assert!(!a.restrict.open && !a.restrict.asking);
        assert!(a.workspace_snapshot().tabs.is_empty());
        a.run("file.new", json!({})).unwrap();
        assert_eq!(a.document_tabs().len(), 1);
    }

    #[test]
    #[cfg(unix)]
    fn closed_document_history_keeps_native_non_unicode_paths() {
        use std::os::unix::ffi::OsStringExt;

        let mut a = app();
        let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/wordcraft-closed-\xff.docx".to_vec()));
        a.session.path = Some(path.clone());
        assert!(a.document_tab_path(0).is_none(), "Copy Full Path must not copy an unresolvable replacement name");
        a.run("file.close", json!({})).unwrap();

        assert_eq!(a.recently_closed.last().map(|closed| closed.path.as_path()), Some(path.as_path()));
        assert!(a.reopen_closed_document_tab().is_err(), "a missing file reports the error and stays in history");
        assert_eq!(a.recently_closed.last().map(|closed| closed.path.as_path()), Some(path.as_path()));
    }

    #[test]
    fn repeated_window_close_and_cancel_preserve_the_entire_workspace() {
        let mut a = app();
        edit(&mut a, "first");
        a.run("file.new", json!({})).unwrap();
        edit(&mut a, "second");
        assert!(!a.close_requested());
        let prompt = serde_json::to_value(&a.dialog).unwrap();
        assert!(!a.close_requested());
        assert_eq!(serde_json::to_value(&a.dialog).unwrap(), prompt);
        assert!(a.run("file.new", json!({})).is_err());
        assert!(!a.switch_document_tab(1));
        a.run("ui.saveChanges", json!({"answer": "dontSave"})).unwrap();
        a.run("ui.saveChanges", json!({"answer": "cancel"})).unwrap();
        assert!(!a.quit_requested);
        assert_eq!(a.document_tabs().len(), 2);
        assert!(a.document_tabs().iter().all(|t| t.dirty));
    }

    #[test]
    fn preferences_follow_existing_and_new_tabs_without_resetting_zoom() {
        let mut a = app();
        a.session.view.zoom = 1.8;
        a.run("file.new", json!({})).unwrap();
        a.session.author = "Tabs author".into();
        a.session.initials = "TA".into();
        a.session.prefs.check_spelling = false;
        a.session.view.zoom = 0.7;
        assert!(a.switch_document_tab(0));
        assert_eq!(a.session.view.zoom, 1.8);
        assert_eq!(a.prefs().author, "Tabs author");
        assert!(!a.prefs().editing.check_spelling);
        a.run("file.new", json!({})).unwrap();
        assert_eq!(a.prefs().initials, "TA");
        assert!(!a.prefs().editing.check_spelling);
    }

    #[test]
    fn pending_export_does_not_mark_a_clean_tab_dirty() {
        let saves = async_saves::AsyncSaves::default();
        let hook = saves.clone();
        let services = Services {
            download: Some(Box::new(move |_, _| {
                hook.start();
                Ok(())
            })),
            async_saves: Some(saves),
            ..Default::default()
        };
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), services);
        a.run("file.exportPdf", json!({})).unwrap();
        assert!(!a.document_tabs()[0].dirty);
        assert!(!a.any_dirty_documents());
        assert!(a.close_requested(), "an export does not require Save Changes");
        assert!(a.dialog.is_none());
    }

    #[test]
    fn inactive_async_failure_restores_its_own_dirty_state() {
        for outcome in [async_saves::SaveOutcome::Cancelled, async_saves::SaveOutcome::Failed("disk full".into())] {
            let mut a = app();
            a.empty_window();
            a.run("file.new", json!({})).unwrap();
            let saves = async_saves::AsyncSaves::default();
            a.services.async_saves = Some(saves.clone());
            let download_saves = saves.clone();
            a.services.download = Some(Box::new(move |_, _| {
                download_saves.start();
                Ok(())
            }));
            edit(&mut a, "original");
            let ticket = a.run("file.save", json!({"path": "first.docx"})).unwrap()["pendingSave"].as_u64().unwrap();
            a.run("file.new", json!({})).unwrap();
            assert!(a.document_tabs()[0].dirty, "pending saves count as unsaved");
            saves.finish(ticket, outcome);
            a.settle_async_saves();
            assert!(!a.session.dirty);
            assert!(a.switch_document_tab(0));
            assert!(a.session.dirty);
        }
    }

    #[test]
    fn pending_save_as_saves_originating_tab_and_picture_pick_cancels_after_switch() {
        let dir = std::env::temp_dir().join(format!("wordcraft-tab-picker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("original.docx");
        let mut a = app();
        edit(&mut a, "source writing");
        let (tx, rx) = std::sync::mpsc::channel();
        let rx = std::cell::RefCell::new(Some(rx));
        a.services.file_dialog = Some(Box::new(move |_| rx.borrow_mut().take().unwrap()));
        a.save_as_dialog();
        a.run("file.new", json!({})).unwrap();
        edit(&mut a, "other writing");
        tx.send(Ok(vec![path.to_string_lossy().into_owned()])).unwrap();
        a.poll_file_dialog().unwrap().unwrap();
        assert_eq!(a.active_document_tab(), 1);
        assert!(a.session.dirty && a.session.path.is_none());
        assert!(a.switch_document_tab(0));
        assert!(!a.session.dirty && a.session.path.as_ref() == Some(&path));
        let (tx, rx) = std::sync::mpsc::channel();
        let rx = std::cell::RefCell::new(Some(rx));
        a.services.file_dialog = Some(Box::new(move |_| rx.borrow_mut().take().unwrap()));
        a.ask_file(file_dialogs::FileDialogRequest::Open { purpose: "picture".into() }, file_dialogs::AfterPick::Picture);
        assert!(a.switch_document_tab(1));
        tx.send(Ok(vec!["/not/a/picture.png".into()])).unwrap();
        assert!(a.poll_file_dialog().unwrap().unwrap_err().contains("no longer active"));
        assert!(a.session.dirty && a.session.path.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn automation_quit_cannot_bypass_inactive_unsaved_work() {
        let mut a = app();
        edit(&mut a, "keep");
        a.run("file.new", json!({})).unwrap();
        let ctx = egui::Context::default();
        let request = control::ControlRequest::new("app.quit", json!({})).0;
        let control::Outcome::Done(result) = control::handle(&mut a, &ctx, &request) else { panic!("expected reply") };
        assert_eq!(result["ok"], false);
        assert!(!a.quit_requested);
        assert!(a.execute("file.close", json!({"quit": true})).is_err());
    }
}

impl WordApp {
    pub(super) fn tick_recovery(&mut self, ctx: &egui::Context) {
        let pending = std::mem::take(&mut self.pending_restores);
        for (document, revision, params) in pending {
            if !self.session.is_open(document) {
                continue;
            }
            let result = self.session.with_document(document, |session| {
                if session.rev() != revision {
                    return Some(Err("Recovery cancelled: the document changed while the copy was loading.".to_string()));
                }
                if session.recovery_ready() && !session.recovery_busy() {
                    Some(session.run("file.recover", &params).map(|_| ()).map_err(|e| e.to_string()))
                } else {
                    None
                }
            });
            match result {
                Ok(Some(Ok(()))) => {}
                Ok(Some(Err(error))) => self.status(error),
                Err(error) => self.status(error.to_string()),
                Ok(None) => self.pending_restores.push((document, revision, params)),
            }
        }
        self.session.recovery_tick();
        if let Some(wait) = self.session.recovery_wait() {
            ctx.request_repaint_after(wait);
        }
        if let Some(error) = self.session.recovery_error().map(str::to_string) {
            self.status(error);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod recovery_tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn snapshots(root: &std::path::Path) -> std::io::Result<usize> {
        let mut count = 0;
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            if entry.path().is_dir() {
                for file in std::fs::read_dir(entry.path())? {
                    if file?.file_name().to_string_lossy().starts_with("snapshot-") {
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    }

    #[test]
    fn inactive_recovery_survives_abrupt_process_termination() {
        if let Some(root) = std::env::var_os("WORDCRAFT_TAB_RECOVERY_CHILD") {
            let root = std::path::PathBuf::from(root);
            let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
            a.enable_recovery(root.join("recovery"), std::sync::Arc::new(|| {})).unwrap();
            a.run("text.insert", json!({"text": "First inactive recovery draft"})).unwrap();
            a.run("file.new", json!({})).unwrap();
            a.run("text.insert", json!({"text": "Second inactive recovery draft"})).unwrap();
            a.run("file.new", json!({})).unwrap();
            a.run("text.insert", json!({"text": "Encrypted draft must not be plaintext"})).unwrap();
            a.run("file.encrypt", json!({"password": "synthetic-test-only"})).unwrap();
            let ctx = egui::Context::default();
            loop {
                a.tick_recovery(&ctx);
                if snapshots(&root.join("recovery")).unwrap() == 2 {
                    std::fs::write(root.join("ready"), "ready").unwrap();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        let root = std::env::temp_dir().join(format!("wordcraft-tab-crash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "document_tabs::recovery_tests::inactive_recovery_survives_abrupt_process_termination", "--nocapture"])
            .env("WORDCRAFT_TAB_RECOVERY_CHILD", &root)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let end = Instant::now() + Duration::from_secs(25);
        while !root.join("ready").exists() && Instant::now() < end {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let ready = root.join("ready").exists();
        let _ = child.kill();
        child.wait().unwrap();
        assert!(ready, "both inactive sessions must snapshot before termination");
        assert_eq!(snapshots(&root.join("recovery")).unwrap(), 2, "encrypted writing never reaches plaintext recovery");
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        a.enable_recovery(root.join("recovery"), std::sync::Arc::new(|| {})).unwrap();
        let ctx = egui::Context::default();
        let end = Instant::now() + Duration::from_secs(5);
        while a.session.recovery_documents().len() != 2 {
            a.tick_recovery(&ctx);
            assert!(Instant::now() < end, "terminated sessions must be discoverable");
            std::thread::sleep(Duration::from_millis(5));
        }
        a.run("text.insert", json!({"text": "Current writing must survive restoration"})).unwrap();
        let id = a.session.recovery_documents()[0].id.clone();
        a.run("file.recover", json!({"restore": id})).unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while !a.session.dirty {
            a.tick_recovery(&ctx);
            assert!(Instant::now() < end, "restoration must complete");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(a.session.doc.plain_text(wordcraft_doc::StoryRef::Body).contains("inactive recovery draft"));
        assert!(a.session.path.is_none() && !a.saved_here());
        assert!(a.switch_document_tab(0));
        assert_eq!(a.session.doc.plain_text(wordcraft_doc::StoryRef::Body), "Current writing must survive restoration");
        a.finish_recovery();
        drop(a);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod close_and_save_tests {
    use super::*;

    #[test]
    fn close_all_includes_pins_and_cancel_stops_remaining_documents() {
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        a.pin_document_tab(0, true);
        a.run("file.new", json!({})).unwrap();
        a.run("text.insert", json!({"text": "first"})).unwrap();
        a.run("file.new", json!({})).unwrap();
        a.run("text.insert", json!({"text": "second"})).unwrap();
        a.run("ui.closeAllDocuments", json!({})).unwrap();
        assert_eq!(
            a.document_tabs().len(),
            2,
            "the clean tab closed first; tabs={:?}, dialog={:?}, pending={:?}",
            a.document_tabs(),
            a.dialog,
            a.pending_closes
        );
        a.run("ui.saveChanges", json!({"answer": "cancel"})).unwrap();
        assert!(a.pending_closes.is_empty() && !a.quit_requested);
        assert!(a.document_tabs().iter().all(|t| !t.pinned), "Close All also closed the clean pinned tab");
        a.run("ui.closeAllDocuments", json!({})).unwrap();
        for _ in 0..2 {
            a.run("ui.saveChanges", json!({"answer": "dontSave"})).unwrap();
        }
        assert!(a.document_tabs().is_empty() && !a.has_document() && !a.quit_requested);
    }

    #[test]
    fn save_as_cannot_take_another_tabs_destination() {
        let root = std::env::temp_dir().join(format!("wordcraft-tab-collision-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("original.docx");
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        a.run("text.insert", json!({"text": "original"})).unwrap();
        a.run("file.save", json!({"path": path})).unwrap();
        a.run("file.new", json!({})).unwrap();
        a.run("text.insert", json!({"text": "second"})).unwrap();
        assert!(a.run("file.saveAs", json!({"path": root.join(".").join("original.docx")})).is_err());
        assert!(a.session.dirty && a.session.path.is_none());
        assert_eq!(wordcraft_engine::io::open_path(&path).unwrap().plain_text(wordcraft_doc::StoryRef::Body), "original");
        a.run("file.open", json!({"path": path})).unwrap();
        assert_eq!(a.document_tabs().len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn opening_an_already_open_file_at_the_tab_limit_selects_its_tab() {
        let root = std::env::temp_dir().join(format!("wordcraft-tab-limit-open-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("original.docx");
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        a.run("text.insert", json!({"text": "original"})).unwrap();
        a.run("file.save", json!({"path": path})).unwrap();
        let original = a.session.document_id();
        for _ in 1..128 {
            a.run("file.new", json!({})).unwrap();
        }
        assert_eq!(a.document_tabs().len(), 128);

        let result = a.run("file.open", json!({"path": path})).unwrap();
        assert_eq!(result["alreadyOpen"], true);
        assert_eq!(result["id"], original);
        assert_eq!(a.session.document_id(), original);
        assert_eq!(a.document_tabs().len(), 128);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn opening_a_file_owned_by_another_window_does_not_add_a_129th_tab() {
        let path = std::env::temp_dir().join(format!("wordcraft-foreign-window-tab-{}.docx", std::process::id()));
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        for _ in 1..128 {
            a.run("file.new", json!({})).unwrap();
        }
        assert_eq!(a.document_tabs().len(), 128);
        let original_window = a.windows.shown();
        a.new_window();
        a.run("file.new", json!({})).unwrap();
        let foreign = a.session.document_id();
        a.session.with_document(foreign, |session| session.path = Some(path.clone())).unwrap();
        assert!(a.enter_window(original_window));

        let result = a.run("file.open", json!({"path": path})).unwrap();

        assert_eq!(result["alreadyOpen"], true);
        assert!(a.tab_ids.contains(&foreign), "opening the document should route to its owning window");
        assert_eq!(a.document_tabs().len(), 2, "the existing target window retains its own tabs");
        assert!(a.other_windows().any(|window| window.tab_ids.len() == 128 && !window.tab_ids.contains(&foreign)));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn quit_reprompts_when_control_edits_a_document_already_reviewed() {
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        a.run("text.insert", json!({"text": "First"})).unwrap();
        a.run("file.new", json!({})).unwrap();
        a.run("text.insert", json!({"text": "Second"})).unwrap();

        assert!(!a.close_requested());
        a.run("ui.saveChanges", json!({"answer": "dontSave"})).unwrap();
        let (reviewed, remaining) = match a.dialog {
            Some(dialogs::Dialog::SaveChanges { document, .. }) => (a.quit_reviewed[0].0, document),
            other => panic!("expected the next dirty document prompt, got {other:?}"),
        };
        assert_ne!(reviewed, remaining);

        let (request, _) =
            crate::control::ControlRequest::new("engine.execute", json!({"command": "text.insert", "params": {"text": " more"}, "doc": reviewed}));
        let crate::control::Outcome::Done(reply) = crate::control::handle(&mut a, &egui::Context::default(), &request) else {
            panic!("an addressed edit should return a normal control reply");
        };
        assert_eq!(reply["ok"], true, "the control channel can edit the other open document");

        a.run("ui.saveChanges", json!({"answer": "dontSave"})).unwrap();
        assert!(matches!(a.dialog, Some(dialogs::Dialog::SaveChanges { document, .. }) if document == reviewed));
        assert!(!a.quit_requested, "Quit must ask again after a reviewed document changes");
        a.run("ui.saveChanges", json!({"answer": "dontSave"})).unwrap();
        assert!(a.quit_requested);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn dropping_an_open_file_keeps_its_existing_autosave_authority() {
        #[derive(Debug)]
        struct Dropped(std::path::PathBuf);
        impl egui::DroppedFile for Dropped {
            fn path(&self) -> &std::path::Path {
                &self.0
            }
            fn bytes(&self) -> Result<Vec<u8>, String> {
                std::fs::read(&self.0).map_err(|e| e.to_string())
            }
        }
        let root = std::env::temp_dir().join(format!("wordcraft-drop-authority-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("original.docx");
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        a.run("text.insert", json!({"text": "saved"})).unwrap();
        a.run("file.save", json!({"path": path})).unwrap();
        let document = a.session.document_id();
        a.run("text.insert", json!({"text": "unsaved edit"})).unwrap();
        a.run("file.new", json!({})).unwrap();
        a.open_dropped_files(vec![std::sync::Arc::new(Dropped(path.clone()))]);
        assert_eq!(a.session.document_id(), document);
        assert_eq!(a.autosave_path.as_ref(), Some(&path));
        assert!(a.autosaves());
        assert!(a.session.dirty);
        assert!(a.session.doc.plain_text(wordcraft_doc::StoryRef::Body).contains("unsaved edit"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_successful_export_does_not_confirm_a_failed_document_save() {
        for outcome in [async_saves::SaveOutcome::Cancelled, async_saves::SaveOutcome::Failed("disk full".into())] {
            let saves = async_saves::AsyncSaves::default();
            let hook = saves.clone();
            let services = Services {
                download: Some(Box::new(move |_, _| {
                    hook.start();
                    Ok(())
                })),
                async_saves: Some(saves.clone()),
                ..Default::default()
            };
            let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), services);
            a.run("text.insert", json!({"text": "unsaved writing"})).unwrap();
            let document = a.session.document_id();
            let save = a.run("file.save", json!({})).unwrap()["pendingSave"].as_u64().unwrap();
            let export = a.run("file.exportPdf", json!({})).unwrap()["pendingSave"].as_u64().unwrap();
            a.run("file.new", json!({})).unwrap();
            saves.finish(export, async_saves::SaveOutcome::Saved);
            a.settle_async_saves();
            saves.finish(save, outcome);
            a.settle_async_saves();
            assert!(a.session.with_document(document, |session| session.dirty).unwrap());
            assert!(!a.confirmed_saves.contains_key(&document));
            assert!(a.switch_document_tab(0));
            assert!(a.execute("file.close", json!({})).is_err());
            assert_eq!(a.document_tabs().len(), 2);
        }
    }

    #[test]
    fn late_failure_of_an_older_save_does_not_unsave_a_confirmed_newer_revision() {
        let saves = async_saves::AsyncSaves::default();
        let hook = saves.clone();
        let services = Services {
            download: Some(Box::new(move |_, _| {
                hook.start();
                Ok(())
            })),
            async_saves: Some(saves.clone()),
            ..Default::default()
        };
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), services);
        a.run("text.insert", json!({"text": "first"})).unwrap();
        let old = a.run("file.save", json!({})).unwrap()["pendingSave"].as_u64().unwrap();
        a.run("text.insert", json!({"text": "newer"})).unwrap();
        let new = a.run("file.save", json!({})).unwrap()["pendingSave"].as_u64().unwrap();
        saves.finish(new, async_saves::SaveOutcome::Saved);
        a.settle_async_saves();
        saves.finish(old, async_saves::SaveOutcome::Failed("old picker failed".into()));
        a.settle_async_saves();
        assert!(!a.session.dirty);
        assert!(!a.save_unconfirmed());
    }
}

impl WordApp {
    pub(super) fn cancel_close_sequence(&mut self) {
        self.closing_all = false;
        self.quit_reviewed.clear();
        self.quit_original_tab = None;
        self.pending_closes.clear();
        self.pending_keep_tab = None;
        self.closing_window = None;
        self.finishing_pending_closes = false;
    }
}

#[cfg(test)]
mod picker_cancel_tests {
    use super::*;

    #[test]
    fn cancelled_save_as_stops_close_all_and_allows_quit_retry() {
        let services = Services { pick_save: Some(Box::new(|_| None)), ..Default::default() };
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), services);
        a.run("text.insert", json!({"text": "first"})).unwrap();
        a.run("file.new", json!({})).unwrap();
        a.run("text.insert", json!({"text": "second"})).unwrap();
        a.run("ui.closeAllDocuments", json!({})).unwrap();
        a.run("ui.saveChanges", json!({"answer": "save"})).unwrap();
        assert!(a.pending_closes.is_empty() && !a.closing_all);
        assert_eq!(a.document_tabs().len(), 2);
        assert!(!a.close_requested());
        assert!(matches!(a.dialog, Some(dialogs::Dialog::SaveChanges { .. })));
        a.run("ui.saveChanges", json!({"answer": "cancel"})).unwrap();
        assert!(!a.quit_requested);
    }
}

#[cfg(test)]
mod close_during_picker_tests {
    use super::*;

    #[test]
    fn open_picker_uses_the_active_window_if_its_origin_was_closed() {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        let origin = app.active_document_id();
        let (tx, rx) = std::sync::mpsc::channel();
        let rx = std::cell::RefCell::new(Some(rx));
        app.services.file_dialog = Some(Box::new(move |_| rx.borrow_mut().take().unwrap()));
        app.ask_file(
            file_dialogs::FileDialogRequest::Open { purpose: "document".into() },
            file_dialogs::AfterPick::OpenDocument { document: origin },
        );

        app.close_active_tab_now().unwrap();
        assert!(!app.session.is_open(origin));
        let path = std::env::temp_dir().join(format!("wordcraft-picked-after-close-{}.txt", std::process::id()));
        std::fs::write(&path, "picked document").unwrap();
        tx.send(Ok(vec![path.to_string_lossy().into_owned()])).unwrap();
        assert!(app.poll_file_dialog().unwrap().unwrap()["path"].as_str().is_some());
        assert_eq!(app.session.path.as_ref(), Some(&path));
        assert!(app.session.doc.plain_text(wordcraft_doc::StoryRef::Body).contains("picked document"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn inactive_save_as_waits_for_the_active_documents_confirmation() {
        let (tx, rx) = std::sync::mpsc::channel();
        let rx = std::cell::RefCell::new(Some(rx));
        let services = Services { file_dialog: Some(Box::new(move |_| rx.borrow_mut().take().unwrap())), ..Default::default() };
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), services);
        app.run("text.insert", json!({"text": "first document"})).unwrap();
        let original = app.session.document_id();
        assert!(matches!(app.save_as(file_dialogs::AfterSave::Nothing), file_dialogs::Asked::Pending));
        app.run("file.new", json!({})).unwrap();
        app.run("text.insert", json!({"text": "second document"})).unwrap();
        let active = app.active_document_id();
        app.run("file.close", json!({})).unwrap();
        let dir = std::env::temp_dir().join(format!("wordcraft-deferred-save-{original}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("first.docx");
        tx.send(Ok(vec![path.to_string_lossy().into_owned()])).unwrap();
        assert!(app.poll_file_dialog().is_none());
        assert!(app.file_dialog_open() && !path.exists());
        assert!(matches!(app.dialog, Some(dialogs::Dialog::SaveChanges { .. })));
        app.run("ui.saveChanges", json!({"answer": "cancel"})).unwrap();
        assert_eq!(app.poll_file_dialog().unwrap().unwrap()["saved"], true);
        assert_eq!(app.active_document_id(), active);
        assert!(app.session.dirty && path.exists());
        assert!(app.switch_document_tab(0));
        assert_eq!(app.session.document_id(), original);
        assert!(!app.session.dirty);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn busy_save_picker_cancels_quit_sequence_and_allows_retry() {
        let (tx, rx) = std::sync::mpsc::channel();
        let rx = std::cell::RefCell::new(Some(rx));
        let services = Services { file_dialog: Some(Box::new(move |_| rx.borrow_mut().take().unwrap())), ..Default::default() };
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), services);
        app.run("text.insert", json!({"text": "unsaved writing"})).unwrap();
        app.run("ui.openFileDialog", json!({})).unwrap();
        assert!(!app.close_requested());
        app.run("ui.saveChanges", json!({"answer": "save"})).unwrap();
        assert!(!app.closing_all && !app.quit_requested && app.session.dirty);
        tx.send(Ok(Vec::new())).unwrap();
        app.poll_file_dialog().unwrap().unwrap();
        assert!(!app.close_requested());
        assert!(matches!(app.dialog, Some(dialogs::Dialog::SaveChanges { .. })));
    }

    #[test]
    fn cancelled_inactive_picker_does_not_report_an_error() {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        let document = app.session.document_id();
        app.change_picture_target = Some("old-image".into());
        app.run("file.new", json!({})).unwrap();
        let result = app.finish_pick(file_dialogs::AfterPick::Bound { document, after: Box::new(file_dialogs::AfterPick::Picture) }, None).unwrap();
        assert_eq!(result["cancelled"], true);
        assert!(app.status_msg.is_none());
        assert_eq!(app.active_document_tab(), 1);
        assert!(app.switch_document_tab(0));
        assert!(app.change_picture_target.is_none());
    }

    #[test]
    fn font_changes_invalidate_inactive_document_layouts() {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        app.run("text.insert", json!({"text": "cached paragraph"})).unwrap();
        let cached = app.session.layout();
        app.run("file.new", json!({})).unwrap();
        app.font_generation = wordcraft_fonts::FontDb::global().generation().wrapping_sub(1);
        app.watch_fonts(&egui::Context::default());
        assert!(app.switch_document_tab(0));
        assert!(!std::sync::Arc::ptr_eq(&cached, &app.session.layout()));
        assert!(app.session.dirty);
    }

    #[test]
    fn inactive_save_as_failure_keeps_its_feedback_after_switching_back() {
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        a.run("text.insert", json!({"text": "unsaved writing"})).unwrap();
        let document = a.session.document_id();
        a.run("file.new", json!({})).unwrap();
        let previous = a.active_document_id();
        a.dialog = Some(dialogs::Dialog::Commands { query: "unapplied dialog draft".into(), swallow: false });
        a.ui.backstage = true;
        a.canvas.want_focus = false;
        a.finish_pick(
            file_dialogs::AfterPick::Bound {
                document,
                after: Box::new(file_dialogs::AfterPick::SaveAs { after: file_dialogs::AfterSave::Nothing, document }),
            },
            None,
        )
        .unwrap();
        assert!(matches!(&a.dialog, Some(dialogs::Dialog::Commands { query, .. }) if query == "unapplied dialog draft"));
        assert!(a.ui.backstage && !a.canvas.want_focus);
        let path = std::env::temp_dir().join(format!("wordcraft-no-parent-{}", document)).join("absent").join("save.docx");
        let result = a
            .finish_pick(
                file_dialogs::AfterPick::Bound {
                    document,
                    after: Box::new(file_dialogs::AfterPick::SaveAs { after: file_dialogs::AfterSave::Nothing, document }),
                },
                Some(path.to_string_lossy().into_owned()),
            )
            .unwrap();
        assert_eq!(result["saved"], false);
        assert_eq!(a.active_document_id(), previous);
        assert!(matches!(&a.dialog, Some(dialogs::Dialog::Commands { query, .. }) if query == "unapplied dialog draft"));
        assert!(a.ui.backstage && !a.canvas.want_focus);
        assert!(a.status_msg.as_ref().is_some_and(|(message, _)| !message.is_empty()));
        assert!(a.switch_document_tab(0));
        assert!(a.session.dirty);
    }

    #[test]
    fn os_close_does_not_replace_a_pending_save_as_continuation() {
        let (tx, rx) = std::sync::mpsc::channel();
        let rx = std::cell::RefCell::new(Some(rx));
        let services = Services { file_dialog: Some(Box::new(move |_| rx.borrow_mut().take().unwrap())), ..Default::default() };
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), services);
        a.run("text.insert", json!({"text": "draft"})).unwrap();
        a.run("file.close", json!({})).unwrap();
        a.run("ui.saveChanges", json!({"answer": "save"})).unwrap();
        assert!(a.file_dialog_is_continuation() && a.dialog.is_none());
        assert!(!a.close_requested());
        assert!(!a.close_requested());
        assert!(a.file_dialog_is_continuation() && a.dialog.is_none());
        assert!(a.status_msg.as_ref().is_some_and(|(message, _)| message.contains("before closing")));
        assert!(!a.closing_all);
        tx.send(Ok(Vec::new())).unwrap();
        a.poll_file_dialog().unwrap().unwrap();
        assert!(a.session.dirty && a.has_document() && !a.quit_requested);
    }
}

#[cfg(test)]
mod overflow_tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn overflow_strip_reveals_the_selected_document() {
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        for index in 0..12 {
            a.run("file.new", json!({})).unwrap();
            a.run("file.properties", json!({"title": format!("Synthetic long document title number {index}")})).unwrap();
        }
        let label = format!("{} — Unsaved changes", a.document_tabs().last().unwrap().name);
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(780.0, 600.0)).build_ui_state(
            |ui, app: &mut WordApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            a,
        );
        h.run_steps(10);
        assert!(h.ctx.content_rect().contains(h.get_by_label(&label).rect().center()));
        h.state_mut().run("ui.switchDocument", json!({"index": 1})).unwrap();
        h.run_steps(10);
        let first = "Synthetic long document title number 0 — Unsaved changes";
        assert!(h.ctx.content_rect().contains(h.get_by_label(first).rect().center()));
    }
}

#[cfg(test)]
mod workspace_limit_tests {
    use super::*;

    #[test]
    fn workspace_restoration_caps_processing_and_reports_skipped_entries() {
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        a.ui.workspace.tabs = vec![WorkspaceTab::default(); 140];
        assert_eq!(a.restore_workspace(), 128);
        assert_eq!(a.document_tabs().len(), 128);
        assert!(a.status_msg.as_ref().is_some_and(|(message, _)| message.contains("128-tab limit")));
        assert!(a.run("file.new", json!({})).is_err());
        assert_eq!(a.document_tabs().len(), 128);
    }
}

#[cfg(test)]
mod interrupted_quit_tests {
    use super::*;

    #[test]
    fn forced_close_during_quit_allows_window_and_close_all_retry() {
        for window_close in [true, false] {
            let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
            for text in ["first", "second", "third"] {
                if text != "first" {
                    a.run("file.new", json!({})).unwrap();
                }
                a.run("text.insert", json!({"text": text})).unwrap();
            }
            assert!(!a.close_requested());
            a.run("ui.saveChanges", json!({"answer": "dontSave"})).unwrap();
            assert_eq!(a.active_document_tab(), 1);
            assert!(a.execute("file.close", json!({})).is_err());
            assert!(a.closing_all && a.dialog.is_some());
            a.execute("file.close", json!({"force": true})).unwrap();
            assert_eq!(a.document_tabs().len(), 2);
            assert!(a.document_tabs().iter().all(|t| t.dirty));
            assert!(!a.quit_requested);
            if window_close {
                assert!(!a.close_requested());
            } else {
                a.run("ui.closeAllDocuments", json!({})).unwrap();
            }
            assert!(matches!(a.dialog, Some(dialogs::Dialog::SaveChanges { document, .. }) if document == a.session.document_id()));
            assert_eq!(a.active_document_tab(), 0, "previously reviewed dirty tabs must be reviewed again");
            a.run("ui.saveChanges", json!({"answer": "cancel"})).unwrap();
            assert_eq!(a.document_tabs().len(), 2);
            assert!(a.document_tabs().iter().all(|t| t.dirty));
            assert!(!a.closing_all && a.pending_closes.is_empty());
        }
    }
}

#[cfg(test)]
mod review_preferences_tests {
    use super::*;

    #[test]
    fn codex_review_new_tabs_keep_effective_editing_language() {
        for language in ["pl-PL", "en-AU"] {
            let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
            a.session.set_editing_language(Some(language));
            a.run("text.insert", json!({"text": "original"})).unwrap();
            a.session.doc.styles.default_chr.lang = Some("de-DE".into());
            a.run("file.new", json!({})).unwrap();
            assert_eq!(a.session.editing_lang.as_deref(), Some(language));
            assert_eq!(a.session.doc.styles.default_chr.lang.as_deref(), Some(language));
            for extension in ["txt", "md"] {
                a.run("file.open", json!({"path": format!("synthetic.{extension}"), "data": "SGVsbG8="})).unwrap();
                assert_eq!(a.session.editing_lang.as_deref(), Some(language));
                assert_eq!(a.session.doc.styles.default_chr.lang.as_deref(), Some(language));
            }
            assert!(a.switch_document_tab(0));
            assert_eq!(a.session.doc.styles.default_chr.lang.as_deref(), Some("de-DE"), "switching must preserve the document language");
        }
    }

    #[test]
    fn codex_review_new_tabs_persist_custom_dictionary_changes() {
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        let root = std::env::temp_dir().join(format!("wordcraft-tab-dictionary-{}-{}", std::process::id(), a.session.document_id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(Session::USER_DICT_FILE);
        a.session.user_dict_path = Some(path.clone());
        a.run("file.new", json!({})).unwrap();
        a.run("review.addToDictionary", json!({"word": "codextabpersistword"})).unwrap();
        let persisted = std::fs::read_to_string(&path);
        a.run("file.options", json!({"removeWords": ["codextabpersistword"]})).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert!(persisted.unwrap().lines().any(|w| w == "codextabpersistword"));
        assert!(a.switch_document_tab(0));
        assert_eq!(a.session.user_dict_path, Some(path));
    }
}

#[cfg(test)]
mod web_picker_review_tests {
    use super::*;
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::mpsc::{Sender, channel},
    };
    type Replies = Rc<RefCell<Vec<Sender<PickedFiles>>>>;

    fn browser_app(replies: Replies) -> WordApp {
        let services = Services {
            open_async: Some(Box::new(move |_| {
                let (tx, rx) = channel();
                replies.borrow_mut().push(tx);
                rx
            })),
            inbox: Some(Inbox::default()),
            ..Default::default()
        };
        WordApp::new(Session::new(wordcraft_doc::Document::new()), services)
    }

    #[test]
    fn inactive_web_picture_result_clears_only_its_owners_target() {
        for cancelled in [false, true] {
            let replies = Replies::default();
            let mut app = browser_app(replies.clone());
            app.change_picture_target = Some("old-image".into());
            app.pick_picture();
            app.run("file.new", json!({})).unwrap();
            app.change_picture_target = Some("current-image".into());
            let files = if cancelled { Vec::new() } else { vec![("image.png".into(), vec![1])] };
            replies.borrow_mut().remove(0).send(files).unwrap();
            app.poll_web_pick();
            assert_eq!(app.change_picture_target.as_deref(), Some("current-image"));
            assert!(app.switch_document_tab(0));
            assert!(app.change_picture_target.is_none());
        }
    }

    #[test]
    fn late_picker_result_cannot_adopt_a_new_tabs_origin() {
        let replies = Replies::default();
        let mut a = browser_app(replies.clone());
        a.pick_recipient_list();
        a.run("file.new", json!({})).unwrap();
        a.pick_recipient_list();
        assert_eq!(replies.borrow().len(), 1, "a second picker cannot replace a pending operation");
        let old = replies.borrow_mut().remove(0);
        old.send(vec![("old.txt".into(), b"OldOwner\nPrivate\n".to_vec())]).unwrap();
        a.drain_inbox();
        assert!(a.session.merge.headers.is_empty());
        assert!(a.status_msg.as_ref().is_some_and(|(message, _)| message.contains("no longer active")));
        assert!(!a.file_dialog_open() && !a.recipient_list_pending);
        a.pick_recipient_list();
        assert!(old.send(vec![("late.csv".into(), b"Stale\nPrivate\n".to_vec())]).is_err(), "extra batches cannot leak to the next picker");
        replies.borrow_mut().remove(0).send(vec![("new.txt".into(), b"NewOwner\nChosen\n".to_vec())]).unwrap();
        a.drain_inbox();
        assert_eq!(a.session.merge.headers, ["NewOwner"]);
        assert!(a.switch_document_tab(0));
        assert!(a.session.merge.headers.is_empty());
    }

    #[test]
    fn a_pending_picker_does_not_claim_unrelated_dropped_documents() {
        let replies = Replies::default();
        let mut a = browser_app(replies.clone());
        a.pick_recipient_list();
        a.run("file.new", json!({})).unwrap();
        a.services.inbox.as_ref().unwrap().lock().unwrap().push(("notes.txt".into(), b"Independent drop".to_vec()));
        a.drain_inbox();
        assert!(a.session.doc.plain_text(wordcraft_doc::StoryRef::Body).contains("Independent drop"));
        assert!(a.file_dialog_open());
        replies.borrow_mut().remove(0).send(Vec::new()).unwrap();
        a.drain_inbox();
        assert!(!a.file_dialog_open() && !a.recipient_list_pending);
    }

    #[test]
    fn cancelled_or_disconnected_picker_does_not_poison_later_results() {
        for disconnect in [true, false] {
            let replies = Replies::default();
            let mut a = browser_app(replies.clone());
            a.pick_recipient_list();
            let tx = replies.borrow_mut().remove(0);
            if !disconnect {
                tx.send(Vec::new()).unwrap();
            }
            drop(tx);
            a.drain_inbox();
            assert!(!a.file_dialog_open() && !a.recipient_list_pending);
            a.services.inbox.as_ref().unwrap().lock().unwrap().push(("notes.txt".into(), b"After cancellation".to_vec()));
            a.drain_inbox();
            assert!(a.session.doc.plain_text(wordcraft_doc::StoryRef::Body).contains("After cancellation"));
            a.pick_recipient_list();
            replies.borrow_mut().remove(0).send(vec![("valid.txt".into(), b"Name\nAda\n".to_vec())]).unwrap();
            a.drain_inbox();
            assert_eq!(a.session.merge.headers, ["Name"]);
        }
    }

    #[test]
    fn all_files_in_a_picture_picker_batch_keep_their_origin() {
        let replies = Replies::default();
        let mut a = browser_app(replies.clone());
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255])))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        a.pick_picture();
        a.run("file.new", json!({})).unwrap();
        let files = vec![("first.png".into(), png.clone()), ("second.png".into(), png.clone())];
        replies.borrow_mut().remove(0).send(files.clone()).unwrap();
        a.drain_inbox();
        assert!(a.session.doc.media.is_empty());
        assert!(a.switch_document_tab(0));
        assert!(a.session.doc.media.is_empty());
        a.pick_picture();
        replies.borrow_mut().remove(0).send(files).unwrap();
        a.drain_inbox();
        assert!(!a.session.doc.media.is_empty(), "an unchanged origin accepts its picture batch");
    }
}
