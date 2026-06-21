//! md-export core library.
//!
//! Converts GitHub-flavored Markdown into a self-contained, styled HTML
//! document (embedded CSS, no external assets) — or a bare HTML fragment.
//!
//! The public entry point is [`render`], which takes a Markdown string plus a
//! set of [`RenderOptions`] and returns an HTML string. All work is local;
//! there is no network or filesystem access in this module.

use std::collections::HashMap;
use std::fmt::Write as _;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{format_html, parse_document, Arena, Options};

/// Color theme for the embedded stylesheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// Light background, dark text.
    Light,
    /// Dark background, light text.
    Dark,
    /// Light by default, switches to dark via `prefers-color-scheme: dark`.
    Auto,
}

impl Theme {
    /// Parse a theme name (`light`, `dark`, `auto`), case-insensitively.
    pub fn parse(s: &str) -> Option<Theme> {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" => Some(Theme::Light),
            "dark" => Some(Theme::Dark),
            "auto" => Some(Theme::Auto),
            _ => None,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Auto
    }
}

/// Options controlling a single [`render`] call.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Explicit document title. If `None`, the title is inferred from the first
    /// level-1 heading, falling back to [`RenderOptions::fallback_title`].
    pub title: Option<String>,
    /// Title used when no explicit title is given and no H1 is present
    /// (typically the input file's stem, or `"Document"` for stdin).
    pub fallback_title: String,
    /// Color theme for the embedded CSS (ignored when `style` is `false`).
    pub theme: Theme,
    /// Emit a table of contents built from the document headings.
    pub toc: bool,
    /// Wrap the rendered body in a full `<!doctype html>` document.
    /// When `false`, only the HTML fragment (body) is returned.
    pub standalone: bool,
    /// Embed a `<style>` block (only meaningful with `standalone == true`).
    /// When `false`, no CSS is emitted (`--no-style`).
    pub style: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            title: None,
            fallback_title: "Document".to_string(),
            theme: Theme::Auto,
            toc: false,
            standalone: true,
            style: true,
        }
    }
}

/// A single heading discovered in the document, used to build the TOC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// Heading level (1–6).
    pub level: u8,
    /// Visible heading text (Markdown inline formatting stripped).
    pub text: String,
    /// Anchor id, matching the `id` attribute comrak emits on the heading.
    pub slug: String,
}

/// Slugify a single heading's text the same way comrak does when
/// `header_id_prefix` is enabled, so generated TOC links match the `id`
/// attributes in the rendered HTML.
///
/// Algorithm (reverse-engineered and verified against comrak 0.52):
/// for each character — keep Unicode alphanumerics and `_` (lowercased);
/// map a space or `-` to `-`; drop everything else (including tabs and
/// punctuation). No collapsing of consecutive dashes, no trimming.
pub fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            out.extend(ch.to_lowercase());
        } else if ch == ' ' || ch == '-' {
            out.push('-');
        }
        // all other characters are dropped
    }
    out
}

/// Disambiguate slugs exactly as comrak does: the first occurrence keeps the
/// base slug; later collisions get `-1`, `-2`, … appended.
fn dedup_slug(base: &str, seen: &mut HashMap<String, usize>) -> String {
    match seen.get_mut(base) {
        None => {
            seen.insert(base.to_string(), 0);
            base.to_string()
        }
        Some(count) => {
            *count += 1;
            format!("{base}-{count}")
        }
    }
}

/// Recursively collect the visible text of a node (heading content), flattening
/// inline formatting. Code spans contribute their literal text.
fn collect_text<'a>(node: &'a AstNode<'a>, out: &mut String) {
    for child in node.children() {
        match &child.data.borrow().value {
            NodeValue::Text(t) => out.push_str(t),
            NodeValue::Code(c) => out.push_str(&c.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
            _ => collect_text(child, out),
        }
    }
}

/// Build comrak options with the GFM feature set this tool supports enabled.
fn comrak_options() -> Options<'static> {
    let mut options = Options::default();
    // GitHub-flavored Markdown extensions.
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.tagfilter = true;
    options.extension.footnotes = true;
    // Emit `id` attributes on headings (empty prefix) for anchor links.
    options.extension.header_id_prefix = Some(String::new());
    // Keep author-supplied raw HTML; this is a trusted local conversion tool.
    options.render.r#unsafe = true;
    options
}

