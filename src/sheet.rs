//! Reading and writing `Job_Applications.xlsx`. The layout written here is the one the workbook
//! had on 2026-11-05, plus columns K to M.

use std::path::Path;

use calamine::{Data, Reader, Xlsx, open_workbook};
use chrono::Datelike;
use rust_xlsxwriter::{
    Color, DataValidation, DataValidationErrorStyle, ExcelDateTime, Format, FormatAlign, Workbook,
    Worksheet, XlsxError,
};

use crate::model::{Application, Cell, DateCell, Status};

pub const SHEET: &str = "Applications";

pub const HEADERS: [&str; 13] = [
    "Company",
    "Role",
    "Date Applied",
    "Location",
    "Compensation",
    "Status",
    "Next Step",
    "Contact",
    "Link",
    "Notes",
    "Source",
    "Resume",
    "Follow-up Date",
];

/// A to J were in the workbook from the start; K to M are added on the first save if missing.
pub const REQUIRED_COLUMNS: usize = 10;

const WIDTHS: [f64; 13] = [
    22.0, 30.0, 13.0, 20.0, 22.0, 14.0, 26.0, 22.0, 40.0, 45.0, 16.0, 34.0, 14.0,
];
const EXTRA_WIDTH: f64 = 20.0;
const HEADER_FILL: u32 = 0x1F3864;
const HEADER_HEIGHT: f64 = 22.0;
/// Data validation covers at least this many rows, as it did before.
const VALIDATION_LAST_ROW: u32 = 499;

/// Suggestions for the Source column. Excel warns on other values but accepts them.
pub const SOURCES: [&str; 7] = [
    "LinkedIn",
    "JobStreet",
    "Indeed",
    "OnlineJobs.ph",
    "Company site",
    "Email",
    "Referral",
];

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Sheet {
    pub rows: Vec<Application>,
    /// Headers of columns past M, written back in the same order.
    pub extra_headers: Vec<String>,
}

#[derive(Debug)]
pub struct ReadResult {
    pub sheet: Sheet,
    /// Every sheet in the workbook, `Applications` included.
    pub sheet_names: Vec<String>,
    /// True when the file stops at J and gains K to M on save.
    pub adds_columns: bool,
}

#[derive(Debug, PartialEq)]
pub enum ReadError {
    Open(String),
    NoSheet {
        found: Vec<String>,
    },
    Header {
        column: String,
        expected: String,
        found: String,
    },
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Open(e) => write!(f, "The file couldn't be read as a workbook: {e}"),
            ReadError::NoSheet { found } => write!(
                f,
                "There is no sheet named \"{SHEET}\". The workbook has: {}.",
                found.join(", ")
            ),
            ReadError::Header {
                column,
                expected,
                found,
            } => {
                if found.is_empty() {
                    write!(
                        f,
                        "Column {column} has no header; it should be \"{expected}\"."
                    )
                } else {
                    write!(
                        f,
                        "Column {column} is headed \"{found}\" but should be \"{expected}\". \
                         Rename it in Excel, then open the file again."
                    )
                }
            }
        }
    }
}

pub fn column_letter(index: usize) -> String {
    let mut n = index + 1;
    let mut s = String::new();
    while n > 0 {
        let r = (n - 1) % 26;
        s.insert(0, (b'A' + r as u8) as char);
        n = (n - 1) / 26;
    }
    s
}

fn text(cell: Option<&Data>) -> String {
    match cell {
        None | Some(Data::Empty) => String::new(),
        Some(Data::String(s)) => s.clone(),
        Some(Data::Float(f)) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Some(Data::Int(i)) => i.to_string(),
        Some(Data::Bool(b)) => b.to_string(),
        Some(Data::DateTime(dt)) => dt
            .as_datetime()
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| dt.as_f64().to_string()),
        Some(Data::DateTimeIso(s)) | Some(Data::DurationIso(s)) => s.clone(),
        Some(Data::Error(e)) => format!("#{e:?}"),
    }
}

fn date(cell: Option<&Data>) -> DateCell {
    match cell {
        Some(Data::DateTime(dt)) => match dt.as_datetime() {
            Some(d) => DateCell::Date(d.date()),
            None => DateCell::Text(dt.as_f64().to_string()),
        },
        Some(Data::DateTimeIso(s)) => match DateCell::parse(s.get(..10).unwrap_or(s)) {
            DateCell::Date(d) => DateCell::Date(d),
            _ => DateCell::Text(s.clone()),
        },
        other => DateCell::parse(&text(other)),
    }
}

