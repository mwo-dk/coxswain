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
