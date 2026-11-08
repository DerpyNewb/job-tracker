//! Reading, writing and saving against `fixtures/legacy.xlsx` (the real layout, fictional rows).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use calamine::{Data, Reader, Xlsx, open_workbook};
use chrono::NaiveDate;
use job_tracker::model::{Application, Cell, DateCell, Status};
use job_tracker::sheet::{self, Sheet};
use job_tracker::store::{self, Change, LoadError, Row, SaveError, SaveOptions};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

const TODAY: &str = "2026-11-05";

/// A fresh folder with a copy of the fixture, named like the real file.
fn workspace(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("job-tracker-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("job-search")).unwrap();
    let wb = dir.join("job-search").join("Job_Applications.xlsx");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy.xlsx"),
        &wb,
    )
    .unwrap();
    (dir, wb)
}

fn opts(dir: &Path, stamp: &str) -> (PathBuf, String) {
    (dir.join("backups"), stamp.to_string())
}

fn save(
    loaded: &mut store::Loaded,
    sheet: &Sheet,
    dir: &Path,
    stamp: &str,
) -> Result<PathBuf, SaveError> {
    let (backups, stamp) = opts(dir, stamp);
    store::save(
        loaded,
        sheet,
        &SaveOptions {
            backup_dir: &backups,
            stamp,
            today: d(TODAY),
            keep: store::KEEP_BACKUPS,
        },
    )
}

