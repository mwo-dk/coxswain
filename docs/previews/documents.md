[← README](../../README.md) · [Docs index](../README.md) · [The preview pane](README.md)

# Documents: PDF, Word, EPUB, notebooks, mail, calendars

PDFs, Word documents, e-books, Jupyter notebooks, e-mails, AsciiDoc and reStructuredText,
calendars and address cards are shown as documents, so you can read one without starting the
program it belongs to.

![The desktop app with launch-report.pdf under the cursor in the Documents folder: the preview pane at the right shows the PDF's first page in the webview's PDF viewer, with page number and zoom controls above it](../screenshots/gui-pdf.png)
*A PDF, fitted to the pane's width. The Documents folder beside it holds one of each kind.*

## How to use it

1. Put the cursor on the document and press **Space** or **F3**.
2. Scroll in the pane with the mouse. A PDF has its viewer's own controls: page, zoom, search.
3. For AsciiDoc, calendars and contacts, **Rendered / Source** switches to the text.
4. **Enter** opens the document in its program.

| Key | Desktop app | Terminal app |
|---|---|---|
| **Space** / **F3** | Shows or hides the pane | **F3** opens the file in your pager |
| **Enter** | Opens the file in its program | The same |

## Formats

| Files | Preview | Done by |
|---|---|---|
| `.pdf` | The PDF, with paging and zoom, fitted to the pane's width | The webview's own PDF viewer |
| `.docx` | Headings, paragraphs, lists, tables and embedded pictures | [mammoth](https://github.com/mwilliamson/mammoth.js) |
| `.epub` | The book's title and its first chapter with real text (covers and title pages are skipped) | Read in Rust |
| `.ipynb` | Markdown cells, highlighted code with its `In [n]` number, and outputs: text, errors, tables, PNG and SVG plots | marked, highlight.js |
| `.eml` | *Subject*, *From*, *To*, *Date*, *Attachments* with sizes, and the text of the message | [mail-parser](https://crates.io/crates/mail-parser) |
| `.adoc`, `.asciidoc` | Headings, lists, tables, admonitions, table of contents | [Asciidoctor.js](https://asciidoctor.org/), in secure mode (no file includes) |
| `.rst`, `.rest` | reStructuredText as HTML | pandoc, installed or in a container ([Previews made by tools](tools.md)) |
| `.ics` | The events: title, start and end, place, description | Built in |
| `.vcf` | The cards: name, title, organisation, e-mail, phone | Built in |

Other document formats have their own pages: [HTML pages](html.md), [PowerPoint and older
Office files](office.md) (`.doc`, `.odt`, `.rtf` go through LibreOffice), and
[LaTeX](latex.md).

![A YAML tree, a SQLite database, a certificate and an e-mail with its subject, sender, recipients, date and attachment](../screenshots/gui-previews-data.png)
*The e-mail preview (right): the header facts, the attachment with its size, then the text.*

## What you see

- **PDF:** the viewer's toolbar above the page, opened at page width.
- **EPUB:** the book's title as a heading, then the chapter's text.
- **Notebook:** each code cell with *In [n]* beside it and its output under it.
- **E-mail:** a list of facts, then the plain text of the message (the first 200,000
  characters).
- **Calendar:** one card per event, *No events* for an empty file; **Contacts:** one card per
  person, *(no name)* where the card has none, *No contacts* for an empty file.
- A Word document or notebook over 25 MB says *Too large to preview*.

## Settings and config.toml

None for these formats. reStructuredText uses pandoc: see [Previews made by tools](tools.md)
and `[preview.images] pandoc` ([Containers](containers.md)).

## In the terminal app

No preview pane (a terminal cannot draw pages or pictures). **Enter** opens the document in its
program; **F3** opens it in your pager, which is useful for `.eml`, `.ics`, `.vcf`, `.adoc` and
`.rst`, which are text. Their contents are searchable in both apps:
[Documents search reads](../search/documents.md).

## Questions

#### Why does the EPUB show only the first chapter?

The pane is for a quick look. It skips the cover and title pages and shows the first chapter
that has real text, so you see what the book is about. Press **Enter** to read it in your
e-book reader.

#### A Word document or notebook says "Too large to preview".

Files over 25 MB are not rendered as documents: reading them would hold up the pane. Press
**Enter** to open it in its program.

#### Why does my `.docx` look plainer than in Word?

mammoth reads the document's structure (headings, lists, tables, pictures), not its page
layout, fonts and colours. For the exact look, install LibreOffice and open it with **Enter**,
or save a PDF.

#### Why is a `.doc` (not `.docx`) file not shown?

The older `.doc` format needs LibreOffice. Install it, so `soffice` is found, or set an image
for `[preview.images] libreoffice`. See [PowerPoint and Office](office.md).

#### Do plots in a notebook show?

Yes: PNG and SVG outputs are shown, as are HTML outputs such as pandas tables (cleaned first).
Interactive outputs that need JavaScript (widgets, Plotly) do not run.

#### Can I open an attachment from the e-mail preview?

No, the preview lists attachments with their sizes only. Open the `.eml` with **Enter** in your
mail program to save one.

#### Why does the PDF viewer look different on Windows or macOS?

It is the webview's own viewer: WebKitGTK's on Linux, WebKit's on macOS, WebView2's on Windows.
Each has its own toolbar.

---
[← Previous: Text and code](text-and-code.md) · [Next: HTML pages, sandboxed →](html.md)