fn cell(data: Option<&Data>) -> Cell {
    match data {
        None | Some(Data::Empty) => Cell::Empty,
        Some(Data::Float(f)) => Cell::Number(*f),
        Some(Data::Int(i)) => Cell::Number(*i as f64),
        Some(Data::Bool(b)) => Cell::Bool(*b),
        Some(Data::DateTime(dt)) => match dt.as_datetime() {
            Some(d) => Cell::DateTime(d),
            None => Cell::Number(dt.as_f64()),
        },
        other => Cell::Text(text(other)),
    }
}

pub fn read(path: &Path) -> Result<ReadResult, ReadError> {
    let mut wb: Xlsx<std::io::BufReader<std::fs::File>> =
        open_workbook(path).map_err(|e: calamine::XlsxError| ReadError::Open(e.to_string()))?;
    let sheet_names = wb.sheet_names();
    if !sheet_names.iter().any(|n| n == SHEET) {
        return Err(ReadError::NoSheet { found: sheet_names });
    }
    let range = wb
        .worksheet_range(SHEET)
        .map_err(|e| ReadError::Open(e.to_string()))?;
    // Absolute positions, so a range that doesn't start at A1 still maps to the right columns.
    let (last_row, last_col) = range
        .end()
        .map(|(r, c)| (r as usize, c as usize))
        .unwrap_or((0, 0));
    let get = |r: usize, c: usize| range.get_value((r as u32, c as u32));
    let header = |c: usize| text(get(0, c)).trim().to_string();
    let column_has_data = |c: usize| (1..=last_row).any(|r| !text(get(r, c)).trim().is_empty());

    for (c, expected) in HEADERS.iter().enumerate().take(REQUIRED_COLUMNS) {
        let found = header(c);
        if found != *expected {
            return Err(ReadError::Header {
                column: column_letter(c),
                expected: expected.to_string(),
                found,
            });
        }
    }
    // K to M: either all absent, or present with the right names.
    let mut adds_columns = false;
    for (c, expected) in HEADERS.iter().enumerate().skip(REQUIRED_COLUMNS) {
        let found = header(c);
        if found.is_empty() && !column_has_data(c) {
            adds_columns = true;
            continue;
        }
        if found != *expected {
            return Err(ReadError::Header {
                column: column_letter(c),
                expected: expected.to_string(),
                found,
            });
        }
    }

    let mut extra_headers = Vec::new();
    if last_col >= HEADERS.len() {
        for c in HEADERS.len()..=last_col {
            let h = header(c);
            if h.is_empty() && !column_has_data(c) {
                extra_headers.push(String::new());
                continue;
            }
            extra_headers.push(h);
        }
        // Trailing columns with no header and no data are just Excel's used range; drop them.
        while extra_headers.last().is_some_and(String::is_empty) {
            extra_headers.pop();
        }
    }

    let mut rows = Vec::new();
    for r in 1..=last_row {
        let blank =
            (0..=last_col.max(HEADERS.len() - 1)).all(|c| text(get(r, c)).trim().is_empty());
        if blank {
            continue;
        }
        let t = |c: usize| text(get(r, c));
        rows.push(Application {
            company: t(0),
            role: t(1),
            date_applied: date(get(r, 2)),
            location: t(3),
            compensation: t(4),
            status: Status::parse(&t(5)),
            next_step: t(6),
            contact: t(7),
            link: t(8),
            notes: t(9),
            source: t(10),
            resume: t(11),
            follow_up: date(get(r, 12)),
            extra: (0..extra_headers.len())
                .map(|i| cell(get(r, HEADERS.len() + i)))
                .collect(),
        });
    }

    Ok(ReadResult {
        sheet: Sheet {
            rows,
            extra_headers,
        },
        sheet_names,
        adds_columns,
    })
}

fn excel_date(d: chrono::NaiveDate) -> Result<ExcelDateTime, XlsxError> {
    ExcelDateTime::from_ymd(d.year() as u16, d.month() as u8, d.day() as u8)
}

fn excel_datetime(d: chrono::NaiveDateTime) -> Result<ExcelDateTime, XlsxError> {
    use chrono::Timelike;
    ExcelDateTime::from_ymd(d.year() as u16, d.month() as u8, d.day() as u8)?.and_hms(
        d.hour() as u16,
        d.minute() as u8,
        d.second() as f64,
    )
}

