//! The local filesystem: Laravel's `Illuminate\Filesystem\Filesystem`, and
//! the `File` facade in front of it.
//!
//! Every method that touches file *contents* or changes the filesystem is
//! `async` and has a blocking `_sync` twin for code that isn't running on
//! the runtime (configuration loading, build scripts, tests). Quick checks
//! like [`Filesystem::exists`] and [`Filesystem::extension`] are plain
//! synchronous functions, exactly as you'd use them in an `if`.

use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};

use illuminate_container::try_app;
use illuminate_support::error::{bail, error};
use illuminate_support::{Collection, Result, Str, Value, collect};

use crate::exceptions::FileNotFoundException;
use crate::path::{
    IntoPaths, detect_mime_type, expand_braces, extension_for_mime, glob_match, has_wildcards,
    pathinfo,
};

/// Version-control directories that listings always skip.
const VCS_DIRECTORIES: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    ".bzr",
    "_darcs",
    "CVS",
    ".arch-params",
    ".monotone",
    "_MTN",
];

/// Run a blocking filesystem operation off the async runtime.
pub(crate) async fn blocking<T, F>(operation: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_err() {
        return operation();
    }
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| error!("The filesystem task failed: {error}"))?
}

/// Local filesystem utilities.
///
/// ```
/// use illuminate_filesystem::Filesystem;
///
/// let files = Filesystem::new();
/// let directory = tempfile::tempdir().unwrap();
/// let path = directory.path().join("hello.txt");
///
/// files.put_sync(&path, "Hello World").unwrap();
///
/// assert!(files.exists(&path));
/// assert_eq!(files.get_sync(&path).unwrap(), "Hello World");
/// assert_eq!(files.extension(&path), "txt");
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filesystem;

impl Filesystem {
    /// Create a new filesystem instance.
    pub fn new() -> Self {
        Self
    }

    // ------------------------------------------------------------------
    // Existence & type checks
    // ------------------------------------------------------------------

    /// Determine if a file or directory exists.
    pub fn exists(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref().exists()
    }

    /// Determine if a file or directory is missing.
    pub fn missing(&self, path: impl AsRef<Path>) -> bool {
        !self.exists(path)
    }

    /// Determine if the given path is a file.
    pub fn is_file(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref().is_file()
    }

    /// Determine if the given path is a directory.
    pub fn is_directory(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref().is_dir()
    }

    /// Determine if the given directory is empty (dot files count).
    pub fn is_empty_directory(&self, directory: impl AsRef<Path>) -> bool {
        Self::is_empty_directory_filtered(directory.as_ref(), false)
    }

    /// Determine if the given directory is empty, ignoring dot files.
    pub fn is_empty_directory_ignoring_dot_files(&self, directory: impl AsRef<Path>) -> bool {
        Self::is_empty_directory_filtered(directory.as_ref(), true)
    }

