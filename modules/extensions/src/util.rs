use std::fs;
use std::path::Path;

use crate::ExtensionError;

pub(crate) fn read_dir_if_exists(dir: &Path) -> Result<Vec<fs::DirEntry>, ExtensionError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).map_err(|err| ExtensionError::ReadDirectory {
        path: dir.display().to_string(),
        message: err.to_string(),
    })? {
        entries.push(entry.map_err(|err| ExtensionError::ReadDirectory {
            path: dir.display().to_string(),
            message: err.to_string(),
        })?);
    }
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}
