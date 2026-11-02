use crate::cli::SetupArgs;
use crate::error::{Error, Result};

pub fn setup(_args: SetupArgs) -> Result<i32> {
    Err(Error::message(
        "setup is not implemented yet; see docs/plan.md",
    ))
}

pub fn gh(_args: Vec<String>) -> Result<i32> {
    Err(Error::message(
        "gh passthrough is not implemented yet; see docs/plan.md",
    ))
}

pub fn git_credential(_args: Vec<String>) -> Result<i32> {
    Err(Error::message(
        "git-credential is not implemented yet; see docs/plan.md",
    ))
}
