#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use job_tracker::prefs::{APP_ID, PREFS_KEY, Prefs};
use job_tracker::{theme, ui::TrackerApp, ui::default_backup_dir};

/// Where the first version kept the last workbook; read once and moved into the Recent list.
const OLD_PATH_KEY: &str = "workbook";

struct App(TrackerApp);

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, PREFS_KEY, &self.0.prefs);
        storage.remove_string(OLD_PATH_KEY);
    }
}

fn load_prefs(storage: Option<&dyn eframe::Storage>) -> Prefs {
    let Some(storage) = storage else {
        return Prefs::default();
    };
    let mut prefs: Prefs = eframe::get_value(storage, PREFS_KEY).unwrap_or_default();
    if prefs.recent.is_empty()
        && let Some(old) = storage.get_string(OLD_PATH_KEY)
    {
        prefs.remember(&PathBuf::from(old));
    }
    prefs.clamp();
    prefs
}

fn main() -> eframe::Result {
    let arg = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Job tracker")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([1040.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            let mut app = TrackerApp::new(default_backup_dir());
            app.prefs = load_prefs(cc.storage);
            let startup = arg.or_else(|| app.prefs.startup_file().map(PathBuf::from));
            if let Some(path) = startup {
                app.open(&path);
            }
            Ok(Box::new(App(app)))
        }),
    )
}
