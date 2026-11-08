//! Loading and saving with the checks that keep the synced workbook safe.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use sha2::{Digest, Sha256};

use crate::model::{Application, Field, validate};
use crate::sheet::{self, ReadError, SHEET, Sheet};

/// Backups kept per workbook.
pub const KEEP_BACKUPS: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    pub fn of(path: &Path) -> std::io::Result<Fingerprint> {
        let bytes = fs::read(path)?;
        Ok(Fingerprint(Sha256::digest(&bytes).into()))
    }
}

/// A workbook as it was when opened.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub path: PathBuf,
    pub sheet: Sheet,
    pub fingerprint: Fingerprint,
    /// Why saving is off for this file, if it is.
    pub read_only: Option<String>,
    pub adds_columns: bool,
}

#[derive(Debug)]
pub enum LoadError {
    Missing(PathBuf),
    Io(String),
    Read(ReadError),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Missing(p) => write!(f, "{} doesn't exist.", p.display()),
            LoadError::Io(e) => write!(f, "The file couldn't be opened: {e}"),
            LoadError::Read(e) => write!(f, "{e}"),
        }
    }
}

pub fn load(path: &Path) -> Result<Loaded, LoadError> {
    if !path.exists() {
        return Err(LoadError::Missing(path.to_path_buf()));
    }
    let fingerprint = Fingerprint::of(path).map_err(|e| LoadError::Io(e.to_string()))?;
    let read = sheet::read(path).map_err(LoadError::Read)?;
    let others: Vec<&String> = read.sheet_names.iter().filter(|n| *n != SHEET).collect();
    let read_only = (!others.is_empty()).then(|| {
        let names: Vec<&str> = others.iter().map(|s| s.as_str()).collect();
        format!(
            "Saving is off: the workbook also has the sheet {}, and saving from this app would \
             delete it. Move it to another file in Excel, then reload.",
            names.join(", ")
        )
    });
    Ok(Loaded {
        path: path.to_path_buf(),
        sheet: read.sheet,
        fingerprint,
        read_only,
        adds_columns: read.adds_columns,
    })
}

/// The lock file Excel keeps next to a workbook it has open.
pub fn excel_lock_file(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let name = path.file_name()?.to_string_lossy().to_string();
    // Excel writes "~$" + name; for long names Office sometimes drops the first two characters.
    let candidates = [
        format!("~${name}"),
        format!("~${}", name.get(2..).unwrap_or("")),
    ];
    candidates
        .into_iter()
        .map(|c| dir.join(c))
        .find(|p| p.exists())
}

#[derive(Debug, PartialEq)]
pub enum SaveError {
    ReadOnly(String),
    Invalid(Vec<(usize, Field, &'static str)>),
    OpenInExcel,
    ChangedOnDisk,
    Io(String),
    /// The saved file didn't read back the same. The backup was put back.
    Verify(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::ReadOnly(why) => write!(f, "{why}"),
            SaveError::Invalid(problems) => {
                let n = problems.len();
                write!(
                    f,
                    "Not saved: {n} field{} fixing. ",
                    if n == 1 { " needs" } else { "s need" }
                )?;
                if let Some((_, _, msg)) = problems.first() {
                    write!(f, "{msg}")?;
                }
                Ok(())
            }
            SaveError::OpenInExcel => {
                write!(
                    f,
                    "Not saved: the workbook is open in Excel. Close it there, then save again."
                )
            }
            SaveError::ChangedOnDisk => write!(
                f,
                "Not saved: the workbook changed since it was opened (Excel, Drive, or another session)."
            ),
            SaveError::Io(e) => write!(f, "Not saved: {e}"),
            SaveError::Verify(e) => write!(
                f,
                "The saved file didn't read back correctly, so the previous version was put back. {e}"
            ),
        }
    }
}

pub fn problems(rows: &[Application], today: NaiveDate) -> Vec<(usize, Field, &'static str)> {
    rows.iter()
        .enumerate()
        .flat_map(|(i, a)| validate(a, today).into_iter().map(move |(f, m)| (i, f, m)))
        .collect()
}

fn backup(path: &Path, backup_dir: &Path, stamp: &str, keep: usize) -> std::io::Result<PathBuf> {
    fs::create_dir_all(backup_dir)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let target = backup_dir.join(format!("{stem} {stamp}.xlsx"));
    fs::copy(path, &target)?;
    let prefix = format!("{stem} ");
    let mut existing: Vec<PathBuf> = fs::read_dir(backup_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".xlsx"))
        })
        .collect();
    existing.sort();
    let excess = existing.len().saturating_sub(keep);
    for old in existing.into_iter().take(excess) {
        fs::remove_file(old)?;
    }
    Ok(target)
}

pub struct SaveOptions<'a> {
    pub backup_dir: &'a Path,
    /// Used in the backup's file name; sortable, e.g. `2026-11-05 101530`.
    pub stamp: String,
    pub today: NaiveDate,
    pub keep: usize,
}