    fn is_empty_directory_filtered(directory: &Path, ignore_dot_files: bool) -> bool {
        match fs::read_dir(directory) {
            Ok(entries) => !entries.flatten().any(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                !(VCS_DIRECTORIES.contains(&name.as_str())
                    || (ignore_dot_files && name.starts_with('.')))
            }),
            Err(_) => true,
        }
    }

    /// Determine if the given path is readable.
    pub fn is_readable(&self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        if path.is_dir() {
            fs::read_dir(path).is_ok()
        } else {
            fs::File::open(path).is_ok()
        }
    }

    /// Determine if the given path is writable.
    pub fn is_writable(&self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        if path.is_dir() {
            let probe = path.join(format!(".illuminate-writable-{}", Str::random(16)));
            let writable = fs::File::create(&probe).is_ok();
            let _ = fs::remove_file(&probe);
            writable
        } else {
            fs::OpenOptions::new().append(true).open(path).is_ok()
        }
    }

    /// Get the file type of a given path: `"file"`, `"dir"`, or `"link"`.
    pub fn type_(&self, path: impl AsRef<Path>) -> Result<String> {
        let metadata = fs::symlink_metadata(path.as_ref())?;
        let file_type = metadata.file_type();
        Ok(if file_type.is_symlink() {
            "link"
        } else if file_type.is_dir() {
            "dir"
        } else if file_type.is_file() {
            "file"
        } else {
            "unknown"
        }
        .to_string())
    }

    // ------------------------------------------------------------------
    // Path information
    // ------------------------------------------------------------------

    /// Extract the file name (without extension) from a path.
    pub fn name(&self, path: impl AsRef<Path>) -> String {
        pathinfo(&path.as_ref().to_string_lossy()).filename
    }

    /// Extract the trailing name component from a path.
    pub fn basename(&self, path: impl AsRef<Path>) -> String {
        pathinfo(&path.as_ref().to_string_lossy()).basename
    }

    /// Extract the parent directory from a path.
    pub fn dirname(&self, path: impl AsRef<Path>) -> String {
        pathinfo(&path.as_ref().to_string_lossy()).dirname
    }

    /// Extract the file extension from a path.
    pub fn extension(&self, path: impl AsRef<Path>) -> String {
        pathinfo(&path.as_ref().to_string_lossy()).extension
    }

    /// Guess the file extension from the MIME type of a given file.
    pub fn guess_extension(&self, path: impl AsRef<Path>) -> Option<String> {
        self.mime_type(path)
            .and_then(|mime| extension_for_mime(&mime))
    }

    /// Get the MIME type of a given file, sniffing its contents when needed.
    pub fn mime_type(&self, path: impl AsRef<Path>) -> Option<String> {
        let path = path.as_ref();
        if !path.is_file() {
            return None;
        }
        let mut sample = vec![0u8; 512];
        let read = fs::File::open(path)
            .and_then(|mut file| file.read(&mut sample))
            .ok()?;
        sample.truncate(read);
        detect_mime_type(&path.to_string_lossy(), Some(&sample))
    }

    /// Get the file size of a given file, in bytes.
    pub fn size(&self, path: impl AsRef<Path>) -> Result<u64> {
        Ok(fs::metadata(path.as_ref())?.len())
    }

    /// Get the file's last modification time, as a UNIX timestamp.
    pub fn last_modified(&self, path: impl AsRef<Path>) -> Result<i64> {
        let modified = fs::metadata(path.as_ref())?.modified()?;
        Ok(match modified.duration_since(std::time::UNIX_EPOCH) {
            Ok(duration) => duration.as_secs() as i64,
            Err(error) => -(error.duration().as_secs() as i64),
        })
    }

    /// Get the permissions of a file or directory, like `"0644"`.
    pub fn permissions(&self, path: impl AsRef<Path>) -> Result<String> {
        let metadata = fs::metadata(path.as_ref())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            Ok(format!("{:04o}", metadata.permissions().mode() & 0o7777))
        }
        #[cfg(not(unix))]
        {
            Ok(if metadata.permissions().readonly() {
                "0444"
            } else {
                "0666"
            }
            .to_string())
        }
    }

    /// Set the permissions (mode) of a file or directory.
    pub fn chmod(&self, path: impl AsRef<Path>, mode: u32) -> Result<()> {
        set_mode(path.as_ref(), mode)
    }

    // ------------------------------------------------------------------
    // Listing
    // ------------------------------------------------------------------

    /// Find path names matching a glob pattern (`*`, `?`, `[a-z]`, `{a,b}`).
    ///
    /// ```
    /// use illuminate_filesystem::Filesystem;
    ///
    /// let files = Filesystem::new();
    /// let directory = tempfile::tempdir().unwrap();
    /// files.put_sync(directory.path().join("a.txt"), "").unwrap();
    /// files.put_sync(directory.path().join("b.md"), "").unwrap();
    ///
    /// let found = files.glob(format!("{}/*.{{txt,md}}", directory.path().display()));
    /// assert_eq!(found.len(), 2);
    /// ```
    pub fn glob(&self, pattern: impl AsRef<str>) -> Vec<PathBuf> {
        let mut results = BTreeSet::new();
        for pattern in expand_braces(pattern.as_ref()) {
            let absolute = pattern.starts_with('/');
            let segments: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
            let start = if absolute {
                PathBuf::from("/")
            } else {
                PathBuf::new()
            };
            glob_walk(start, &segments, &mut results);
        }
        results.into_iter().collect()
    }

    /// Get all of the files within a directory (not recursive), sorted by name.
    /// Dot files are skipped.
    pub fn files(&self, directory: impl AsRef<Path>) -> Vec<PathBuf> {
        list(directory.as_ref(), false, false, true)
    }

    /// Get all of the files within a directory, including dot files.
    pub fn files_with_hidden(&self, directory: impl AsRef<Path>) -> Vec<PathBuf> {
        list(directory.as_ref(), false, true, true)
    }

    /// Get all of the files from the given directory (recursive), sorted by path.
    pub fn all_files(&self, directory: impl AsRef<Path>) -> Vec<PathBuf> {
        list(directory.as_ref(), true, false, true)
    }

    /// Get all of the files from the given directory (recursive), including dot files.
    pub fn all_files_with_hidden(&self, directory: impl AsRef<Path>) -> Vec<PathBuf> {
        list(directory.as_ref(), true, true, true)
    }

    /// Get all of the directories within a given directory (not recursive).
    pub fn directories(&self, directory: impl AsRef<Path>) -> Vec<PathBuf> {
        list(directory.as_ref(), false, false, false)
    }

    /// Get all of the directories within a given directory, recursively.
    pub fn all_directories(&self, directory: impl AsRef<Path>) -> Vec<PathBuf> {
        list(directory.as_ref(), true, false, false)
    }

    // ------------------------------------------------------------------
    // Reading
    // ------------------------------------------------------------------

    /// Get the contents of a file as a string.
    ///
    /// Returns a [`FileNotFoundException`] when the file doesn't exist.
    pub async fn get(&self, path: impl AsRef<Path>) -> Result<String> {
        let path = path.as_ref().to_path_buf();
        blocking(move || Filesystem.get_sync(path)).await
    }

    /// Blocking variant of [`Filesystem::get`].
    pub fn get_sync(&self, path: impl AsRef<Path>) -> Result<String> {
        let path = path.as_ref();
        let bytes = self.get_bytes_sync(path)?;
        String::from_utf8(bytes).map_err(|_| {
            error!(
                "The file at path {} is not valid UTF-8; read it with `get_bytes` instead.",
                path.display()
            )
        })
    }

    /// Get the raw contents of a file.
    pub async fn get_bytes(&self, path: impl AsRef<Path>) -> Result<Vec<u8>> {
        let path = path.as_ref().to_path_buf();
        blocking(move || Filesystem.get_bytes_sync(path)).await
    }

    /// Blocking variant of [`Filesystem::get_bytes`].
    pub fn get_bytes_sync(&self, path: impl AsRef<Path>) -> Result<Vec<u8>> {
        let path = path.as_ref();
        if !path.is_file() {
            return Err(FileNotFoundException::new(path.display().to_string()).into());
        }
        Ok(fs::read(path)?)
    }

    /// Get the contents of a file while holding a shared lock on it.
    pub async fn shared_get(&self, path: impl AsRef<Path>) -> Result<String> {
        let path = path.as_ref().to_path_buf();
        blocking(move || Filesystem.shared_get_sync(path)).await
    }

    /// Blocking variant of [`Filesystem::shared_get`].
    pub fn shared_get_sync(&self, path: impl AsRef<Path>) -> Result<String> {
        let path = path.as_ref();
        let mut file = fs::File::open(path)
            .map_err(|_| FileNotFoundException::new(path.display().to_string()))?;
        file.lock_shared()?;
        let mut contents = String::new();
        let result = file.read_to_string(&mut contents);
        file.unlock()?;
        result?;
        Ok(contents)
    }

    /// Get the decoded JSON contents of a file.
    pub async fn json(&self, path: impl AsRef<Path>) -> Result<Value> {
        let path = path.as_ref().to_path_buf();
        blocking(move || Filesystem.json_sync(path)).await
    }

    /// Blocking variant of [`Filesystem::json`].
    pub fn json_sync(&self, path: impl AsRef<Path>) -> Result<Value> {
        Ok(serde_json::from_str(&self.get_sync(path)?)?)
    }

    /// Get the lines of a file (line endings removed).
    pub async fn lines(&self, path: impl AsRef<Path>) -> Result<Collection<String>> {
        let path = path.as_ref().to_path_buf();
        blocking(move || Filesystem.lines_sync(path)).await
    }

    /// Blocking variant of [`Filesystem::lines`].
    pub fn lines_sync(&self, path: impl AsRef<Path>) -> Result<Collection<String>> {
        let contents = self.get_sync(path)?;
        Ok(collect(contents.split('\n').map(|line| {
            line.strip_suffix('\r').unwrap_or(line).to_string()
        })))
    }

    /// Get the MD5 hash of the file at the given path.
    pub async fn hash(&self, path: impl AsRef<Path>) -> Result<String> {
        self.hash_with(path, "md5").await
    }

    /// Get the hash of the file at the given path using the given algorithm
    /// (`md5`, `sha1`, `sha256`, `sha384`, or `sha512`).
    pub async fn hash_with(&self, path: impl AsRef<Path>, algorithm: &str) -> Result<String> {
        let path = path.as_ref().to_path_buf();
        let algorithm = algorithm.to_string();
        blocking(move || Filesystem.hash_with_sync(path, &algorithm)).await
    }

    /// Blocking variant of [`Filesystem::hash_with`].
    pub fn hash_with_sync(&self, path: impl AsRef<Path>, algorithm: &str) -> Result<String> {
        let bytes = self.get_bytes_sync(path)?;
        hash_bytes(&bytes, algorithm)
    }

    /// Determine if two files have the same contents.
    pub async fn has_same_hash(&self, first: impl AsRef<Path>, second: impl AsRef<Path>) -> bool {
        let (first, second) = (first.as_ref().to_path_buf(), second.as_ref().to_path_buf());
        blocking(move || Ok(Filesystem.has_same_hash_sync(first, second)))
            .await
            .unwrap_or(false)
    }

    /// Blocking variant of [`Filesystem::has_same_hash`].
    pub fn has_same_hash_sync(&self, first: impl AsRef<Path>, second: impl AsRef<Path>) -> bool {
        match (
            self.hash_with_sync(first, "sha256"),
            self.hash_with_sync(second, "sha256"),
        ) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }

    // ------------------------------------------------------------------
    // Writing
    // ------------------------------------------------------------------

    /// Write the contents of a file, returning the number of bytes written.
    pub async fn put(&self, path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result<usize> {
        let path = path.as_ref().to_path_buf();
        let contents = contents.as_ref().to_vec();
        blocking(move || Filesystem.put_sync(path, contents)).await
    }

    /// Blocking variant of [`Filesystem::put`].
    pub fn put_sync(&self, path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result<usize> {
        let contents = contents.as_ref();
        fs::write(path.as_ref(), contents)?;
        Ok(contents.len())
    }

    /// Write the contents of a file while holding an exclusive lock on it.
    pub async fn put_locked(
        &self,
        path: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> Result<usize> {
        let path = path.as_ref().to_path_buf();
        let contents = contents.as_ref().to_vec();
        blocking(move || Filesystem.put_locked_sync(path, contents)).await
    }

    /// Blocking variant of [`Filesystem::put_locked`].
    pub fn put_locked_sync(
        &self,
        path: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> Result<usize> {
        let contents = contents.as_ref();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path.as_ref())?;
        file.lock()?;
        let result = file.set_len(0).and_then(|_| file.write_all(contents));
        file.unlock()?;
        result?;
        Ok(contents.len())
    }

    /// Write the contents of a file atomically: the new contents are written
    /// to a temporary file which then replaces the original.
    pub async fn replace(&self, path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        let contents = contents.as_ref().to_vec();
        blocking(move || Filesystem.replace_sync(path, contents)).await
    }

    /// Blocking variant of [`Filesystem::replace`].
    pub fn replace_sync(&self, path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result<()> {
        let path = path.as_ref();
        // If the path is a symlink, replace the file it points to.
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let directory = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temporary = tempfile::Builder::new()
            .prefix(".laravel-replace-")
            .tempfile_in(directory)?;
        temporary.write_all(contents.as_ref())?;
        #[cfg(unix)]
        {
            let mode = fs::metadata(&path)
                .map(|m| {
                    use std::os::unix::fs::PermissionsExt;
                    m.permissions().mode() & 0o7777
                })
                .unwrap_or(0o644);
            set_mode(temporary.path(), mode)?;
        }
        temporary.persist(&path).map_err(|error| error.error)?;
        Ok(())
    }

    /// Replace a given string within a file.
    pub async fn replace_in_file(
        &self,
        search: &str,
        replace: &str,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        let (search, replace) = (search.to_string(), replace.to_string());
        blocking(move || Filesystem.replace_in_file_sync(&search, &replace, path)).await
    }

    /// Blocking variant of [`Filesystem::replace_in_file`].
    pub fn replace_in_file_sync(
        &self,
        search: &str,
        replace: &str,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)?;
        fs::write(path, contents.replace(search, replace))?;
        Ok(())
    }

    /// Prepend to a file, creating it if it doesn't exist.
    pub async fn prepend(&self, path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> Result<usize> {
        let path = path.as_ref().to_path_buf();
        let data = data.as_ref().to_vec();
        blocking(move || Filesystem.prepend_sync(path, data)).await
    }

    /// Blocking variant of [`Filesystem::prepend`].
    pub fn prepend_sync(&self, path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> Result<usize> {
        let path = path.as_ref();
        let mut contents = data.as_ref().to_vec();
        if path.exists() {
            contents.extend(self.get_bytes_sync(path)?);
        }
        self.put_sync(path, contents)
    }

    /// Append to a file, creating it if it doesn't exist.
    pub async fn append(&self, path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> Result<usize> {
        let path = path.as_ref().to_path_buf();
        let data = data.as_ref().to_vec();
        blocking(move || Filesystem.append_sync(path, data)).await
    }

    /// Blocking variant of [`Filesystem::append`].
    pub fn append_sync(&self, path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> Result<usize> {
        let data = data.as_ref();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_ref())?;
        file.write_all(data)?;
        Ok(data.len())
    }

    /// Delete the file (or files) at the given paths. Returns `false` if any
    /// of them could not be deleted.
    pub async fn delete(&self, paths: impl IntoPaths) -> bool {
        let paths = paths.into_paths();
        blocking(move || Ok(Filesystem.delete_sync(paths)))
            .await
            .unwrap_or(false)
    }

    /// Blocking variant of [`Filesystem::delete`].
    pub fn delete_sync(&self, paths: impl IntoPaths) -> bool {
        let mut success = true;
        for path in paths.into_paths() {
            if fs::remove_file(path).is_err() {
                success = false;
            }
        }
        success
    }

    /// Move a file to a new location.
    pub async fn move_(&self, path: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<()> {
        let (path, target) = (path.as_ref().to_path_buf(), target.as_ref().to_path_buf());
        blocking(move || Filesystem.move_sync(path, target)).await
    }

    /// Blocking variant of [`Filesystem::move_`].
    pub fn move_sync(&self, path: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<()> {
        Ok(fs::rename(path.as_ref(), target.as_ref())?)
    }

    /// Copy a file to a new location.
    pub async fn copy(&self, path: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<()> {
        let (path, target) = (path.as_ref().to_path_buf(), target.as_ref().to_path_buf());
        blocking(move || Filesystem.copy_sync(path, target)).await
    }

    /// Blocking variant of [`Filesystem::copy`].
    pub fn copy_sync(&self, path: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<()> {
        fs::copy(path.as_ref(), target.as_ref())?;
        Ok(())
    }

    /// Create a symlink to the target file or directory.
    pub async fn link(&self, target: impl AsRef<Path>, link: impl AsRef<Path>) -> Result<()> {
        let (target, link) = (target.as_ref().to_path_buf(), link.as_ref().to_path_buf());
        blocking(move || Filesystem.link_sync(target, link)).await
    }

    /// Blocking variant of [`Filesystem::link`].
    pub fn link_sync(&self, target: impl AsRef<Path>, link: impl AsRef<Path>) -> Result<()> {
        let (target, link) = (target.as_ref(), link.as_ref());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link)?;
        }
        #[cfg(windows)]
        {
            if target.is_dir() {
                std::os::windows::fs::symlink_dir(target, link)?;
            } else {
                std::os::windows::fs::symlink_file(target, link)?;
            }
        }
        Ok(())
    }

    /// Create a relative symlink to the target file or directory.
    pub async fn relative_link(
        &self,
        target: impl AsRef<Path>,
        link: impl AsRef<Path>,
    ) -> Result<()> {
        let (target, link) = (target.as_ref().to_path_buf(), link.as_ref().to_path_buf());
        blocking(move || Filesystem.relative_link_sync(target, link)).await
    }

    /// Blocking variant of [`Filesystem::relative_link`].
    pub fn relative_link_sync(
        &self,
        target: impl AsRef<Path>,
        link: impl AsRef<Path>,
    ) -> Result<()> {
        let (target, link) = (target.as_ref(), link.as_ref());
        let base = link.parent().unwrap_or(Path::new("."));
        let relative = relative_path(&absolute(target)?, &absolute(base)?);
        self.link_sync(relative, link)
    }

    // ------------------------------------------------------------------
    // Directories
    // ------------------------------------------------------------------

    /// Create a directory with the given mode, optionally creating parents.
    pub async fn make_directory(
        &self,
        path: impl AsRef<Path>,
        mode: u32,
        recursive: bool,
    ) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        blocking(move || Filesystem.make_directory_sync(path, mode, recursive)).await
    }

    /// Blocking variant of [`Filesystem::make_directory`].
    pub fn make_directory_sync(
        &self,
        path: impl AsRef<Path>,
        mode: u32,
        recursive: bool,
    ) -> Result<()> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(recursive);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(mode);
        }
        #[cfg(not(unix))]
        let _ = mode;
        builder.create(path.as_ref())?;
        Ok(())
    }

    /// Ensure a directory exists, creating it (and its parents) with `0755`.
    pub async fn ensure_directory_exists(&self, path: impl AsRef<Path>) -> Result<()> {
        self.ensure_directory_exists_with_mode(path, 0o755).await
    }

    /// Ensure a directory exists, creating it with the given mode.
    pub async fn ensure_directory_exists_with_mode(
        &self,
        path: impl AsRef<Path>,
        mode: u32,
    ) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        blocking(move || Filesystem.ensure_directory_exists_with_mode_sync(path, mode)).await
    }

    /// Blocking variant of [`Filesystem::ensure_directory_exists`].
    pub fn ensure_directory_exists_sync(&self, path: impl AsRef<Path>) -> Result<()> {
        self.ensure_directory_exists_with_mode_sync(path, 0o755)
    }

    /// Blocking variant of [`Filesystem::ensure_directory_exists_with_mode`].
    pub fn ensure_directory_exists_with_mode_sync(
        &self,
        path: impl AsRef<Path>,
        mode: u32,
    ) -> Result<()> {
        let path = path.as_ref();
        if !path.is_dir() {
            self.make_directory_sync(path, mode, true)?;
        }
        Ok(())
    }

    /// Move a directory. With `overwrite`, an existing destination is removed first.
    pub async fn move_directory(
        &self,
        from: impl AsRef<Path>,
        to: impl AsRef<Path>,
        overwrite: bool,
    ) -> bool {
        let (from, to) = (from.as_ref().to_path_buf(), to.as_ref().to_path_buf());
        blocking(move || Ok(Filesystem.move_directory_sync(from, to, overwrite)))
            .await
            .unwrap_or(false)
    }

    /// Blocking variant of [`Filesystem::move_directory`].
    pub fn move_directory_sync(
        &self,
        from: impl AsRef<Path>,
        to: impl AsRef<Path>,
        overwrite: bool,
    ) -> bool {
        let to = to.as_ref();
        if overwrite && to.is_dir() && !self.delete_directory_sync(to) {
            return false;
        }
        fs::rename(from.as_ref(), to).is_ok()
    }

    /// Copy a directory from one location to another. Returns `false` when
    /// the source is not a directory.
    pub async fn copy_directory(
        &self,
        directory: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<bool> {
        let (directory, destination) = (
            directory.as_ref().to_path_buf(),
            destination.as_ref().to_path_buf(),
        );
        blocking(move || Filesystem.copy_directory_sync(directory, destination)).await
    }

    /// Blocking variant of [`Filesystem::copy_directory`].
    pub fn copy_directory_sync(
        &self,
        directory: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<bool> {
        let (directory, destination) = (directory.as_ref(), destination.as_ref());
        if !directory.is_dir() {
            return Ok(false);
        }
        self.ensure_directory_exists_with_mode_sync(destination, 0o777)?;
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let target = destination.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                if !self.copy_directory_sync(entry.path(), &target)? {
                    return Ok(false);
                }
            } else {
                fs::copy(entry.path(), &target)?;
            }
        }
        Ok(true)
    }

    /// Recursively delete a directory. Returns `false` if it isn't a directory.
    pub async fn delete_directory(&self, directory: impl AsRef<Path>) -> bool {
        let directory = directory.as_ref().to_path_buf();
        blocking(move || Ok(Filesystem.delete_directory_sync(directory)))
            .await
            .unwrap_or(false)
    }

    /// Blocking variant of [`Filesystem::delete_directory`].
    pub fn delete_directory_sync(&self, directory: impl AsRef<Path>) -> bool {
        delete_directory(directory.as_ref(), false)
    }

    /// Remove all of the directories within a given directory.
    pub async fn delete_directories(&self, directory: impl AsRef<Path>) -> bool {
        let directory = directory.as_ref().to_path_buf();
        blocking(move || Ok(Filesystem.delete_directories_sync(directory)))
            .await
            .unwrap_or(false)
    }

    /// Blocking variant of [`Filesystem::delete_directories`].
    pub fn delete_directories_sync(&self, directory: impl AsRef<Path>) -> bool {
        let directories = self.directories(directory);
        if directories.is_empty() {
            return false;
        }
        for directory in directories {
            self.delete_directory_sync(directory);
        }
        true
    }

    /// Empty the given directory of all files and folders, keeping the directory.
    pub async fn clean_directory(&self, directory: impl AsRef<Path>) -> bool {
        let directory = directory.as_ref().to_path_buf();
        blocking(move || Ok(Filesystem.clean_directory_sync(directory)))
            .await
            .unwrap_or(false)
    }

    /// Blocking variant of [`Filesystem::clean_directory`].
    pub fn clean_directory_sync(&self, directory: impl AsRef<Path>) -> bool {
        delete_directory(directory.as_ref(), true)
    }
}

// ----------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------

pub(crate) fn hash_bytes(bytes: &[u8], algorithm: &str) -> Result<String> {
    Ok(match algorithm.to_ascii_lowercase().as_str() {
        "md5" => hex::encode(Md5::digest(bytes)),
        "sha1" => hex::encode(Sha1::digest(bytes)),
        "sha256" => hex::encode(Sha256::digest(bytes)),
        "sha384" => hex::encode(Sha384::digest(bytes)),
        "sha512" => hex::encode(Sha512::digest(bytes)),
        other => bail!("Unsupported hashing algorithm [{other}]."),
    })
}

pub(crate) fn set_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_readonly(mode & 0o200 == 0);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn delete_directory(directory: &Path, preserve: bool) -> bool {
    if !directory.is_dir() {
        return false;
    }
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_real_directory = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_real_directory {
                delete_directory(&path, false);
            } else {
                let _ = fs::remove_file(&path);
            }
        }
    }
    if !preserve {
        let _ = fs::remove_dir(directory);
    }
    true
}

/// List directory entries the way Symfony's Finder does: dot files and VCS
/// directories are skipped (unless `hidden`), and results are sorted by path.
fn list(directory: &Path, recursive: bool, hidden: bool, want_files: bool) -> Vec<PathBuf> {
    let mut results = Vec::new();
    walk(directory, recursive, hidden, want_files, &mut results);
    results.sort();
    results
}

fn walk(
    directory: &Path,
    recursive: bool,
    hidden: bool,
    want_files: bool,
    results: &mut Vec<PathBuf>,
) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if VCS_DIRECTORIES.contains(&name.as_str()) || (!hidden && name.starts_with('.')) {
            continue;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // Follow symlinks for classification, but never recurse through them.
        let is_dir = if file_type.is_symlink() {
            path.is_dir()
        } else {
            file_type.is_dir()
        };
        if is_dir {
            if !want_files {
                results.push(path.clone());
            }
            if recursive && !file_type.is_symlink() {
                walk(&path, recursive, hidden, want_files, results);
            }
        } else if want_files {
            results.push(path);
        }
    }
}

