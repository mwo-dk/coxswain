[← README](../../README.md) · [Docs index](../README.md) · [The preview pane](README.md)

# HTML pages, sandboxed

An `.html`, `.htm` or `.xhtml` file is shown as a browser shows it, with its own styles,
pictures and fonts, but inside a sandbox: no script runs and nothing is fetched from the web.
A page you downloaded cannot run code or tell anyone that you opened it.

<!-- screenshot: previews-html.png: desktop app, Cyber theme: an index.html from the demo website folder under the cursor; the preview pane shows the page with its own CSS and a local picture on a white background, with Rendered / Source at the top right -->

## How to use it

1. Put the cursor on the HTML file and press **Space** or **F3**.
2. **Rendered** (the default) shows the page; **Source** shows the HTML, highlighted.
3. **Enter** opens the page in your browser, where its scripts do run.

| Key | Desktop app | Terminal app |
|---|---|---|
| **Space** / **F3** | Shows or hides the pane | **F3** opens the HTML source in your pager |
| **Enter** | Opens the page in your browser | The same |

## What is allowed, and what is blocked

The page is put in a frame with an empty `sandbox` attribute and a content security policy
placed before the page's own head, where the page cannot undo it.

| | In the preview |
|---|---|
| The page's HTML and inline `<style>` | Shown |
| Stylesheets, pictures, fonts, video and audio next to the page (`style.css`, `img/logo.png`) | Loaded, from disk through the app's file protocol; relative paths resolve from the page's folder |
| Pictures and fonts written into the page as `data:` URLs | Shown |
| `<script>`, `onclick=` and other scripts | Never run (the frame allows no scripts) |
| Anything from the web: `https://` pictures, stylesheets, web fonts, trackers | Blocked (`default-src 'none'`) |
| Frames inside the page, forms, pop-ups, plug-ins | Blocked |
| Access to the app or to other pages | None (the frame has no origin of its own) |

The frame has a white background whatever the theme, as a web page expects.

## What you see

- The page, scrollable, filling the pane.
- Pictures from the web show as empty boxes or their alt text; web fonts fall back to local
  ones.
- A page that builds itself with JavaScript (many web apps) shows only what its HTML holds,
  often an empty page or a *"please enable JavaScript"* note.

## Settings and config.toml

None. The sandbox cannot be switched off: use **Enter** to see a page with its scripts.

## In the terminal app

No preview pane; a terminal cannot draw a page. **F3** shows the HTML source in your pager,
**Enter** opens the page in your browser.

## Questions

#### Why does the page look broken or empty?

Its content comes from JavaScript, or its styles come from the web. Neither reaches the preview,
on purpose. Press **Enter** to open it in your browser, or **Source** to read the HTML.

#### Why are HTML files not just shown as code, as before?

They were, until Coxswain 1.17.0. With scripts and the network cut off,
a page can be shown safely, and seeing it is what you usually want. **Source** gives the old
view, and the choice sticks.

#### Can a downloaded page find out that I looked at it?

No. Tracking pixels, web fonts and scripts are how a page reports back, and all three are
blocked: nothing leaves your machine when the preview shows a page.

#### Are pictures next to the page shown?

Yes. A `<base>` pointing at the page's folder is added, so `img/logo.png` and `../shared/site.css`
load from disk as they would in a browser opening the file.

#### Can I click links in the preview?

The preview is for looking. Open the page with **Enter** to follow its links in your browser.

#### Why is the page white in a dark theme?

A web page sets its own colours and expects a white page under them; the frame gives it one, as
a browser does.

#### A very large HTML file is cut off.

The preview reads the first 512 KB of a text file, HTML included, and says so under the page
(*Showing the first 512 KB*). Open larger pages with **Enter**.

---
[← Previous: Documents](documents.md) · [Next: PowerPoint and Office →](office.md)