/// Saves `sheet` over the workbook `loaded` came from. On success `loaded` is updated to match.
pub fn save(loaded: &mut Loaded, sheet: &Sheet, opts: &SaveOptions) -> Result<PathBuf, SaveError> {
    if let Some(why) = &loaded.read_only {
        return Err(SaveError::ReadOnly(why.clone()));
    }
    let bad = problems(&sheet.rows, opts.today);
    if !bad.is_empty() {
        return Err(SaveError::Invalid(bad));
    }
    let path = loaded.path.clone();
    if excel_lock_file(&path).is_some() {
        return Err(SaveError::OpenInExcel);
    }
    match Fingerprint::of(&path) {
        Ok(fp) if fp == loaded.fingerprint => {}
        _ => return Err(SaveError::ChangedOnDisk),
    }

    let backup_path = backup(&path, opts.backup_dir, &opts.stamp, opts.keep)
        .map_err(|e| SaveError::Io(format!("couldn't make a backup ({e}).")))?;

    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.saving"));
    if let Err(e) = sheet::write(&tmp, sheet) {
        let _ = fs::remove_file(&tmp);
        return Err(SaveError::Io(format!("couldn't write the workbook ({e}).")));
    }
    if let Err(e) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(SaveError::Io(format!(
            "couldn't replace the workbook ({e}). Nothing changed."
        )));
    }

    let check = sheet::read(&path);
    let matches = matches!(&check, Ok(r) if r.sheet == *sheet);
    if !matches {
        let detail = match check {
            Err(e) => e.to_string(),
            Ok(_) => "Some values came back different.".into(),
        };
        let _ = fs::copy(&backup_path, &path);
        return Err(SaveError::Verify(detail));
    }

    loaded.sheet = sheet.clone();
    loaded.adds_columns = false;
    loaded.fingerprint = Fingerprint::of(&path).map_err(|e| SaveError::Io(e.to_string()))?;
    Ok(backup_path)
}

/// One row in the app, with the index of the row it came from when the file was opened.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub app: Application,
    pub origin: Option<usize>,
}

pub fn rows_from(sheet: &Sheet) -> Vec<Row> {
    sheet
        .rows
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, app)| Row {
            app,
            origin: Some(i),
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Added(Application),
    Edited {
        before: Box<Application>,
        after: Box<Application>,
    },
    Deleted(Application),
}

/// What the user changed since the file was opened.
pub fn changes(original: &[Application], rows: &[Row]) -> Vec<Change> {
    let mut out = Vec::new();
    let mut seen = vec![false; original.len()];
    for row in rows {
        match row.origin {
            Some(i) if i < original.len() => {
                seen[i] = true;
                if original[i] != row.app {
                    out.push(Change::Edited {
                        before: Box::new(original[i].clone()),
                        after: Box::new(row.app.clone()),
                    });
                }
            }
            _ => out.push(Change::Added(row.app.clone())),
        }
    }
    for (i, s) in seen.iter().enumerate() {
        if !s {
            out.push(Change::Deleted(original[i].clone()));
        }
    }
    out
}

/// Applies the user's changes to a freshly read workbook. Rows are matched by their content when
/// the file was first opened. Returns the new rows and a note for each change that couldn't be
/// matched because someone else changed or removed that row too.
pub fn reapply(fresh: &Sheet, changes: &[Change]) -> (Vec<Row>, Vec<String>) {
    let mut rows = rows_from(fresh);
    let mut used = vec![false; rows.len()];
    let mut notes = Vec::new();
    let find = |rows: &Vec<Row>, used: &mut Vec<bool>, target: &Application| {
        let i = rows
            .iter()
            .enumerate()
            .position(|(i, r)| !used[i] && r.app == *target)?;
        used[i] = true;
        Some(i)
    };
    let name = |a: &Application| format!("{} ({})", a.company, a.role);
    let mut deletions = Vec::new();
    for change in changes {
        match change {
            Change::Edited { before, after } => match find(&rows, &mut used, before) {
                Some(i) => rows[i].app = (**after).clone(),
                None => {
                    notes.push(format!(
                        "{} was also changed outside the app; your version was added as a separate row.",
                        name(after)
                    ));
                    rows.push(Row {
                        app: (**after).clone(),
                        origin: None,
                    });
                }
            },
            Change::Deleted(before) => match find(&rows, &mut used, before) {
                Some(i) => deletions.push(i),
                None => notes.push(format!(
                    "{} was changed outside the app, so it wasn't deleted.",
                    name(before)
                )),
            },
            Change::Added(a) => rows.push(Row {
                app: a.clone(),
                origin: None,
            }),
        }
    }
    deletions.sort_unstable();
    for i in deletions.into_iter().rev() {
        rows.remove(i);
    }
    (rows, notes)
}
