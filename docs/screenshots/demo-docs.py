"""Real documents for the demo home: a spreadsheet, a Word file, a notebook, Markdown with a
diagram and math, and a copy of a system font. Standard library only.

    python3 docs/screenshots/demo-docs.py DEMO_HOME
"""
import base64, json, os, shutil, subprocess, sys, zipfile

home = sys.argv[1]
docs = os.path.join(home, "Documents")


def xml(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


# A spreadsheet (xlsx is a zip of XML parts; inline strings keep it to one sheet file).
rows = [["Month", "Fuel (t)", "Launches", "Cost (kEUR)"],
        ["January", 42.5, 2, 1840], ["February", 38.0, 2, 1715], ["March", 51.2, 3, 2290],
        ["April", 47.9, 3, 2105], ["May", 55.1, 4, 2480], ["June", 60.3, 4, 2655]]
def cell(ref, v):
    if isinstance(v, str):
        return f'<c r="{ref}" t="inlineStr"><is><t>{xml(v)}</t></is></c>'
    return f'<c r="{ref}"><v>{v}</v></c>'
sheet = "".join(
    f'<row r="{r + 1}">' + "".join(cell(f"{'ABCD'[c]}{r + 1}", v) for c, v in enumerate(row)) + "</row>"
    for r, row in enumerate(rows))
with zipfile.ZipFile(os.path.join(docs, "budget.xlsx"), "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("[Content_Types].xml", '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>')
    z.writestr("_rels/.rels", '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>')
    z.writestr("xl/workbook.xml", '<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="2026" sheetId="1" r:id="rId1"/></sheets></workbook>')
    z.writestr("xl/_rels/workbook.xml.rels", '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>')
    z.writestr("xl/worksheets/sheet1.xml", f'<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{sheet}</sheetData></worksheet>')

# A Word document.
paras = [("Heading1", "Flight 7 debrief"), ("", "The ascent was nominal until T+92 s, when the engine cut off early."),
         ("Heading2", "Findings"), ("ListParagraph", "• Fuel pressure dropped 8% below the model."),
         ("ListParagraph", "• The valve log shows a late close command."), ("Heading2", "Next steps"),
         ("", "Re-test the valve on the bench before flight 8.")]
def para(style, text):
    ppr = f'<w:pPr><w:pStyle w:val="{style}"/></w:pPr>' if style else ""
    return f'<w:p>{ppr}<w:r><w:t xml:space="preserve">{xml(text)}</w:t></w:r></w:p>'
body = "".join(para(st, t) for st, t in paras)
with zipfile.ZipFile(os.path.join(docs, "debrief.docx"), "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("[Content_Types].xml", '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>')
    z.writestr("_rels/.rels", '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
    z.writestr("word/document.xml", f'<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>')

# Markdown with a Mermaid diagram and math.
open(os.path.join(docs, "architecture.md"), "w").write("""# Launch sequence

```mermaid
flowchart TD
    A[Countdown] --> B{Go / no-go}
    B -->|go| C[Ignition]
    B -->|hold| A
    C --> D[Lift-off]
    D --> E[Stage separation]
```

The rocket equation gives the change in speed:

$$\\Delta v = v_e \\ln \\frac{m_0}{m_f}$$

With $v_e = 3.1$ km/s and a mass ratio of 8, that is about $6.4$ km/s.
""")

# A notebook with markdown, code, text output and a plotted image.
png = os.path.join(docs, ".plot.png")
subprocess.run(["magick", "-size", "480x220", "xc:white", "-stroke", "#2a9d8f", "-strokewidth", "3", "-fill", "none",
                "-draw", "polyline 20,200 90,170 160,120 230,95 300,60 370,45 460,20",
                "-stroke", "#999", "-strokewidth", "1", "-draw", "line 20,200 460,200", png], check=True)
image = base64.b64encode(open(png, "rb").read()).decode()
os.remove(png)
nb = {"nbformat": 4, "nbformat_minor": 5,
      "metadata": {"kernelspec": {"name": "python3", "language": "python", "display_name": "Python 3"}},
      "cells": [
          {"cell_type": "markdown", "metadata": {}, "source": ["# Speed over time\n", "Telemetry from flight 7."]},
          {"cell_type": "code", "execution_count": 1, "metadata": {}, "outputs": [
              {"output_type": "stream", "name": "stdout", "text": ["max speed: 612.4 m/s at t=92s\n"]}],
           "source": ["import pandas as pd\n", "t = pd.read_csv('telemetry.csv')\n", "print(f\"max speed: {t.speed.max():.1f} m/s at t={t.speed.idxmax()}s\")"]},
          {"cell_type": "code", "execution_count": 2, "metadata": {}, "outputs": [
              {"output_type": "display_data", "metadata": {}, "data": {"image/png": image, "text/plain": ["<Figure>"]}}],
           "source": ["t.plot(x='time', y='speed')"]}]}
json.dump(nb, open(os.path.join(docs, "telemetry.ipynb"), "w"), indent=1)

# A font file (a copy of the system sans; the Noto fonts are OFL-licensed).
font = subprocess.run(["fc-match", "-f", "%{file}", "sans"], capture_output=True, text=True).stdout
if font.endswith((".ttf", ".otf")):
    shutil.copy(font, os.path.join(home, "Downloads", os.path.basename(font)))

# ---------------------------------------------------------------- part 2 formats
import plistlib, sqlite3

db = sqlite3.connect(os.path.join(docs, "launches.db"))
db.executescript("""
CREATE TABLE launch(id INTEGER PRIMARY KEY, date TEXT, vehicle TEXT, outcome TEXT);
CREATE TABLE crew(id INTEGER PRIMARY KEY, name TEXT, role TEXT);
CREATE TABLE telemetry(launch_id INTEGER REFERENCES launch(id), t REAL, speed REAL);
CREATE VIEW failures AS SELECT * FROM launch WHERE outcome != 'nominal';
""")
db.executemany("INSERT INTO launch(date, vehicle, outcome) VALUES (?, ?, ?)",
               [(f"2026-0{m}-1{m}", "Rocket 1", "nominal" if m != 7 else "early cut-off") for m in range(1, 8)])
db.executemany("INSERT INTO crew(name, role) VALUES (?, ?)", [("Ada", "flight director"), ("Grace", "propulsion"), ("Linus", "telemetry")])
db.executemany("INSERT INTO telemetry VALUES (?, ?, ?)", [(7, t / 10, t * 6.6) for t in range(930)])
db.commit()
db.close()

open(os.path.join(docs, "flight7.eml"), "w").write(
    "From: Ada Lovelace <ada@example.com>\r\nTo: Launch team <team@example.com>\r\n"
    "Subject: Flight 7 debrief on Friday\r\nDate: Mon, 28 Sep 2026 09:12:00 +0200\r\n"
    "MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=b1\r\n\r\n"
    "--b1\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n"
    "Hi all,\r\n\r\nThe engine cut off at T+92 s. Grace has the valve logs; let us go through them\r\n"
    "on Friday at 10:00 before we book the bench test.\r\n\r\nAda\r\n"
    "--b1\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"launch-report.pdf\"\r\n"
    "Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--b1--\r\n")

open(os.path.join(docs, "schedule.ics"), "w").write("\r\n".join([
    "BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//demo//EN",
    "BEGIN:VEVENT", "UID:1", "SUMMARY:Flight 7 debrief", "DTSTART:20261002T080000Z", "DTEND:20261002T090000Z", "LOCATION:Mission control", "END:VEVENT",
    "BEGIN:VEVENT", "UID:2", "SUMMARY:Valve bench test", "DTSTART:20261006T120000Z", "DTEND:20261006T150000Z", "LOCATION:Test stand B",
    "DESCRIPTION:Bring the pressure logger.", "END:VEVENT",
    "BEGIN:VEVENT", "UID:3", "SUMMARY:Flight 8 go / no-go", "DTSTART;VALUE=DATE:20261015", "END:VEVENT",
    "END:VCALENDAR", ""]))

open(os.path.join(docs, "team.vcf"), "w").write("\r\n".join([
    "BEGIN:VCARD", "VERSION:4.0", "FN:Ada Lovelace", "TITLE:Flight director", "ORG:Rocket Team", "EMAIL:ada@example.com", "TEL:+45 12 34 56 78", "END:VCARD",
    "BEGIN:VCARD", "VERSION:4.0", "FN:Grace Hopper", "TITLE:Propulsion", "ORG:Rocket Team", "EMAIL:grace@example.com", "END:VCARD", ""]))

with open(os.path.join(docs, "telemetry.jsonl"), "w") as f:
    for t in range(0, 100, 10):
        f.write(json.dumps({"t": t, "speed": round(t * 6.6, 1), "altitude": t * t * 3, "stage": 1 if t < 60 else 2}) + "\n")

open(os.path.join(docs, "mission.yaml"), "w").write("""mission: Flight 8
vehicle:
  name: Rocket 1
  stages: 2
  engines: [main, vernier]
window:
  opens: 2026-10-15T08:00:00Z
  closes: 2026-10-15T11:00:00Z
crew:
  - name: Ada
    role: flight director
  - name: Grace
    role: propulsion
checks:
  valve_bench_test: true
  weather_minimum_kt: 25
""")

open(os.path.join(docs, "launch.log"), "w").write("\n".join([
    "2026-09-27 08:00:00 INFO  countdown started, T-600",
    "2026-09-27 08:09:30 INFO  go / no-go poll: go",
    "2026-09-27 08:10:00 INFO  ignition",
    "2026-09-27 08:10:01 DEBUG thrust 98.7 kN, chamber pressure 6.9 MPa",
    "2026-09-27 08:11:20 WARN  fuel pressure 8% below model",
    "2026-09-27 08:11:32 ERROR main engine cut-off at T+92 s (valve close command)",
    "2026-09-27 08:11:33 INFO  coasting, telemetry nominal", ""]))

with open(os.path.join(docs, "Info.plist"), "wb") as f:
    plistlib.dump({"CFBundleName": "Rocket", "CFBundleVersion": "0.1.0", "LSMinimumSystemVersion": "13.0", "NSHighResolutionCapable": True}, f, fmt=plistlib.FMT_BINARY)

chapter = "<p>" + " ".join(["A rocket works by throwing mass backwards very fast; the faster it is thrown, the less of it you need."] * 6) + "</p>"
with zipfile.ZipFile(os.path.join(home, "Downloads", "rocketry-primer.epub"), "w") as z:
    z.writestr("mimetype", "application/epub+zip", compress_type=zipfile.ZIP_STORED)
    z.writestr("META-INF/container.xml", '<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>')
    z.writestr("OEBPS/content.opf", '<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>A Rocketry Primer</dc:title></metadata><manifest><item id="cover" href="cover.xhtml" media-type="application/xhtml+xml"/><item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="cover"/><itemref idref="c1"/></spine></package>')
    z.writestr("OEBPS/cover.xhtml", '<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>A Rocketry Primer</h1></body></html>')
    z.writestr("OEBPS/ch1.xhtml", f'<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>1. Throwing things backwards</h1>{chapter}</body></html>')

if shutil.which("openssl"):
    subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "20", "-subj", "/CN=launch.example.com/O=Rocket Team",
                    "-addext", "subjectAltName=DNS:launch.example.com,DNS:telemetry.example.com",
                    "-keyout", os.devnull, "-out", os.path.join(docs, "launch.example.com.pem")], check=True, capture_output=True)
