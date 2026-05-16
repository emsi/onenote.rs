//! File system abstraction used by the OneNote parser.

use bytes::Bytes;
#[cfg(feature = "native-fs")]
use std::fs;
#[cfg(feature = "native-fs")]
use std::fs::File;
use std::io::{Error, Read};
#[cfg(feature = "native-fs")]
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Abstraction over file system operations.
///
/// This trait provides an interface for file system operations used by the OneNote parser.
/// It enables dependency injection for testing and alternative file system implementations.
///
/// All implementations must be thread-safe (`Send + Sync`) as the parser may be used
/// across threads.
///
/// Implementations must also be `Copy` so callers can pass the filesystem handle
/// by value.
pub trait FileSystem: Send + Sync + Copy {
    /// Checks if the given path points to a directory.
    ///
    /// Mirrors the semantics of [`std::path::Path::is_dir`]: a missing path is
    /// not an error. Use [`FileSystem::exists`] if the existence check itself
    /// matters.
    ///
    /// # Arguments
    /// * `path` - The path to check
    ///
    /// # Returns
    /// * `Ok(true)` if the path exists and is a directory
    /// * `Ok(false)` if the path does not exist, or exists but is not a directory
    /// * `Err` only on I/O errors that aren't "not found" (e.g. permission denied)
    ///
    /// # Usage
    /// Used by the parser to distinguish between section files (.one) and section groups
    /// (directories containing .onetoc2 files).
    fn is_directory(&self, path: &Path) -> Result<bool, Error>;

    /// Lists all entries in a directory.
    ///
    /// # Arguments
    /// * `path` - The directory path to read
    ///
    /// # Returns
    /// A vector of paths for all entries in the directory, or an error if the
    /// directory cannot be read.
    ///
    /// # Usage
    /// Used to enumerate section files and subdirectories when parsing section groups.
    fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, Error>;

    /// Reads the entire contents of a file into memory.
    ///
    /// # Arguments
    /// * `path` - The file path to read
    ///
    /// # Returns
    /// The complete file contents as a byte vector, or an error if the file
    /// cannot be read.
    ///
    /// # Usage
    /// Used to load OneNote files (.one, .onetoc2) for parsing. Files are read
    /// entirely into memory as the parser needs random access to the data.
    fn read_file(&self, path: &Path) -> Result<Vec<u8>, Error>;

    /// Writes data to a file, replacing any existing content.
    ///
    /// # Arguments
    /// * `path` - The file path to write to
    /// * `data` - The data to write
    ///
    /// # Returns
    /// Ok(()) on success, or an error if the file cannot be written.
    ///
    /// # Usage
    /// May be used for extracting embedded content or creating output files.
    fn write_file(&self, path: &Path, data: &[u8]) -> Result<(), Error>;

    /// Stream the contents of `reader` to a file, replacing any existing
    /// content.
    ///
    /// Intended for writing large attachment payloads out without
    /// materialising them in memory. Implementations should write in
    /// fixed-size chunks (`std::io::copy` on native, chunked
    /// `appendFileSync`-style writes on WASM) rather than buffering the
    /// whole stream — for that you can call [`FileSystem::write_file`]
    /// directly.
    ///
    /// # Errors
    ///
    /// On failure mid-stream the destination file may be left in a
    /// partially-written state.
    fn stream_to_file(&self, path: &Path, reader: &mut dyn Read) -> Result<(), Error>;

    /// Creates a directory, including any missing parent directories.
    ///
    /// # Arguments
    /// * `path` - The directory path to create
    ///
    /// # Returns
    /// `Ok(())` if the directory was created or already exists as a directory,
    /// or an error if the directory cannot be created.
    ///
    /// # Note
    /// This method is idempotent when `path` already exists as a directory.
    /// If `path` exists but is not a directory (e.g. a regular file or a
    /// symlink to one), implementations must return an error rather than
    /// silently succeeding.
    fn make_dir(&self, path: &Path) -> Result<(), Error>;

    /// Checks if a path exists in the file system.
    ///
    /// # Arguments
    /// * `path` - The path to check
    ///
    /// # Returns
    /// * `Ok(true)` if the path exists (file or directory)
    /// * `Ok(false)` if the path does not exist
    /// * `Err` if the existence check fails due to permissions or other I/O errors
    ///
    /// # Usage
    /// Used to filter out non-existent section entries and verify paths before
    /// attempting to parse them.
    fn exists(&self, path: &Path) -> Result<bool, Error>;

    /// Opens a file as a [`FileSource`] for parsing.
    ///
    /// The default implementation reads the entire file via
    /// [`FileSystem::read_file`] and wraps the resulting buffer in a
    /// [`BytesSource`] — eager, simple, and suitable for any backend that
    /// can hand back a `Vec<u8>`. Implementations that can serve bytes
    /// without materialising the whole file in process memory (native
    /// memory-mapping; WASM-side `Blob`-chunked reads) should override
    /// this.
    ///
    /// # I/O contract
    ///
    /// Callers MUST NOT mutate the underlying file while the returned
    /// `FileSource` (or anything refcount-shared off it — attachments,
    /// images, embedded files) is alive. With the default [`NativeFs`]
    /// backend, files are memory-mapped; truncating or replacing the file
    /// during parsing causes SIGBUS on access.
    fn open_file(&self, path: &Path) -> Result<Arc<dyn FileSource>, Error> {
        let bytes = Bytes::from(self.read_file(path)?);
        Ok(Arc::new(BytesSource::new(bytes)))
    }
}