fn write_text(ws: &mut Worksheet, r: u32, c: u16, s: &str, f: &Format) -> Result<(), XlsxError> {
    if s.is_empty() {
        ws.write_blank(r, c, f)?;
    } else {
        ws.write_string_with_format(r, c, s, f)?;
    }
    Ok(())
}

fn write_date(
    ws: &mut Worksheet,
    r: u32,
    c: u16,
    d: &DateCell,
    f: &Format,
    df: &Format,
) -> Result<(), XlsxError> {
    match d {
        DateCell::Empty => {
            ws.write_blank(r, c, df)?;
        }
        DateCell::Date(d) => {
            ws.write_datetime_with_format(r, c, excel_date(*d)?, df)?;
        }
        DateCell::Text(s) => write_text(ws, r, c, s, f)?,
    }
    Ok(())
}

pub fn write(path: &Path, sheet: &Sheet) -> Result<(), XlsxError> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name(SHEET)?;

    let header = Format::new()
        .set_bold()
        .set_font_color(Color::White)
        .set_background_color(Color::RGB(HEADER_FILL));
    let body = Format::new().set_text_wrap().set_align(FormatAlign::Top);
    let date_fmt = body.clone().set_num_format("yyyy-mm-dd");
    let datetime_fmt = body.clone().set_num_format("yyyy-mm-dd hh:mm");

    let all_headers: Vec<&str> = HEADERS
        .iter()
        .copied()
        .chain(sheet.extra_headers.iter().map(String::as_str))
        .collect();
    let last_col = (all_headers.len() - 1) as u16;

    for (c, h) in all_headers.iter().enumerate() {
        ws.write_string_with_format(0, c as u16, *h, &header)?;
        ws.set_column_width(c as u16, WIDTHS.get(c).copied().unwrap_or(EXTRA_WIDTH))?;
    }
    ws.set_row_height(0, HEADER_HEIGHT)?;

    for (i, a) in sheet.rows.iter().enumerate() {
        let r = i as u32 + 1;
        write_text(ws, r, 0, &a.company, &body)?;
        write_text(ws, r, 1, &a.role, &body)?;
        write_date(ws, r, 2, &a.date_applied, &body, &date_fmt)?;
        write_text(ws, r, 3, &a.location, &body)?;
        write_text(ws, r, 4, &a.compensation, &body)?;
        write_text(ws, r, 5, a.status.label(), &body)?;
        write_text(ws, r, 6, &a.next_step, &body)?;
        write_text(ws, r, 7, &a.contact, &body)?;
        write_text(ws, r, 8, &a.link, &body)?;
        write_text(ws, r, 9, &a.notes, &body)?;
        write_text(ws, r, 10, &a.source, &body)?;
        write_text(ws, r, 11, &a.resume, &body)?;
        write_date(ws, r, 12, &a.follow_up, &body, &date_fmt)?;
        for (j, v) in a.extra.iter().enumerate() {
            let c = (HEADERS.len() + j) as u16;
            match v {
                Cell::Empty => {
                    ws.write_blank(r, c, &body)?;
                }
                Cell::Text(s) => write_text(ws, r, c, s, &body)?,
                Cell::Number(n) => {
                    ws.write_number_with_format(r, c, *n, &body)?;
                }
                Cell::Bool(b) => {
                    ws.write_boolean_with_format(r, c, *b, &body)?;
                }
                Cell::DateTime(d) => {
                    ws.write_datetime_with_format(r, c, excel_datetime(*d)?, &datetime_fmt)?;
                }
            }
        }
    }

    let last_row = sheet.rows.len() as u32;
    let validation_end = VALIDATION_LAST_ROW.max(last_row + 1);
    let statuses: Vec<&str> = Status::KNOWN.iter().map(Status::label).collect();
    ws.add_data_validation(
        1,
        5,
        validation_end,
        5,
        &DataValidation::new().allow_list_strings(&statuses)?,
    )?;
    let sources = DataValidation::new()
        .allow_list_strings(&SOURCES)?
        .set_error_style(DataValidationErrorStyle::Warning);
    ws.add_data_validation(1, 10, validation_end, 10, &sources)?;

    ws.set_freeze_panes(1, 0)?;
    ws.autofilter(0, 0, last_row.max(1), last_col)?;

    wb.save(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_letters() {
        assert_eq!(column_letter(0), "A");
        assert_eq!(column_letter(12), "M");
        assert_eq!(column_letter(25), "Z");
        assert_eq!(column_letter(26), "AA");
    }
}
