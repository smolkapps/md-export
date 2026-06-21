//! Integration tests for the `md-export` binary, driving the real CLI via
//! `assert_cmd`: file→file, stdin→stdout, and the major flags.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::tempdir;

fn bin() -> Command {
    Command::cargo_bin("md-export").expect("binary builds")
}

const SAMPLE: &str = "# Sample Title\n\n\
Intro paragraph with ~~strike~~ and a link https://example.com\n\n\
## Features\n\n\
- [x] tables\n- [ ] more\n\n\
| col | val |\n|-----|-----|\n| a   | 1   |\n\n\
```rust\nfn main() { println!(\"hi\"); }\n```\n";

#[test]
fn stdin_to_stdout_standalone() {
    bin()
        .write_stdin(SAMPLE)
        .assert()
        .success()
        .stdout(contains("<!doctype html>"))
        .stdout(contains("<style>"))
        .stdout(contains("<title>Sample Title</title>"))
        .stdout(contains("id=\"features\""))
        .stdout(contains("<code class=\"language-rust\">"))
        .stdout(contains("<table>"))
        .stdout(contains("type=\"checkbox\""))
        .stdout(contains("<del>strike</del>"));
}

#[test]
fn file_to_file_writes_standalone_html() {
    let dir = tempdir().unwrap();
    let in_path = dir.path().join("in.md");
    let out_path = dir.path().join("out.html");
    fs::write(&in_path, SAMPLE).unwrap();

    bin()
        .arg(&in_path)
        .arg("-o")
        .arg(&out_path)
        .assert()
        .success();

    let html = fs::read_to_string(&out_path).unwrap();
    assert!(
        html.contains("<!doctype html>"),
        "no doctype in file output"
    );
    assert!(html.contains("<style>"), "no embedded style in file output");
    assert!(
        html.contains("Sample Title"),
        "content missing in file output"
    );
    // Self-contained: no external asset references.
    assert!(!html.contains("<link "), "external <link> present");
    assert!(!html.contains("<script"), "external <script> present");
}

#[test]
fn title_inferred_from_filename_when_no_h1() {
    let dir = tempdir().unwrap();
    let in_path = dir.path().join("my-notes.md");
    fs::write(&in_path, "just a paragraph, no heading\n").unwrap();

    bin()
        .arg(&in_path)
        .assert()
        .success()
        .stdout(contains("<title>my-notes</title>"));
}

#[test]
fn explicit_title_flag_wins() {
    bin()
        .arg("--title")
        .arg("Custom")
        .write_stdin("# Heading In Doc\n")
        .assert()
        .success()
        .stdout(contains("<title>Custom</title>"));
}

#[test]
fn toc_flag_emits_nav_with_matching_anchors() {
    bin()
        .arg("--toc")
        .write_stdin("# Top\n\n## One\n\n## Two\n")
        .assert()
        .success()
        .stdout(contains("class=\"toc\""))
        .stdout(contains("href=\"#one\""))
        .stdout(contains("href=\"#two\""))
        .stdout(contains("id=\"one\""))
        .stdout(contains("id=\"two\""));
}

#[test]
fn no_style_emits_bare_fragment() {
    bin()
        .arg("--no-style")
        .write_stdin("# Hi\n\nbody\n")
        .assert()
        .success()
        .stdout(contains("<h1").and(contains("Hi")))
        .stdout(contains("<!doctype html>").not())
        .stdout(contains("<style>").not())
        .stdout(contains("<html").not());
}

#[test]
fn dark_theme_embeds_dark_palette() {
    bin()
        .arg("--theme")
        .arg("dark")
        .write_stdin("# Hi\n")
        .assert()
        .success()
        .stdout(contains("#0d1117"));
}

#[test]
fn light_theme_embeds_light_palette() {
    bin()
        .arg("--theme")
        .arg("light")
        .write_stdin("# Hi\n")
        .assert()
        .success()
        .stdout(contains("#ffffff"));
}

#[test]
fn invalid_theme_is_rejected() {
    bin()
        .arg("--theme")
        .arg("rainbow")
        .write_stdin("# Hi\n")
        .assert()
        .failure()
        .stderr(contains("invalid theme"));
}

#[test]
fn missing_input_file_errors() {
    bin()
        .arg("/no/such/file/exists.md")
        .assert()
        .failure()
        .stderr(contains("reading input file"));
}
