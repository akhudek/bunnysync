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

/// Get all files in a directory and its subdirectories.
pub fn get_files(path: &Path) -> Result<Vec<LocalFile>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(path) {
        let entry = entry?;
        let file_path = entry.path();
        let relative_path = file_path.strip_prefix(path)?;
        let metadata = entry.metadata()?;
        let file_type = metadata.file_type();
        let last_changed = metadata.modified()?;
        let file = LocalFile {
            path: file_path.to_path_buf(),
            relative_path: relative_path.to_path_buf(),
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
pub fn get_path(local_base: &str, zone_name: &str, remote_path: &str) -> PathBuf {
    let mut local_base: PathBuf = local_base.into();
    let zone_prefix = format!("/{}/", zone_name);

    let remote_path: PathBuf = remote_path.into();

    // If the remote path starts with the zone name, strip it.
    let remote_path = remote_path
        .strip_prefix(&zone_prefix)
        .unwrap_or(remote_path.as_path());

    // Append the remote path to the local base.
    local_base.push(&remote_path);

    local_base.canonicalize().unwrap_or(local_base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
}
