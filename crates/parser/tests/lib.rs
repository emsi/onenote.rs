use insta::assert_debug_snapshot;
use onenote_parser::Parser;
use std::path::{Path, PathBuf};

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

#[test]
fn test_onenote_joplin_examples() {
    let parser = Parser::new();
    fn assert_section_snapshot(parser: &Parser, path: &str) {
        assert_debug_snapshot!(parser.parse_section(Path::new(path)).unwrap());
    }

    fn assert_notebook_snapshot(parser: &Parser, path: &str) {
        assert_debug_snapshot!(parser.parse_notebook(Path::new(path)).unwrap());
    }

    assert_section_snapshot(&parser, "tests/samples/joplin/checkboxes_and_unicode.one");
    assert_section_snapshot(&parser, "tests/samples/joplin/Math.one");
    assert_section_snapshot(&parser, "tests/samples/joplin/onenote_desktop.one");
    assert_section_snapshot(&parser, "tests/samples/joplin/aaa/Quick Notes.one");
    assert_section_snapshot(&parser, "tests/samples/joplin/audio-test/Quick Notes.one");
    assert_notebook_snapshot(
        &parser,
        "tests/samples/joplin/default-edited/Открыть записную книжку.onetoc2",
    );
    assert_section_snapshot(
        &parser,
        "tests/samples/joplin/hyperlink_is_broken/Quick Notes.one",
    );
    assert_notebook_snapshot(
        &parser,
        "tests/samples/joplin/Notebook created on OneNote App/Abrir Bloco de Anotações.onetoc2",
    );
    assert_section_snapshot(
        &parser,
        "tests/samples/joplin/Notebook with subsections and subpages/Section 1.one",
    );
    assert_section_snapshot(
        &parser,
        "tests/samples/joplin/notebook_with_chinese_char_on_link/Quick Notes.one",
    );
    assert_section_snapshot(
        &parser,
        "tests/samples/joplin/Simple notebook/Quick Notes.one",
    );
    assert_section_snapshot(&parser, "tests/samples/joplin/new_section.one");
}