/// A random-access byte source backing a parse.
///
/// The parser reads notebook data through this trait. Reads take an
/// absolute `offset` and may happen in any order; the trait holds no
/// position of its own.
///
/// If you can hand the parser an in-memory `Bytes`, you almost certainly
/// don't need to implement this trait — use [`BytesSource`] (or the
/// default [`FileSystem::open_file`] impl). Implement `FileSource`
/// directly when you want to serve bytes without materialising the whole
/// file in memory (e.g. a WASM `Blob` you read chunks from on demand).
///
/// Implementations are `Send + Sync` and the returned [`Bytes`] are too.
pub trait FileSource: Send + Sync {
    /// Total length of the underlying source in bytes.
    fn byte_length(&self) -> u64;

    /// Read `len` bytes starting at absolute `offset`.
    ///
    /// For in-memory backings, return a refcount-shared slice
    /// (`Bytes::slice`); for backings that fetch on demand, allocate.
    fn read_at(&self, offset: u64, len: usize) -> Result<Bytes, Error>;

    /// Return the entire source as a refcount-shared buffer when it is
    /// fully held in memory.
    ///
    /// The parser uses this as a fast path for hot per-byte indexing.
    /// Return `None` if the bytes aren't all resident; the parser will
    /// fall back to [`read_at`](FileSource::read_at).
    fn as_bytes(&self) -> Option<Bytes> {
        None
    }
}

/// A [`FileSource`] backed by an in-memory [`Bytes`] buffer.
///
/// `read_at` returns a refcount-shared slice into the buffer (zero-copy);
/// `as_bytes` returns the full buffer. Used by the default
/// [`FileSystem::open_file`] and as the wrapper around `Bytes::from_owner(mmap)`
/// on native.
pub struct BytesSource {
    bytes: Bytes,
}

impl BytesSource {
    /// Wrap a [`Bytes`] buffer as a [`FileSource`].
    pub fn new(bytes: Bytes) -> Self {
        Self { bytes }
    }
}

impl FileSource for BytesSource {
    fn byte_length(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Bytes, Error> {
        let start = offset as usize;
        let end = start
            .checked_add(len)
            .filter(|&e| e <= self.bytes.len())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!(
                        "BytesSource::read_at out of bounds: offset={offset} len={len} byte_length={}",
                        self.bytes.len()
                    ),
                )
            })?;
        Ok(self.bytes.slice(start..end))
    }

    fn as_bytes(&self) -> Option<Bytes> {
        Some(self.bytes.clone())
    }
}

/// Native file system implementation using standard library I/O operations.
///
/// This is the default implementation of [`FileSystem`] that performs actual
/// file system operations using Rust's standard library.
#[cfg(feature = "native-fs")]
#[derive(Clone, Copy)]
pub struct NativeFs {}

#[cfg(feature = "native-fs")]
impl FileSystem for NativeFs {
    fn is_directory(&self, path: &Path) -> Result<bool, Error> {
        match fs::metadata(path) {
            Ok(meta) => Ok(meta.is_dir()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, Error> {
        let mut result = Vec::new();

        for item in fs::read_dir(path)? {
            let item = item?.path();
            result.push(item)
        }

        Ok(result)
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>, Error> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        let mut data = Vec::with_capacity(size as usize);

        let mut buf = BufReader::new(file);
        buf.read_to_end(&mut data)?;

        Ok(data)
    }

    fn write_file(&self, path: &Path, data: &[u8]) -> Result<(), Error> {
        fs::write(path, data)
    }

    /// Streams `reader` directly into the destination file via
    /// `std::io::copy`, so large payloads never need to be materialised
    /// in process memory.
    fn stream_to_file(&self, path: &Path, reader: &mut dyn Read) -> Result<(), Error> {
        let mut file = File::create(path)?;
        std::io::copy(reader, &mut file)?;
        Ok(())
    }

    fn make_dir(&self, path: &Path) -> Result<(), Error> {
        let result = fs::create_dir_all(path);

        // Don't fail if it already existed as a directory; surface other errors
        // (e.g. path exists as a file).
        if self.is_directory(path)? {
            Ok(())
        } else {
            result
        }
    }

    fn exists(&self, path: &Path) -> Result<bool, Error> {
        fs::exists(path)
    }

    /// Opens the file using a memory-mapped backing.
    ///
    /// The mapping is exposed through a [`BytesSource`] over
    /// `Bytes::from_owner(mmap)`. Reads of the file's contents go through
    /// the kernel's page cache and don't require resident allocation, so
    /// `.one` files larger than the process's RAM still parse correctly.
    ///
    /// # Safety
    ///
    /// Memory-mapped I/O exposes the file's contents directly. If the
    /// underlying file is truncated, replaced, or resized while the parser
    /// holds the mapping, accessing the mapped region produces SIGBUS.
    /// Callers MUST NOT mutate the file while a parse is in progress.
    fn open_file(&self, path: &Path) -> Result<Arc<dyn FileSource>, Error> {
        let file = File::open(path)?;
        // SAFETY: see method docs — caller must not mutate the file during parse.
        let mmap = unsafe { memmap2::Mmap::map(&file)? };
        Ok(Arc::new(BytesSource::new(Bytes::from_owner(mmap))))
    }
}
