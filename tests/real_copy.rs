//! Saves a COPY of the real workbook through the app and checks it reads back the same.
//! Ignored by default. Never point this at the real file:
//!   $env:JT_REAL_COPY = "C:\...\scratch\Job_Applications.xlsx"; cargo test --test real_copy -- --ignored

use std::path::PathBuf;

use job_tracker::model::DateCell;
use job_tracker::store::{self, SaveOptions};

#[test]
#[ignore]
fn real_copy_round_trips() {
    let path = PathBuf::from(std::env::var("JT_REAL_COPY").expect("set JT_REAL_COPY to a copy"));
    assert!(
        !path.to_string_lossy().contains("Work Stuff"),
        "use a copy outside the synced folder"
    );
    let mut loaded = store::load(&path).unwrap();
    let before = loaded.sheet.clone();
    println!(
        "rows: {}, adds columns: {}, read only: {:?}",
        before.rows.len(),
        loaded.adds_columns,
        loaded.read_only
    );
    for r in &before.rows {
        let today = chrono::Local::now().date_naive();
        println!(
            "  {} | {} | {:?} | {} | attention: {:?}",
            r.company,
            r.status.label(),
            r.date_applied,
            matches!(r.date_applied, DateCell::Date(_)),
            r.attention(today, job_tracker::model::DEFAULT_STALE_DAYS)
                .map(|a| a.reason())
        );
    }
    let dir = path.parent().unwrap().join("backups");
    store::save(
        &mut loaded,
        &before,
        &SaveOptions {
            backup_dir: &dir,
            stamp: "check".into(),
            today: chrono::Local::now().date_naive(),
            keep: 20,
        },
    )
    .unwrap();
    assert_eq!(store::load(&path).unwrap().sheet, before);
}
