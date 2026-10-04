[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Scans, pictures and older Office files

Some files have words only an installed program can get: screenshots, scans, scanned PDFs, and
older Office files such as `.doc` and `.ppt`. When the program is there, Coxswain uses it by itself,
and a search finds those words too.

![Settings, Search inside files, Programs that read more: ✗ tesseract · not installed, ✓ pdftoppm and ✓ LibreOffice](../screenshots/search-scans-tools.png)

## How to use it

1. Install the program (table below).
2. Wait for the [search helper](helper.md)'s next scan, within ten minutes, or click **Index
   now** under *Settings → Search inside files*: the helper looks for the programs at every scan.
3. The files of that kind are then read again, by themselves. Search them as any
   [text](text.md): **Shift+F7**, your words.

| Program | Reads | Notes |
|---|---|---|
| `tesseract` (OCR) | The words in screenshots, scans and pictures: `.png` `.jpg` `.jpeg` `.jpe` `.tif` `.tiff` `.webp` `.bmp` | Photos from a camera (a JPEG whose EXIF names the camera maker) are skipped: they seldom have words and there are thousands. It reads English and your system's language, when tesseract has them |
| `pdftoppm` (with tesseract) | Scanned PDFs, which hold pictures of pages instead of text | Up to 30 pages, at 200 dpi, in grey. Only a PDF with no text of its own |
| LibreOffice (`soffice`) | Older Office files and others no reader here knows: `.doc` `.dot` `.wps` `.wpd` `.pub` `.ppt` `.pps` `.pot` `.vsd` `.vsdx` `.odg` `.sxw` `.sxi` `.sxd` `.lwp` `.pages` `.key` | Made into a PDF in a temporary folder, read, and removed. It has its own profile, so a LibreOffice you have open is not disturbed |

| System | tesseract | pdftoppm | LibreOffice |
|---|---|---|---|
| Debian, Ubuntu | `apt install tesseract-ocr` | `apt install poppler-utils` | `apt install libreoffice` |
| Arch | `pacman -S tesseract tesseract-data-eng` | `pacman -S poppler` | `pacman -S libreoffice-fresh` |
| macOS | `brew install tesseract` | `brew install poppler` | LibreOffice from its site (`/Applications/LibreOffice.app` is looked in) |
| Windows | An installer that puts it in `C:\Program Files\Tesseract-OCR` (looked in), or anywhere on `PATH` | poppler for Windows, on `PATH` | LibreOffice's installer (`C:\Program Files\LibreOffice\program` is looked in) |

For other languages than English and yours, install tesseract's language data too
(`tesseract-ocr-deu`, `tesseract-data-deu` …).

## What you see

*Settings → Search inside files → Programs that read more* lists the three, each with ✓ or ✗:

- *tesseract: words in screenshots, scans and pictures (not camera photos)*
- *pdftoppm: scanned PDFs, with tesseract*
- *LibreOffice: older Office files (.doc, .ppt), Publisher, Visio*

A missing one adds *· not installed*. While tesseract is missing and text search is on, both apps
show the [notice](notices.md) *Install tesseract to search the words in scans, screenshots and
pictures*: the desktop app under *Settings → What's new* until dismissed, the terminal app once in
its status line.

A hit from a picture shows the words tesseract read, as any text hit does. OCR is not perfect:
a word may be read wrong, and then it is found only as it was read.

## Settings and config.toml

None of its own. The programs are found on `PATH` (and in the usual install folders on Windows and
macOS). They run at the lowest priority the system offers, with a time limit of two minutes a
file. The usual limits apply: `text_max_size` (20 MB) and the [folders read](folders.md).

## In the terminal app

The same: the helper reads for both apps. The terminal app has no Settings list of the programs;
the tesseract notice shows in its status line the same way.

## Questions

#### How do I get the words in my screenshots found?
Install tesseract and, for scanned PDFs, poppler's `pdftoppm`. When the helper next starts it finds
them and reads the pictures again. **Index now** in Settings reads the backlog at full speed.

#### I installed tesseract and nothing happened.
The helper looks for the programs once, when it starts. Close every Coxswain window and wait ten
minutes, or make any change under *Settings → Search inside files*. With the helper started with
your session, untick and tick *Start the search helper with my session* (or run
`coxswain --index-service off` and `on`). The ✓ in Settings shows it was found.

#### Why are my holiday photos not read?
A JPEG whose EXIF names the camera maker is a photo, and photos are skipped on purpose: they seldom
have words and there are thousands.

#### Does it read my scanned PDFs page by page?
The first 30 pages, turned into pictures at 200 dpi by `pdftoppm` and read by tesseract. Only a PDF
with no text of its own is handled so.

#### Will reading pictures slow my machine?
The programs run at the lowest priority, one file at a time, with rests between batches, and not
on [battery](battery.md). The first pass over many pictures takes a while; Settings counts
*still to read*.

#### Does LibreOffice need to be closed?
No. It runs with a profile of its own, so the LibreOffice you have open is not touched.

#### Is anything sent anywhere to be read?
No. The programs run on your machine, into a temporary folder that is removed afterwards.

---
[← Previous: Documents it reads](documents.md) · [Next: Diagrams read as sentences →](diagrams.md)
