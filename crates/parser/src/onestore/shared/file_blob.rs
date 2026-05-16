use crate::errors::Result;
use crate::fs::{BytesSource, FileSource};
use bytes::Bytes;
use std::fmt::Debug;
use std::io::{Cursor, Read};
use std::sync::Arc;

/// A reference to a contiguous run of bytes within a parser-managed
/// [`FileSource`].
///
/// Carries an `Arc` to the source plus an `(offset, size)` slice. Cloning
/// is a refcount bump; the underlying bytes are never copied. Many
/// attachment references can be held simultaneously without proportional
/// memory cost.
///
/// [`FileBlob::read`] gives a [`Read`] over the blob without
/// materialising it; [`FileBlob::to_bytes`] returns an owned `Bytes`
/// (zero-copy when the backing is in-memory).
#[derive(Clone)]
pub struct FileBlob {
    source: Arc<dyn FileSource>,
    offset: u64,
    size: u64,
}

impl FileBlob {
    /// Construct a `FileBlob` over a slice of a [`FileSource`].
    pub fn from_source(source: Arc<dyn FileSource>, offset: u64, size: u64) -> Self {
        Self {
            source,
            offset,
            size,
        }
    }

    /// Construct a `FileBlob` from a stand-alone [`Bytes`] buffer (e.g.
    /// FSSHTTPB wire data that was decoded from the network rather than
    /// pulled from a file).
    pub fn from_bytes(bytes: Bytes) -> Self {
        let size = bytes.len() as u64;
        Self {
            source: Arc::new(BytesSource::new(bytes)),
            offset: 0,
            size,
        }
    }

    /// The size of the blob in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Materialise the blob as a [`Bytes`] buffer.
    ///
    /// Zero-copy when the backing is in-memory; one allocation otherwise.
    pub fn to_bytes(&self) -> Result<Bytes> {
        Ok(self.source.read_at(self.offset, self.size as usize)?)
    }

    /// A [`Read`] over the blob.
    ///
    /// For in-memory backings the read cursor is over a refcount-shared
    /// slice; for lazy-read backings the bytes are fetched on demand in
    /// chunks sized by the caller's read buffer.
    #[allow(dead_code)] // used by Image::read / EmbeddedFile::read in #15195
    pub fn read(&self) -> Box<dyn Read> {
        if let Some(buf) = self.source.as_bytes() {
            let start = self.offset as usize;
            let end = start + self.size as usize;
            return Box::new(Cursor::new(buf.slice(start..end)));
        }
        Box::new(FileBlobReader {
            source: self.source.clone(),
            offset: self.offset,
            end: self.offset + self.size,
        })
    }
}

impl Debug for FileBlob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(buf) = self.source.as_bytes() {
            let start = self.offset as usize;
            let end = start + self.size as usize;
            let slice = &buf[start..end];
            let first_32 = slice
                .iter()
                .take(32)
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            let last_32 = slice
                .iter()
                .rev()
                .take(32)
                .rev()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            write!(
                f,
                "FileBlob [ {} ... {}; {:?} KiB ]",
                first_32,
                last_32,
                slice.len() / 1024
            )
        } else {
            write!(f, "FileBlob [ <lazy>; {:?} KiB ]", self.size / 1024)
        }
    }
}

impl PartialEq for FileBlob {
    fn eq(&self, other: &Self) -> bool {
        // Compare by identity of the underlying source and slice range.
        // Cheap and good enough for the parser's needs (deduplicating
        // references to the same attachment).
        Arc::ptr_eq(&self.source, &other.source)
            && self.offset == other.offset
            && self.size == other.size
    }
}

impl Eq for FileBlob {}

impl Default for FileBlob {
    fn default() -> Self {
        Self::from_bytes(Bytes::new())
    }
}

impl From<Bytes> for FileBlob {
    fn from(value: Bytes) -> Self {
        Self::from_bytes(value)
    }
}

impl From<Vec<u8>> for FileBlob {
    fn from(value: Vec<u8>) -> Self {
        Self::from_bytes(Bytes::from(value))
    }
}

impl<'a> From<&'a [u8]> for FileBlob {
    fn from(value: &'a [u8]) -> Self {
        Self::from_bytes(Bytes::copy_from_slice(value))
    }
}

struct FileBlobReader {
    source: Arc<dyn FileSource>,
    offset: u64,
    end: u64,
}

impl Read for FileBlobReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self.end.saturating_sub(self.offset);
        if remaining == 0 {
            return Ok(0);
        }
        let n = (buf.len() as u64).min(remaining) as usize;
        let bytes = self.source.read_at(self.offset, n)?;
        buf[..n].copy_from_slice(&bytes);
        self.offset += n as u64;
        Ok(n)
    }
}
