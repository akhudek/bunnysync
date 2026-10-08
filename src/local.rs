use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

pub struct LocalFile {
    pub relative_path: PathBuf,
    pub path: PathBuf,
    pub is_directory: bool,
    pub last_changed: DateTime<Utc>,
    pub length: u64,
}

/// Get all files in a directory and its subdirectories. If `path` is a file,
/// a single entry is returned.
pub fn get_files(path: &Path) -> Result<Vec<LocalFile>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(path) {
        let entry = entry?;
        let file_path = entry.path();
        let metadata = entry.metadata()?;
        let file_type = metadata.file_type();
        let last_changed = metadata.modified()?;
        let relative_path = file_path.strip_prefix(path)?;
        // The root of a single file source has an empty relative path; keep
        // the file name instead so it is not lost when the remote path is
        // built.
        let relative_path = if relative_path.as_os_str().is_empty() && file_type.is_file() {
            PathBuf::from(file_path.file_name().unwrap_or_default())
        } else {
            relative_path.to_path_buf()
        };
        let file = LocalFile {
            path: file_path.to_path_buf(),
            relative_path,
            is_directory: file_type.is_dir(),
            last_changed: last_changed.into(),
            length: metadata.len(),
        };
        files.push(file);
    }
    Ok(files)
}

/// Get a local file path for the supplied remote path. For example, if
/// the local base is `./thing` and the remote path is `zone://my-zone/path/to/file.txt`,
/// the local path will be `./thing/path/to/file.txt`.
pub fn get_path(local_base: &str, remote_base: &str, remote_path: &str) -> PathBuf {
    let mut local_base: PathBuf = local_base.into();
    let remote_base = format!("/{}", remote_base);

    let remote_path: PathBuf = remote_path.into();

    // If the remote path starts with the remote base, strip it.
    let remote_path = remote_path
        .strip_prefix(&remote_base)
        .unwrap_or(remote_path.as_path());

    // Append the remote path to the local base.
    local_base.push(&remote_path);

    local_base.canonicalize().unwrap_or(local_base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TempDir;
    use std::path::PathBuf;

    #[test]
    fn test_get_files_single_file() {
        // A single file source keeps its file name as the relative path.
        let dir = TempDir::new("get-files-single-file");
        let path = dir.path().join("app.msix");
        std::fs::write(&path, b"data").unwrap();

        let files = get_files(&path).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, path);
        assert_eq!(files[0].relative_path, PathBuf::from("app.msix"));
        assert!(!files[0].is_directory);
        assert_eq!(files[0].length, 4);
    }

    #[test]
    fn test_get_files_directory() {
        // Contents of a directory source are relative to the directory root.
        let dir = TempDir::new("get-files-directory");
        std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("b.txt"), b"b").unwrap();

        let files = get_files(dir.path()).unwrap();
        let mut relative_paths: Vec<_> = files
            .iter()
            .filter(|file| !file.is_directory)
            .map(|file| file.relative_path.clone())
            .collect();
        relative_paths.sort();
        assert_eq!(
            relative_paths,
            vec![PathBuf::from("a.txt"), PathBuf::from("sub").join("b.txt")]
        );
    }

    #[test]
    fn test_basic_path_combination() {
        // Test basic path combination
        let result = get_path("/local/base", "myzone", "/myzone/path/to/file");
        assert_eq!(
            result,
            PathBuf::from("/local/base/path/to/file"),
            "Basic path combination failed"
        );
    }

    #[test]
    fn test_with_trailing_slash_in_local_base() {
        // Test when local base has a trailing slash
        let result = get_path("/local/base/", "myzone", "/myzone/path/to/file");
        assert_eq!(
            result,
            PathBuf::from("/local/base/path/to/file"),
            "Trailing slash handling failed"
        );
    }

    #[test]
    fn test_without_zone_prefix() {
        // Test when remote path doesn't start with zone prefix
        let result = get_path("/local/base", "myzone", "path/to/file");
        assert_eq!(
            result,
            PathBuf::from("/local/base/path/to/file"),
            "Path without zone prefix failed"
        );
    }

    #[test]
    fn test_empty_remote_path() {
        // Test with empty remote path
        let result = get_path("/local/base", "myzone", "");
        assert_eq!(
            result,
            PathBuf::from("/local/base"),
            "Empty remote path handling failed"
        );
    }

    #[test]
    fn test_root_remote_path() {
        // Test when remote path is just the zone root
        let result = get_path("/local/base", "myzone", "/myzone");
        assert_eq!(
            result,
            PathBuf::from("/local/base"),
            "Root remote path handling failed"
        );
    }

    #[test]
    fn test_with_special_characters() {
        // Test with special characters in paths
        let result = get_path(
            "/local/base",
            "myzone",
            "/myzone/path/with spaces/and$pecial@chars",
        );
        assert_eq!(
            result,
            PathBuf::from("/local/base/path/with spaces/and$pecial@chars"),
            "Special character handling failed"
        );
    }

    #[test]
    fn test_with_parent_directory() {
        // Test with parent directory references
        let result = get_path("/local/base", "myzone", "/myzone/path/../to/file");
        // The exact result depends on how dunce::canonicalize handles it
        let expected = if cfg!(windows) {
            PathBuf::from(r"\local\base\path\..\to\file")
        } else {
            PathBuf::from("/local/base/path/../to/file")
        };
        assert_eq!(result, expected, "Parent directory handling failed");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_windows_paths() {
        // Test Windows-specific paths
        let result = get_path(r"C:\local\base", "myzone", r"\myzone\path\to\file");
        assert_eq!(
            result,
            PathBuf::from(r"C:\local\base\path\to\file"),
            "Windows path handling failed"
        );
    }

    #[test]
    fn test_relative_local_base() {
        // Test with relative local base path
        let result = get_path("local/base", "myzone", "/myzone/path/to/file");
        let expected = if cfg!(windows) {
            PathBuf::from(r"local\base\path\to\file")
        } else {
            PathBuf::from("local/base/path/to/file")
        };
        assert_eq!(result, expected, "Relative local base path handling failed");
    }

    #[test]
    fn test_unicode_paths() {
        // Test with Unicode characters
        let result = get_path("/local/基礎", "myzone", "/myzone/パス/ファイル");
        assert_eq!(
            result,
            PathBuf::from("/local/基礎/パス/ファイル"),
            "Unicode path handling failed"
        );
    }

    #[test]
    fn test_multiple_zone_prefixes() {
        // Test with multiple zone prefixes (should only remove the first one)
        let result = get_path("/local/base", "myzone", "/myzone/myzone/path/to/file");
        assert_eq!(
            result,
            PathBuf::from("/local/base/myzone/path/to/file"),
            "Multiple zone prefix handling failed"
        );
    }

    #[test]
    fn test_with_current_directory() {
        // Test with current directory references
        let result = get_path("/local/base", "myzone", "/myzone/./path/./to/./file");
        assert_eq!(
            result,
            PathBuf::from("/local/base/path/to/file"),
            "Current directory handling failed"
        );
    }

    #[test]
    fn test_with_leading_dot() {
        // Test with current directory references
        let result = get_path("./thing/", "myzone/thing", "/myzone/thing/file");
        assert_eq!(
            result,
            PathBuf::from("./thing/file"),
            "Local path starting with ./ failed."
        );
    }
}
