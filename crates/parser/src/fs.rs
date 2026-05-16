//! File system abstraction used by the OneNote parser.

use bytes::Bytes;
#[cfg(feature = "native-fs")]
use std::fs;
#[cfg(feature = "native-fs")]
use std::fs::File;
#[cfg(feature = "native-fs")]
use std::io::BufReader;
use std::io::{Error, Read};
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
    /// without materialising the whole file in process memory (positional
    /// disk reads, WASM-side `Blob`-chunked reads) should override this.
    ///
    /// Callers must not mutate the underlying file while the returned
    /// [`FileSource`], or any attachment refcount-shared off it, is alive.
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
/// [`FileSystem::open_file`] when the consumer only provides
/// [`FileSystem::read_file`].
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

    /// Opens the file as an on-demand [`FileSource`].
    ///
    /// Each [`FileSource::read_at`] call issues a positional read against
    /// the open file handle (`pread` on Unix, overlapped `ReadFile` on
    /// Windows), so the file's bytes never need to live in process memory
    /// in their entirety — multi-GB notebooks parse with a working set
    /// proportional to active reads, not file size. The kernel's page
    /// cache fronts repeated reads cheaply.
    ///
    /// # File mutation during parsing
    ///
    /// Reads are positional and recoverable: a truncated or replaced file
    /// surfaces as an [`std::io::Error`] (or short read) from `read_at`,
    /// not a signal. The parse may still produce garbage or
    /// `MalformedData` if a concurrent writer mutates bytes the parser
    /// has yet to read — there's no way to make that consistent — but the
    /// process won't be aborted.
    fn open_file(&self, path: &Path) -> Result<Arc<dyn FileSource>, Error> {
        let file = File::open(path)?;
        let byte_length = file.metadata()?.len();
        let raw = FileBackedSource { file, byte_length };
        Ok(Arc::new(CachedFileSource::new(raw)))
    }
}

/// Page size for [`CachedFileSource`]'s LRU cache, in bytes.
///
/// 4 KiB matches the OS page granularity, so every miss against a
/// `NativeFs` source is exactly one kernel page fault into the OS page
/// cache. Larger pages amortise per-fetch setup but waste cache slots
/// on the small (often single-byte) reads that dominate the
/// structural-parse hot loop; smaller pages do the opposite.
/// Empirically a tie with 16 KiB on small fixtures and a clear win on
/// multi-MB files.
const PAGE_SIZE: u64 = 4096;

/// LRU page-cache capacity for [`CachedFileSource`].
///
/// Empirically chosen across fixtures from 22 KB to 154 MB. The benefit
/// plateaus past ~16 pages on small workloads; large-FSSHTTPB
/// workloads continue to benefit (marginally) up to 1024. 1024 pages ×
/// 4 KiB = 4 MiB of resident cache per source.
const CACHE_PAGES: std::num::NonZeroUsize = std::num::NonZeroUsize::new(1024).unwrap();

/// A [`FileSource`] backed by an open [`std::fs::File`] handle.
///
/// Issues positional reads via `FileExt::read_exact_at` (Unix) or
/// `FileExt::seek_read` (Windows). No caching of its own — wrap in
/// [`CachedFileSource`] for that, which is what [`NativeFs::open_file`]
/// does.
#[cfg(feature = "native-fs")]
struct FileBackedSource {
    file: File,
    byte_length: u64,
}

