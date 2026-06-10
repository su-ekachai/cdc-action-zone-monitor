//! Filesystem helpers shared by state and watchlist persistence.

use anyhow::Context;

/// Writes `contents` to `path` atomically: write to a `.tmp` sibling, then rename.
///
/// A rename within the same directory is atomic on POSIX, so a crash mid-write
/// (cron timeout, OOM kill) leaves either the old file or the new one — never a
/// truncated mix.
///
/// # Errors
///
/// Returns an error if the temp file cannot be written or the rename fails.
pub fn write_atomic(path: &str, contents: &str) -> anyhow::Result<()> {
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, contents).with_context(|| format!("Failed to write temp file {tmp}"))?;
    std::fs::rename(&tmp, path).with_context(|| format!("Failed to rename {tmp} to {path}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_atomic_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.json");
        let path_str = path.to_str().unwrap();

        write_atomic(path_str, "hello").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn test_write_atomic_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.json");
        let path_str = path.to_str().unwrap();
        std::fs::write(&path, "old").unwrap();

        write_atomic(path_str, "new").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn test_write_atomic_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.json");
        let path_str = path.to_str().unwrap();

        write_atomic(path_str, "data").unwrap();

        let entries = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(entries, 1);
    }
}
