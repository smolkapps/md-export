//! md-export — Markdown → standalone styled HTML.
//!
//! Thin CLI wrapper around the [`md_export`] library. Handles argument parsing,
//! input selection (file or stdin), title fallback derivation, and output
//! routing (file or stdout). All conversion logic lives in the library.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;

use md_export::{render, RenderOptions, Theme};

/// Convert GitHub-flavored Markdown into a self-contained, styled HTML file.
#[derive(Parser, Debug)]
#[command(
    name = "md-export",
    version,
    about = "Markdown → standalone styled HTML (embedded CSS, no external assets)",
    long_about = None,
)]
struct Cli {
    /// Input Markdown file. Omit or use "-" to read from stdin.
    input: Option<PathBuf>,

    /// Output HTML file. Omit or use "-" to write to stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Document title. Defaults to the first H1, then the input filename.
    #[arg(long)]
    title: Option<String>,

    /// Color theme for the embedded stylesheet.
    #[arg(long, value_parser = parse_theme, default_value = "auto")]
    theme: Theme,

    /// Generate a table of contents from the document headings.
    #[arg(long)]
    toc: bool,

    /// Emit a bare HTML fragment (no <html>/<head> wrapper, no CSS).
    #[arg(long, conflicts_with_all = ["standalone"])]
    no_style: bool,

    /// Emit a full standalone HTML document (default).
    #[arg(long)]
    standalone: bool,
}

/// clap value parser for `--theme`.
fn parse_theme(s: &str) -> Result<Theme, String> {
    Theme::parse(s).ok_or_else(|| format!("invalid theme '{s}' (expected light, dark, or auto)"))
}

/// Derive the fallback title from the input path's file stem, or "Document".
fn fallback_title_from(input: &Option<PathBuf>) -> String {
    match input {
        Some(p) if p.as_os_str() != "-" => p
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Document".to_string()),
        _ => "Document".to_string(),
    }
}

/// Read all Markdown input from the given path, or stdin if `None`/`"-"`.
fn read_input(input: &Option<PathBuf>) -> Result<String> {
    match input {
        Some(p) if p.as_os_str() != "-" => {
            fs::read_to_string(p).with_context(|| format!("reading input file {}", p.display()))
        }
        _ => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .context("reading Markdown from stdin")?;
            Ok(buf)
        }
    }
}

/// Write the rendered HTML to the given path, or stdout if `None`/`"-"`.
fn write_output(output: &Option<PathBuf>, html: &str) -> Result<()> {
    match output {
        Some(p) if p.as_os_str() != "-" => write_output_file(p, html)
            .with_context(|| format!("writing output file {}", p.display())),
        _ => {
            let stdout = io::stdout();
            let mut lock = stdout.lock();
            lock.write_all(html.as_bytes())
                .context("writing HTML to stdout")?;
            Ok(())
        }
    }
}

fn write_output_file(path: &Path, html: &str) -> io::Result<()> {
    fs::write(path, html.as_bytes())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let markdown = read_input(&cli.input)?;
    let fallback_title = fallback_title_from(&cli.input);

    let options = RenderOptions {
        title: cli.title.clone(),
        fallback_title,
        theme: cli.theme,
        toc: cli.toc,
        // --no-style implies a bare fragment; otherwise a full standalone doc.
        standalone: !cli.no_style,
        style: !cli.no_style,
    };

    let html = render(&markdown, &options);
    write_output(&cli.output, &html)?;

    Ok(())
}
