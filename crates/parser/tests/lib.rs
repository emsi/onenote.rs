use insta::assert_debug_snapshot;
use onenote_parser::Parser;
use std::path::{Path, PathBuf};
use yare::parameterized;

#[test]
fn test_parse_section() {
    let path = PathBuf::from("tests/samples/New Section 1.one");

    let parser = Parser::new();
    assert_debug_snapshot!(parser.parse_section(&path).unwrap());
}

#[test]
fn test_parse_notebook() {
    let path = PathBuf::from("tests/samples/Open Notebook.onetoc2");

    let parser = Parser::new();
    assert_debug_snapshot!(parser.parse_notebook(&path).unwrap());
}

#[test]
fn test_parse_notebook_new() {
    let path = PathBuf::from("tests/samples/non-legacy/Open Notebook.onetoc2");

    let parser = Parser::new();
    assert_debug_snapshot!(parser.parse_notebook(&path).unwrap());
}

#[test]
fn test_parse_section_with_image_missing_last_modified() {
    let path = PathBuf::from("tests/samples/Schnelle Notizen.one");

    let parser = Parser::new();
    assert_debug_snapshot!(parser.parse_section(&path).unwrap());
}

#[test]
fn test_readme_example_parse_notebook() {
    let parser = Parser::new();
    let notebook = parser
        .parse_notebook(Path::new("tests/samples/Open Notebook.onetoc2"))
        .unwrap();

    assert!(!notebook.entries().is_empty());
}

#[test]
fn test_onenote_2016_parse_notebook() {
    let path = Path::new("tests/samples/onenote-2016/OneWithFileData.one");
    let parser = Parser::new();

    assert_debug_snapshot!(parser.parse_section(path).unwrap())
}

#[parameterized(
    checkboxes_and_unicode = { "checkboxes_and_unicode.one" },
    math = { "Math.one" },
    onenote_desktop = { "onenote_desktop.one" },
    quick_notes = { "aaa/Quick Notes.one" },
    audio_test = { "audio-test/Quick Notes.one" },
    hyperlink_is_broken = { "hyperlink_is_broken/Quick Notes.one" },
    subsections_subpages = { "Notebook with subsections and subpages/Section 1.one" },
    notebook_with_chinese_char_on_link = { "notebook_with_chinese_char_on_link/Quick Notes.one" },
    simple_notebook = { "Simple notebook/Quick Notes.one" },
    new_section = { "new_section.one" },
    scaled_ink = { "scaled_ink.one" },
    desktop_missing_ink = { "desktop_missing_ink.one" }
)]
fn test_onenote_joplin_examples_section(path: &str) {
    let parser = Parser::new();
    assert_debug_snapshot!(
        parser
            .parse_section(Path::new(&*format!("tests/samples/joplin/{path}")))
            .unwrap()
    );
}

#[parameterized(
    default_edited = { "default-edited/Открыть записную книжку.onetoc2" },
    onenote_app = { "Notebook created on OneNote App/Abrir Bloco de Anotações.onetoc2" }
)]
fn test_onenote_joplin_examples_notebook(path: &str) {
    let parser = Parser::new();
    assert_debug_snapshot!(
        parser
            .parse_notebook(Path::new(&*format!("tests/samples/joplin/{path}")))
            .unwrap()
    );
}
