//! Writes the fictional sample workbook: `cargo run --example make_sample -- <path.xlsx>`.
//! Default path: sample/Sample_Job_Applications.xlsx. Refuses to overwrite a file whose name
//! isn't a sample's, so it can't replace the real workbook by mistake.

use std::path::PathBuf;

fn main() {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("sample/Sample_Job_Applications.xlsx"));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if path.exists() && !name.contains("sample") {
        eprintln!(
            "{} exists and isn't a sample file; not overwriting it.",
            path.display()
        );
        std::process::exit(1);
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).expect("create folder");
    }
    let today = chrono::Local::now().date_naive();
    let sheet = job_tracker::sample::sample_sheet(today);
    job_tracker::sheet::write(&path, &sheet).expect("write workbook");
    println!(
        "Wrote {} ({} rows, dates counted from {today}).",
        path.display(),
        sheet.rows.len()
    );
}