/// Extract the ordered list of headings (for the TOC) and the inferred title
/// (text of the first H1, if any) from the parsed document.
fn collect_headings<'a>(root: &'a AstNode<'a>) -> (Vec<Heading>, Option<String>) {
    let mut headings = Vec::new();
    let mut first_h1: Option<String> = None;
    let mut seen: HashMap<String, usize> = HashMap::new();

    for node in root.descendants() {
        let level = match &node.data.borrow().value {
            NodeValue::Heading(h) => h.level,
            _ => continue,
        };
        let mut text = String::new();
        collect_text(node, &mut text);
        let text = text.trim().to_string();

        if level == 1 && first_h1.is_none() && !text.is_empty() {
            first_h1 = Some(text.clone());
        }

        let slug = dedup_slug(&slugify(&text), &mut seen);
        headings.push(Heading { level, text, slug });
    }

    (headings, first_h1)
}

/// Escape text for safe inclusion in HTML element content / attribute values.
fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Render a table-of-contents `<nav>` fragment from the collected headings.
///
/// Headings are rendered as a flat nested-by-indent list; each entry links to
/// the matching heading anchor. Returns an empty string if there are no
/// headings.
pub fn render_toc(headings: &[Heading]) -> String {
    if headings.is_empty() {
        return String::new();
    }
    // Normalize so the shallowest heading present sits at indent 0.
    let min_level = headings.iter().map(|h| h.level).min().unwrap_or(1);
    let mut out = String::new();
    out.push_str("<nav class=\"toc\" aria-label=\"Table of contents\">\n");
    out.push_str("<p class=\"toc-title\">Contents</p>\n<ul>\n");
    for h in headings {
        let depth = (h.level - min_level) as usize;
        // 2 spaces of base indent + nesting for readability of the output.
        let pad = "  ".repeat(depth + 1);
        let _ = write!(
            out,
            "{pad}<li class=\"toc-l{}\"><a href=\"#{}\">{}</a></li>\n",
            h.level,
            escape_html(&h.slug),
            escape_html(&h.text),
        );
    }
    out.push_str("</ul>\n</nav>\n");
    out
}

/// Convert Markdown to the inner HTML body (no `<html>`/`<head>` wrapper).
/// Includes the TOC when requested. This is what `--no-style`-with-standalone
/// and the fragment path both build on.
fn render_body(markdown: &str, options: &RenderOptions) -> (String, Option<String>) {
    let arena = Arena::new();
    let comrak_opts = comrak_options();
    let root = parse_document(&arena, markdown, &comrak_opts);

    let (headings, first_h1) = collect_headings(root);

    // comrak 0.52's `format_html` writes into a `std::fmt::Write` sink (a String).
    let mut html = String::new();
    format_html(root, &comrak_opts, &mut html).expect("formatting HTML into a String cannot fail");

    let body = if options.toc {
        let toc = render_toc(&headings);
        format!("{toc}{html}")
    } else {
        html
    };

    (body, first_h1)
}

/// Resolve the document title: explicit option, else first H1, else fallback.
fn resolve_title(options: &RenderOptions, first_h1: &Option<String>) -> String {
    if let Some(t) = &options.title {
        return t.clone();
    }
    if let Some(h1) = first_h1 {
        return h1.clone();
    }
    options.fallback_title.clone()
}

/// Render Markdown to HTML according to `options`.
///
/// - When `options.standalone` is `true`, returns a complete
///   `<!doctype html>` document, with an embedded `<style>` block unless
///   `options.style` is `false`.
/// - When `options.standalone` is `false`, returns just the HTML fragment
///   (body), with no wrapper and no CSS.
pub fn render(markdown: &str, options: &RenderOptions) -> String {
    let (body, first_h1) = render_body(markdown, options);

    if !options.standalone {
        return body;
    }

    let title = resolve_title(options, &first_h1);
    let style_block = if options.style {
        format!("<style>\n{}\n</style>\n", theme_css(options.theme))
    } else {
        String::new()
    };

    format!(
        "<!doctype html>\n\
<html lang=\"en\">\n\
<head>\n\
<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{title}</title>\n\
{style}\
</head>\n\
<body>\n\
<main class=\"markdown-body\">\n\
{body}\
</main>\n\
</body>\n\
</html>\n",
        title = escape_html(&title),
        style = style_block,
        body = body,
    )
}

