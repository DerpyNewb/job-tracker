//! Renders the window headless with egui_kittest. Snapshots land in tests/snapshots/*.png, so the
//! screens can be looked at without opening the app. Rows are fictional (tests/fixtures).

use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use egui_kittest::{Harness, SnapshotResults};
use job_tracker::model::{DateCell, Status};
use job_tracker::prefs::ThemeChoice;
use job_tracker::theme;
use job_tracker::ui::TrackerApp;

/// wgpu crashed (access violation) when several harnesses rendered at once, so tests take turns.
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 5).unwrap()
}

fn workspace(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("job-tracker-screens-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("job-search")).unwrap();
    fs::create_dir_all(dir.join("resumes").join("per-posting")).unwrap();
    for r in [
        "Yohann_Resume",
        "Yohann_Resume_data_analyst",
        "per-posting/Yohann_Gayao_Resume_QA_Engineer",
    ] {
        fs::write(dir.join("resumes").join(format!("{r}.pdf")), b"%PDF").unwrap();
    }
    let wb = dir.join("job-search").join("Job_Applications.xlsx");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy.xlsx"),
        &wb,
    )
    .unwrap();
    (dir, wb)
}

fn app(name: &str) -> (TrackerApp, PathBuf) {
    let (dir, wb) = workspace(name);
    let mut app = TrackerApp::new(dir.join("backups"));
    app.fixed_today = Some(today());
    app.open(&wb);
    // Give the first fixture row a follow-up date that is due, so every reason shows.
    if let Some(doc) = app.doc.as_mut() {
        doc.rows[3].app.follow_up = DateCell::Date(NaiveDate::from_ymd_opt(2026, 11, 3).unwrap());
        doc.loaded.sheet.rows[3].follow_up = doc.rows[3].app.follow_up.clone();
    }
    (app, wb)
}

fn harness(mut app: TrackerApp, theme_: egui::Theme) -> Harness<'static, TrackerApp> {
    // The app applies its Theme setting; "Match Windows" would follow the harness, so pin it.
    app.prefs.theme = match theme_ {
        egui::Theme::Light => ThemeChoice::Light,
        egui::Theme::Dark => ThemeChoice::Dark,
    };
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_theme(theme_)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut TrackerApp| app.ui(ui), app)
}

#[test]
fn screens() {
    let _gpu = gpu();
    let mut results = SnapshotResults::new();

    // Overview, light and dark.
    let (a, _) = app("overview-light");
    let mut h = harness(a, egui::Theme::Light);
    h.run();
    results.add(h.try_snapshot("overview_light"));
    let (a, _) = app("overview-dark");
    let mut h = harness(a, egui::Theme::Dark);
    h.run();
    results.add(h.try_snapshot("overview_dark"));

    // A row picked from the Follow up queue opens the editor.
    let (a, _) = app("editor");
    let mut h = harness(a, egui::Theme::Light);
    h.run();
    h.get_by_label_contains("Kennon Labs AI Engineer").click();
    h.run();
    assert_eq!(h.state().selected, Some(3));
    results.add(h.try_snapshot("editor_light"));

    // Delete asks first. The button sits below the fold at 800 px, so make the window taller.
    h.set_size(egui::vec2(1280.0, 1200.0));
    h.run();
    h.get_by_label("Delete application").click();
    h.run();
    results.add(h.try_snapshot("confirm_delete"));
    h.get_by_label("Keep it").click();
    h.run();
    assert_eq!(h.state().doc.as_ref().unwrap().rows.len(), 6);

    // Nothing open yet, with two recent workbooks: one there, one moved away.
    let (_, wb) = workspace("welcome");
    let mut a = TrackerApp::new(std::env::temp_dir());
    a.fixed_today = Some(today());
    a.prefs
        .remember(Path::new(r"C:\old-laptop\Job_Applications.xlsx"));
    a.prefs.remember(&wb);
    let mut h = harness(a, egui::Theme::Light);
    h.run();
    results.add(h.try_snapshot("welcome_recent"));
    h.get_by_label(&format!(
        "Open Job_Applications.xlsx from {}",
        wb.parent().unwrap().display()
    ))
    .click();
    h.run();
    assert_eq!(
        h.state().path(),
        Some(wb.as_path()),
        "a recent workbook opens with one click"
    );

    // Settings.
    h.get_by_label("Settings").click();
    h.run();
    results.add(h.try_snapshot("settings"));
    // Taller window: the whole dialog, Backups and Resumes included.
    h.set_size(egui::vec2(1280.0, 1100.0));
    h.run();
    results.add(h.try_snapshot("settings_full"));
    h.get_by_label("Done").click();
    h.run();
}

