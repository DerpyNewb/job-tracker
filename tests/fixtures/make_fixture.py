"""Writes legacy.xlsx: the real workbook's layout as of 2026-11-05 (columns A to J, text dates),
with fictional rows. Run with: python -I make_fixture.py"""
from pathlib import Path

from openpyxl import Workbook
from openpyxl.styles import Alignment, Font, PatternFill
from openpyxl.worksheet.datavalidation import DataValidation

HEADERS = ["Company", "Role", "Date Applied", "Location", "Compensation", "Status",
           "Next Step", "Contact", "Link", "Notes"]
WIDTHS = [22, 30, 13, 20, 22, 14, 26, 22, 40, 45]
ROWS = [
    ("Pinecone Analytics", "Data Analyst", "2026-10-01", "Remote", "PHP 45,000/mo", "Applied",
     "Wait for reply", None, "https://example.com/jobs/pinecone-analyst", "Applied through the site."),
    ("Session Road Games", "Junior Game Developer", "2026-10-20", "Baguio City", None, "Screening",
     "Finish the Godot test", "hr@example.com", "https://example.com/jobs/srg-junior",
     "Take-home due in a week."),
    ("Burnham Data Co.", "QA Tester", "2026-09-02", "Work from Home", None, "Rejected", None, None,
     None, "Rejection email 2026-09-20."),
    ("Kennon Labs", "AI Engineer", "2026-10-28", "Remote", "$30-40/hr (contract)", "Interview",
     "Prepare system design examples", "Ana Reyes, ana@example.com",
     "https://example.com/jobs/kennon-ai", None),
    ("Mines View Studio", "Unity Developer", None, "Baguio City", None, "Not applied", None, None,
     "https://example.com/jobs/mvs-unity", "Saved from the shortlist."),
    ("Teachers Camp Tech", "Backend Developer", "late Sep 2026", "Remote", None, "Applied",
     "Check inbox", None, None, "Date is approximate."),
]

wb = Workbook()
ws = wb.active
ws.title = "Applications"
ws.append(HEADERS)
for r in ROWS:
    ws.append(list(r))
head_font = Font(bold=True, color="FFFFFF")
head_fill = PatternFill("solid", fgColor="1F3864")
for c in ws[1]:
    c.font = head_font
    c.fill = head_fill
for row in ws.iter_rows(min_row=2):
    for c in row:
        c.alignment = Alignment(wrap_text=True, vertical="top")
for i, w in enumerate(WIDTHS):
    ws.column_dimensions[chr(65 + i)].width = w
ws.row_dimensions[1].height = 22
ws.freeze_panes = "A2"
ws.auto_filter.ref = f"A1:J{len(ROWS) + 1}"
dv = DataValidation(type="list", formula1='"Applied,Not applied,Screening,Interview,Offer,Rejected,Ghosted,Withdrawn"',
                    allow_blank=True)
dv.add("F2:F500")
ws.add_data_validation(dv)
wb.save(Path(__file__).with_name("legacy.xlsx"))
