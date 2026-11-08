//! One application and the rules about it. No egui and no xlsx here, so tests can use it directly.

use chrono::{NaiveDate, NaiveDateTime};

/// Default days after applying, with no follow-up date, before an Applied or Screening row needs
/// attention. The owner picked 14 on 2026-11-05; Settings can change it.
pub const DEFAULT_STALE_DAYS: i64 = 14;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Applied,
    NotApplied,
    Screening,
    Interview,
    Offer,
    Rejected,
    Ghosted,
    Withdrawn,
    /// A value typed in Excel that is not one of the eight. Kept as typed. Empty means no status.
    Other(String),
}

impl Status {
    /// The order of the Status dropdown in the workbook.
    pub const KNOWN: [Status; 8] = [
        Status::Applied,
        Status::NotApplied,
        Status::Screening,
        Status::Interview,
        Status::Offer,
        Status::Rejected,
        Status::Ghosted,
        Status::Withdrawn,
    ];

    pub fn label(&self) -> &str {
        match self {
            Status::Applied => "Applied",
            Status::NotApplied => "Not applied",
            Status::Screening => "Screening",
            Status::Interview => "Interview",
            Status::Offer => "Offer",
            Status::Rejected => "Rejected",
            Status::Ghosted => "Ghosted",
            Status::Withdrawn => "Withdrawn",
            Status::Other(s) => s,
        }
    }

    pub fn parse(text: &str) -> Status {
        let t = text.trim();
        Status::KNOWN
            .iter()
            .find(|s| s.label().eq_ignore_ascii_case(t))
            .cloned()
            .unwrap_or_else(|| Status::Other(text.to_string()))
    }

    /// Still in progress with the employer.
    pub fn is_open(&self) -> bool {
        matches!(
            self,
            Status::Applied | Status::Screening | Status::Interview | Status::Offer
        )
    }

    /// Finished one way or another.
    pub fn is_closed(&self) -> bool {
        matches!(self, Status::Rejected | Status::Ghosted | Status::Withdrawn)
    }
}

/// A date column's cell. Text that is not a date stays text rather than being dropped.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum DateCell {
    #[default]
    Empty,
    Date(NaiveDate),
    Text(String),
}

impl DateCell {
    pub fn parse(text: &str) -> DateCell {
        let t = text.trim();
        if t.is_empty() {
            DateCell::Empty
        } else if let Ok(d) = NaiveDate::parse_from_str(t, "%Y-%m-%d") {
            DateCell::Date(d)
        } else {
            DateCell::Text(text.to_string())
        }
    }

    pub fn date(&self) -> Option<NaiveDate> {
        match self {
            DateCell::Date(d) => Some(*d),
            _ => None,
        }
    }

    pub fn display(&self) -> String {
        match self {
            DateCell::Empty => String::new(),
            DateCell::Date(d) => d.format("%Y-%m-%d").to_string(),
            DateCell::Text(s) => s.clone(),
        }
    }
}

/// A cell in a column the app does not manage. Written back with its original type.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Cell {
    #[default]
    Empty,
    Text(String),
    Number(f64),
    Bool(bool),
    DateTime(NaiveDateTime),
}

