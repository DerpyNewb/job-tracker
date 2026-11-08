//! The window: top bar, Follow up queue and status filter, table, and the editor.

use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDate};
use egui::{
    Align, Button, Color32, ComboBox, CornerRadius, Frame, Id, Key, KeyboardShortcut, Label,
    Layout, Margin, Modal, Modifiers, Panel, RichText, Role, ScrollArea, Sense, TextEdit, Ui,
    UiBuilder, ViewportCommand, WidgetInfo,
};
use egui_extras::{Column, DatePickerButton, TableBuilder};

use crate::model::{Application, Attention, DateCell, Field, Status};
use crate::prefs::{Accent, Prefs, ThemeChoice};
use crate::sheet::{SOURCES, Sheet};
use crate::store::{self, Change, Loaded, Row, SaveError, SaveOptions};
use crate::theme::{self, Palette, strong};

pub struct Doc {
    pub loaded: Loaded,
    pub rows: Vec<Row>,
}

impl Doc {
    pub fn changes(&self) -> Vec<Change> {
        store::changes(&self.loaded.sheet.rows, &self.rows)
    }

    pub fn sheet(&self) -> Sheet {
        Sheet {
            rows: self.rows.iter().map(|r| r.app.clone()).collect(),
            extra_headers: self.loaded.sheet.extra_headers.clone(),
        }
    }

    pub fn file_name(&self) -> String {
        self.loaded
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    All,
    NeedsAttention,
    Status(Status),
    OtherStatus,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SortKey {
    Company,
    Role,
    Applied,
    Status,
    FollowUp,
}

#[derive(Clone, Debug, PartialEq)]
enum Pending {
    ConfirmDelete(usize),
    ConfirmClose,
    ChangedOnDisk,
    /// Opening another workbook while this one has unsaved changes.
    ConfirmSwitch(PathBuf),
    Settings,
}

#[derive(Clone, Debug)]
struct Message {
    error: bool,
    text: String,
}

pub struct TrackerApp {
    pub doc: Option<Doc>,
    pub load_error: Option<String>,
    /// Set in tests so the screens don't depend on the real date.
    pub fixed_today: Option<NaiveDate>,
    pub backup_dir: PathBuf,
    pub resumes: Vec<String>,
    pub selected: Option<usize>,
    pub search: String,
    pub filter: Filter,
    pub sort: (SortKey, bool),
    pub prefs: Prefs,
    /// Theme and text size last applied to the context, so they're only set when they change.
    applied: Option<(ThemeChoice, Accent, f32)>,
    /// Window title last sent, so it's only set when it changes.
    title: String,
    pending: Option<Pending>,
    message: Option<Message>,
    close_confirmed: bool,
    focus_search: bool,
    focus_company: bool,
}

pub fn default_backup_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("job-tracker")
        .join("backups")
}

/// Resume PDFs next to the workbook's folder, as `name` or `per-posting/name`, without `.pdf`.
/// `archive/` is left out on purpose: those are superseded.
pub fn scan_resumes(workbook: &Path, folder: Option<&Path>) -> Vec<String> {
    let Some(root) = folder
        .map(Path::to_path_buf)
        .or_else(|| auto_resumes_dir(workbook))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (dir, prefix) in [
        (root.clone(), ""),
        (root.join("per-posting"), "per-posting/"),
    ] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf"))
                && let Some(stem) = p.file_stem()
            {
                out.push(format!("{prefix}{}", stem.to_string_lossy()));
            }
        }
    }
    out.sort();
    out
}

/// `resumes/` beside the workbook's `job-search/` folder, if it exists.
pub fn auto_resumes_dir(workbook: &Path) -> Option<PathBuf> {
    workbook
        .parent()
        .and_then(Path::parent)
        .map(|p| p.join("resumes"))
        .filter(|p| p.is_dir())
}

fn to_jiff(d: NaiveDate) -> jiff::civil::Date {
    jiff::civil::date(d.year() as i16, d.month() as i8, d.day() as i8)
}

fn from_jiff(d: jiff::civil::Date) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(d.year() as i32, d.month() as u32, d.day() as u32)
}

impl TrackerApp {
    pub fn new(backup_dir: PathBuf) -> Self {
        TrackerApp {
            doc: None,
            load_error: None,
            fixed_today: None,
            backup_dir,
            resumes: Vec::new(),
            selected: None,
            search: String::new(),
            filter: Filter::All,
            sort: (SortKey::Applied, true),
            prefs: Prefs::default(),
            applied: None,
            title: String::new(),
            pending: None,
            message: None,
            close_confirmed: false,
            focus_search: false,
            focus_company: false,
        }
    }

    pub fn today(&self) -> NaiveDate {
        self.fixed_today
            .unwrap_or_else(|| chrono::Local::now().date_naive())
    }

    pub fn path(&self) -> Option<&Path> {
        self.doc.as_ref().map(|d| d.loaded.path.as_path())
    }

