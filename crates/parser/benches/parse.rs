//! Performance baseline for the parser.
//!
//! Two groups of benchmarks:
//!
//! - `parse_section_buffer/...` — runs `Parser::parse_section_buffer` against
//!   pre-loaded byte slices. Isolates parsing cost from I/O so regressions
//!   in the format machinery (file-node walk, property-set decoding, etc.)
//!   stand out cleanly.
//! - `parse_section/...` — runs `Parser::parse_section` against the same
//!   fixtures on disk. Measures the full pread-backed `NativeFs` path,
//!   so regressions in `read_at` chunking or `FileSource` overhead show up.
//! - `parse_notebook` — exercises the multi-section toc walk.
//!
//! All fixtures live under `crates/parser/tests/samples/`. The corpus
//! covers a tiny FSSHTTPB section (`Math.one`), a tiny desktop-format
//! section, a medium FSSHTTPB section, and a notebook of small sections.
//! Pick the bench that matches what you're trying to measure rather than
//! reading the aggregate as one number.

use criterion::{Criterion, criterion_group, criterion_main};
use onenote_parser::Parser;
use std::hint::black_box;
use std::path::{Path, PathBuf};

const SECTION_FIXTURES: &[(&str, &str)] = &[
    ("math_tiny_fsshttpb", "tests/samples/joplin/Math.one"),
    ("onenote_desktop", "tests/samples/joplin/onenote_desktop.one"),
    ("new_section", "tests/samples/New Section 1.one"),
    ("schnelle_notizen", "tests/samples/Schnelle Notizen.one"),
];

const NOTEBOOK_FIXTURE: &str = "tests/samples/Open Notebook.onetoc2";

fn parse_section_buffer(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_section_buffer");
    for (name, path) in SECTION_FIXTURES {
        let data = std::fs::read(path).expect("fixture not found");
        let path = PathBuf::from(path);
        group.bench_function(*name, |b| {
            b.iter(|| {
                let parser = Parser::new();
                let _ = black_box(parser.parse_section_buffer(black_box(&data), black_box(&path)));
            });
        });
    }
    group.finish();
}

fn parse_section_from_disk(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_section");
    for (name, path) in SECTION_FIXTURES {
        let path = Path::new(path);
        group.bench_function(*name, |b| {
            b.iter(|| {
                let parser = Parser::new();
                let _ = black_box(parser.parse_section(black_box(path)));
            });
        });
    }
    group.finish();
}

fn parse_notebook(c: &mut Criterion) {
    let path = Path::new(NOTEBOOK_FIXTURE);
    c.bench_function("parse_notebook/open_notebook", |b| {
        b.iter(|| {
            let parser = Parser::new();
            let _ = black_box(parser.parse_notebook(black_box(path)));
        });
    });
}

criterion_group!(
    benches,
    parse_section_buffer,
    parse_section_from_disk,
    parse_notebook
);
criterion_main!(benches);