#[test]
fn switching_workbooks_asks_about_unsaved_changes() {
    let _gpu = gpu();
    let (a, first) = app("switch");
    let (_, second) = workspace("switch-second");
    let mut h = harness(a, egui::Theme::Light);
    h.run();
    h.get_by_label("Add application").click();
    h.run();
    h.state_mut().request_open(&second);
    h.run();
    assert!(h.query_by_label_contains("haven't been saved").is_some());
    h.get_by_label("Cancel").click();
    h.run();
    assert_eq!(h.state().path(), Some(first.as_path()), "Cancel stays put");
    assert!(h.state().is_dirty());

    h.state_mut().request_open(&second);
    h.run();
    h.get_by_label("Open without saving").click();
    h.run();
    assert_eq!(h.state().path(), Some(second.as_path()));
    assert!(!h.state().is_dirty());
    assert_eq!(h.state().prefs.recent[0], second, "most recent first");
}

#[test]
fn new_applications_can_get_a_follow_up_date() {
    let _gpu = gpu();
    let (mut a, _) = app("auto-follow-up");
    a.prefs.follow_up_after_days = 7;
    let mut h = harness(a, egui::Theme::Light);
    h.run();
    h.get_by_label("Add application").click();
    h.run();
    let doc = h.state().doc.as_ref().unwrap();
    assert_eq!(
        doc.rows.last().unwrap().app.follow_up,
        DateCell::Date(NaiveDate::from_ymd_opt(2026, 11, 12).unwrap())
    );
}

#[test]
fn add_edit_and_save_from_the_window() {
    let _gpu = gpu();
    let (a, wb) = app("add-save");
    let mut h = harness(a, egui::Theme::Light);
    h.run();
    h.get_by_label("Add application").click();
    h.run();
    // type_text goes to the focused widget, so click each field first.
    for (field, text) in [
        ("Company", "Upper Session Labs"),
        ("Role", "Data Analyst"),
        ("Next step", "Email the recruiter"),
    ] {
        h.get_by_role_and_label(Role::TextInput, field).click();
        h.run();
        h.get_by_role_and_label(Role::TextInput, field)
            .type_text(text);
        h.run();
    }
    h.get_by_label("Save").click();
    h.run();

    let saved = job_tracker::store::load(&wb).unwrap();
    assert_eq!(saved.sheet.rows.len(), 7);
    let new = saved.sheet.rows.last().unwrap();
    assert_eq!(new.company, "Upper Session Labs");
    assert_eq!(new.role, "Data Analyst");
    assert_eq!(new.status, Status::Applied);
    assert_eq!(new.date_applied, DateCell::Date(today()));
    assert_eq!(new.next_step, "Email the recruiter");
    assert!(!h.state().is_dirty());
}

#[test]
fn save_with_a_missing_role_shows_the_problem() {
    let _gpu = gpu();
    let (a, wb) = app("invalid");
    let before = fs::read(&wb).unwrap();
    let mut h = harness(a, egui::Theme::Light);
    h.run();
    h.get_by_label("Add application").click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Company")
        .type_text("Half Done Inc");
    h.run();
    h.get_by_label("Save").click();
    h.run();
    assert_eq!(fs::read(&wb).unwrap(), before, "nothing written");
    // Once under the field, once in the message bar.
    assert_eq!(
        h.query_all_by_label_contains("Role is required.").count(),
        2
    );
    h.try_snapshot("invalid_save").unwrap();
}

#[test]
fn sample_workbook_screen() {
    let _gpu = gpu();
    let (dir, _) = workspace("sample");
    let path = dir.join("job-search").join("Sample_Job_Applications.xlsx");
    job_tracker::sheet::write(&path, &job_tracker::sample::sample_sheet(today())).unwrap();
    let mut a = TrackerApp::new(dir.join("backups"));
    a.fixed_today = Some(today());
    a.open(&path);
    let mut h = harness(a, egui::Theme::Light);
    h.run();
    h.try_snapshot("sample_overview").unwrap();
}