#[cfg(feature = "native-fs")]
impl FileSource for FileBackedSource {
    fn byte_length(&self) -> u64 {
        self.byte_length
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Bytes, Error> {
        let mut buf = vec![0u8; len];
        read_exact_at(&self.file, offset, &mut buf)?;
        Ok(Bytes::from(buf))
    }
    // `as_bytes` defaults to `None` — the file isn't resident.
}

/// A [`FileSource`] decorator that fronts another `FileSource` with a
/// page-aligned LRU cache.
///
/// Any [`FileSource`] implementation can wrap itself in
/// `CachedFileSource` to amortize fetch cost across consecutive
/// structural reads. The cache holds [`CACHE_PAGES`] pages of
/// [`PAGE_SIZE`] bytes each (≈ 4 MiB resident); a single-slot
/// last-access fast path sidesteps the LRU's hash lookup + reorder
/// when consecutive reads hit the same page (the dominant pattern in
/// `compact_u64` / `ObjectHeader::parse` peek sequences).
///
/// The decorator is most useful when the inner source's `read_at` is
/// expensive — a `pread` syscall (native), a JS/WASM callback, an HTTP
/// range request. For sources whose `as_bytes()` already returns the
/// full buffer in memory (e.g. [`BytesSource`]), the wrapper is a
/// pass-through: the parser's [`Reader`](crate::reader) takes its
/// own fast path off the cached `Bytes`.
pub struct CachedFileSource<S: FileSource> {
    inner: S,
    cache: std::sync::Mutex<lru::LruCache<u64, Bytes>>,
    /// Single-slot last-access cache, checked before the LRU.
    last: std::sync::Mutex<Option<(u64, Bytes)>>,
}

impl<S: FileSource> CachedFileSource<S> {
    /// Wrap `inner` with the default page cache configuration.
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            cache: std::sync::Mutex::new(lru::LruCache::new(CACHE_PAGES)),
            last: std::sync::Mutex::new(None),
        }
    }

    /// Fetch a single page (≤ [`PAGE_SIZE`] bytes near EOF), preferring
    /// the last-access cache, then the LRU, then the inner source.
    fn page(&self, page_start: u64) -> Result<Bytes, Error> {
        // Fast path: same page as the previous call.
        if let Some((p, b)) = &*self.last.lock().unwrap()
            && *p == page_start
        {
            return Ok(b.clone());
        }

        let page = {
            let mut cache = self.cache.lock().unwrap();
            if let Some(page) = cache.get(&page_start) {
                page.clone()
            } else {
                let len = ((self.inner.byte_length() - page_start).min(PAGE_SIZE)) as usize;
                let p = self.inner.read_at(page_start, len)?;
                cache.put(page_start, p.clone());
                p
            }
        };

        *self.last.lock().unwrap() = Some((page_start, page.clone()));
        Ok(page)
    }
}

impl<S: FileSource> FileSource for CachedFileSource<S> {
    fn byte_length(&self) -> u64 {
        self.inner.byte_length()
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Bytes, Error> {
        if len == 0 {
            return Ok(Bytes::new());
        }
        let end = offset + len as u64;
        let first_page = (offset / PAGE_SIZE) * PAGE_SIZE;
        let last_page = ((end - 1) / PAGE_SIZE) * PAGE_SIZE;

        if first_page == last_page {
            // Fast path: read fits within a single cached page.
            let page = self.page(first_page)?;
            let inner = (offset - first_page) as usize;
            return Ok(page.slice(inner..inner + len));
        }

        // Slow path: stitch across pages into a fresh buffer.
        let mut buf = vec![0u8; len];
        let mut written = 0;
        let mut page_start = first_page;
        while page_start <= last_page {
            let page = self.page(page_start)?;
            let copy_start = offset.max(page_start) - page_start;
            let copy_end = end.min(page_start + page.len() as u64) - page_start;
            let chunk = &page[copy_start as usize..copy_end as usize];
            buf[written..written + chunk.len()].copy_from_slice(chunk);
            written += chunk.len();
            page_start += PAGE_SIZE;
        }
        Ok(Bytes::from(buf))
    }

    /// Forwards the inner source's `as_bytes`. The `Reader`'s cached
    /// path then bypasses our `read_at` entirely when the inner source
    /// is fully in memory.
    fn as_bytes(&self) -> Option<Bytes> {
        self.inner.as_bytes()
    }
}

#[cfg(all(feature = "native-fs", unix))]
fn read_exact_at(file: &File, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(buf, offset)
}

#[cfg(all(feature = "native-fs", windows))]
fn read_exact_at(file: &File, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
    use std::os::windows::fs::FileExt;
    let mut total = 0;
    while total < buf.len() {
        let n = file.seek_read(&mut buf[total..], offset + total as u64)?;
        if n == 0 {
            return Err(Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "FileBackedSource::read_at hit end of file before satisfying request",
            ));
        }
        total += n;
    }
    Ok(())
}