    pub fn is_dirty(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| !d.changes().is_empty())
    }

    pub fn open(&mut self, path: &Path) {
        match store::load(path) {
            Ok(loaded) => {
                self.prefs.remember(path);
                self.resumes = scan_resumes(path, self.prefs.resumes_dir.as_deref());
                self.message = if loaded.read_only.is_some() {
                    None
                } else if loaded.adds_columns {
                    Some(Message {
                        error: false,
                        text: "This workbook gets three new columns on its next save: Source, \
                               Resume and Follow-up Date (K to M). Dates are saved as Excel dates."
                            .into(),
                    })
                } else {
                    None
                };
                self.doc = Some(Doc {
                    rows: store::rows_from(&loaded.sheet),
                    loaded,
                });
                self.load_error = None;
                self.selected = None;
                self.pending = None;
            }
            Err(e) => {
                if matches!(e, store::LoadError::Missing(_)) {
                    self.prefs.forget(path);
                }
                self.doc = None;
                self.load_error = Some(format!("{}\n\n{e}", path.display()));
            }
        }
    }

    /// Opens `path`, asking first if the current workbook has unsaved changes.
    pub fn request_open(&mut self, path: &Path) {
        if self.is_dirty() {
            self.pending = Some(Pending::ConfirmSwitch(path.to_path_buf()));
        } else {
            self.open(path);
        }
    }

    fn pick_file(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("Excel workbook", &["xlsx"]);
        let start = self
            .path()
            .or(self.prefs.recent.first().map(PathBuf::as_path));
        if let Some(dir) = start.and_then(Path::parent) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(p) = dialog.pick_file() {
            self.request_open(&p);
        }
    }

    /// Re-reads the Resume sent list after the folder setting changes.
    fn rescan_resumes(&mut self) {
        if let Some(path) = self.path().map(Path::to_path_buf) {
            self.resumes = scan_resumes(&path, self.prefs.resumes_dir.as_deref());
        }
    }

    /// Shows the open workbook in the window title (and so the taskbar), with a mark when unsaved.
    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match &self.doc {
            Some(doc) if self.is_dirty() => format!("* {} - Job tracker", doc.file_name()),
            Some(doc) => format!("{} - Job tracker", doc.file_name()),
            None => "Job tracker".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// Applies the theme and text size settings when they change.
    pub fn apply_prefs(&mut self, ctx: &egui::Context) {
        let want = (self.prefs.theme, self.prefs.accent, self.prefs.text_scale);
        if self.applied != Some(want) {
            ctx.set_theme(self.prefs.theme.preference());
            theme::set_accent(ctx, self.prefs.accent);
            ctx.set_zoom_factor(self.prefs.text_scale);
            self.applied = Some(want);
        }
    }

    pub fn add(&mut self) {
        let today = self.today();
        let Some(doc) = self.doc.as_mut() else { return };
        let extra = doc.loaded.sheet.extra_headers.len();
        let mut app = Application::new_today(today, extra);
        if self.prefs.follow_up_after_days > 0 {
            app.follow_up =
                DateCell::Date(today + chrono::Duration::days(self.prefs.follow_up_after_days));
        }
        doc.rows.push(Row { app, origin: None });
        self.selected = Some(doc.rows.len() - 1);
        self.filter = Filter::All;
        self.search.clear();
        self.focus_company = true;
    }

    pub fn delete(&mut self, index: usize) {
        if let Some(doc) = self.doc.as_mut()
            && index < doc.rows.len()
        {
            let gone = doc.rows.remove(index);
            self.selected = None;
            self.message = Some(Message {
                error: false,
                text: format!(
                    "Deleted {} from the list. It leaves the workbook when you save.",
                    if gone.app.company.is_empty() {
                        "the new row"
                    } else {
                        &gone.app.company
                    }
                ),
            });
        }
    }

    pub fn save(&mut self) -> bool {
        let today = self.today();
        let Some(doc) = self.doc.as_mut() else {
            return false;
        };
        let sheet = doc.sheet();
        let opts = SaveOptions {
            backup_dir: &self.backup_dir,
            stamp: chrono::Local::now().format("%Y-%m-%d %H%M%S").to_string(),
            today,
            keep: self.prefs.keep_backups,
        };
        match store::save(&mut doc.loaded, &sheet, &opts) {
            Ok(backup) => {
                doc.rows = store::rows_from(&doc.loaded.sheet);
                self.message = Some(Message {
                    error: false,
                    text: format!(
                        "Saved. The previous version is backed up as {}.",
                        backup
                            .file_name()
                            .map(|n| n.to_string_lossy())
                            .unwrap_or_default()
                    ),
                });
                true
            }
            Err(SaveError::ChangedOnDisk) => {
                self.pending = Some(Pending::ChangedOnDisk);
                false
            }
            Err(e) => {
                if let SaveError::Invalid(problems) = &e
                    && let Some((row, _, _)) = problems.first()
                {
                    self.selected = Some(*row);
                    self.filter = Filter::All;
                    self.search.clear();
                }
                self.message = Some(Message {
                    error: true,
                    text: e.to_string(),
                });
                false
            }
        }
    }

    /// Reads the workbook again and puts the unsaved changes back on top of it.
    pub fn reload_keeping_changes(&mut self) {
        let Some(doc) = self.doc.as_ref() else { return };
        let path = doc.loaded.path.clone();
        let changes = doc.changes();
        match store::load(&path) {
            Ok(fresh) => {
                let (rows, notes) = store::reapply(&fresh.sheet, &changes);
                self.doc = Some(Doc {
                    loaded: fresh,
                    rows,
                });
                self.selected = None;
                let text = if changes.is_empty() {
                    "Reloaded.".to_string()
                } else if notes.is_empty() {
                    format!(
                        "Reloaded, and your {} unsaved change{} put back on top. Save when ready.",
                        changes.len(),
                        if changes.len() == 1 { " was" } else { "s were" }
                    )
                } else {
                    format!("Reloaded. {}", notes.join(" "))
                };
                self.message = Some(Message {
                    error: !notes.is_empty(),
                    text,
                });
            }
            Err(e) => {
                self.message = Some(Message {
                    error: true,
                    text: e.to_string(),
                })
            }
        }
    }

    fn visible_rows(&self, today: NaiveDate) -> Vec<usize> {
        let stale = self.prefs.stale_days;
        let Some(doc) = &self.doc else {
            return Vec::new();
        };
        let q = self.search.trim().to_lowercase();
        let mut idx: Vec<usize> = doc
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                let a = &r.app;
                let in_filter = match &self.filter {
                    Filter::All => true,
                    Filter::NeedsAttention => a.attention(today, stale).is_some(),
                    Filter::Status(s) => a.status == *s,
                    Filter::OtherStatus => matches!(a.status, Status::Other(_)),
                };
                let in_search = q.is_empty()
                    || [
                        &a.company,
                        &a.role,
                        &a.notes,
                        &a.next_step,
                        &a.contact,
                        &a.source,
                    ]
                    .iter()
                    .any(|f| f.to_lowercase().contains(&q));
                in_filter && in_search
            })
            .map(|(i, _)| i)
            .collect();
        let (key, desc) = self.sort;
        let rows = &doc.rows;
        idx.sort_by(|&x, &y| {
            let (a, b) = (&rows[x].app, &rows[y].app);
            let ord = match key {
                SortKey::Company => a.company.to_lowercase().cmp(&b.company.to_lowercase()),
                SortKey::Role => a.role.to_lowercase().cmp(&b.role.to_lowercase()),
                // Real dates outrank text left in the column, which outranks blanks.
                SortKey::Applied => date_rank(&a.date_applied).cmp(&date_rank(&b.date_applied)),
                SortKey::Status => status_rank(&a.status).cmp(&status_rank(&b.status)),
                SortKey::FollowUp => {
                    let u = |a: &Application| {
                        a.attention(today, stale).map_or(i64::MIN, |t| t.urgency())
                    };
                    u(a).cmp(&u(b))
                        .then_with(|| b.follow_up.display().cmp(&a.follow_up.display()))
                }
            };
            if desc { ord.reverse() } else { ord }
        });
        idx
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let shortcut = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
        if self.pending.is_some() {
            return;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&shortcut(Key::S))) && self.doc.is_some() {
            self.save();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&shortcut(Key::N))) {
            self.add();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&shortcut(Key::F))) {
            self.focus_search = true;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&shortcut(Key::O))) {
            self.pick_file();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&shortcut(Key::Comma))) {
            self.pending = Some(Pending::Settings);
        }
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.is_dirty() && !self.close_confirmed
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.pending = Some(Pending::ConfirmClose);
        }
    }

    /// Draws the whole window into `ui`.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.apply_prefs(&ctx);
        self.update_title(&ctx);
        self.handle_keys(&ctx);
        self.handle_close(&ctx);
        let p = theme::palette(ctx.theme(), self.prefs.accent);
        let today = self.today();

        Panel::top("top")
            .frame(
                Frame::new()
                    .fill(p.page)
                    .inner_margin(Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| self.top_bar(ui, &p));

        if let Some(msg) = self.message.clone() {
            Panel::bottom("message")
                .frame(
                    Frame::new()
                        .fill(p.raised)
                        .inner_margin(Margin::symmetric(16, 8)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let color = if msg.error { p.error } else { p.ink };
                        ui.add(Label::new(RichText::new(&msg.text).color(color)).wrap());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Dismiss").clicked() {
                                self.message = None;
                            }
                        });
                    });
                });
        }

        if self.doc.is_none() {
            egui::CentralPanel::default()
                .frame(Frame::new().fill(p.page).inner_margin(Margin::same(32)))
                .show(ui, |ui| self.welcome(ui, &p));
        } else {
            Panel::left("queue")
                .resizable(false)
                .exact_size(260.0)
                .frame(
                    Frame::new()
                        .fill(p.raised)
                        .inner_margin(Margin::symmetric(14, 14)),
                )
                .show(ui, |ui| self.left_column(ui, &p, today));
            if self.selected.is_some() {
                Panel::right("editor")
                    .resizable(true)
                    .default_size(360.0)
                    .min_size(320.0)
                    .frame(
                        Frame::new()
                            .fill(p.page)
                            .inner_margin(Margin::symmetric(16, 14)),
                    )
                    .show(ui, |ui| self.editor(ui, &p, today));
            }
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(p.page)
                        .inner_margin(Margin::symmetric(16, 14)),
                )
                .show(ui, |ui| self.table(ui, &p, today));
        }

        self.modals(&ctx, &p);
    }

    fn top_bar(&mut self, ui: &mut Ui, p: &Palette) {
        ui.horizontal(|ui| {
            match &self.doc {
                Some(doc) => {
                    ui.label(strong(doc.file_name()).size(17.0).color(p.ink));
                    let n = doc.changes().len();
                    let state = if let Some(why) = &doc.loaded.read_only {
                        let _ = why;
                        "Saving is off".to_string()
                    } else if n == 0 && doc.loaded.adds_columns {
                        "Not saved yet in the new layout".to_string()
                    } else if n == 0 {
                        "All changes saved".to_string()
                    } else {
                        format!("{n} unsaved change{}", if n == 1 { "" } else { "s" })
                    };
                    ui.label(RichText::new(state).color(p.muted));
                }
                None => {
                    ui.label(strong("Job tracker").size(17.0).color(p.ink));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.doc.is_some() {
                    let add = Button::new(RichText::new("Add application").color(p.on_pine))
                        .fill(p.pine)
                        .stroke(egui::Stroke::NONE);
                    if ui.add(add).on_hover_text("Ctrl+N").clicked() {
                        self.add();
                    }
                    let can_save =
                        self.is_dirty() || self.doc.as_ref().is_some_and(|d| d.loaded.adds_columns);
                    let can_save = can_save
                        && self
                            .doc
                            .as_ref()
                            .is_some_and(|d| d.loaded.read_only.is_none());
                    if ui
                        .add_enabled(can_save, Button::new("Save"))
                        .on_hover_text("Ctrl+S")
                        .clicked()
                    {
                        self.save();
                    }
                    if ui
                        .button("Reload")
                        .on_hover_text("Read the workbook again. Unsaved changes are kept.")
                        .clicked()
                    {
                        self.reload_keeping_changes();
                    }
                }
                self.open_menu(ui, p);
                if ui.button("Settings").on_hover_text("Ctrl+,").clicked() {
                    self.pending = Some(Pending::Settings);
                }
            });
        });
    }

    /// The Open menu: browse, or pick one of the recent workbooks.
    fn open_menu(&mut self, ui: &mut Ui, p: &Palette) {
        let mut browse = false;
        let mut chosen = None;
        let mut clear = false;
        let current = self.path().map(Path::to_path_buf);
        ui.menu_button("Open", |ui| {
            ui.set_min_width(340.0);
            if ui
                .button("Browse for a workbook…")
                .on_hover_text("Ctrl+O")
                .clicked()
            {
                browse = true;
                ui.close();
            }
            if !self.prefs.recent.is_empty() {
                ui.separator();
                ui.label(RichText::new("Recent").small().color(p.muted));
                for path in &self.prefs.recent {
                    let is_current = current.as_deref() == Some(path.as_path());
                    let exists = path.exists();
                    let name = file_name(path);
                    let detail = if !exists {
                        "not found".to_string()
                    } else if is_current {
                        "open now".to_string()
                    } else {
                        short_folder(path)
                    };
                    let b = Button::new(name).right_text(RichText::new(detail).color(p.muted));
                    if ui
                        .add_enabled(exists && !is_current, b)
                        .on_hover_text(path.display().to_string())
                        .on_disabled_hover_text(path.display().to_string())
                        .clicked()
                    {
                        chosen = Some(path.clone());
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Clear recent list").clicked() {
                    clear = true;
                    ui.close();
                }
            }
        });
        if browse {
            self.pick_file();
        }
        if let Some(path) = chosen {
            self.request_open(&path);
        }
        if clear {
            self.prefs.recent.retain(|r| Some(r) == current.as_ref());
        }
    }

    fn welcome(&mut self, ui: &mut Ui, p: &Palette) {
        ui.vertical(|ui| {
            ui.set_max_width(520.0);
            if let Some(err) = self.load_error.clone() {
                ui.label(
                    strong("That workbook couldn't be opened")
                        .size(19.0)
                        .color(p.ink),
                );
                ui.add_space(6.0);
                ui.add(Label::new(RichText::new(err).color(p.error)).wrap());
                ui.add_space(14.0);
                if ui.button("Open another workbook…").clicked() {
                    self.pick_file();
                }
            } else {
                ui.label(
                    strong("Open your applications workbook")
                        .size(19.0)
                        .color(p.ink),
                );
                ui.add_space(6.0);
                ui.add(
                    Label::new(
                        RichText::new(
                            "Pick Job_Applications.xlsx in job-search. It needs a sheet named \
                             Applications with the columns Company to Notes. Changes are written \
                             back to that file when you save.",
                        )
                        .color(p.muted),
                    )
                    .wrap(),
                );
                ui.add_space(14.0);
                let open =
                    Button::new(RichText::new("Open workbook…").color(p.on_pine)).fill(p.pine);
                if ui.add(open).clicked() {
                    self.pick_file();
                }
            }

            let recent: Vec<PathBuf> = self.prefs.recent.clone();
            if !recent.is_empty() {
                ui.add_space(28.0);
                ui.label(strong("Recent workbooks").size(17.0).color(p.ink));
                ui.add_space(4.0);
                for (i, path) in recent.iter().enumerate() {
                    if recent_row(ui, p, i, path).clicked() {
                        self.request_open(path);
                    }
                }
            }
        });
    }

    fn left_column(&mut self, ui: &mut Ui, p: &Palette, today: NaiveDate) {
        let stale = self.prefs.stale_days;
        let Some(doc) = &self.doc else { return };
        let mut queue: Vec<(usize, Attention)> = doc
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.app.attention(today, stale).map(|a| (i, a)))
            .collect();
        queue.sort_by_key(|(_, a)| -a.urgency());

        let counts =
            |f: &dyn Fn(&Application) -> bool| doc.rows.iter().filter(|r| f(&r.app)).count();
        let total = doc.rows.len();
        let attention = queue.len();
        let status_counts: Vec<(Status, usize)> = Status::KNOWN
            .iter()
            .map(|s| (s.clone(), counts(&|a: &Application| a.status == *s)))
            .collect();
        let other = counts(&|a: &Application| matches!(a.status, Status::Other(_)));
        let mut clicked_row = None;
        let mut new_filter = None;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(strong("Follow up").size(18.0).color(p.ink));
                    if attention > 0 {
                        ui.label(RichText::new(attention.to_string()).color(p.muted));
                    }
                });
                ui.add_space(4.0);
                if queue.is_empty() {
                    ui.add(
                        Label::new(
                            RichText::new("Nothing needs a follow-up today.").color(p.muted),
                        )
                        .wrap(),
                    );
                }
                for (i, att) in &queue {
                    let app = &doc.rows[*i].app;
                    let selected = self.selected == Some(*i);
                    if queue_item(ui, p, *i, app, att, selected).clicked() {
                        clicked_row = Some(*i);
                    }
                }

                ui.add_space(20.0);
                ui.label(strong("Status").size(18.0).color(p.ink));
                ui.add_space(2.0);
                let f = &self.filter;
                if list_row(ui, p, *f == Filter::All, "All", total).clicked() {
                    new_filter = Some(Filter::All);
                }
                if list_row(
                    ui,
                    p,
                    *f == Filter::NeedsAttention,
                    "Needs follow-up",
                    attention,
                )
                .clicked()
                {
                    new_filter = Some(Filter::NeedsAttention);
                }
                ui.add_space(6.0);
                for (s, n) in &status_counts {
                    let on = *f == Filter::Status(s.clone());
                    if list_row(ui, p, on, s.label(), *n).clicked() {
                        new_filter = Some(Filter::Status(s.clone()));
                    }
                }
                if other > 0 && list_row(ui, p, *f == Filter::OtherStatus, "Other", other).clicked()
                {
                    new_filter = Some(Filter::OtherStatus);
                }
            });

        if let Some(i) = clicked_row {
            self.selected = Some(i);
            if !self.visible_rows(today).contains(&i) {
                self.filter = Filter::All;
                self.search.clear();
            }
        }
        if let Some(f) = new_filter {
            self.filter = f;
        }
    }

    fn table(&mut self, ui: &mut Ui, p: &Palette, today: NaiveDate) {
        let stale = self.prefs.stale_days;
        let visible = self.visible_rows(today);
        let total = self.doc.as_ref().map_or(0, |d| d.rows.len());

        ui.horizontal(|ui| {
            let label = ui.label(RichText::new("Search").color(p.muted));
            let resp = ui
                .add(
                    TextEdit::singleline(&mut self.search)
                        .hint_text("Company, role, notes, contact")
                        .desired_width(260.0),
                )
                .labelled_by(label.id);
            if self.focus_search {
                resp.request_focus();
                self.focus_search = false;
            }
            if !self.search.is_empty() || self.filter != Filter::All {
                ui.label(
                    RichText::new(format!("Showing {} of {total}", visible.len())).color(p.muted),
                );
            }
        });
        ui.add_space(8.0);

        if total == 0 {
            ui.add_space(24.0);
            ui.label(
                RichText::new("No applications yet. Add the first one with Add application.")
                    .color(p.muted),
            );
            return;
        }
        if visible.is_empty() {
            ui.add_space(24.0);
            ui.label(
                RichText::new("Nothing matches. Clear the search or pick All under Status.")
                    .color(p.muted),
            );
            return;
        }

        let Some(doc) = &self.doc else { return };
        let mut clicked = None;
        let mut sort = self.sort;
        let selected = self.selected;
        let heading = |ui: &mut Ui, sort: &mut (SortKey, bool), key: SortKey, name: &str| {
            let mark = if sort.0 == key {
                if sort.1 { " ↓" } else { " ↑" }
            } else {
                ""
            };
            let text =
                strong(format!("{name}{mark}")).color(if sort.0 == key { p.ink } else { p.muted });
            if ui
                .add(Button::new(text).frame(false))
                .on_hover_text("Sort")
                .clicked()
            {
                *sort = if sort.0 == key {
                    (key, !sort.1)
                } else {
                    (key, key == SortKey::Applied || key == SortKey::FollowUp)
                };
            }
        };

        TableBuilder::new(ui)
            .striped(false)
            .sense(Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::initial(140.0).at_least(90.0).clip(true))
            .column(Column::remainder().at_least(100.0).clip(true))
            .column(Column::exact(86.0))
            .column(Column::exact(86.0))
            .column(Column::exact(148.0))
            .header(28.0, |mut h| {
                h.col(|ui| heading(ui, &mut sort, SortKey::Company, "Company"));
                h.col(|ui| heading(ui, &mut sort, SortKey::Role, "Role"));
                h.col(|ui| heading(ui, &mut sort, SortKey::Applied, "Applied"));
                h.col(|ui| heading(ui, &mut sort, SortKey::Status, "Status"));
                h.col(|ui| heading(ui, &mut sort, SortKey::FollowUp, "Follow up"));
            })
            .body(|body| {
                body.rows(32.0, visible.len(), |mut row| {
                    let i = visible[row.index()];
                    let a = &doc.rows[i].app;
                    row.set_selected(selected == Some(i));
                    let cell = |ui: &mut Ui, text: RichText| {
                        ui.add(Label::new(text).truncate().selectable(false));
                    };
                    let company = if a.company.is_empty() {
                        "(no company)".into()
                    } else {
                        a.company.clone()
                    };
                    row.col(|ui| cell(ui, strong(company).color(p.ink)));
                    row.col(|ui| cell(ui, RichText::new(&a.role).color(p.ink)));
                    row.col(|ui| cell(ui, RichText::new(a.date_applied.display()).color(p.muted)));
                    let status_color = if a.status.is_closed() { p.muted } else { p.ink };
                    row.col(|ui| cell(ui, RichText::new(a.status.label()).color(status_color)));
                    row.col(|ui| match a.attention(today, stale) {
                        Some(att) => cell(ui, strong(att.short()).color(p.pine)),
                        None => cell(ui, RichText::new(a.follow_up.display()).color(p.muted)),
                    });
                    if row.response().clicked() {
                        clicked = Some(i);
                    }
                });
            });

        self.sort = sort;
        if let Some(i) = clicked {
            self.selected = if self.selected == Some(i) {
                None
            } else {
                Some(i)
            };
        }
    }

    fn editor(&mut self, ui: &mut Ui, p: &Palette, today: NaiveDate) {
        let Some(i) = self.selected else { return };
        let resumes = self.resumes.clone();
        let focus_company = std::mem::take(&mut self.focus_company);
        let Some(doc) = self.doc.as_mut() else { return };
        if i >= doc.rows.len() {
            self.selected = None;
            return;
        }
        let extra_headers = doc.loaded.sheet.extra_headers.clone();
        let is_new = doc.rows[i].origin.is_none();
        let a = &mut doc.rows[i].app;
        let errors = crate::model::validate(a, today);
        let error_for = |f: Field| errors.iter().find(|(x, _)| *x == f).map(|(_, m)| *m);
        let mut close = false;
        let mut delete = false;

        ui.horizontal(|ui| {
            let title = if a.company.trim().is_empty() {
                if is_new {
                    "New application".to_string()
                } else {
                    "(no company)".to_string()
                }
            } else {
                a.company.clone()
            };
            ui.add(Label::new(strong(title).size(19.0).color(p.ink)).truncate());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .button("Close")
                    .on_hover_text("Close the editor")
                    .clicked()
                {
                    close = true;
                }
            });
        });
        ui.add_space(8.0);

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let w = ui.available_width();
                let field = |ui: &mut Ui, label: &str, value: &mut String, err: Option<&str>| {
                    let l = ui.label(RichText::new(label).color(p.muted));
                    let r = ui
                        .add(TextEdit::singleline(value).desired_width(w))
                        .labelled_by(l.id);
                    error_line(ui, p, err);
                    ui.add_space(6.0);
                    r
                };

                let r = field(ui, "Company", &mut a.company, error_for(Field::Company));
                if focus_company {
                    r.request_focus();
                }
                field(ui, "Role", &mut a.role, error_for(Field::Role));

                let l = ui.label(RichText::new("Status").color(p.muted));
                ComboBox::from_id_salt("status")
                    .width(w.min(220.0))
                    .selected_text(a.status.label().to_string())
                    .show_ui(ui, |ui| {
                        for s in Status::KNOWN.iter() {
                            ui.selectable_value(&mut a.status, s.clone(), s.label());
                        }
                    })
                    .response
                    .labelled_by(l.id);
                ui.add_space(6.0);

                ui.columns(2, |cols| {
                    date_field(
                        &mut cols[0],
                        p,
                        "applied",
                        "Date applied",
                        &mut a.date_applied,
                        today,
                    );
                    error_line(&mut cols[0], p, error_for(Field::DateApplied));
                    date_field(
                        &mut cols[1],
                        p,
                        "follow-up",
                        "Follow-up date",
                        &mut a.follow_up,
                        today,
                    );
                });
                ui.add_space(6.0);

                field(ui, "Next step", &mut a.next_step, None);

                let l = ui.label(RichText::new("Source").color(p.muted));
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(&mut a.source).desired_width(w - 80.0))
                        .labelled_by(l.id);
                    ui.menu_button("Pick", |ui| {
                        for s in SOURCES {
                            if ui.button(s).clicked() {
                                a.source = s.to_string();
                            }
                        }
                    });
                });
                ui.add_space(6.0);

                let l = ui.label(RichText::new("Resume sent").color(p.muted));
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(&mut a.resume).desired_width(w - 80.0))
                        .labelled_by(l.id);
                    ui.add_enabled_ui(!resumes.is_empty(), |ui| {
                        ui.menu_button("Pick", |ui| {
                            ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                                for r in &resumes {
                                    if ui.button(r).clicked() {
                                        a.resume = r.clone();
                                    }
                                }
                            });
                        })
                        .response
                        .on_disabled_hover_text("No resumes folder found next to job-search");
                    });
                });
                ui.add_space(6.0);

                field(ui, "Location", &mut a.location, None);
                field(ui, "Compensation", &mut a.compensation, None);

                let l = ui.label(RichText::new("Contact").color(p.muted));
                ui.add(
                    TextEdit::multiline(&mut a.contact)
                        .desired_rows(2)
                        .desired_width(w),
                )
                .labelled_by(l.id);
                ui.add_space(6.0);

                let l = ui.label(RichText::new("Link").color(p.muted));
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(&mut a.link).desired_width(w - 80.0))
                        .labelled_by(l.id);
                    let link = a.link.trim().to_string();
                    let ok = link.starts_with("http://") || link.starts_with("https://");
                    if ui
                        .add_enabled(ok, Button::new("Open"))
                        .on_hover_text("Open in the browser")
                        .clicked()
                    {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(link));
                    }
                });
                error_line(ui, p, error_for(Field::Link));
                ui.add_space(6.0);

                let l = ui.label(RichText::new("Notes").color(p.muted));
                ui.add(
                    TextEdit::multiline(&mut a.notes)
                        .desired_rows(6)
                        .desired_width(w),
                )
                .labelled_by(l.id);

                if !extra_headers.is_empty() {
                    ui.add_space(12.0);
                    ui.label(strong("Other columns").color(p.ink));
                    ui.label(
                        RichText::new("Edit these in Excel; the app keeps them as they are.")
                            .small()
                            .color(p.muted),
                    );
                    for (h, v) in extra_headers.iter().zip(a.extra.iter()) {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(if h.is_empty() { "(no header)" } else { h })
                                    .color(p.muted),
                            );
                            ui.label(RichText::new(v.display()).color(p.ink));
                        });
                    }
                }

                ui.add_space(18.0);
                let del = Button::new(RichText::new("Delete application").color(p.error))
                    .fill(Color32::TRANSPARENT)
                    .stroke(egui::Stroke::new(1.0, p.lines));
                if ui.add(del).clicked() {
                    delete = true;
                }
                ui.add_space(8.0);
            });

        if close {
            self.selected = None;
        }
        if delete {
            self.pending = Some(Pending::ConfirmDelete(i));
        }
    }

    fn modals(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(pending) = self.pending.clone() else {
            return;
        };
        let frame = Frame::popup(&ctx.global_style())
            .fill(p.page)
            .inner_margin(Margin::same(20));
        let mut done = false;
        let response = Modal::new(Id::unique("job-tracker-modal")).frame(frame).show(ctx, |ui| {
            ui.set_max_width(420.0);
            match pending.clone() {
                Pending::ConfirmDelete(i) => {
                    let name = self
                        .doc
                        .as_ref()
                        .and_then(|d| d.rows.get(i))
                        .map(|r| format!("{} ({})", r.app.company, r.app.role))
                        .unwrap_or_default();
                    ui.label(strong("Delete this application?").size(18.0).color(p.ink));
                    ui.add_space(4.0);
                    ui.add(Label::new(RichText::new(format!(
                        "{name} leaves the list now, and the workbook when you save. The copy in Google Drive goes too."
                    )).color(p.muted)).wrap());
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let del = Button::new(RichText::new("Delete").color(p.on_pine)).fill(p.error);
                        if ui.add(del).clicked() {
                            self.delete(i);
                            done = true;
                        }
                        if ui.button("Keep it").clicked() {
                            done = true;
                        }
                    });
                }
                Pending::ConfirmClose => {
                    let n = self.doc.as_ref().map_or(0, |d| d.changes().len());
                    ui.label(strong("Close without saving?").size(18.0).color(p.ink));
                    ui.add_space(4.0);
                    ui.add(Label::new(RichText::new(format!(
                        "{n} change{} to {} haven't been saved.",
                        if n == 1 { "" } else { "s" },
                        self.doc.as_ref().map(|d| d.file_name()).unwrap_or_default()
                    )).color(p.muted)).wrap());
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let save = Button::new(RichText::new("Save and close").color(p.on_pine)).fill(p.pine);
                        if ui.add(save).clicked() {
                            done = true;
                            if self.save() {
                                self.close_confirmed = true;
                                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                            }
                        }
                        if ui.button("Close without saving").clicked() {
                            done = true;
                            self.close_confirmed = true;
                            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                        }
                        if ui.button("Keep editing").clicked() {
                            done = true;
                        }
                    });
                }
                Pending::ChangedOnDisk => {
                    ui.label(strong("The workbook changed since it was opened").size(18.0).color(p.ink));
                    ui.add_space(4.0);
                    ui.add(Label::new(RichText::new(
                        "Something else saved it: Excel, Google Drive from another computer, or a Claude Code \
                         session. Reload it to see those changes; your unsaved changes are put back on top, \
                         then you can save.",
                    ).color(p.muted)).wrap());
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let reload = Button::new(RichText::new("Reload and keep my changes").color(p.on_pine)).fill(p.pine);
                        if ui.add(reload).clicked() {
                            done = true;
                            // Set before reloading, so a message from the reload isn't cleared below.
                            self.pending = None;
                            self.reload_keeping_changes();
                        }
                        if ui.button("Not now").clicked() {
                            done = true;
                        }
                    });
                }
                Pending::ConfirmSwitch(path) => {
                    let n = self.doc.as_ref().map_or(0, |d| d.changes().len());
                    let current = self.doc.as_ref().map(|d| d.file_name()).unwrap_or_default();
                    ui.label(strong(format!("Open {} without saving?", file_name(&path))).size(18.0).color(p.ink));
                    ui.add_space(4.0);
                    ui.add(Label::new(RichText::new(format!(
                        "{n} change{} to {current} haven't been saved.",
                        if n == 1 { "" } else { "s" },
                    )).color(p.muted)).wrap());
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let save = Button::new(RichText::new("Save, then open").color(p.on_pine)).fill(p.pine);
                        if ui.add(save).clicked() {
                            done = true;
                            if self.save() {
                                self.open(&path);
                            }
                        }
                        if ui.button("Open without saving").clicked() {
                            done = true;
                            self.open(&path);
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    });
                }
                Pending::Settings => {
                    if self.settings(ui, p) {
                        done = true;
                    }
                }
            }
        });
        // Only clear the dialog that was shown: an action inside it may have raised another one
        // (a save that finds the file changed on disk asks about that next).
        if (done || response.should_close()) && self.pending.as_ref() == Some(&pending) {
            self.pending = None;
        }
    }

    /// The Settings dialog. Changes apply as they're made. Returns true when Done is clicked.
    fn settings(&mut self, ui: &mut Ui, p: &Palette) -> bool {
        let mut done = false;
        let mut rescan = false;
        ui.set_width(760.0);
        ui.spacing_mut().slider_width = 230.0;
        ui.label(strong("Settings").size(19.0).color(p.ink));
        ui.add_space(8.0);
        let section = |ui: &mut Ui, title: &str| {
            ui.add_space(10.0);
            ui.label(strong(title).color(p.ink));
            ui.add_space(2.0);
        };
        let prefs = &mut self.prefs;

        ui.columns(2, |cols| {
            let (left, right) = cols.split_at_mut(1);
            let ui = &mut left[0];
            section(ui, "Appearance");
            let l = ui.label(RichText::new("Theme").color(p.muted));
            ui.horizontal(|ui| {
                for t in [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark] {
                    ui.radio_value(&mut prefs.theme, t, t.label())
                        .labelled_by(l.id);
                }
            });
            ui.label(RichText::new("Accent colour").color(p.muted));
            ui.horizontal_wrapped(|ui| {
                for a in Accent::ALL {
                    // Each name is drawn in its own colour, so the choice previews itself.
                    let shown = theme::palette(ui.ctx().theme(), a).pine;
                    ui.radio_value(&mut prefs.accent, a, RichText::new(a.label()).color(shown));
                }
            });
            let l = ui.label(RichText::new("Text size").color(p.muted));
            let mut pct = (prefs.text_scale * 100.0).round() as i32;
            if ui
                .add(
                    egui::Slider::new(&mut pct, 80..=160)
                        .step_by(10.0)
                        .suffix("%"),
                )
                .labelled_by(l.id)
                .changed()
            {
                prefs.text_scale = pct as f32 / 100.0;
            }

            section(ui, "Follow-ups");
            let l = ui.label(
                RichText::new("Flag Applied or Screening with no follow-up date after")
                    .color(p.muted),
            );
            ui.add(egui::Slider::new(&mut prefs.stale_days, 3..=60).suffix(" days"))
                .labelled_by(l.id);
            let mut auto = prefs.follow_up_after_days > 0;
            if ui
                .checkbox(&mut auto, "Give new applications a follow-up date")
                .changed()
            {
                prefs.follow_up_after_days = if auto { 7 } else { 0 };
            }
            if auto {
                let l = ui.label(RichText::new("Days after applying").color(p.muted));
                ui.add(egui::Slider::new(&mut prefs.follow_up_after_days, 1..=30).suffix(" days"))
                    .labelled_by(l.id);
            }

            let ui = &mut right[0];
            section(ui, "Starting up");
            ui.checkbox(
                &mut prefs.reopen_last,
                "Open the most recent workbook when the app starts",
            );
            ui.horizontal(|ui| {
                let n = prefs.recent.len();
                ui.label(
                    RichText::new(format!(
                        "{n} workbook{} in the Recent list",
                        if n == 1 { "" } else { "s" }
                    ))
                    .color(p.muted),
                );
                if ui.add_enabled(n > 0, Button::new("Clear list")).clicked() {
                    prefs.recent.clear();
                }
            });

            section(ui, "Backups");
            let l = ui.label(RichText::new("Backups to keep for each workbook").color(p.muted));
            ui.add(egui::Slider::new(&mut prefs.keep_backups, 5..=100))
                .labelled_by(l.id);
            ui.add(
                Label::new(
                    RichText::new(self.backup_dir.display().to_string())
                        .small()
                        .color(p.muted),
                )
                .truncate(),
            );
            ui.horizontal(|ui| {
                if ui.button("Open backups folder").clicked() {
                    let _ = std::fs::create_dir_all(&self.backup_dir);
                    let _ = std::process::Command::new("explorer")
                        .arg(&self.backup_dir)
                        .spawn();
                }
            });

            section(ui, "Resumes");
            let auto_dir = self
                .doc
                .as_ref()
                .and_then(|d| auto_resumes_dir(&d.loaded.path));
            let shown = match (&prefs.resumes_dir, &auto_dir) {
                (Some(dir), _) => dir.display().to_string(),
                (None, Some(dir)) => format!("{} (found next to job-search)", dir.display()),
                (None, None) => "Not found. Choose the folder that holds your resume PDFs.".into(),
            };
            ui.add(Label::new(RichText::new(shown).small().color(p.muted)).wrap());
            ui.horizontal(|ui| {
                if ui.button("Choose folder…").clicked()
                    && let Some(dir) = rfd::FileDialog::new().pick_folder()
                {
                    prefs.resumes_dir = Some(dir);
                    rescan = true;
                }
                if prefs.resumes_dir.is_some() && ui.button("Find it automatically").clicked() {
                    prefs.resumes_dir = None;
                    rescan = true;
                }
            });
        });

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            let b = Button::new(RichText::new("Done").color(p.on_pine)).fill(p.pine);
            if ui.add(b).clicked() {
                done = true;
            }
            if ui.button("Reset to defaults").clicked() {
                let recent = std::mem::take(&mut self.prefs.recent);
                self.prefs = Prefs {
                    recent,
                    ..Prefs::default()
                };
                rescan = true;
            }
        });
        if rescan {
            self.rescan_resumes();
        }
        done
    }
}