impl Cell {
    pub fn display(&self) -> String {
        match self {
            Cell::Empty => String::new(),
            Cell::Text(s) => s.clone(),
            Cell::Number(n) => n.to_string(),
            Cell::Bool(b) => b.to_string(),
            Cell::DateTime(d) => d.format("%Y-%m-%d %H:%M").to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Application {
    pub company: String,
    pub role: String,
    pub date_applied: DateCell,
    pub location: String,
    pub compensation: String,
    pub status: Status,
    pub next_step: String,
    pub contact: String,
    pub link: String,
    pub notes: String,
    pub source: String,
    pub resume: String,
    pub follow_up: DateCell,
    /// Values for columns past M, in the order of `Sheet::extra_headers`.
    pub extra: Vec<Cell>,
}

impl Default for Application {
    fn default() -> Self {
        Application {
            company: String::new(),
            role: String::new(),
            date_applied: DateCell::Empty,
            location: String::new(),
            compensation: String::new(),
            status: Status::Applied,
            next_step: String::new(),
            contact: String::new(),
            link: String::new(),
            notes: String::new(),
            source: String::new(),
            resume: String::new(),
            follow_up: DateCell::Empty,
            extra: Vec::new(),
        }
    }
}

impl Application {
    /// A new row as the Add button creates it: applied today.
    pub fn new_today(today: NaiveDate, extra_columns: usize) -> Self {
        Application {
            date_applied: DateCell::Date(today),
            extra: vec![Cell::Empty; extra_columns],
            ..Application::default()
        }
    }

    pub fn days_since_applied(&self, today: NaiveDate) -> Option<i64> {
        self.date_applied.date().map(|d| (today - d).num_days())
    }

    pub fn attention(&self, today: NaiveDate, stale_days: i64) -> Option<Attention> {
        if self.status.is_closed() {
            return None;
        }
        if let Some(due) = self.follow_up.date() {
            let late = (today - due).num_days();
            if late >= 0 {
                return Some(Attention::FollowUpDue { days_late: late });
            }
        } else if matches!(self.status, Status::Applied | Status::Screening)
            && let Some(days) = self.days_since_applied(today)
            && days >= stale_days
        {
            return Some(Attention::Waiting {
                status: self.status.clone(),
                days,
            });
        }
        if self.status.is_open() && self.next_step.trim().is_empty() {
            return Some(Attention::NoNextStep);
        }
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Attention {
    FollowUpDue { days_late: i64 },
    Waiting { status: Status, days: i64 },
    NoNextStep,
}

impl Attention {
    /// Plain-words reason shown in the Follow up queue.
    pub fn reason(&self) -> String {
        match self {
            Attention::FollowUpDue { days_late: 0 } => "Follow-up is due today".into(),
            Attention::FollowUpDue { days_late: 1 } => "Follow-up was due yesterday".into(),
            Attention::FollowUpDue { days_late } => {
                format!("Follow-up was due {days_late} days ago")
            }
            Attention::Waiting { status, days } => match status {
                Status::Screening => format!("In screening, applied {days} days ago"),
                _ => format!("Applied {days} days ago, no reply logged"),
            },
            Attention::NoNextStep => "No next step written".into(),
        }
    }

    /// Short form for the table's Follow up column.
    pub fn short(&self) -> String {
        match self {
            Attention::FollowUpDue { days_late: 0 } => "Due today".into(),
            Attention::FollowUpDue { days_late: 1 } => "Due yesterday".into(),
            Attention::FollowUpDue { days_late } => format!("Due {days_late} days ago"),
            Attention::Waiting { days, .. } => format!("{days} days, no reply"),
            Attention::NoNextStep => "No next step".into(),
        }
    }

    /// Sort key: most urgent first.
    pub fn urgency(&self) -> i64 {
        match self {
            Attention::FollowUpDue { days_late } => 100_000 + days_late,
            Attention::Waiting { days, .. } => *days,
            Attention::NoNextStep => -1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Company,
    Role,
    DateApplied,
    FollowUp,
    Link,
}

/// Problems that block saving. Text left in a date column by Excel is not one: it is kept as typed.
pub fn validate(app: &Application, today: NaiveDate) -> Vec<(Field, &'static str)> {
    let mut out = Vec::new();
    if app.company.trim().is_empty() {
        out.push((Field::Company, "Company is required."));
    }
    if app.role.trim().is_empty() {
        out.push((Field::Role, "Role is required."));
    }
    if let Some(d) = app.date_applied.date()
        && d > today
    {
        out.push((Field::DateApplied, "Date applied can't be in the future."));
    }
    let link = app.link.trim();
    if !link.is_empty() && !(link.starts_with("http://") || link.starts_with("https://")) {
        out.push((Field::Link, "Link must start with http:// or https://."));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn app(status: Status, applied: &str, follow_up: &str, next: &str) -> Application {
        Application {
            company: "Pinecone Analytics".into(),
            role: "Data Analyst".into(),
            status,
            date_applied: DateCell::parse(applied),
            follow_up: DateCell::parse(follow_up),
            next_step: next.into(),
            ..Application::default()
        }
    }

    #[test]
    fn status_parses_case_insensitively_and_keeps_unknowns() {
        assert_eq!(Status::parse(" not applied "), Status::NotApplied);
        assert_eq!(Status::parse("Interview"), Status::Interview);
        assert_eq!(Status::parse("On hold"), Status::Other("On hold".into()));
        assert_eq!(Status::parse(""), Status::Other(String::new()));
    }

    #[test]
    fn date_cell_keeps_text_it_cannot_read() {
        assert_eq!(
            DateCell::parse("2026-09-30"),
            DateCell::Date(d("2026-09-30"))
        );
        assert_eq!(
            DateCell::parse("late Sep"),
            DateCell::Text("late Sep".into())
        );
        assert_eq!(DateCell::parse("  "), DateCell::Empty);
    }

    #[test]
    fn follow_up_date_on_or_before_today_needs_attention() {
        let today = d("2026-11-05");
        let a = app(Status::Interview, "2026-10-01", "2026-11-03", "Send thanks");
        assert_eq!(
            a.attention(today, DEFAULT_STALE_DAYS),
            Some(Attention::FollowUpDue { days_late: 2 })
        );
        let a = app(Status::Interview, "2026-10-01", "2026-11-05", "Send thanks");
        assert_eq!(
            a.attention(today, DEFAULT_STALE_DAYS),
            Some(Attention::FollowUpDue { days_late: 0 })
        );
        let a = app(Status::Interview, "2026-10-01", "2026-11-06", "Send thanks");
        assert_eq!(a.attention(today, DEFAULT_STALE_DAYS), None);
    }

    #[test]
    fn applied_14_days_without_follow_up_date_is_waiting() {
        let today = d("2026-11-05");
        let a = app(Status::Applied, "2026-10-22", "", "Wait");
        assert_eq!(
            a.attention(today, DEFAULT_STALE_DAYS),
            Some(Attention::Waiting {
                status: Status::Applied,
                days: 14
            })
        );
        let a = app(Status::Applied, "2026-10-23", "", "Wait");
        assert_eq!(a.attention(today, DEFAULT_STALE_DAYS), None);
        // A follow-up date in the future replaces the 14-day rule.
        let a = app(Status::Applied, "2026-08-01", "2026-11-20", "Wait");
        assert_eq!(a.attention(today, DEFAULT_STALE_DAYS), None);
        // Interview is not covered by the 14-day rule.
        let a = app(Status::Interview, "2026-08-01", "", "Prepare");
        assert_eq!(a.attention(today, DEFAULT_STALE_DAYS), None);
    }

    #[test]
    fn the_waiting_threshold_is_a_setting() {
        let today = d("2026-11-05");
        let a = app(Status::Applied, "2026-10-29", "", "Wait");
        assert_eq!(a.attention(today, 14), None);
        assert_eq!(
            a.attention(today, 7),
            Some(Attention::Waiting {
                status: Status::Applied,
                days: 7
            })
        );
    }

    #[test]
    fn open_without_next_step_needs_attention_closed_never_does() {
        let today = d("2026-11-05");
        let a = app(Status::Offer, "2026-11-01", "", " ");
        assert_eq!(
            a.attention(today, DEFAULT_STALE_DAYS),
            Some(Attention::NoNextStep)
        );
        let a = app(Status::Rejected, "2026-01-01", "2026-02-01", "");
        assert_eq!(a.attention(today, DEFAULT_STALE_DAYS), None);
        let a = app(Status::NotApplied, "", "", "");
        assert_eq!(a.attention(today, DEFAULT_STALE_DAYS), None);
    }

    #[test]
    fn validation_names_the_field() {
        let today = d("2026-11-05");
        let mut a = app(Status::Applied, "2026-11-06", "", "x");
        a.company.clear();
        a.link = "indeed.com/job".into();
        let fields: Vec<Field> = validate(&a, today).into_iter().map(|(f, _)| f).collect();
        assert_eq!(
            fields,
            vec![Field::Company, Field::DateApplied, Field::Link]
        );
        let ok = app(Status::Applied, "2026-11-05", "", "x");
        assert!(validate(&ok, today).is_empty());
    }
}
