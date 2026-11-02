use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;

fn command() -> Command {
    Command::cargo_bin("gh-app").unwrap()
}

fn isolated(dir: &Path) -> Command {
    let mut cmd = command();
    cmd.env("GH_APP_CONFIG_DIR", dir)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("GH_APP_CACHE_DIR");
    cmd
}

fn write_config(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join("config.toml");
    fs::write(
        &path,
        r#"
version = 1

[hosts."github.com"]
app_id = 123
installation_id = 456
private_key = "keys/github.com-123.pem"
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
fn version_is_printed() {
    command()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("gh-app 0.1.0"));
}

#[test]
fn auth_list_without_config() {
    let dir = tempfile::tempdir().unwrap();
    isolated(dir.path())
        .args(["auth", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No hosts configured"));
}

#[test]
fn auth_list_with_host() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path());
    isolated(dir.path())
        .args(["auth", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("github.com"))
        .stdout(predicate::str::contains("123"))
        .stdout(predicate::str::contains("456"))
        .stdout(predicate::str::contains("keys/github.com-123.pem"));
}

#[test]
fn auth_login_is_stubbed() {
    let dir = tempfile::tempdir().unwrap();
    isolated(dir.path())
        .args(["auth", "login"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not implemented yet"));
}

#[test]
fn doctor_reports_missing_key() {
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path());
    isolated(dir.path())
        .arg("doctor")
        .assert()
        .failure()
        .stdout(predicate::str::contains("FAIL"));
}

#[test]
fn doctor_without_config_is_ok() {
    let dir = tempfile::tempdir().unwrap();
    isolated(dir.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("not configured"));
}

#[cfg(unix)]
#[test]
fn gh_symlink_uses_passthrough_stub() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let binary = assert_cmd::cargo::cargo_bin("gh-app");
    let link = dir.path().join("gh");
    symlink(binary, &link).unwrap();
    let mut cmd = Command::new(link);
    cmd.env("GH_APP_CONFIG_DIR", dir.path())
        .env_remove("XDG_CONFIG_HOME")
        .assert()
        .failure()
        .stderr(predicate::str::contains("gh passthrough"));
}

#[cfg(unix)]
#[test]
fn credential_helper_symlink_is_stubbed() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let binary = assert_cmd::cargo::cargo_bin("gh-app");
    let link = dir.path().join("git-credential-gh-app");
    symlink(binary, &link).unwrap();
    let mut cmd = Command::new(link);
    cmd.env("GH_APP_CONFIG_DIR", dir.path())
        .env_remove("XDG_CONFIG_HOME")
        .assert()
        .failure()
        .stderr(predicate::str::contains("git-credential"));
}
