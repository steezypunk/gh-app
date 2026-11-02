use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;

fn command() -> Command {
    Command::cargo_bin("gh-app").unwrap()
}

fn isolated(dir: &Path) -> Command {
    let mut cmd = command();
    cmd.env("GH_APP_CONFIG_DIR", dir)
        .env("GH_APP_CACHE_DIR", dir.join("cache"))
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("GH_APP_PRIVATE_KEY")
        .env_remove("GH_APP_PRIVATE_KEY_PATH")
        .env_remove("GH_APP_REAL_GH")
        .env_remove("GH_APP_HOST")
        .env_remove("GH_APP_API_URL")
        .env_remove("GH_APP_APP_ID")
        .env_remove("GH_APP_INSTALLATION_ID")
        .env_remove("GH_APP_NO_CACHE")
        .env_remove("GH_APP_STRICT")
        .env_remove("GH_APP_FALLBACK")
        .env_remove("GH_HOST")
        .env_remove("GH_TOKEN")
        .env_remove("GH_ENTERPRISE_TOKEN")
        .env_remove("GITHUB_TOKEN");
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

fn write_test_key(path: &Path) {
    use rsa::pkcs1::EncodeRsaPrivateKey;

    let mut rng = rand::thread_rng();
    let key = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let pem = key.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap();
    fs::write(path, pem).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

struct MockGitHub {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl MockGitHub {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::clone(&requests);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                handle_connection(stream, &shared);
            }
        });
        Self { base, requests }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    fn mint_count(&self) -> usize {
        self.requests()
            .iter()
            .filter(|line| line.contains("POST /app/installations/456/access_tokens"))
            .count()
    }
}