/// Return the embedded CSS for a theme. Self-contained: no external fonts,
/// no `@import`, no asset URLs.
pub fn theme_css(theme: Theme) -> String {
    // Shared structural CSS (theme-independent).
    let base = r#":root {
  --maxw: 820px;
}
* { box-sizing: border-box; }
html { -webkit-text-size-adjust: 100%; }
body {
  margin: 0;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  font-size: 16px;
  line-height: 1.6;
  background: var(--bg);
  color: var(--fg);
}
.markdown-body {
  max-width: var(--maxw);
  margin: 0 auto;
  padding: 2.5rem 1.25rem 4rem;
}
.markdown-body h1, .markdown-body h2, .markdown-body h3,
.markdown-body h4, .markdown-body h5, .markdown-body h6 {
  line-height: 1.25;
  margin: 1.6em 0 0.6em;
  font-weight: 600;
}
.markdown-body h1 { font-size: 2rem; border-bottom: 1px solid var(--border); padding-bottom: .3em; }
.markdown-body h2 { font-size: 1.5rem; border-bottom: 1px solid var(--border); padding-bottom: .3em; }
.markdown-body h3 { font-size: 1.25rem; }
.markdown-body h4 { font-size: 1rem; }
.markdown-body p, .markdown-body ul, .markdown-body ol, .markdown-body blockquote, .markdown-body table {
  margin: 0 0 1rem;
}
.markdown-body a { color: var(--link); text-decoration: none; }
.markdown-body a:hover { text-decoration: underline; }
.markdown-body a.anchor { float: left; margin-left: -1em; padding-right: .25em; }
.markdown-body code {
  font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace;
  font-size: 0.9em;
  background: var(--code-bg);
  padding: 0.2em 0.4em;
  border-radius: 4px;
}
.markdown-body pre {
  background: var(--code-bg);
  padding: 1rem;
  border-radius: 8px;
  overflow: auto;
  border: 1px solid var(--border);
}
.markdown-body pre code { background: none; padding: 0; font-size: 0.875em; }
.markdown-body blockquote {
  margin-left: 0;
  padding: 0.25rem 1rem;
  border-left: 4px solid var(--border);
  color: var(--muted);
}
.markdown-body table { border-collapse: collapse; width: 100%; display: block; overflow-x: auto; }
.markdown-body th, .markdown-body td { border: 1px solid var(--border); padding: 0.5rem 0.75rem; }
.markdown-body th { background: var(--code-bg); font-weight: 600; }
.markdown-body tr:nth-child(2n) td { background: var(--stripe); }
.markdown-body img { max-width: 100%; }
.markdown-body hr { border: none; border-top: 1px solid var(--border); margin: 2rem 0; }
.markdown-body ul.contains-task-list { list-style: none; padding-left: 1.2em; }
.markdown-body input[type="checkbox"] { margin-right: 0.4em; }
.markdown-body del { color: var(--muted); }
nav.toc {
  background: var(--code-bg);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 0.5rem 1.25rem 1rem;
  margin: 0 0 2rem;
}
nav.toc .toc-title { font-weight: 600; margin: 0.75rem 0 0.25rem; }
nav.toc ul { list-style: none; padding-left: 0; margin: 0; }
nav.toc li { margin: 0.15rem 0; }
nav.toc li.toc-l2 { padding-left: 1rem; }
nav.toc li.toc-l3 { padding-left: 2rem; }
nav.toc li.toc-l4 { padding-left: 3rem; }
nav.toc li.toc-l5 { padding-left: 4rem; }
nav.toc li.toc-l6 { padding-left: 5rem; }
"#;

    let light_vars = r#":root {
  --bg: #ffffff;
  --fg: #1f2328;
  --muted: #59636e;
  --border: #d1d9e0;
  --link: #0969da;
  --code-bg: #f6f8fa;
  --stripe: #f6f8fa;
}
"#;

    let dark_vars = r#":root {
  --bg: #0d1117;
  --fg: #e6edf3;
  --muted: #9198a1;
  --border: #30363d;
  --link: #4493f8;
  --code-bg: #161b22;
  --stripe: #161b22;
}
"#;

    match theme {
        Theme::Light => format!("{light_vars}{base}"),
        Theme::Dark => format!("{dark_vars}{base}"),
        Theme::Auto => {
            format!("{light_vars}@media (prefers-color-scheme: dark) {{\n{dark_vars}}}\n{base}",)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light_standalone() -> RenderOptions {
        RenderOptions {
            theme: Theme::Light,
            ..RenderOptions::default()
        }
    }

    #[test]
    fn slugify_matches_comrak_basic() {
        assert_eq!(slugify("Foo Bar"), "foo-bar");
        assert_eq!(slugify("Mixed CASE Title"), "mixed-case-title");
    }

    #[test]
    fn slugify_matches_comrak_punctuation() {
        // Verified against comrak 0.52 output.
        assert_eq!(slugify("Foo & Bar (baz)!"), "foo--bar-baz");
        assert_eq!(slugify("Hello, World -- Test"), "hello-world----test");
        assert_eq!(slugify("100% Done"), "100-done");
        assert_eq!(slugify("a.b.c"), "abc");
        assert_eq!(slugify("+ plus start"), "-plus-start");
        assert_eq!(
            slugify("under_score and dash-dash"),
            "under_score-and-dash-dash"
        );
    }

    #[test]
    fn slugify_keeps_unicode_letters() {
        assert_eq!(slugify("café déjà vu"), "café-déjà-vu");
    }

    #[test]
    fn slugify_drops_tabs() {
        assert_eq!(slugify("Tabs\there"), "tabshere");
    }

    #[test]
    fn headings_render_with_slug_ids() {
        let html = render("# Hello World\n\n## Sub Section\n", &light_standalone());
        assert!(html.contains("id=\"hello-world\""), "missing h1 id: {html}");
        assert!(html.contains("id=\"sub-section\""), "missing h2 id: {html}");
        assert!(
            html.contains("<h1>") || html.contains("<h1 "),
            "no <h1>: {html}"
        );
        assert!(
            html.contains("<h2>") || html.contains("<h2 "),
            "no <h2>: {html}"
        );
    }

    #[test]
    fn duplicate_headings_get_unique_ids() {
        let html = render("## Dup\n\n## Dup\n", &light_standalone());
        assert!(html.contains("id=\"dup\""), "missing first dup id: {html}");
        assert!(html.contains("id=\"dup-1\""), "missing deduped id: {html}");
    }

    #[test]
    fn gfm_table_renders() {
        let md = "| a | b |\n|---|---|\n| 1 | 2 |\n";
        let html = render(md, &light_standalone());
        assert!(html.contains("<table>"), "no table: {html}");
        assert!(html.contains("<th>a</th>"), "no header cell: {html}");
        assert!(html.contains("<td>1</td>"), "no data cell: {html}");
    }

    #[test]
    fn fenced_rust_block_gets_language_class() {
        let md = "```rust\nfn main() {}\n```\n";
        let html = render(md, &light_standalone());
        assert!(
            html.contains("<code class=\"language-rust\">"),
            "no language-rust class: {html}"
        );
    }

    #[test]
    fn task_list_renders_checkboxes() {
        let md = "- [x] done\n- [ ] todo\n";
        let html = render(md, &light_standalone());
        assert!(html.contains("type=\"checkbox\""), "no checkbox: {html}");
        assert!(html.contains("checked"), "no checked box: {html}");
        assert!(html.contains("disabled"), "checkbox not disabled: {html}");
    }

    #[test]
    fn strikethrough_renders() {
        let html = render("~~gone~~\n", &light_standalone());
        assert!(html.contains("<del>gone</del>"), "no strikethrough: {html}");
    }

    #[test]
    fn autolink_renders() {
        let html = render("Visit https://example.com today\n", &light_standalone());
        assert!(
            html.contains("<a href=\"https://example.com\">"),
            "no autolink: {html}"
        );
    }

    #[test]
    fn toc_produces_anchor_links_matching_ids() {
        let opts = RenderOptions {
            toc: true,
            ..light_standalone()
        };
        let html = render("# Title\n\n## Alpha\n\n## Beta\n", &opts);
        // TOC nav present.
        assert!(html.contains("class=\"toc\""), "no toc nav: {html}");
        // Each TOC link target must have a matching heading id.
        assert!(
            html.contains("href=\"#alpha\""),
            "no toc link to alpha: {html}"
        );
        assert!(
            html.contains("href=\"#beta\""),
            "no toc link to beta: {html}"
        );
        assert!(html.contains("id=\"alpha\""), "no heading id alpha: {html}");
        assert!(html.contains("id=\"beta\""), "no heading id beta: {html}");
    }

    #[test]
    fn toc_links_match_deduped_ids() {
        let opts = RenderOptions {
            toc: true,
            ..light_standalone()
        };
        let html = render("## Dup\n\n## Dup\n", &opts);
        assert!(html.contains("href=\"#dup\""), "no link to dup: {html}");
        assert!(html.contains("href=\"#dup-1\""), "no link to dup-1: {html}");
    }

    #[test]
    fn standalone_has_doctype_and_style() {
        let html = render("# Hi\n", &light_standalone());
        assert!(html.contains("<!doctype html>"), "no doctype: {html}");
        assert!(html.contains("<style>"), "no style block: {html}");
        assert!(html.contains("<title>Hi</title>"), "no/wrong title: {html}");
        assert!(html.contains("</html>"), "no closing html: {html}");
    }

    #[test]
    fn no_style_omits_style_block() {
        let opts = RenderOptions {
            style: false,
            ..light_standalone()
        };
        let html = render("# Hi\n", &opts);
        assert!(html.contains("<!doctype html>"), "no doctype: {html}");
        assert!(
            !html.contains("<style>"),
            "style block present but shouldn't be: {html}"
        );
    }

    #[test]
    fn fragment_mode_has_no_wrapper() {
        let opts = RenderOptions {
            standalone: false,
            ..light_standalone()
        };
        let html = render("# Hi\n", &opts);
        assert!(
            !html.contains("<!doctype html>"),
            "fragment has doctype: {html}"
        );
        assert!(!html.contains("<style>"), "fragment has style: {html}");
        assert!(!html.contains("<html"), "fragment has <html>: {html}");
        assert!(
            html.contains("<h1") && html.contains("Hi"),
            "fragment missing content: {html}"
        );
    }

    #[test]
    fn title_inferred_from_first_h1() {
        let html = render("# My Great Doc\n\nbody\n", &light_standalone());
        assert!(
            html.contains("<title>My Great Doc</title>"),
            "title not inferred: {html}"
        );
    }

    #[test]
    fn explicit_title_overrides_h1() {
        let opts = RenderOptions {
            title: Some("Override".to_string()),
            ..light_standalone()
        };
        let html = render("# Heading\n", &opts);
        assert!(
            html.contains("<title>Override</title>"),
            "explicit title ignored: {html}"
        );
    }

    #[test]
    fn fallback_title_when_no_h1() {
        let opts = RenderOptions {
            fallback_title: "fallback-name".to_string(),
            ..light_standalone()
        };
        let html = render("just a paragraph\n", &opts);
        assert!(
            html.contains("<title>fallback-name</title>"),
            "fallback title missing: {html}"
        );
    }

    #[test]
    fn title_is_html_escaped() {
        let opts = RenderOptions {
            title: Some("A & B <x>".to_string()),
            ..light_standalone()
        };
        let html = render("body\n", &opts);
        assert!(
            html.contains("<title>A &amp; B &lt;x&gt;</title>"),
            "title not escaped: {html}"
        );
    }

    #[test]
    fn dark_theme_css_differs_from_light() {
        let light = theme_css(Theme::Light);
        let dark = theme_css(Theme::Dark);
        assert!(light.contains("#ffffff"), "light missing bg");
        assert!(dark.contains("#0d1117"), "dark missing bg");
        assert_ne!(light, dark);
    }

    #[test]
    fn auto_theme_has_media_query() {
        let css = theme_css(Theme::Auto);
        assert!(
            css.contains("prefers-color-scheme: dark"),
            "auto missing media query"
        );
    }

    #[test]
    fn theme_parse_roundtrip() {
        assert_eq!(Theme::parse("light"), Some(Theme::Light));
        assert_eq!(Theme::parse("DARK"), Some(Theme::Dark));
        assert_eq!(Theme::parse(" auto "), Some(Theme::Auto));
        assert_eq!(Theme::parse("nope"), None);
    }

    #[test]
    fn standalone_output_is_self_contained() {
        // No external asset references should appear in a standalone doc.
        let opts = RenderOptions {
            toc: true,
            ..light_standalone()
        };
        let html = render("# T\n\n## S\n\n![alt](pic.png)\n", &opts);
        assert!(!html.contains("<link "), "has <link> external ref: {html}");
        assert!(!html.contains("<script"), "has <script>: {html}");
        assert!(!html.contains("@import"), "css has @import: {html}");
        // The only http(s) tokens permitted come from user content, not our chrome;
        // our CSS/markup must not introduce a CDN/font URL.
        assert!(
            !html.contains("fonts.googleapis"),
            "pulls google fonts: {html}"
        );
        assert!(!html.contains("cdn."), "pulls a cdn: {html}");
    }
}
