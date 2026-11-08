//! `jt`: the job tracker from a terminal, for scripts and Claude Code sessions. Console app, so its
//! output reaches the terminal (the window app is built without one). Saves go through the same
//! checks and backups as the window's Save.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::NaiveDate;
use job_tracker::model::{Application, DateCell, Status};
use job_tracker::prefs::Prefs;
use job_tracker::sheet::{HEADERS, Sheet};
use job_tracker::store::{self, Loaded, SaveOptions};
use job_tracker::ui::default_backup_dir;

const USAGE: &str = "\
jt: the job tracker from a terminal

  jt list [--all]               open applications (--all: every row)
  jt due                        what needs a follow-up, most urgent first
  jt show <row>                 every column of one row
  jt add --company X --role Y [--field value ...]
  jt set <row> --field value [--field value ...]

<row> is the Excel row number from `list`, or text matching one company.
Fields: company, role, applied, location, compensation, status, next-step, contact,
link, notes, source, resume, follow-up. Dates take YYYY-MM-DD or today.
A value of - clears the field (\"\" works too, but PowerShell 5.1 drops it).
Statuses: Applied, Not applied, Screening, Interview, Offer, Rejected, Ghosted, Withdrawn.
add refuses a company and role already listed; --force adds it anyway.

Workbook: --file PATH, else the one the window opened last, else
job-search/Job_Applications.xlsx under the current folder.
Saving refuses while Excel has the file open, and backs up to
%LOCALAPPDATA%\\job-tracker\\backups first.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

struct Args {
    command: String,
    target: Option<String>,
    fields: Vec<(String, String)>,
    file: Option<PathBuf>,
    all: bool,
    force: bool,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut it = args.iter();
    let command = it.next().cloned().ok_or(USAGE)?;
    let mut out = Args {
        command,
        target: None,
        fields: Vec::new(),
        file: None,
        all: false,
        force: false,
    };
    while let Some(a) = it.next() {
        match a.strip_prefix("--") {
            Some("all") => out.all = true,
            Some("force") => out.force = true,
            Some(key) => {
                let value = it
                    .next()
                    .filter(|v| !v.starts_with("--"))
                    .ok_or(format!("--{key} needs a value (- clears the field)."))?;
                if key == "file" {
                    out.file = Some(PathBuf::from(value));
                } else {
                    out.fields.push((key.to_string(), value.clone()));
                }
            }
            None if out.target.is_none() => out.target = Some(a.clone()),
            None => return Err(format!("Unexpected \"{a}\". Values go after a --field.")),
        }
    }
    Ok(out)
}

fn workbook(file: Option<PathBuf>, prefs: &Prefs) -> Result<PathBuf, String> {
    let local = Path::new("job-search").join("Job_Applications.xlsx");
    file.or_else(|| prefs.startup_file().map(PathBuf::from))
        .or_else(|| local.exists().then_some(local))
        .ok_or_else(|| "No workbook found. Pass --file PATH.".to_string())
}

fn run(args: &[String]) -> Result<(), String> {
    let a = parse(args)?;
    if matches!(a.command.as_str(), "help" | "--help" | "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    let prefs = Prefs::load_saved();
    let today = chrono::Local::now().date_naive();
    let path = workbook(a.file.clone(), &prefs)?;
    let mut loaded = store::load(&path).map_err(|e| e.to_string())?;
    let rows = &loaded.sheet.rows;
    let no_fields = || {
        if a.fields.is_empty() {
            Ok(())
        } else {
            Err(format!("{} takes no --field options.", a.command))
        }
    };

    match a.command.as_str() {
        "list" => {
            no_fields()?;
            for (i, app) in rows.iter().enumerate() {
                if a.all || !app.status.is_closed() {
                    println!("{}", line(i, app, today, prefs.stale_days));
                }
            }
            Ok(())
        }
        "due" => {
            no_fields()?;
            let mut due: Vec<_> = rows
                .iter()
                .enumerate()
                .filter_map(|(i, app)| Some((i, app, app.attention(today, prefs.stale_days)?)))
                .collect();
            due.sort_by_key(|(_, _, att)| -att.urgency());
            for (i, app, att) in due {
                println!(
                    "row {} | {} | {} | {}",
                    i + 2,
                    app.company,
                    app.role,
                    att.reason()
                );
            }
            Ok(())
        }
        "show" => {
            no_fields()?;
            let i = find(rows, a.target.as_deref())?;
            for (name, value) in columns(&loaded.sheet, i) {
                println!("{name}: {value}");
            }
            Ok(())
        }
        "add" => {
            if a.target.is_some() {
                return Err("add takes --company and --role, not a row.".into());
            }
            let mut app = Application::new_today(today, loaded.sheet.extra_headers.len());
            if prefs.follow_up_after_days > 0 {
                app.follow_up =
                    DateCell::Date(today + chrono::Duration::days(prefs.follow_up_after_days));
            }
            for (k, v) in &a.fields {
                set_field(&mut app, k, v, today)?;
            }
            let same = |x: &str, y: &str| x.trim().eq_ignore_ascii_case(y.trim());
            if let Some(i) = rows
                .iter()
                .position(|r| same(&r.company, &app.company) && same(&r.role, &app.role))
                && !a.force
            {
                return Err(format!(
                    "Row {} already has {} ({}). Not added; pass --force to add it anyway.",
                    i + 2,
                    rows[i].company,
                    rows[i].role
                ));
            }
            let mut sheet = loaded.sheet.clone();
            sheet.rows.push(app);
            let n = sheet.rows.len() + 1;
            save(&mut loaded, &sheet, &prefs, today)?;
            println!("Added row {n}.");
            Ok(())
        }
        "set" => {
            if a.fields.is_empty() {
                return Err("set needs at least one --field value.".into());
            }
            let i = find(rows, a.target.as_deref())?;
            let mut sheet = loaded.sheet.clone();
            for (k, v) in &a.fields {
                set_field(&mut sheet.rows[i], k, v, today)?;
            }
            let before = columns(&loaded.sheet, i);
            let after = columns(&sheet, i);
            save(&mut loaded, &sheet, &prefs, today)?;
            println!(
                "Row {}: {} ({})",
                i + 2,
                sheet.rows[i].company,
                sheet.rows[i].role
            );
            for ((name, old), (_, new)) in before.iter().zip(&after) {
                if old != new {
                    println!("  {name}: \"{old}\" -> \"{new}\"");
                }
            }
            Ok(())
        }
        other => Err(format!("Unknown command \"{other}\".\n\n{USAGE}")),
    }
}

fn line(i: usize, app: &Application, today: NaiveDate, stale_days: i64) -> String {
    let follow = app
        .attention(today, stale_days)
        .map(|a| a.short())
        .unwrap_or_default();
    format!(
        "row {} | {} | {} | {} | {} | next: {} | {}",
        i + 2,
        app.company,
        app.role,
        app.status.label(),
        app.date_applied.display(),
        app.next_step,
        follow
    )
}

/// Every column of row `i`, headed as in the workbook.
fn columns(sheet: &Sheet, i: usize) -> Vec<(String, String)> {
    let a = &sheet.rows[i];
    let fixed = [
        a.company.clone(),
        a.role.clone(),
        a.date_applied.display(),
        a.location.clone(),
        a.compensation.clone(),
        a.status.label().to_string(),
        a.next_step.clone(),
        a.contact.clone(),
        a.link.clone(),
        a.notes.clone(),
        a.source.clone(),
        a.resume.clone(),
        a.follow_up.display(),
    ];
    let extra = sheet
        .extra_headers
        .iter()
        .zip(&a.extra)
        .map(|(h, c)| (h.clone(), c.display()));
    HEADERS
        .iter()
        .map(|h| h.to_string())
        .zip(fixed)
        .chain(extra)
        .collect()
}

/// A row by its Excel row number (2 is the first application) or by text in one company name.
fn find(rows: &[Application], target: Option<&str>) -> Result<usize, String> {
    let target = target.ok_or("Which row? Give its number from `jt list`, or a company name.")?;
    if let Ok(n) = target.parse::<usize>() {
        return (n >= 2 && n - 2 < rows.len()).then(|| n - 2).ok_or(format!(
            "There is no row {n}. Rows run from 2 to {}.",
            rows.len() + 1
        ));
    }
    let t = target.to_lowercase();
    let hits: Vec<usize> = (0..rows.len())
        .filter(|&i| rows[i].company.to_lowercase().contains(&t))
        .collect();
    match hits.as_slice() {
        [i] => Ok(*i),
        [] => Err(format!("No company matches \"{target}\".")),
        many => Err(format!(
            "\"{target}\" matches rows {}. Use the row number.",
            many.iter()
                .map(|i| (i + 2).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn set_field(a: &mut Application, key: &str, value: &str, today: NaiveDate) -> Result<(), String> {
    let value = if value.trim() == "-" { "" } else { value };
    let date = |v: &str| match v.trim() {
        "" => Ok(DateCell::Empty),
        "today" => Ok(DateCell::Date(today)),
        t => NaiveDate::parse_from_str(t, "%Y-%m-%d")
            .map(DateCell::Date)
            .map_err(|_| format!("--{key} takes YYYY-MM-DD, today, or -, not \"{v}\".")),
    };
    let text = value.to_string();
    match key {
        "company" => a.company = text,
        "role" => a.role = text,
        "applied" => a.date_applied = date(value)?,
        "location" => a.location = text,
        "compensation" => a.compensation = text,
        "status" => {
            // The window keeps statuses typed in Excel, but a typo here would make a new one.
            a.status = match Status::parse(value) {
                Status::Other(_) => {
                    let known: Vec<&str> = Status::KNOWN.iter().map(|s| s.label()).collect();
                    return Err(format!(
                        "Unknown status \"{value}\". Use one of: {}.",
                        known.join(", ")
                    ));
                }
                s => s,
            }
        }
        "next-step" => a.next_step = text,
        "contact" => a.contact = text,
        "link" => a.link = text,
        "notes" => a.notes = text,
        "source" => a.source = text,
        "resume" => a.resume = text,
        "follow-up" => a.follow_up = date(value)?,
        _ => {
            return Err(format!(
                "Unknown option --{key}. Run `jt help` for the list."
            ));
        }
    }
    Ok(())
}

fn save(loaded: &mut Loaded, sheet: &Sheet, prefs: &Prefs, today: NaiveDate) -> Result<(), String> {
    let adds_columns = loaded.adds_columns;
    let opts = SaveOptions {
        backup_dir: &default_backup_dir(),
        // Milliseconds too: two saves in one second would otherwise share a backup name.
        stamp: chrono::Local::now()
            .format("%Y-%m-%d %H%M%S%3f")
            .to_string(),
        today,
        keep: prefs.keep_backups,
    };
    let backup = store::save(loaded, sheet, &opts).map_err(|e| e.to_string())?;
    if adds_columns {
        println!("First save of this workbook: added Source, Resume and Follow-up Date (K to M).");
    }
    println!(
        "Saved {}. Backup: {}",
        loaded.path.display(),
        backup.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn rows(companies: &[&str]) -> Vec<Application> {
        companies
            .iter()
            .map(|c| Application {
                company: c.to_string(),
                role: "Analyst".into(),
                ..Application::default()
            })
            .collect()
    }

    #[test]
    fn finds_rows_by_number_or_one_company() {
        let r = rows(&["Pinecone Analytics", "Pine Labs", "Harbor QA"]);
        assert_eq!(find(&r, Some("2")), Ok(0));
        assert_eq!(find(&r, Some("4")), Ok(2));
        assert!(find(&r, Some("5")).is_err());
        assert!(find(&r, Some("1")).is_err(), "row 1 is the header");
        assert_eq!(find(&r, Some("harbor")), Ok(2));
        assert_eq!(
            find(&r, Some("pine")).unwrap_err(),
            "\"pine\" matches rows 2, 3. Use the row number."
        );
        assert!(find(&r, Some("nowhere")).is_err());
    }

    #[test]
    fn fields_check_dates_and_statuses() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 8).unwrap();
        let mut a = Application::default();
        set_field(&mut a, "status", "interview", today).unwrap();
        assert_eq!(a.status, Status::Interview);
        assert!(set_field(&mut a, "status", "Intervew", today).is_err());
        set_field(&mut a, "follow-up", "today", today).unwrap();
        assert_eq!(a.follow_up, DateCell::Date(today));
        assert!(set_field(&mut a, "applied", "Nov 8", today).is_err());
        set_field(&mut a, "follow-up", "-", today).unwrap();
        assert_eq!(a.follow_up, DateCell::Empty);
        set_field(&mut a, "notes", "-", today).unwrap();
        assert_eq!(a.notes, "");
        assert!(set_field(&mut a, "salary", "1", today).is_err());
    }

    #[test]
    fn parses_target_fields_and_flags() {
        let a = parse(&s(&[
            "set", "Harbor", "--status", "Offer", "--file", "x.xlsx", "--force",
        ]))
        .unwrap();
        assert_eq!(a.target.as_deref(), Some("Harbor"));
        assert_eq!(a.fields, vec![("status".to_string(), "Offer".to_string())]);
        assert_eq!(a.file, Some(PathBuf::from("x.xlsx")));
        assert!(a.force);
        assert!(parse(&s(&["set", "2", "--notes"])).is_err());
        // PowerShell 5.1 drops an empty "", so the next option must not be taken as the value.
        assert!(parse(&s(&["set", "2", "--follow-up", "--file", "x.xlsx"])).is_err());
        assert!(parse(&s(&["set", "2", "3"])).is_err());
    }

    /// add, set and a duplicate add on a copy of the test workbook, read back from disk.
    #[test]
    fn add_and_set_save_through_the_checks() {
        let dir = std::env::temp_dir().join(format!("jt-cli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stem = format!("jt-cli-test-{}", std::process::id());
        let file = dir.join(format!("{stem}.xlsx"));
        std::fs::copy("tests/fixtures/legacy.xlsx", &file).unwrap();
        let f = file.to_string_lossy().to_string();
        let before = store::load(&file).unwrap().sheet.rows.len();

        run(&s(&[
            "add",
            "--company",
            "Fjord Data",
            "--role",
            "SQL Analyst",
            "--file",
            &f,
        ]))
        .unwrap();
        let err = run(&s(&[
            "add",
            "--company",
            "fjord data",
            "--role",
            "sql analyst",
            "--file",
            &f,
        ]));
        assert!(err.unwrap_err().contains("already has"));
        run(&s(&[
            "set",
            "Fjord",
            "--status",
            "Screening",
            "--follow-up",
            "2026-11-20",
            "--file",
            &f,
        ]))
        .unwrap();

        let rows = store::load(&file).unwrap().sheet.rows;
        assert_eq!(rows.len(), before + 1);
        let new = rows.last().unwrap();
        assert_eq!(
            (new.company.as_str(), &new.status),
            ("Fjord Data", &Status::Screening)
        );
        assert_eq!(new.follow_up.display(), "2026-11-20");
        std::fs::remove_dir_all(dir).unwrap();
        // The two saves backed up to the real backups folder; remove those.
        for e in std::fs::read_dir(default_backup_dir()).unwrap().flatten() {
            if e.file_name()
                .to_string_lossy()
                .starts_with(&format!("{stem} "))
            {
                std::fs::remove_file(e.path()).unwrap();
            }
        }
    }
}