fn sheet_xml(path: &Path) -> String {
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    zip.by_name("xl/worksheets/sheet1.xml")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

#[test]
fn reads_the_legacy_layout() {
    let (_dir, wb) = workspace("read");
    let loaded = store::load(&wb).unwrap();
    assert!(
        loaded.adds_columns,
        "a workbook that stops at J gains K to M"
    );
    assert!(loaded.read_only.is_none());
    let rows = &loaded.sheet.rows;
    assert_eq!(rows.len(), 6);
    assert_eq!(rows[0].company, "Pinecone Analytics");
    assert_eq!(rows[0].date_applied, DateCell::Date(d("2026-10-01")));
    assert_eq!(rows[0].status, Status::Applied);
    assert_eq!(rows[2].next_step, "");
    assert_eq!(rows[4].status, Status::NotApplied);
    assert_eq!(rows[4].date_applied, DateCell::Empty);
    assert_eq!(rows[5].date_applied, DateCell::Text("late Sep 2026".into()));
    assert!(
        rows.iter()
            .all(|r| r.source.is_empty() && r.follow_up == DateCell::Empty)
    );
}

#[test]
fn first_save_adds_columns_and_writes_real_dates() {
    let (dir, wb) = workspace("first-save");
    let mut loaded = store::load(&wb).unwrap();
    let sheet = loaded.sheet.clone();
    save(&mut loaded, &sheet, &dir, "2026-11-05 100000").unwrap();
    assert!(!loaded.adds_columns);

    let again = store::load(&wb).unwrap();
    assert_eq!(again.sheet, sheet, "every value reads back the same");
    assert!(!again.adds_columns);

    let mut x: Xlsx<_> = open_workbook(&wb).unwrap();
    let range = x.worksheet_range("Applications").unwrap();
    assert_eq!(
        range.get_value((0, 10)),
        Some(&Data::String("Source".into()))
    );
    assert_eq!(
        range.get_value((0, 12)),
        Some(&Data::String("Follow-up Date".into()))
    );
    assert!(
        matches!(range.get_value((1, 2)), Some(Data::DateTime(_))),
        "C2 is an Excel date"
    );
    assert_eq!(
        range.get_value((6, 2)),
        Some(&Data::String("late Sep 2026".into())),
        "unreadable date kept as text"
    );
}

#[test]
fn layout_survives_a_save() {
    let (dir, wb) = workspace("layout");
    let mut loaded = store::load(&wb).unwrap();
    let sheet = loaded.sheet.clone();
    save(&mut loaded, &sheet, &dir, "2026-11-05 100000").unwrap();
    let xml = sheet_xml(&wb);
    assert!(xml.contains(r#"topLeftCell="A2""#), "header row frozen");
    assert!(
        xml.contains(r#"<autoFilter ref="A1:M7"/>"#),
        "filter covers every row and column"
    );
    assert!(xml.contains(r#"sqref="F2:F500""#), "status dropdown range");
    assert!(
        xml.contains("Applied,Not applied,Screening,Interview,Offer,Rejected,Ghosted,Withdrawn")
    );
    assert!(
        xml.contains(r#"errorStyle="warning""#) && xml.contains(r#"sqref="K2:K500""#),
        "source suggestions warn only"
    );
    assert_eq!(xml.matches("<col ").count(), 13, "a width for each column");
    // 22 pt in whole pixels (29 px) is 21.75 pt; rust_xlsxwriter stores heights in pixels.
    assert!(
        xml.contains(r#"<row r="1" spans="1:13" ht="21.75" customHeight="1">"#),
        "header row height"
    );

    let mut zip = zip::ZipArchive::new(fs::File::open(&wb).unwrap()).unwrap();
    let mut styles = String::new();
    zip.by_name("xl/styles.xml")
        .unwrap()
        .read_to_string(&mut styles)
        .unwrap();
    assert!(styles.contains("FF1F3864"), "header fill");
    assert!(styles.contains("yyyy-mm-dd"), "date format");
    assert!(styles.contains(r#"wrapText="1""#) && styles.contains(r#"vertical="top""#));
}

#[test]
fn unknown_values_and_extra_columns_survive() {
    let (dir, wb) = workspace("extra");
    let mut sheet = Sheet {
        rows: vec![Application {
            company: "Camp John Hay Partners".into(),
            role: "Analyst".into(),
            status: Status::Other("On hold".into()),
            date_applied: DateCell::Text("around Oct 3".into()),
            follow_up: DateCell::Date(d("2026-11-10")),
            extra: vec![Cell::Number(3.0), Cell::Text("Ms. Cruz".into())],
            ..Application::default()
        }],
        extra_headers: vec!["Rounds".into(), "Recruiter".into()],
    };
    sheet::write(&wb, &sheet).unwrap();
    let mut loaded = store::load(&wb).unwrap();
    assert_eq!(loaded.sheet, sheet);
    sheet.rows[0].notes = "Asked for a portfolio link.".into();
    save(&mut loaded, &sheet, &dir, "2026-11-05 100000").unwrap();
    assert_eq!(store::load(&wb).unwrap().sheet, sheet);
}

#[test]
fn a_date_typed_in_excel_is_read_as_a_date() {
    let (_dir, wb) = workspace("excel-date");
    let mut book = rust_xlsxwriter::Workbook::new();
    let ws = book.add_worksheet();
    ws.set_name("Applications").unwrap();
    for (c, h) in sheet::HEADERS.iter().enumerate().take(10) {
        ws.write_string(0, c as u16, *h).unwrap();
    }
    ws.write_string(1, 0, "Loakan Systems").unwrap();
    ws.write_string(1, 1, "Support").unwrap();
    let fmt = rust_xlsxwriter::Format::new().set_num_format("d mmm yyyy");
    ws.write_datetime_with_format(
        1,
        2,
        rust_xlsxwriter::ExcelDateTime::from_ymd(2026, 10, 9).unwrap(),
        &fmt,
    )
    .unwrap();
    book.save(&wb).unwrap();
    let loaded = store::load(&wb).unwrap();
    assert_eq!(
        loaded.sheet.rows[0].date_applied,
        DateCell::Date(d("2026-10-09"))
    );
}

#[test]
fn refuses_to_save_while_open_in_excel() {
    let (dir, wb) = workspace("lock");
    let before = fs::read(&wb).unwrap();
    let mut loaded = store::load(&wb).unwrap();
    fs::write(wb.with_file_name("~$Job_Applications.xlsx"), b"owner").unwrap();
    let sheet = loaded.sheet.clone();
    assert_eq!(
        save(&mut loaded, &sheet, &dir, "s"),
        Err(SaveError::OpenInExcel)
    );
    assert_eq!(fs::read(&wb).unwrap(), before);
    assert!(!dir.join("backups").exists(), "no backup either");
}

#[test]
fn refuses_to_save_when_the_file_changed_after_loading() {
    let (dir, wb) = workspace("changed");
    let mut loaded = store::load(&wb).unwrap();
    let mut other = loaded.sheet.clone();
    other.rows[0].notes = "Edited in Excel meanwhile.".into();
    sheet::write(&wb, &other).unwrap();
    let theirs = fs::read(&wb).unwrap();
    let sheet = loaded.sheet.clone();
    assert_eq!(
        save(&mut loaded, &sheet, &dir, "s"),
        Err(SaveError::ChangedOnDisk)
    );
    assert_eq!(fs::read(&wb).unwrap(), theirs);
}

#[test]
fn a_second_sheet_turns_saving_off() {
    let (dir, wb) = workspace("two-sheets");
    let mut book = rust_xlsxwriter::Workbook::new();
    let ws = book.add_worksheet();
    ws.set_name("Applications").unwrap();
    for (c, h) in sheet::HEADERS.iter().enumerate().take(10) {
        ws.write_string(0, c as u16, *h).unwrap();
    }
    book.add_worksheet().set_name("Interview notes").unwrap();
    book.save(&wb).unwrap();
    let before = fs::read(&wb).unwrap();
    let mut loaded = store::load(&wb).unwrap();
    let why = loaded.read_only.clone().expect("read only");
    assert!(why.contains("Interview notes"), "{why}");
    let sheet = loaded.sheet.clone();
    assert!(matches!(
        save(&mut loaded, &sheet, &dir, "s"),
        Err(SaveError::ReadOnly(_))
    ));
    assert_eq!(fs::read(&wb).unwrap(), before);
}

#[test]
fn invalid_rows_block_the_save() {
    let (dir, wb) = workspace("invalid");
    let before = fs::read(&wb).unwrap();
    let mut loaded = store::load(&wb).unwrap();
    let mut sheet = loaded.sheet.clone();
    sheet.rows[1].role.clear();
    sheet.rows[3].date_applied = DateCell::Date(d("2026-12-01"));
    match save(&mut loaded, &sheet, &dir, "s") {
        Err(SaveError::Invalid(p)) => {
            assert_eq!(p.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 3])
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert_eq!(fs::read(&wb).unwrap(), before);
}

#[test]
fn wrong_header_names_the_column() {
    let (_dir, wb) = workspace("header");
    let mut book = rust_xlsxwriter::Workbook::new();
    let ws = book.add_worksheet();
    ws.set_name("Applications").unwrap();
    for (c, h) in sheet::HEADERS.iter().enumerate().take(10) {
        ws.write_string(0, c as u16, if c == 6 { "Next Action" } else { h })
            .unwrap();
    }
    book.save(&wb).unwrap();
    let err = store::load(&wb).unwrap_err();
    assert!(
        matches!(&err, LoadError::Read(sheet::ReadError::Header { column, .. }) if column == "G"),
        "{err}"
    );
    assert!(err.to_string().contains("\"Next Action\""));
}

#[test]
fn backups_are_kept_outside_and_pruned() {
    let (dir, wb) = workspace("backups");
    let mut loaded = store::load(&wb).unwrap();
    let mut sheet = loaded.sheet.clone();
    for i in 0..23 {
        sheet.rows[0].notes = format!("Save number {i}");
        save(&mut loaded, &sheet, &dir, &format!("2026-11-05 10{i:04}")).unwrap();
    }
    let names: Vec<String> = fs::read_dir(dir.join("backups"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(names.len(), store::KEEP_BACKUPS);
    assert!(
        !names.iter().any(|n| n.contains("100000")),
        "oldest went first"
    );
    let leftovers: Vec<_> = fs::read_dir(wb.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        leftovers,
        vec!["Job_Applications.xlsx"],
        "no temp file left beside the workbook"
    );
}

#[test]
fn changes_are_put_back_on_a_reloaded_file() {
    let (_dir, wb) = workspace("reapply");
    let loaded = store::load(&wb).unwrap();
    let original = loaded.sheet.rows.clone();
    let mut rows = store::rows_from(&loaded.sheet);
    rows[0].app.status = Status::Screening; // edit
    rows.remove(2); // delete Burnham
    rows.push(Row {
        app: Application {
            company: "New Co".into(),
            role: "Dev".into(),
            ..Application::default()
        },
        origin: None,
    });
    rows[2].app.notes = "My note".into(); // Kennon, now at index 2
    let changes = store::changes(&original, &rows);
    assert_eq!(changes.len(), 4);
    assert!(
        changes
            .iter()
            .any(|c| matches!(c, Change::Deleted(a) if a.company == "Burnham Data Co."))
    );

    // Meanwhile someone else edits Kennon in Excel and adds a row.
    let mut theirs = loaded.sheet.clone();
    theirs.rows[3].next_step = "Changed elsewhere".into();
    theirs.rows.insert(
        0,
        Application {
            company: "Theirs".into(),
            role: "QA".into(),
            ..Application::default()
        },
    );

    let (merged, notes) = store::reapply(&theirs, &changes);
    let names: Vec<&str> = merged.iter().map(|r| r.app.company.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Theirs",
            "Pinecone Analytics",
            "Session Road Games",
            "Kennon Labs",
            "Mines View Studio",
            "Teachers Camp Tech",
            "Kennon Labs",
            "New Co"
        ]
    );
    assert_eq!(merged[1].app.status, Status::Screening);
    assert_eq!(
        merged[3].app.next_step, "Changed elsewhere",
        "their version stays"
    );
    assert_eq!(merged[6].app.notes, "My note", "ours is added beside it");
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("Kennon Labs"));
}

#[test]
fn the_sample_workbook_covers_every_case_and_round_trips() {
    use job_tracker::model::{Attention, DEFAULT_STALE_DAYS};
    let (dir, _) = workspace("sample");
    let path = dir.join("Sample_Job_Applications.xlsx");
    let today = d(TODAY);
    let sheet = job_tracker::sample::sample_sheet(today);
    sheet::write(&path, &sheet).unwrap();

    let loaded = store::load(&path).unwrap();
    assert_eq!(loaded.sheet, sheet, "reads back exactly");
    assert!(!loaded.adds_columns && loaded.read_only.is_none());
    assert!(
        store::problems(&loaded.sheet.rows, today).is_empty(),
        "no row blocks a save"
    );

    let rows = &loaded.sheet.rows;
    for s in Status::KNOWN.iter() {
        assert!(
            rows.iter().any(|r| r.status == *s),
            "has a {} row",
            s.label()
        );
    }
    assert!(
        rows.iter().any(|r| matches!(r.status, Status::Other(_))),
        "has a hand-typed status"
    );
    assert!(
        rows.iter()
            .any(|r| matches!(r.date_applied, DateCell::Text(_))),
        "has a text date"
    );
    assert_eq!(
        loaded.sheet.extra_headers,
        vec![job_tracker::sample::EXTRA_HEADER.to_string()]
    );

    let reasons: Vec<Attention> = rows
        .iter()
        .filter_map(|r| r.attention(today, DEFAULT_STALE_DAYS))
        .collect();
    assert!(
        reasons
            .iter()
            .any(|a| matches!(a, Attention::FollowUpDue { .. }))
    );
    assert!(
        reasons
            .iter()
            .any(|a| matches!(a, Attention::Waiting { .. }))
    );
    assert!(reasons.iter().any(|a| matches!(a, Attention::NoNextStep)));
}
