use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

use crate::error::{Error, Result};

#[allow(dead_code)] // Used by config save and login in later milestones.
pub fn ensure_dir_700(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|e| Error::io(path, e))?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| Error::io(path, e))?;
    Ok(())
}

pub fn read_private_key(path: &Path) -> Result<Vec<u8>> {
    let mut file = open_private_key(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|e| Error::io(path, e))?;
    Ok(buffer)
}

pub fn check_no_group_other(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let metadata = fs::metadata(path).map_err(|e| Error::io(path, e))?;
        let mode = metadata.mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(Error::message(format!(
                "{} has group/other permissions ({mode:o}); run: chmod 600 {}",
                path.display(),
                path.display()
            )));
        }
    }
    Ok(())
}

fn open_private_key(path: &Path) -> Result<File> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::message(format!("{} has no parent directory", path.display())))?;

    #[cfg(unix)]
    {
        let parent_metadata = fs::metadata(parent).map_err(|e| Error::io(parent, e))?;
        if parent_metadata.mode() & 0o002 != 0 {
            return Err(Error::message(format!(
                "{} is world-writable; move the key to a private directory",
                parent.display()
            )));
        }
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);

    let file = options.open(path).map_err(|e| Error::io(path, e))?;

    #[cfg(unix)]
    {
        let metadata = file.metadata().map_err(|e| Error::io(path, e))?;
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(Error::message(format!(
                "{} is not owned by the current user",
                path.display()
            )));
        }
        let mode = metadata.mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(Error::message(format!(
                "{} has group/other permissions ({mode:o}); run: chmod 600 {}",
                path.display(),
                path.display()
            )));
        }
    }

    Ok(file)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn write_key(path: &Path, mode: u32) {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(mode)
            .open(path)
            .unwrap();
        use std::io::Write;
        file.write_all(b"test key").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn accepts_0600_key() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("key.pem");
        write_key(&key, 0o600);
        assert_eq!(read_private_key(&key).unwrap(), b"test key");
    }

    #[test]
    fn rejects_0644_key() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("key.pem");
        write_key(&key, 0o644);
        let error = read_private_key(&key).unwrap_err().to_string();
        assert!(error.contains("group/other permissions"));
    }

    #[test]
    fn rejects_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("key.pem");
        write_key(&key, 0o600);
        let link = dir.path().join("link.pem");
        symlink(&key, &link).unwrap();
        assert!(read_private_key(&link).is_err());
    }

    #[test]
    fn rejects_world_writable_parent() {
        let dir = tempfile::tempdir().unwrap();
        let open = dir.path().join("open");
        fs::create_dir(&open).unwrap();
        fs::set_permissions(&open, fs::Permissions::from_mode(0o777)).unwrap();
        let key = open.join("key.pem");
        write_key(&key, 0o600);
        let error = read_private_key(&key).unwrap_err().to_string();
        assert!(error.contains("world-writable"));
    }

    #[test]
    fn config_mode_check() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        write_key(&config, 0o600);
        assert!(check_no_group_other(&config).is_ok());
        fs::set_permissions(&config, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(check_no_group_other(&config).is_err());
    }

    #[test]
    fn creates_private_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b");
        ensure_dir_700(&nested).unwrap();
        let mode = fs::metadata(&nested).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}