fn handle_connection(mut stream: TcpStream, requests: &Mutex<Vec<String>>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buffer.extend_from_slice(&chunk[..n]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&buffer);
    let line = text.lines().next().unwrap_or_default().to_string();
    requests.lock().unwrap().push(line.clone());
    let path = line.split_whitespace().nth(1).unwrap_or("/");
    let method = line.split_whitespace().next().unwrap_or("GET");
    let (status, body) = match path {
        "/app" => (
            200,
            serde_json::json!({
                "id": 123,
                "slug": "test-app",
                "client_id": "Iv1.test",
            })
            .to_string(),
        ),
        "/app/installations/456" => (
            200,
            serde_json::json!({
                "id": 456,
                "app_id": 123,
                "account": {"login": "acme"},
                "repository_selection": "all",
                "permissions": {"contents": "write"},
            })
            .to_string(),
        ),
        "/app/installations/456/access_tokens" if method == "POST" => (
            201,
            serde_json::json!({
                "token": "ghs_test_token",
                "expires_at": "2030-01-01T00:00:00Z",
                "permissions": {"contents": "write"},
                "repository_selection": "all",
            })
            .to_string(),
        ),
        path if path.starts_with("/installation/repositories") => {
            (200, serde_json::json!({"total_count": 1}).to_string())
        }
        _ => (404, serde_json::json!({"message": "not found"}).to_string()),
    };
    let reason = if status == 200 {
        "OK"
    } else if status == 201 {
        "Created"
    } else {
        "Not Found"
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
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
fn auth_login_requires_app_id() {
    let dir = tempfile::tempdir().unwrap();
    isolated(dir.path())
        .args(["auth", "login"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("app id is required"));
}

#[test]
fn auth_login_no_verify_then_logout() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source-key.pem");
    write_test_key(&source);

    isolated(dir.path())
        .args([
            "auth",
            "login",
            "--hostname",
            "github.com",
            "--app-id",
            "123",
            "--installation-id",
            "456",
            "--private-key",
            source.to_str().unwrap(),
            "--no-verify",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Configured github.com"))
        .stdout(predicate::str::contains("Verification: skipped"));

    let managed = dir.path().join("keys/github.com-123.pem");
    assert!(managed.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&managed).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    isolated(dir.path())
        .args(["auth", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("github.com"));

    isolated(dir.path())
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not cached"));

    isolated(dir.path())
        .args(["auth", "logout", "--yes"])
        .assert()
        .success();
    assert!(!managed.exists());

    isolated(dir.path())
        .args(["auth", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No hosts configured"));
}

#[test]
fn login_verify_token_cache_refresh_and_status() {
    let server = MockGitHub::start();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source-key.pem");
    write_test_key(&source);

    isolated(dir.path())
        .args([
            "auth",
            "login",
            "--hostname",
            "github.com",
            "--api-url",
            &server.base,
            "--app-id",
            "123",
            "--installation-id",
            "456",
            "--private-key",
            source.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Configured github.com"))
        .stdout(predicate::str::contains("Repositories: 1"));
    assert_eq!(server.mint_count(), 1);

    isolated(dir.path())
        .args(["auth", "token"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ghs_test_token"));
    assert_eq!(server.mint_count(), 1);

    isolated(dir.path())
        .args(["auth", "token"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ghs_test_token"));
    assert_eq!(server.mint_count(), 1);

    isolated(dir.path())
        .args(["auth", "refresh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Refreshed"));
    assert_eq!(server.mint_count(), 2);

    isolated(dir.path())
        .args(["auth", "status", "--verify"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ghs_******"));
    assert_eq!(server.mint_count(), 3);

    let output = isolated(dir.path())
        .args(["auth", "status", "--json"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("ghs_test_token"));
    assert!(stdout.contains("ghs_******"));

    let tokens_dir = dir.path().join("cache/tokens");
    let token_file = fs::read_dir(&tokens_dir)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .find(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .expect("token cache file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(token_file.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    let fake_gh = dir.path().join("fake-gh");
    fs::write(&fake_gh, "#!/bin/sh\nprintf '%s\\n' \"$GH_TOKEN\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_gh, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let shim = dir.path().join("gh");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(assert_cmd::cargo::cargo_bin("gh-app"), &shim).unwrap();
    }
    let mut shim_cmd = Command::new(&shim);
    shim_cmd
        .env("GH_APP_CONFIG_DIR", dir.path())
        .env("GH_APP_CACHE_DIR", dir.path().join("cache"))
        .env("GH_APP_REAL_GH", &fake_gh)
        .env_remove("GH_TOKEN")
        .env_remove("XDG_CONFIG_HOME")
        .args(["auth", "token"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ghs_test_token"));
    assert_eq!(server.mint_count(), 3);

    isolated(dir.path())
        .args(["git-credential", "get"])
        .write_stdin("protocol=https\nhost=github.com\n\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("username=x-access-token"))
        .stdout(predicate::str::contains("password=ghs_test_token"))
        .stdout(predicate::str::contains("password_expiry_utc="));

    isolated(dir.path())
        .args(["git-credential", "get"])
        .write_stdin("capability[]=authtype\nprotocol=https\nhost=github.com\n\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("authtype=Basic"))
        .stdout(predicate::str::contains("ephemeral=1"))
        .stdout(predicate::str::contains("credential="));

    let before_no_cache = server.mint_count();
    for _ in 0..2 {
        isolated(dir.path())
            .env("GH_APP_NO_CACHE", "1")
            .args(["auth", "token"])
            .assert()
            .success()
            .stdout(predicate::str::contains("ghs_test_token"));
    }
    assert_eq!(server.mint_count(), before_no_cache + 2);

    isolated(dir.path())
        .args(["auth", "logout", "--yes"])
        .assert()
        .success();
    assert!(!dir.path().join("keys/github.com-123.pem").exists());
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

#[test]
fn setup_git_writes_helper() {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    write_config(dir.path());
    let gitconfig = home.path().join(".gitconfig");
    command()
        .env("GH_APP_CONFIG_DIR", dir.path())
        .env("GH_APP_CACHE_DIR", dir.path().join("cache"))
        .env("HOME", home.path())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .args(["auth", "setup-git", "--hostname", "github.com"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Configured git credential helper"));

    let output = std::process::Command::new("git")
        .env("HOME", dir.path())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .args([
            "config",
            "--global",
            "--get-all",
            "credential.https://github.com.helper",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("git-credential"));
    assert!(stdout.contains('!'));
}

#[cfg(unix)]
#[test]
fn setup_installs_shim() {
    let dir = tempfile::tempdir().unwrap();
    let shim_dir = dir.path().join("bin");
    fs::create_dir(&shim_dir).unwrap();
    let path = std::env::var_os("PATH").unwrap();
    let mut new_path = std::ffi::OsString::from(shim_dir.as_os_str());
    new_path.push(":");
    new_path.push(path);

    command()
        .env("GH_APP_CONFIG_DIR", dir.path())
        .env("GH_APP_CACHE_DIR", dir.path().join("cache"))
        .env("HOME", dir.path())
        .env("PATH", &new_path)
        .args([
            "setup",
            "--shim-dir",
            shim_dir.to_str().unwrap(),
            "--no-git",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Installed gh shim"));

    let shim = shim_dir.join("gh");
    assert!(fs::symlink_metadata(&shim).is_ok());
    let target = fs::read_link(&shim).unwrap();
    assert!(target.is_file());
}

#[cfg(unix)]
#[test]
fn gh_symlink_passthrough() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let fake_gh = dir.path().join("fake-gh");
    fs::write(&fake_gh, "#!/bin/sh\nprintf 'fake-gh %s\\n' \"$*\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_gh, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let binary = assert_cmd::cargo::cargo_bin("gh-app");
    let link = dir.path().join("gh");
    symlink(binary, &link).unwrap();
    let mut cmd = Command::new(link);
    cmd.env("GH_APP_CONFIG_DIR", dir.path())
        .env("GH_APP_REAL_GH", &fake_gh)
        .env_remove("XDG_CONFIG_HOME")
        .args(["--version"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fake-gh --version"));
}

#[cfg(unix)]
#[test]
fn gh_shim_injects_token_and_respects_explicit_tokens() {
    use std::os::unix::fs::symlink;

    let server = MockGitHub::start();
    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path());
    fs::create_dir_all(dir.path().join("keys")).unwrap();
    write_test_key(&dir.path().join("keys/github.com-123.pem"));
    let mut config = fs::OpenOptions::new()
        .append(true)
        .open(dir.path().join("config.toml"))
        .unwrap();
    writeln!(config, "api_url = \"{}\"", server.base).unwrap();

    let fake_gh = dir.path().join("fake-gh");
    fs::write(
        &fake_gh,
        "#!/bin/sh\nprintf '%s\\n' \"${GH_TOKEN-unset}\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_gh, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let link = dir.path().join("gh");
    symlink(assert_cmd::cargo::cargo_bin("gh-app"), &link).unwrap();

    let mut cmd = Command::new(&link);
    cmd.env("GH_APP_CONFIG_DIR", dir.path())
        .env("GH_APP_CACHE_DIR", dir.path().join("cache"))
        .env("GH_APP_REAL_GH", &fake_gh)
        .env_remove("GH_TOKEN")
        .env_remove("XDG_CONFIG_HOME")
        .args(["api", "user"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ghs_test_token"));
    assert_eq!(server.mint_count(), 1);

    let mut cmd = Command::new(&link);
    cmd.env("GH_APP_CONFIG_DIR", dir.path())
        .env("GH_APP_CACHE_DIR", dir.path().join("cache"))
        .env("GH_APP_REAL_GH", &fake_gh)
        .env("GH_TOKEN", "explicit-token")
        .env_remove("XDG_CONFIG_HOME")
        .args(["api", "user"])
        .assert()
        .success()
        .stdout(predicate::str::contains("explicit-token"));
    assert_eq!(server.mint_count(), 1);

    let mut cmd = Command::new(&link);
    cmd.env("GH_APP_CONFIG_DIR", dir.path())
        .env("GH_APP_CACHE_DIR", dir.path().join("cache"))
        .env("GH_APP_REAL_GH", &fake_gh)
        .env("GH_TOKEN", "")
        .env_remove("XDG_CONFIG_HOME")
        .args(["api", "user"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ghs_test_token"));
    assert_eq!(server.mint_count(), 1);
}

#[cfg(unix)]
#[test]
fn gh_symlink_strict_without_config() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let fake_gh = dir.path().join("fake-gh");
    fs::write(&fake_gh, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_gh, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let link = dir.path().join("gh");
    symlink(assert_cmd::cargo::cargo_bin("gh-app"), &link).unwrap();
    let mut cmd = Command::new(link);
    cmd.env("GH_APP_CONFIG_DIR", dir.path())
        .env("GH_APP_CACHE_DIR", dir.path().join("cache"))
        .env("GH_APP_REAL_GH", &fake_gh)
        .env("GH_APP_STRICT", "1")
        .env_remove("XDG_CONFIG_HOME")
        .args(["api", "user"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("GH_APP_STRICT"));
}

#[cfg(unix)]
#[test]
fn gh_symlink_fallback_on_mint_failure() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    write_config(dir.path());
    fs::create_dir_all(dir.path().join("keys")).unwrap();
    write_test_key(&dir.path().join("keys/github.com-123.pem"));
    let config_path = dir.path().join("config.toml");
    let mut config = fs::OpenOptions::new()
        .append(true)
        .open(&config_path)
        .unwrap();
    writeln!(config, "api_url = \"http://127.0.0.1:1\"").unwrap();

    let fake_gh = dir.path().join("fake-gh");
    fs::write(&fake_gh, "#!/bin/sh\nprintf 'fallback-ok\\n'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_gh, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let link = dir.path().join("gh");
    symlink(assert_cmd::cargo::cargo_bin("gh-app"), &link).unwrap();
    let mut cmd = Command::new(link);
    cmd.env("GH_APP_CONFIG_DIR", dir.path())
        .env("GH_APP_CACHE_DIR", dir.path().join("cache"))
        .env("GH_APP_REAL_GH", &fake_gh)
        .env("GH_APP_FALLBACK", "1")
        .env_remove("XDG_CONFIG_HOME")
        .args(["api", "user"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fallback-ok"));
}

#[cfg(unix)]
#[test]
fn credential_helper_without_config_is_noop() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let binary = assert_cmd::cargo::cargo_bin("gh-app");
    let link = dir.path().join("git-credential-gh-app");
    symlink(binary, &link).unwrap();
    let mut cmd = Command::new(link);
    cmd.env("GH_APP_CONFIG_DIR", dir.path())
        .env("GH_APP_CACHE_DIR", dir.path().join("cache"))
        .env_remove("XDG_CONFIG_HOME")
        .write_stdin("protocol=https\nhost=github.com\n\n")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}