fn glob_walk(base: PathBuf, segments: &[&str], results: &mut BTreeSet<PathBuf>) {
    let Some((segment, rest)) = segments.split_first() else {
        if base.exists() {
            results.insert(base);
        }
        return;
    };

    if !has_wildcards(segment) {
        let next = base.join(segment);
        if rest.is_empty() {
            if next.exists() {
                results.insert(next);
            }
        } else if next.is_dir() {
            glob_walk(next, rest, results);
        }
        return;
    }

    let directory = if base.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        base.clone()
    };
    let Ok(entries) = fs::read_dir(&directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !glob_match(segment, &name) {
            continue;
        }
        let next = base.join(&name);
        if rest.is_empty() {
            results.insert(next);
        } else if next.is_dir() {
            glob_walk(next, rest, results);
        }
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if let Ok(canonical) = fs::canonicalize(path) {
        return Ok(canonical);
    }
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    })
}

/// The path to `target`, relative to the `base` directory.
fn relative_path(target: &Path, base: &Path) -> PathBuf {
    let target: Vec<_> = target.components().collect();
    let base: Vec<_> = base.components().collect();
    let common = target
        .iter()
        .zip(base.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut relative = PathBuf::new();
    for _ in common..base.len() {
        relative.push("..");
    }
    for component in &target[common..] {
        relative.push(component.as_os_str());
    }
    relative
}

// ----------------------------------------------------------------------
// The `File` facade
// ----------------------------------------------------------------------

/// The `File` facade: Laravel's local filesystem utilities.
///
/// ```
/// use illuminate_filesystem::File;
///
/// let directory = tempfile::tempdir().unwrap();
/// let path = directory.path().join("notes.txt");
///
/// File::put_sync(&path, "Remember the milk").unwrap();
///
/// assert!(File::exists(&path));
/// assert_eq!(File::name(&path), "notes");
/// ```
pub struct File;

fn files() -> std::sync::Arc<Filesystem> {
    try_app::<Filesystem>().unwrap_or_default()
}

impl File {
    pub fn exists(path: impl AsRef<Path>) -> bool {
        files().exists(path)
    }

    pub fn missing(path: impl AsRef<Path>) -> bool {
        files().missing(path)
    }

    pub fn is_file(path: impl AsRef<Path>) -> bool {
        files().is_file(path)
    }

    pub fn is_directory(path: impl AsRef<Path>) -> bool {
        files().is_directory(path)
    }

    pub fn is_empty_directory(directory: impl AsRef<Path>) -> bool {
        files().is_empty_directory(directory)
    }

    pub fn is_readable(path: impl AsRef<Path>) -> bool {
        files().is_readable(path)
    }

    pub fn is_writable(path: impl AsRef<Path>) -> bool {
        files().is_writable(path)
    }

    pub fn type_(path: impl AsRef<Path>) -> Result<String> {
        files().type_(path)
    }

    pub fn name(path: impl AsRef<Path>) -> String {
        files().name(path)
    }

    pub fn basename(path: impl AsRef<Path>) -> String {
        files().basename(path)
    }

    pub fn dirname(path: impl AsRef<Path>) -> String {
        files().dirname(path)
    }

    pub fn extension(path: impl AsRef<Path>) -> String {
        files().extension(path)
    }

    pub fn guess_extension(path: impl AsRef<Path>) -> Option<String> {
        files().guess_extension(path)
    }

    pub fn mime_type(path: impl AsRef<Path>) -> Option<String> {
        files().mime_type(path)
    }

    pub fn size(path: impl AsRef<Path>) -> Result<u64> {
        files().size(path)
    }

    pub fn last_modified(path: impl AsRef<Path>) -> Result<i64> {
        files().last_modified(path)
    }

    pub fn permissions(path: impl AsRef<Path>) -> Result<String> {
        files().permissions(path)
    }

    pub fn chmod(path: impl AsRef<Path>, mode: u32) -> Result<()> {
        files().chmod(path, mode)
    }

    pub fn glob(pattern: impl AsRef<str>) -> Vec<PathBuf> {
        files().glob(pattern)
    }

    pub fn files(directory: impl AsRef<Path>) -> Vec<PathBuf> {
        files().files(directory)
    }

    pub fn all_files(directory: impl AsRef<Path>) -> Vec<PathBuf> {
        files().all_files(directory)
    }

    pub fn directories(directory: impl AsRef<Path>) -> Vec<PathBuf> {
        files().directories(directory)
    }

    pub fn all_directories(directory: impl AsRef<Path>) -> Vec<PathBuf> {
        files().all_directories(directory)
    }

    pub async fn get(path: impl AsRef<Path>) -> Result<String> {
        files().get(path).await
    }

    pub fn get_sync(path: impl AsRef<Path>) -> Result<String> {
        files().get_sync(path)
    }

    pub async fn get_bytes(path: impl AsRef<Path>) -> Result<Vec<u8>> {
        files().get_bytes(path).await
    }

    pub async fn shared_get(path: impl AsRef<Path>) -> Result<String> {
        files().shared_get(path).await
    }

    pub async fn json(path: impl AsRef<Path>) -> Result<Value> {
        files().json(path).await
    }

    pub fn json_sync(path: impl AsRef<Path>) -> Result<Value> {
        files().json_sync(path)
    }

    pub async fn lines(path: impl AsRef<Path>) -> Result<Collection<String>> {
        files().lines(path).await
    }

    pub async fn hash(path: impl AsRef<Path>) -> Result<String> {
        files().hash(path).await
    }

    pub async fn hash_with(path: impl AsRef<Path>, algorithm: &str) -> Result<String> {
        files().hash_with(path, algorithm).await
    }

    pub async fn has_same_hash(first: impl AsRef<Path>, second: impl AsRef<Path>) -> bool {
        files().has_same_hash(first, second).await
    }

    pub async fn put(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result<usize> {
        files().put(path, contents).await
    }

    pub fn put_sync(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result<usize> {
        files().put_sync(path, contents)
    }

    pub async fn replace(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result<()> {
        files().replace(path, contents).await
    }

    pub async fn replace_in_file(
        search: &str,
        replace: &str,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        files().replace_in_file(search, replace, path).await
    }

    pub async fn prepend(path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> Result<usize> {
        files().prepend(path, data).await
    }

    pub async fn append(path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> Result<usize> {
        files().append(path, data).await
    }

    pub async fn delete(paths: impl IntoPaths) -> bool {
        files().delete(paths).await
    }

    pub async fn move_(path: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<()> {
        files().move_(path, target).await
    }

    pub async fn copy(path: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<()> {
        files().copy(path, target).await
    }

    pub async fn link(target: impl AsRef<Path>, link: impl AsRef<Path>) -> Result<()> {
        files().link(target, link).await
    }

    pub async fn relative_link(target: impl AsRef<Path>, link: impl AsRef<Path>) -> Result<()> {
        files().relative_link(target, link).await
    }

    pub async fn make_directory(path: impl AsRef<Path>, mode: u32, recursive: bool) -> Result<()> {
        files().make_directory(path, mode, recursive).await
    }

    pub async fn ensure_directory_exists(path: impl AsRef<Path>) -> Result<()> {
        files().ensure_directory_exists(path).await
    }

    pub fn ensure_directory_exists_sync(path: impl AsRef<Path>) -> Result<()> {
        files().ensure_directory_exists_sync(path)
    }

    pub async fn move_directory(
        from: impl AsRef<Path>,
        to: impl AsRef<Path>,
        overwrite: bool,
    ) -> bool {
        files().move_directory(from, to, overwrite).await
    }

    pub async fn copy_directory(
        directory: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<bool> {
        files().copy_directory(directory, destination).await
    }

    pub async fn delete_directory(directory: impl AsRef<Path>) -> bool {
        files().delete_directory(directory).await
    }

    pub async fn delete_directories(directory: impl AsRef<Path>) -> bool {
        files().delete_directories(directory).await
    }

    pub async fn clean_directory(directory: impl AsRef<Path>) -> bool {
        files().clean_directory(directory).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[tokio::test]
    async fn it_reads_and_writes_files() {
        let dir = temp();
        let files = Filesystem::new();
        let path = dir.path().join("file.txt");

        assert_eq!(files.put(&path, "Hello").await.unwrap(), 5);
        assert!(files.exists(&path));
        assert!(files.is_file(&path));
        assert!(!files.is_directory(&path));
        assert_eq!(files.get(&path).await.unwrap(), "Hello");
        assert_eq!(files.get_bytes(&path).await.unwrap(), b"Hello");
        assert_eq!(files.shared_get(&path).await.unwrap(), "Hello");
        assert_eq!(files.size(&path).unwrap(), 5);
        assert!(files.last_modified(&path).unwrap() > 0);

        files.append(&path, " World").await.unwrap();
        files.prepend(&path, ">> ").await.unwrap();
        assert_eq!(files.get(&path).await.unwrap(), ">> Hello World");

        files
            .replace_in_file("World", "Laravel", &path)
            .await
            .unwrap();
        assert_eq!(files.get(&path).await.unwrap(), ">> Hello Laravel");

        files.replace(&path, "Replaced").await.unwrap();
        assert_eq!(files.get(&path).await.unwrap(), "Replaced");

        files.put_locked(&path, "Locked").await.unwrap();
        assert_eq!(files.get(&path).await.unwrap(), "Locked");
    }

    #[tokio::test]
    async fn replace_creates_a_missing_file() {
        let dir = temp();
        let files = Filesystem::new();
        let path = dir.path().join("file.txt");

        files.replace(&path, "Created").await.unwrap();
        assert_eq!(files.get(&path).await.unwrap(), "Created");
    }

    #[tokio::test]
    async fn missing_files_throw_file_not_found() {
        let dir = temp();
        let error = Filesystem
            .get(dir.path().join("nope.txt"))
            .await
            .unwrap_err();
        assert!(error.downcast_ref::<FileNotFoundException>().is_some());
        assert!(error.to_string().starts_with("File does not exist at path"));
        assert!(Filesystem.missing(dir.path().join("nope.txt")));
    }

    #[tokio::test]
    async fn it_reads_json_lines_and_hashes() {
        let dir = temp();
        let files = Filesystem::new();
        let json = dir.path().join("composer.json");
        files
            .put(&json, r#"{"name": "laravel/framework"}"#)
            .await
            .unwrap();
        assert_eq!(
            files.json(&json).await.unwrap()["name"],
            "laravel/framework"
        );

        let lines = dir.path().join("lines.txt");
        files.put(&lines, "one\r\ntwo\nthree").await.unwrap();
        assert_eq!(
            files.lines(&lines).await.unwrap().into_vec(),
            vec!["one", "two", "three"]
        );

        let hello = dir.path().join("hello.txt");
        files.put(&hello, "hello").await.unwrap();
        assert_eq!(
            files.hash(&hello).await.unwrap(),
            "5d41402abc4b2a76b9719d911017c592"
        );
        assert_eq!(
            files.hash_with(&hello, "sha256").await.unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert!(files.hash_with(&hello, "crc").await.is_err());

        let copy = dir.path().join("copy.txt");
        files.copy(&hello, &copy).await.unwrap();
        assert!(files.has_same_hash(&hello, &copy).await);
        assert!(!files.has_same_hash(&hello, &json).await);
    }

    #[test]
    fn path_information_mirrors_pathinfo() {
        let files = Filesystem::new();
        assert_eq!(files.name("/var/www/app.blade.php"), "app.blade");
        assert_eq!(files.basename("/var/www/app.blade.php"), "app.blade.php");
        assert_eq!(files.dirname("/var/www/app.blade.php"), "/var/www");
        assert_eq!(files.extension("/var/www/app.blade.php"), "php");
        assert_eq!(files.extension("/var/www/Makefile"), "");
    }

    #[tokio::test]
    async fn it_detects_types_and_mime_types() {
        let dir = temp();
        let files = Filesystem::new();
        let text = dir.path().join("readme");
        files.put(&text, "plain words").await.unwrap();
        assert_eq!(files.mime_type(&text).unwrap(), "text/plain");
        assert_eq!(files.guess_extension(&text).unwrap(), "txt");
        assert_eq!(files.type_(&text).unwrap(), "file");
        assert_eq!(files.type_(dir.path()).unwrap(), "dir");
        assert!(files.mime_type(dir.path().join("missing")).is_none());

        let png = dir.path().join("image.bin");
        files.put(&png, b"\x89PNG\r\n\x1a\n0000").await.unwrap();
        assert_eq!(files.mime_type(&png).unwrap(), "image/png");
        assert_eq!(files.guess_extension(&png).unwrap(), "png");
    }

    #[tokio::test]
    async fn it_deletes_moves_and_copies_files() {
        let dir = temp();
        let files = Filesystem::new();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let c = dir.path().join("c.txt");
        files.put(&a, "a").await.unwrap();

        files.move_(&a, &b).await.unwrap();
        assert!(files.missing(&a));
        files.copy(&b, &c).await.unwrap();
        assert_eq!(files.get(&c).await.unwrap(), "a");

        assert!(files.delete([&b, &c]).await);
        assert!(files.missing(&b) && files.missing(&c));
        assert!(!files.delete(&b).await);
        assert!(files.move_(&a, &b).await.is_err());
    }

    #[tokio::test]
    async fn it_lists_files_and_directories() {
        let dir = temp();
        let files = Filesystem::new();
        let root = dir.path();
        files
            .ensure_directory_exists(root.join("nested/deeper"))
            .await
            .unwrap();
        files.put(root.join("b.txt"), "").await.unwrap();
        files.put(root.join("a.txt"), "").await.unwrap();
        files.put(root.join(".hidden"), "").await.unwrap();
        files.put(root.join("nested/c.txt"), "").await.unwrap();
        files
            .put(root.join("nested/deeper/d.txt"), "")
            .await
            .unwrap();

        assert_eq!(
            files.files(root),
            vec![root.join("a.txt"), root.join("b.txt")]
        );
        assert_eq!(files.files_with_hidden(root).len(), 3);
        assert_eq!(
            files.all_files(root),
            vec![
                root.join("a.txt"),
                root.join("b.txt"),
                root.join("nested/c.txt"),
                root.join("nested/deeper/d.txt")
            ]
        );
        assert_eq!(files.directories(root), vec![root.join("nested")]);
        assert_eq!(
            files.all_directories(root),
            vec![root.join("nested"), root.join("nested/deeper")]
        );
        assert_eq!(
            files.glob(format!("{}/*.txt", root.display())),
            vec![root.join("a.txt"), root.join("b.txt")]
        );
        assert_eq!(
            files.glob(format!("{}/nested/*/*.txt", root.display())),
            vec![root.join("nested/deeper/d.txt")]
        );
    }

    #[tokio::test]
    async fn it_manages_directories() {
        let dir = temp();
        let files = Filesystem::new();
        let root = dir.path();
        let source = root.join("source");

        files.make_directory(&source, 0o755, false).await.unwrap();
        assert!(
            files
                .make_directory(root.join("x/y"), 0o755, false)
                .await
                .is_err()
        );
        assert!(files.is_empty_directory(&source));
        files.put(source.join(".gitkeep"), "").await.unwrap();
        assert!(!files.is_empty_directory(&source));
        assert!(files.is_empty_directory_ignoring_dot_files(&source));

        files
            .ensure_directory_exists(source.join("sub"))
            .await
            .unwrap();
        files
            .put(source.join("sub/file.txt"), "content")
            .await
            .unwrap();

        let copy = root.join("copy");
        assert!(files.copy_directory(&source, &copy).await.unwrap());
        assert_eq!(
            files.get(copy.join("sub/file.txt")).await.unwrap(),
            "content"
        );
        assert!(
            !files
                .copy_directory(root.join("missing"), &copy)
                .await
                .unwrap()
        );

        let moved = root.join("moved");
        assert!(files.move_directory(&copy, &moved, false).await);
        assert!(files.missing(&copy));

        assert!(files.clean_directory(&moved).await);
        assert!(files.is_directory(&moved));
        assert!(files.is_empty_directory(&moved));

        assert!(files.delete_directories(root).await);
        assert!(files.directories(root).is_empty());
        assert!(!files.delete_directory(root.join("missing")).await);
    }

    #[tokio::test]
    async fn it_creates_links() {
        let dir = temp();
        let files = Filesystem::new();
        let target = dir.path().join("target.txt");
        files.put(&target, "linked").await.unwrap();

        let link = dir.path().join("link.txt");
        files.link(&target, &link).await.unwrap();
        assert_eq!(files.type_(&link).unwrap(), "link");
        assert_eq!(files.get(&link).await.unwrap(), "linked");

        files
            .ensure_directory_exists(dir.path().join("public"))
            .await
            .unwrap();
        let relative = dir.path().join("public/relative.txt");
        files.relative_link(&target, &relative).await.unwrap();
        assert_eq!(
            fs::read_link(&relative).unwrap(),
            PathBuf::from("../target.txt")
        );
        assert_eq!(files.get(&relative).await.unwrap(), "linked");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn it_reads_and_sets_permissions() {
        let dir = temp();
        let files = Filesystem::new();
        let path = dir.path().join("secret.txt");
        files.put(&path, "shh").await.unwrap();
        files.chmod(&path, 0o600).unwrap();
        assert_eq!(files.permissions(&path).unwrap(), "0600");
        assert!(files.is_readable(&path));
        assert!(files.is_writable(&path));
        assert!(files.is_writable(dir.path()));
    }

    #[test]
    fn sync_variants_work_without_a_runtime() {
        let dir = temp();
        let path = dir.path().join("deep/file.txt");
        Filesystem
            .ensure_directory_exists_sync(dir.path().join("deep"))
            .unwrap();
        Filesystem.put_sync(&path, "sync").unwrap();
        assert_eq!(Filesystem.get_sync(&path).unwrap(), "sync");
        assert!(Filesystem.delete_sync(&path));
    }

    #[tokio::test]
    async fn the_facade_uses_the_container_binding() {
        let container = std::sync::Arc::new(illuminate_container::Container::new());
        let _guard = illuminate_container::Container::set_local_instance(container.clone());
        container.instance(Filesystem::new());

        let dir = temp();
        let path = dir.path().join("facade.txt");
        File::put(&path, "facade").await.unwrap();
        assert!(File::exists(&path));
        assert_eq!(File::get(&path).await.unwrap(), "facade");
        assert_eq!(File::extension(&path), "txt");
    }
}