fn date_rank(d: &DateCell) -> (u8, String) {
    match d {
        DateCell::Date(_) => (2, d.display()),
        DateCell::Text(s) => (1, s.to_lowercase()),
        DateCell::Empty => (0, String::new()),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// The last two folders of a path, e.g. `Work Stuff\job-search`, to tell same-named files apart.
fn short_folder(path: &Path) -> String {
    let parts: Vec<String> = path
        .parent()
        .map(|p| {
            p.components()
                .rev()
                .take(2)
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    parts.into_iter().rev().collect::<Vec<_>>().join("\\")
}

/// One workbook in the welcome screen's Recent list.
fn recent_row(ui: &mut Ui, p: &Palette, i: usize, path: &Path) -> egui::Response {
    let exists = path.exists();
    let name = file_name(path);
    let resp = ui
        .add_enabled_ui(exists, |ui| {
            ui.scope_builder(
                UiBuilder::new()
                    .id_salt(("recent", i))
                    .sense(Sense::click()),
                |ui| {
                    let hovered = ui.response().hovered();
                    Frame::new()
                        .fill(if hovered { p.hover } else { p.page })
                        .stroke(egui::Stroke::new(1.0, p.lines))
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(Label::new(strong(&name).color(p.ink)).selectable(false));
                            let detail = if exists {
                                path.parent()
                                    .map(|d| d.display().to_string())
                                    .unwrap_or_default()
                            } else {
                                format!("Not found: {}", path.display())
                            };
                            ui.add(
                                Label::new(RichText::new(detail).small().color(p.muted))
                                    .truncate()
                                    .selectable(false),
                            );
                        });
                },
            )
            .response
        })
        .inner;
    let folder = path
        .parent()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    resp.widget_info(|| {
        WidgetInfo::labeled(Role::Button, exists, format!("Open {name} from {folder}"))
    });
    ui.add_space(6.0);
    resp
}

fn status_rank(s: &Status) -> usize {
    Status::KNOWN
        .iter()
        .position(|k| k == s)
        .unwrap_or(Status::KNOWN.len())
}

fn error_line(ui: &mut Ui, p: &Palette, err: Option<&str>) {
    if let Some(e) = err {
        ui.label(RichText::new(e).color(p.error).small());
    }
}

fn date_field(
    ui: &mut Ui,
    p: &Palette,
    id: &'static str,
    label: &str,
    cell: &mut DateCell,
    today: NaiveDate,
) {
    let l = ui.label(RichText::new(label).color(p.muted));
    ui.horizontal(|ui| match cell.clone() {
        DateCell::Empty => {
            if ui.button("Add date").labelled_by(l.id).clicked() {
                *cell = DateCell::Date(today);
            }
        }
        DateCell::Date(d) => {
            let mut j = to_jiff(d);
            ui.add(
                DatePickerButton::new(&mut j)
                    .id_salt(id)
                    .show_icon(false)
                    .calendar_week(false)
                    .highlight_weekends(false),
            )
            .labelled_by(l.id);
            if let Some(nd) = from_jiff(j)
                && nd != d
            {
                *cell = DateCell::Date(nd);
            }
            if ui
                .small_button("Clear")
                .on_hover_text(format!("Clear {}", label.to_lowercase()))
                .clicked()
            {
                *cell = DateCell::Empty;
            }
        }
        DateCell::Text(s) => {
            ui.vertical(|ui| {
                ui.label(RichText::new(format!("\"{s}\" (kept as typed)")).color(p.ink));
                if ui
                    .small_button("Use today instead")
                    .labelled_by(l.id)
                    .clicked()
                {
                    *cell = DateCell::Date(today);
                }
            });
        }
    });
}

/// A full-width row in the Status list: name on the left, count on the right.
fn list_row(ui: &mut Ui, p: &Palette, selected: bool, name: &str, count: usize) -> egui::Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 28.0), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(Role::Button, true, selected, name));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if selected {
            p.selected
        } else if resp.hovered() {
            p.hover
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(rect, CornerRadius::same(4), fill);
        if selected {
            let bar = egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height()));
            painter.rect_filled(bar, CornerRadius::same(1), p.pine);
        }
        if resp.has_focus() {
            painter.rect_stroke(
                rect,
                CornerRadius::same(4),
                egui::Stroke::new(1.5, p.pine),
                egui::StrokeKind::Inside,
            );
        }
        let font = if selected {
            egui::FontId::new(15.0, egui::FontFamily::Name(theme::SEMIBOLD.into()))
        } else {
            egui::FontId::proportional(15.0)
        };
        let ink = if count == 0 && !selected {
            p.muted
        } else {
            p.ink
        };
        painter.text(
            rect.left_center() + egui::vec2(12.0, 0.0),
            egui::Align2::LEFT_CENTER,
            name,
            font,
            ink,
        );
        painter.text(
            rect.right_center() - egui::vec2(10.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            count.to_string(),
            egui::FontId::proportional(15.0),
            p.muted,
        );
    }
    resp
}

