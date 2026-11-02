use std::fs;
use std::path::Path;

use crate::cli::{SetupArgs, SetupGitArgs};
use crate::commands::auth;
use crate::error::{Error, Result};

pub fn run(args: SetupArgs) -> Result<i32> {
    let shim_dir = match args.shim_dir {
        Some(path) => path,
        None => dirs::home_dir()
            .ok_or_else(|| Error::message("could not determine home directory"))?
            .join(".local/bin"),
    };
    fs::create_dir_all(&shim_dir).map_err(|e| Error::io(&shim_dir, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&shim_dir)
            .map_err(|e| Error::io(&shim_dir, e))?
            .permissions()
            .mode();
        if mode & 0o002 != 0 {
            return Err(Error::message(format!(
                "{} is world-writable; choose a private shim directory",
                shim_dir.display()
            )));
        }
    }

    if !args.no_shim {
        install_shim(&shim_dir, args.force)?;
    }
    if !args.no_git {
        auth::setup_git(SetupGitArgs {
            hostname: None,
            force: false,
        })?;
    }

    if !args.no_shim {
        let shim = shim_dir.join(shim_name());
        if shim_is_first(&shim) {
            println!("Installed gh shim: {}", shim.display());
        } else {
            println!("Installed gh shim at {}", shim.display());
            println!("The shim is not first in PATH. Add it with:");
            println!("  export PATH=\"{}:$PATH\"", shim_dir.display());
            println!("Then run: hash -r");
            return Ok(1);
        }
    }
    println!("Next: run gh-app doctor");
    Ok(0)
}

fn install_shim(shim_dir: &Path, force: bool) -> Result<()> {
    let current = std::env::current_exe()
        .map_err(|e| Error::message(format!("could not find current executable: {e}")))?;
    let shim = shim_dir.join(shim_name());
    if fs::symlink_metadata(&shim).is_ok() {
        if !force {
            return Err(Error::message(format!(
                "{} already exists; use --force to replace it",
                shim.display()
            )));
        }
        fs::remove_file(&shim).map_err(|e| Error::io(&shim, e))?;
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&current, &shim).map_err(|e| Error::io(&shim, e))?;
    }
    #[cfg(not(unix))]
    {
        fs::copy(&current, &shim).map_err(|e| Error::io(&shim, e))?;
    }
    Ok(())
}

fn shim_name() -> &'static str {
    if cfg!(windows) { "gh.exe" } else { "gh" }
}

fn shim_is_first(shim: &Path) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(shim_name());
        if fs::symlink_metadata(&candidate).is_ok() {
            return same_file(&candidate, shim);
        }
    }
    false
}

fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (fs::metadata(a), fs::metadata(b)) {
            return a.dev() == b.dev() && a.ino() == b.ino();
        }
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shim_name_matches_platform() {
        if cfg!(windows) {
            assert_eq!(shim_name(), "gh.exe");
        } else {
            assert_eq!(shim_name(), "gh");
        }
    }
}
