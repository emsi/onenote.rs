//! Support for OneNote `.onepkg` package files.
//!
//! `.onepkg` is the bundle format produced by OneNote's "Export notebook"
//! feature. A package is a Microsoft cabinet (CAB) archive that contains a
//! single `.onetoc2` table-of-contents plus the `.one` section files that make
//! up the notebook.
//!
//! This module decompresses a package into memory and exposes its contents
//! through the [`FileSystem`](crate::FileSystem) abstraction so the rest of
//! the parser can consume them with no special-casing. The entry point is
//! [`Parser::parse_package`](crate::Parser::parse_package).
//!
//! Available only with the `onepkg` feature enabled.

use crate::FileSystem;
use crate::errors::{ErrorKind, Result};
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Error as IoError, ErrorKind as IoErrorKind, Read};
use std::path::{Path, PathBuf};

/// In-memory snapshot of the files contained in a `.onepkg` cabinet.
pub(crate) struct PackageStore {
    files: HashMap<PathBuf, Vec<u8>>,
    dirs: HashSet<PathBuf>,
    toc_path: PathBuf,
}

impl PackageStore {
    /// Decompress a `.onepkg` cabinet from raw bytes.
    pub(crate) fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut cabinet = cab::Cabinet::new(Cursor::new(data)).map_err(|err| {
            ErrorKind::MalformedPackage {
                message: format!("failed to read cabinet: {err}").into(),
            }
        })?;

        let names: Vec<String> = cabinet
            .folder_entries()
            .flat_map(|folder| folder.file_entries())
            .map(|entry| entry.name().to_string())
            .collect();

        let mut files: HashMap<PathBuf, Vec<u8>> = HashMap::new();
        let mut dirs: HashSet<PathBuf> = HashSet::new();
        let mut toc_path: Option<PathBuf> = None;

        for name in names {
            let normalized = normalize_path(&name);
            if normalized.as_os_str().is_empty() {
                continue;
            }

            let mut reader = cabinet.read_file(&name).map_err(|err| {
                ErrorKind::MalformedPackage {
                    message: format!("failed to read cabinet entry {name}: {err}").into(),
                }
            })?;
            let mut buf = Vec::new();
            reader.read_to_end(&mut buf)?;

            for ancestor in normalized.ancestors().skip(1) {
                if ancestor.as_os_str().is_empty() {
                    continue;
                }
                dirs.insert(ancestor.to_path_buf());
            }

            if normalized.extension().is_some_and(|ext| ext == "onetoc2") {
                let depth = normalized.components().count();
                let prefer = toc_path
                    .as_ref()
                    .map(|existing| depth < existing.components().count())
                    .unwrap_or(true);
                if prefer {
                    toc_path = Some(normalized.clone());
                }
            }

            files.insert(normalized, buf);
        }

        let toc_path = toc_path.ok_or_else(|| ErrorKind::MalformedPackage {
            message: "package does not contain a .onetoc2 file".into(),
        })?;

        Ok(PackageStore {
            files,
            dirs,
            toc_path,
        })
    }

    pub(crate) fn toc_path(&self) -> &Path {
        &self.toc_path
    }
}

/// A read-only [`FileSystem`] view over an extracted [`PackageStore`].
#[derive(Clone, Copy)]
pub(crate) struct PackageFs<'a> {
    store: &'a PackageStore,
}

impl<'a> PackageFs<'a> {
    pub(crate) fn new(store: &'a PackageStore) -> Self {
        Self { store }
    }
}

impl FileSystem for PackageFs<'_> {
    fn is_directory(&self, path: &Path) -> std::io::Result<bool> {
        Ok(self.store.dirs.contains(path))
    }

    fn read_dir(&self, path: &Path) -> std::io::Result<Vec<PathBuf>> {
        let parent = if path.as_os_str().is_empty() {
            None
        } else {
            Some(path)
        };

        let matches = |child: &Path| -> bool {
            match (child.parent(), parent) {
                (Some(p), Some(target)) => p == target,
                (Some(p), None) => p.as_os_str().is_empty(),
                _ => false,
            }
        };

        let mut entries = Vec::new();
        for file_path in self.store.files.keys() {
            if matches(file_path) {
                entries.push(file_path.clone());
            }
        }
        for dir_path in &self.store.dirs {
            if matches(dir_path) {
                entries.push(dir_path.clone());
            }
        }
        Ok(entries)
    }

    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.store.files.get(path).cloned().ok_or_else(|| {
            IoError::new(
                IoErrorKind::NotFound,
                format!("not found in package: {}", path.display()),
            )
        })
    }

    fn write_file(&self, _path: &Path, _data: &[u8]) -> std::io::Result<()> {
        Err(IoError::new(
            IoErrorKind::Unsupported,
            "package file system is read-only",
        ))
    }

    fn make_dir(&self, _path: &Path) -> std::io::Result<()> {
        Err(IoError::new(
            IoErrorKind::Unsupported,
            "package file system is read-only",
        ))
    }

    fn exists(&self, path: &Path) -> std::io::Result<bool> {
        Ok(self.store.files.contains_key(path) || self.store.dirs.contains(path))
    }
}

/// Convert a CAB entry name (Windows-style, with `/` or `\` separators) into a
/// normalized relative [`PathBuf`]. `.` is dropped, `..` pops the previous
/// component, and any attempt to ascend above the root is clamped so nothing in
/// the cabinet can escape its own namespace.
fn normalize_path(name: &str) -> PathBuf {
    let mut parts: Vec<&str> = Vec::new();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_separators_and_traversal() {
        assert_eq!(normalize_path("a/b/c.one"), PathBuf::from("a/b/c.one"));
        assert_eq!(normalize_path("a\\b\\c.one"), PathBuf::from("a/b/c.one"));
        assert_eq!(normalize_path("./a/../b.one"), PathBuf::from("b.one"));
        assert_eq!(normalize_path(""), PathBuf::from(""));
    }
}
