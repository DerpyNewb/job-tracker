# Job tracker

A Windows desktop app for logging job applications. It reads and writes
`job-search/Job_Applications.xlsx` directly. The workbook stays the record: Excel, Google Drive
and Claude Code sessions keep working with it as before.

Design and reasons: `docs/superpowers/specs/2026-11-05-job-tracker-design.md`.

## Use

```powershell
cargo run --release -- "C:\path\to\Job_Applications.xlsx"
```

With no path it reopens the most recent workbook (Settings can turn that off), or shows the
Recent list and an Open button.

- **Follow up** (left): what needs attention, most urgent first. A row lands here when its
  follow-up date is today or past; when it has no follow-up date and has sat at Applied or
  Screening for 14 days; or when it is open (Applied, Screening, Interview, Offer) with no next step.
- **Status** (left): counts per status; click one to filter the table.
- **Table** (middle): click a heading to sort, a row to edit it. Search covers company, role,
  notes, next step, contact and source.
- **Editor** (right): every column. Resume sent offers the PDFs in `resumes/` and
  `resumes/per-posting/`.
- **Open** (top bar): browse, or pick one of the last 8 workbooks. Opening another workbook with
  unsaved changes asks first. The window title shows the open workbook, with `*` when unsaved.
- **Settings** (top bar): theme, text size, the days before an unanswered application is flagged
  (default 14), an optional follow-up date for new applications, reopening the last workbook,
  backups kept (default 20) and the backups folder, and the resumes folder. Stored by eframe in
  `%APPDATA%\Job tracker\data\app.ron`.
- Keys: Ctrl+S save, Ctrl+N add, Ctrl+F search, Ctrl+O open, Ctrl+, settings.

What could come next, and what each needs decided: `docs/superpowers/plans/2026-11-05-job-tracker-roadmap.md`.

## From a terminal (`jt`)

`jt.exe` is the same tracker as a console program, for scripts and Claude Code sessions. Its saves
go through the same checks and backups as the window's Save (see below). It uses the window's
Settings (days before flagging, follow-up date for new rows, backups kept) once the window has
saved them; until then, the defaults.

```powershell
$jt = "C:/rust-target/job-tracker/release/jt.exe"   # after cargo build --release
& $jt due                                           # the Follow up queue, most urgent first
& $jt list                                          # open applications; --all for every row
& $jt show 5                                        # every column of row 5
& $jt add --company "Pinecone Analytics" --role "Data Analyst" --source Indeed --link https://...
& $jt set pinecone --status Interview --next-step "Prepare SQL questions" --follow-up 2026-11-20
& $jt set 5 --follow-up -                           # - clears a field
& $jt help
```

A row is its Excel row number, or text that matches exactly one company. Statuses must be one of
the eight; dates are YYYY-MM-DD or `today`. `add` refuses a company and role already listed unless
given `--force`. The workbook is `--file PATH`, else the one the window opened last, else
`job-search/Job_Applications.xlsx` under the current folder. There is no delete; use the window.
If the window is open with unsaved edits when `jt` saves, the window's next Save refuses and Reload
puts its edits back on top.

## Try it on the sample

`sample/Sample_Job_Applications.xlsx` is a fictional workbook in the full layout: every status, a
hand-typed status (On hold), a text date, an extra column (Interview Rounds), and rows for each
Follow up reason. Open it from the app, or remake it with dates counted from today:

```powershell
cargo run --example make_sample                     # sample/Sample_Job_Applications.xlsx
cargo run --example make_sample -- C:/temp/Sample_Job_Applications.xlsx
```

It only overwrites files with "sample" in the name, so it can't replace the real workbook. Its
Resume values name real resume variants; the Pick list is empty unless Settings points the
resumes folder at `resumes/`.

## What saving does

1. Refuses if the workbook is open in Excel (its `~$` lock file exists), if the file changed
   since it was opened, or if the workbook has a second sheet (which a save would delete).
   When the file changed, Reload puts your unsaved changes back on top of the new version.
2. Copies the current file to `%LOCALAPPDATA%\job-tracker\backups\` (last 20 kept; not synced).
3. Writes the whole workbook to a temp file beside it, then swaps it in.
4. Reads it back; if anything differs, the backup is put back.

Each save rewrites the sheet with its fixed layout: navy header, column widths, frozen header,
filter, Status dropdown. Formatting added by hand in Excel is not kept.

The first save of the 2026-11-05 workbook adds Source, Resume and Follow-up Date (K to M) and
turns the text dates into Excel dates.

## Develop

Needs Rust and the MSVC build tools with a Windows SDK (see `docs/Rust_Desktop_App_Guide.md`).
Run cargo from PowerShell, not Git Bash. Build output goes to `C:/rust-target/job-tracker`
(set in `.cargo/config.toml`) so it stays out of Google Drive; the release .exe is
`C:/rust-target/job-tracker/release/job-tracker.exe`.

```powershell
cargo test                       # model, workbook, save checks, and rendered screens
cargo clippy --all-targets -- -D warnings
$env:UPDATE_SNAPSHOTS="true"; cargo test --test screens   # refresh tests/snapshots/*.png
```

`tests/real_copy.rs` saves a copy of the real workbook and checks it reads back the same. It is
ignored by default; set `JT_REAL_COPY` to a copy outside the synced folder and pass `--ignored`.

`tests/screens.rs` renders the window headless with egui_kittest into `tests/snapshots/`, so
each screen can be checked as an image. All test data is fictional
(`tests/fixtures/make_fixture.py` writes `legacy.xlsx` in the real layout). The real workbook
never goes into this folder.

Font: Source Sans 3 (SIL Open Font License, `assets/fonts/OFL.txt`).