/// One entry in the Follow up queue.
fn queue_item(
    ui: &mut Ui,
    p: &Palette,
    i: usize,
    a: &Application,
    att: &Attention,
    selected: bool,
) -> egui::Response {
    let label = format!("{} {}: {}", a.company, a.role, att.reason());
    let resp = ui
        .scope_builder(
            UiBuilder::new().id_salt(("queue", i)).sense(Sense::click()),
            |ui| {
                let hovered = ui.response().hovered();
                let fill = if selected {
                    p.selected
                } else if hovered {
                    p.hover
                } else {
                    p.page
                };
                Frame::new()
                    .fill(fill)
                    .stroke(egui::Stroke::new(
                        1.0,
                        if selected { p.pine } else { p.lines },
                    ))
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let company = if a.company.is_empty() {
                            "(no company)"
                        } else {
                            &a.company
                        };
                        ui.add(
                            Label::new(strong(company).color(p.ink))
                                .truncate()
                                .selectable(false),
                        );
                        ui.add(
                            Label::new(RichText::new(&a.role).small().color(p.muted))
                                .truncate()
                                .selectable(false),
                        );
                        ui.add(
                            Label::new(RichText::new(att.reason()).small().color(p.pine))
                                .wrap()
                                .selectable(false),
                        );
                        if !a.next_step.trim().is_empty() && !matches!(att, Attention::NoNextStep) {
                            ui.add(
                                Label::new(
                                    RichText::new(format!("Next: {}", a.next_step))
                                        .small()
                                        .color(p.ink),
                                )
                                .wrap()
                                .selectable(false),
                            );
                        }
                    });
            },
        )
        .response;
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, &label));
    ui.add_space(6.0);
    resp
}
