# md-export

Convert GitHub-flavored Markdown into a **self-contained, styled HTML file** —
embedded CSS, no external assets, no network, no JavaScript required.

Point it at a `.md` file and you get a single `.html` you can open in any
browser, email as an attachment, or drop on a static host. Everything (styles,
table-of-contents, code-block classes) is inlined into the one file.

## Install / build

```sh
cargo build --release
# binary at target/release/md-export
```

## Usage

```sh
md-export in.md -o out.html            # file → standalone HTML doc
md-export in.md > out.html             # stdout
cat in.md | md-export                  # stdin → stdout
md-export in.md --toc -o out.html      # with a table of contents
md-export in.md --theme dark -o out.html
md-export in.md --no-style > frag.html # bare HTML fragment (no wrapper/CSS)
```

### Options

| Flag              | Description                                                            |
|-------------------|------------------------------------------------------------------------|
| `<input>`         | Input Markdown file; omit or `-` to read **stdin**.                     |
| `-o, --output`    | Output HTML file; omit or `-` to write **stdout**.                      |
| `--title <T>`     | Document title. Defaults to the first `# H1`, then the input filename.  |
| `--theme <T>`     | `light`, `dark`, or `auto` (default). Controls the embedded CSS.        |
| `--toc`           | Generate a table of contents from headings, with anchor links.         |
| `--standalone`    | Emit a full `<!doctype html>` document (this is the default).           |
| `--no-style`      | Emit a bare HTML fragment — no `<html>`/`<head>` wrapper and no CSS.    |

`--theme auto` ships both palettes and switches via
`@media (prefers-color-scheme: dark)`.

## GitHub-flavored Markdown support

- **Tables** → `<table>`
- **Strikethrough** (`~~text~~`) → `<del>`
- **Task lists** (`- [x]` / `- [ ]`) → disabled checkboxes
- **Autolinks** → bare URLs become links
- **Fenced code blocks** → `<pre><code class="language-rust">…</code></pre>`

Code blocks are emitted with a `language-*` class so a client-side highlighter
(e.g. highlight.js / Prism) *can* style them, but none is required or bundled —
the output renders fine as-is.

### Heading anchors & TOC

Every heading gets a GitHub-style slug `id` (e.g. `## Sub Section` →
`id="sub-section"`, duplicates disambiguated with `-1`, `-2`, …). With `--toc`,
the generated table-of-contents links point at exactly those ids.

## Library

The conversion logic lives in a small library (`md_export`) with a single
entry point:

```rust
use md_export::{render, RenderOptions, Theme};

let opts = RenderOptions { toc: true, theme: Theme::Dark, ..Default::default() };
let html = render("# Hello\n\nworld\n", &opts);
```

`render` is pure (Markdown string + options → HTML string); `slugify`,
`render_toc`, and `theme_css` are also public.

## Example

```sh
md-export examples/sample.md --toc -o sample.html
```

produces a standalone `sample.html` containing the doctype, an embedded
`<style>` block, a table of contents, and the rendered document.

## License

MIT — see [LICENSE](LICENSE).
