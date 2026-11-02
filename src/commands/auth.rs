use std::collections::BTreeMap;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::cli::{
    AuthCommand, LoginArgs, LogoutArgs, RefreshArgs, SetupGitArgs, StatusArgs, TokenArgs,
};
use crate::config::io::{load, load_required, save};
use crate::config::permissions::{ensure_dir_700, read_private_key};
use crate::config::schema::{Config, HostConfig, KeySourceKind};
use crate::error::{Error, Result};
use crate::github::models::Installation;
use crate::github::{GitHubClient, HostInfo, MintRequest};
use crate::jwt;
use crate::key::Signer;
use crate::keyring_store;
use crate::paths;
use crate::token;

pub fn run(command: AuthCommand) -> Result<i32> {
    match command {
        AuthCommand::Login(args) => login(args),
        AuthCommand::Status(args) => status(args),
        AuthCommand::Token(args) => token_command(args),
        AuthCommand::Refresh(args) => refresh(args),
        AuthCommand::Logout(args) => logout(args),
        AuthCommand::List => list(),
        AuthCommand::SetupGit(args) => setup_git(args),
        AuthCommand::GitCredential { operation } => crate::git_credential::run(vec![operation]),
    }
}

pub fn list() -> Result<i32> {
    let path = paths::config_file()?;
    let config = load(&path)?;
    if config.hosts.is_empty() {
        println!("No hosts configured. Run: gh-app auth login");
        return Ok(0);
    }

    let width = config
        .hosts
        .keys()
        .map(String::len)
        .max()
        .unwrap_or(0)
        .max(4);
    println!(
        "{:<width$}  {:>10}  {:>14}  key",
        "HOST", "APP", "INSTALLATION"
    );
    for (host, host_config) in &config.hosts {
        let key = host_config
            .key_source_label()
            .unwrap_or_else(|e| format!("invalid ({e})"));
        println!(
            "{:<width$}  {:>10}  {:>14}  {}",
            host, host_config.app_id, host_config.installation_id, key
        );
    }
    Ok(0)
}

fn login(args: LoginArgs) -> Result<i32> {
    if args.keyring && args.reference_key {
        return Err(Error::message(
            "--keyring and --reference-key cannot be used together",
        ));
    }

    let config_path = paths::config_file()?;
    let mut config = load(&config_path)?;
    let hostname = match args.hostname.clone().or_else(|| env_string("GH_APP_HOST")) {
        Some(hostname) => hostname,
        None if io::stdin().is_terminal() => {
            prompt("GitHub hostname", Some(&config.settings.default_host))?
        }
        None => config.settings.default_host.clone(),
    };
    let api_url = args
        .api_url
        .clone()
        .or_else(|| env_string("GH_APP_API_URL"));
    let host = HostInfo::new(&hostname)?.with_api_url(api_url.as_deref())?;
    let app_id = match args.app_id.or(env_u64("GH_APP_APP_ID")?) {
        Some(app_id) => app_id,
        None if io::stdin().is_terminal() => prompt("GitHub App ID", None)?
            .parse::<u64>()
            .map_err(|_| Error::message("app ID must be an integer"))?,
        None => {
            return Err(Error::message(
                "app id is required: --app-id or GH_APP_APP_ID",
            ));
        }
    };

    if config.hosts.contains_key(&hostname) && !args.force {
        return Err(Error::message(format!(
            "{hostname} is already configured; use --force to replace it"
        )));
    }

    let (pem, source_path) = read_key_input(&args.private_key, &config, &hostname)?;
    let signer = Signer::from_pem(&pem)?;
    let ca_bundle = args
        .cacert
        .clone()
        .or_else(|| env_string("GH_APP_CA_BUNDLE").map(PathBuf::from));
    let ca_bundle = ca_bundle.map(|path| absolutize(&path)).transpose()?;
    let client = GitHubClient::new(&host, ca_bundle.as_deref())?;
    let issuer = args
        .client_id
        .clone()
        .or_else(|| env_string("GH_APP_CLIENT_ID"))
        .unwrap_or_else(|| app_id.to_string());

    let installation_id = match args.installation_id.or(env_u64("GH_APP_INSTALLATION_ID")?) {
        Some(id) => id,
        None => {
            let token = jwt::build(&signer, &issuer, jwt::unix_now()?)?;
            let installations = client.list_installations(&token)?;
            select_installation(&installations)?
        }
    };

    let mut client_id = args.client_id.clone();
    let mut app_slug = None;
    let mut account_login = None;
    let mut token_summary = None;
    let mut minted_response = None;

    if !args.no_verify {
        let app_token = jwt::build(&signer, &app_id.to_string(), jwt::unix_now()?)?;
        let app = client.get_app(&app_token)?;
        if app.id != app_id {
            return Err(Error::message(format!(
                "app ID mismatch: configured {app_id}, GitHub returned {}",
                app.id
            )));
        }
        client_id = client_id.or(app.client_id.clone());
        app_slug = Some(app.slug.clone());

        let installation = client.get_installation(&app_token, installation_id)?;
        if installation.app_id != app_id {
            return Err(Error::message(format!(
                "installation {installation_id} belongs to app {}, not {app_id}",
                installation.app_id
            )));
        }
        if installation.suspended_at.is_some() {
            return Err(Error::message(format!(
                "installation {installation_id} is suspended"
            )));
        }
        if installation.repository_selection == "selected" {
            eprintln!("note: installation is scoped to selected repositories");
        }
        account_login = Some(installation.account.login.clone());
        match installation.permissions.get("contents").map(String::as_str) {
            Some("write") => {}
            Some(value) => {
                eprintln!("warning: contents permission is {value}; HTTPS pushes will fail")
            }
            None => eprintln!("warning: contents permission is not granted; git access will fail"),
        }

        let minted =
            client.mint_token(&app_token, installation_id, Some(&MintRequest::default()))?;
        let repositories = client.list_repositories(&minted.token)?;
        token_summary = Some((minted.expires_at.clone(), repositories.total_count));
        minted_response = Some(minted);
    }

    let stored = store_key(&args, &hostname, app_id, &pem, source_path.as_deref())?;
    let host_config = HostConfig {
        app_id,
        installation_id,
        client_id,
        app_slug,
        account_login,
        private_key: if stored.keyring {
            None
        } else {
            Some(stored.value.clone())
        },
        private_key_keyring: if stored.keyring {
            Some(stored.value.clone())
        } else {
            None
        },
        private_key_env: None,
        private_key_command: None,
        api_url,
        ca_bundle: ca_bundle.as_ref().map(|path| path.display().to_string()),
        repositories: None,
        permissions: None,
        extra: BTreeMap::new(),
    };

    config.hosts.insert(hostname.clone(), host_config);
    if token::cache_enabled(config.settings.cache_tokens)
        && let Some(response) = minted_response
        && let Err(error) = token::store_response(&host, &config.hosts[&hostname], response)
    {
        eprintln!("warning: could not cache the minted token: {error}");
    }
    if config.hosts.len() == 1 {
        config.settings.default_host = hostname.clone();
    }
    if let Err(error) = save(&config_path, &config) {
        if let Some(path) = stored.managed_path {
            let _ = fs::remove_file(path);
        }
        return Err(error);
    }

    println!("Configured {hostname}");
    println!("  App ID:       {app_id}");
    println!("  Installation: {installation_id}");
    if let Some(login) = &config.hosts[&hostname].account_login {
        println!("  Account:      {login}");
    }
    if let Some(slug) = &config.hosts[&hostname].app_slug {
        println!("  App:          {slug}");
    }
    println!(
        "  Key:          {} ({} bits, {})",
        stored.value,
        signer.bits(),
        signer.fingerprint()
    );
    if let Some((expires_at, repositories)) = token_summary {
        println!("  Token:        valid until {expires_at}");
        println!("  Repositories: {repositories}");
    } else {
        println!("  Verification: skipped (--no-verify)");
    }
    println!("Next: gh-app auth setup-git");
    Ok(0)
}

fn status(args: StatusArgs) -> Result<i32> {
    let path = paths::config_file()?;
    let config = load(&path)?;
    if config.hosts.is_empty() {
        println!("No hosts configured. Run: gh-app auth login");
        return Ok(0);
    }

    let mut rows = Vec::new();
    let mut failures = 0;
    for (hostname, host_config) in &config.hosts {
        if let Some(filter) = &args.hostname
            && filter != hostname
        {
            continue;
        }
        let key = token::cache_key(hostname, host_config, &token::scope_hash(host_config));
        let cached = if token::cache_enabled(config.settings.cache_tokens) {
            match token::load(&key, config.settings.refresh_skew_seconds) {
                Ok(cached) => cached,
                Err(error) => {
                    eprintln!("{hostname}: {error}");
                    failures += 1;
                    None
                }
            }
        } else {
            None
        };
        let mut verify_error = None;
        let verified = if args.verify {
            match signer_for(host_config) {
                Ok(signer) => match host_info(hostname, host_config) {
                    Ok(host) => match GitHubClient::new(&host, ca_bundle(host_config).as_deref()) {
                        Ok(client) => match token::get_or_mint(
                            &host,
                            host_config,
                            &signer,
                            &client,
                            true,
                            config.settings.refresh_skew_seconds,
                            token::cache_enabled(config.settings.cache_tokens),
                        ) {
                            Ok(token) => Some(token),
                            Err(error) => {
                                verify_error = Some(error.to_string());
                                None
                            }
                        },
                        Err(error) => {
                            verify_error = Some(error.to_string());
                            None
                        }
                    },
                    Err(error) => {
                        verify_error = Some(error.to_string());
                        None
                    }
                },
                Err(error) => {
                    verify_error = Some(error.to_string());
                    None
                }
            }
        } else {
            None
        };
        let token = verified.or(cached);
        if verify_error.is_some() {
            failures += 1;
        }

        let row = serde_json::json!({
            "host": hostname,
            "app_id": host_config.app_id,
            "installation_id": host_config.installation_id,
            "account": host_config.account_login,
            "app_slug": host_config.app_slug,
            "key_source": host_config.key_source_label().unwrap_or_else(|e| format!("invalid ({e})")),
            "token": token.as_ref().map(|token| serde_json::json!({
                "masked": token.masked_token(),
                "expires_at": token.expires_at,
                "remaining_seconds": token.remaining_seconds(),
            })),
            "error": verify_error,
        });
        rows.push(row);

        if !args.json {
            println!("{hostname}");
            println!(
                "  App:          {} (id {})",
                host_config.app_slug.as_deref().unwrap_or("unknown"),
                host_config.app_id
            );
            println!(
                "  Installation: {} ({})",
                host_config.installation_id,
                host_config.account_login.as_deref().unwrap_or("unknown")
            );
            match &token {
                Some(token) => println!(
                    "  Token:        {} expires in {}s",
                    token.masked_token(),
                    token.remaining_seconds()
                ),
                None => println!("  Token:        not cached"),
            }
            if let Some(error) = &verify_error {
                println!("  Error:        {error}");
            }
        }
    }

    if args.hostname.is_some() && rows.is_empty() {
        return Err(Error::message("host is not configured"));
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rows)
                .map_err(|e| Error::message(format!("could not render JSON: {e}")))?
        );
    }
    Ok(if failures == 0 { 0 } else { 1 })
}

fn token_command(args: TokenArgs) -> Result<i32> {
    let path = paths::config_file()?;
    let config = load_required(&path)?;
    let (hostname, host_config) = select_host(&config, args.hostname.as_deref())?;
    let signer = signer_for(&host_config)?;
    let host = host_info(&hostname, &host_config)?;
    let client = GitHubClient::new(&host, ca_bundle(&host_config).as_deref())?;
    let token = token::get_or_mint(
        &host,
        &host_config,
        &signer,
        &client,
        args.refresh,
        config.settings.refresh_skew_seconds,
        token::cache_enabled(config.settings.cache_tokens),
    )?;
    println!("{}", token.token);
    Ok(0)
}

fn refresh(args: RefreshArgs) -> Result<i32> {
    let path = paths::config_file()?;
    let config = load_required(&path)?;
    let (hostname, host_config) = select_host(&config, args.hostname.as_deref())?;
    let signer = signer_for(&host_config)?;
    let host = host_info(&hostname, &host_config)?;
    let client = GitHubClient::new(&host, ca_bundle(&host_config).as_deref())?;
    let token = token::get_or_mint(
        &host,
        &host_config,
        &signer,
        &client,
        true,
        config.settings.refresh_skew_seconds,
        token::cache_enabled(config.settings.cache_tokens),
    )?;
    println!(
        "Refreshed {hostname}; token expires at {}",
        token.expires_at
    );
    Ok(0)
}

fn logout(args: LogoutArgs) -> Result<i32> {
    let config_path = paths::config_file()?;
    let mut config = load(&config_path)?;
    if config.hosts.is_empty() {
        println!("No hosts configured");
        return Ok(0);
    }

    let hosts: Vec<String> = if args.all {
        config.hosts.keys().cloned().collect()
    } else {
        let (host, _) = select_host(&config, args.hostname.as_deref())?;
        vec![host]
    };

    if !args.yes {
        if io::stdin().is_terminal() {
            print!("Remove credentials for {}? [y/N] ", hosts.join(", "));
            io::stdout()
                .flush()
                .map_err(|e| Error::message(format!("could not flush stdout: {e}")))?;
            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .map_err(|e| Error::message(format!("could not read confirmation: {e}")))?;
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Cancelled");
                return Ok(0);
            }
        } else {
            return Err(Error::message(
                "refusing to remove credentials without --yes in a non-interactive session",
            ));
        }
    }

    for hostname in &hosts {
        if let Some(host_config) = config.hosts.get(hostname) {
            let key = token::cache_key(hostname, host_config, &token::scope_hash(host_config));
            token::delete(&key)?;
            if host_config.key_source_kind() == Ok(KeySourceKind::File)
                && let Some(value) = &host_config.private_key
            {
                let key_path = resolve_key_path(value, &paths::config_dir()?);
                let keys_dir = paths::keys_dir()?;
                if path_is_within(&key_path, &keys_dir) {
                    let _ = fs::remove_file(&key_path);
                }
            }
            if host_config.key_source_kind() == Ok(KeySourceKind::Keyring)
                && let Some(value) = &host_config.private_key_keyring
            {
                keyring_store::delete(value)?;
            }
        }
        config.hosts.remove(hostname);
    }
    if args.all {
        token::delete_all()?;
    }
    let _ = args.keep_git_config;
    save(&config_path, &config)?;
    println!("Removed {}", hosts.join(", "));
    Ok(0)
}

fn read_key_input(
    explicit: &Option<String>,
    config: &Config,
    hostname: &str,
) -> Result<(Zeroizing<Vec<u8>>, Option<PathBuf>)> {
    if let Some(value) = explicit {
        if value == "-" {
            let mut pem = Vec::new();
            io::stdin().read_to_end(&mut pem).map_err(|e| {
                Error::message(format!("could not read private key from stdin: {e}"))
            })?;
            return Ok((Zeroizing::new(pem), None));
        }
        let path = PathBuf::from(value);
        let pem = read_private_key(&path)?;
        return Ok((Zeroizing::new(pem), Some(path)));
    }

    if let Ok(value) = std::env::var("GH_APP_PRIVATE_KEY")
        && !value.is_empty()
    {
        return Ok((Zeroizing::new(value.into_bytes()), None));
    }
    if let Some(value) = env_string("GH_APP_PRIVATE_KEY_PATH") {
        let path = PathBuf::from(value);
        let pem = read_private_key(&path)?;
        return Ok((Zeroizing::new(pem), Some(path)));
    }
    if let Some(existing) = config.hosts.get(hostname)
        && let Some(value) = &existing.private_key
    {
        let path = resolve_key_path(value, &paths::config_dir()?);
        let pem = read_private_key(&path)?;
        return Ok((Zeroizing::new(pem), Some(path)));
    }
    if io::stdin().is_terminal() {
        let value = prompt("Private key path", None)?;
        if value == "-" {
            return Err(Error::message(
                "use --private-key - to read a key from stdin",
            ));
        }
        let path = PathBuf::from(value);
        let pem = read_private_key(&path)?;
        return Ok((Zeroizing::new(pem), Some(path)));
    }

    Err(Error::message(
        "private key is required: --private-key PATH, --private-key -, GH_APP_PRIVATE_KEY, or GH_APP_PRIVATE_KEY_PATH",
    ))
}

struct StoredKey {
    value: String,
    managed_path: Option<PathBuf>,
    keyring: bool,
}

fn store_key(
    args: &LoginArgs,
    hostname: &str,
    app_id: u64,
    pem: &[u8],
    source_path: Option<&Path>,
) -> Result<StoredKey> {
    if args.keyring {
        let value = keyring_store::default_value(hostname, app_id);
        keyring_store::store(&value, pem)?;
        return Ok(StoredKey {
            value,
            managed_path: None,
            keyring: true,
        });
    }

    if args.reference_key {
        let source = source_path
            .ok_or_else(|| Error::message("--reference-key requires a key file path"))?;
        let absolute = absolutize(source)?;
        return Ok(StoredKey {
            value: absolute.display().to_string(),
            managed_path: None,
            keyring: false,
        });
    }

    let keys_dir = paths::keys_dir()?;
    ensure_dir_700(&keys_dir)?;
    let filename = format!("{hostname}-{app_id}.pem");
    let destination = keys_dir.join(&filename);
    let relative = format!("keys/{filename}");

    if let Some(source) = source_path
        && paths_equal(source, &destination)
    {
        return Ok(StoredKey {
            value: relative,
            managed_path: None,
            keyring: false,
        });
    }
    if destination.exists() && !args.force {
        return Err(Error::message(format!(
            "{} already exists; use --force to replace it",
            destination.display()
        )));
    }

    let mut temp = tempfile::Builder::new()
        .prefix(".key.")
        .tempfile_in(&keys_dir)
        .map_err(|e| Error::io(&keys_dir, e))?;
    temp.write_all(pem)
        .map_err(|e| Error::io(&destination, e))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| Error::io(&destination, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o600))
            .map_err(|e| Error::io(temp.path(), e))?;
    }
    temp.persist(&destination)
        .map_err(|e| Error::io(&destination, e.error))?;
    Ok(StoredKey {
        value: relative,
        managed_path: Some(destination),
        keyring: false,
    })
}

pub(crate) fn select_host(
    config: &Config,
    requested: Option<&str>,
) -> Result<(String, HostConfig)> {
    let hostname = requested
        .map(str::to_string)
        .unwrap_or_else(|| config.settings.default_host.clone());
    if let Some(host_config) = config.hosts.get(&hostname) {
        return Ok((hostname, host_config.clone()));
    }
    if requested.is_none() && config.hosts.len() == 1 {
        let (host, host_config) = config.hosts.iter().next().unwrap();
        return Ok((host.clone(), host_config.clone()));
    }
    Err(Error::message(format!("{hostname} is not configured")))
}

pub(crate) fn signer_for(host_config: &HostConfig) -> Result<Signer> {
    match host_config.key_source_kind().map_err(Error::message)? {
        KeySourceKind::File => {
            let value = host_config
                .private_key
                .as_deref()
                .ok_or_else(|| Error::message("private key path is missing"))?;
            Signer::from_file(&resolve_key_path(value, &paths::config_dir()?))
        }
        KeySourceKind::Env => {
            let name = host_config
                .private_key_env
                .as_deref()
                .ok_or_else(|| Error::message("private key env name is missing"))?;
            Signer::from_env(name)
        }
        KeySourceKind::Keyring => {
            let value = host_config
                .private_key_keyring
                .as_deref()
                .ok_or_else(|| Error::message("keyring entry is missing"))?;
            let pem = Zeroizing::new(keyring_store::load(value)?);
            Signer::from_pem(&pem)
        }
        KeySourceKind::Command => Err(Error::message(
            "private_key_command is not implemented in this milestone",
        )),
    }
}

pub(crate) fn host_info(hostname: &str, config: &HostConfig) -> Result<HostInfo> {
    HostInfo::new(hostname)?.with_api_url(config.api_url.as_deref())
}

pub(crate) fn ca_bundle(config: &HostConfig) -> Option<PathBuf> {
    config
        .ca_bundle
        .clone()
        .map(PathBuf::from)
        .or_else(|| env_string("GH_APP_CA_BUNDLE").map(PathBuf::from))
}

fn resolve_key_path(value: &str, base: &Path) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|e| Error::message(format!("could not resolve {}: {e}", path.display())))
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn path_is_within(path: &Path, dir: &Path) -> bool {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    path.starts_with(dir)
}

fn env_string(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn env_u64(name: &str) -> Result<Option<u64>> {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => value
            .parse::<u64>()
            .map(Some)
            .map_err(|_| Error::message(format!("{name} must be an integer"))),
        _ => Ok(None),
    }
}

pub(crate) fn setup_git(args: SetupGitArgs) -> Result<i32> {
    let config = load(&paths::config_file()?)?;
    let hosts: Vec<String> = if let Some(hostname) = args.hostname {
        if !config.hosts.contains_key(&hostname) && !args.force {
            return Err(Error::message(format!(
                "{hostname} is not configured; use --force to configure it anyway"
            )));
        }
        vec![hostname]
    } else {
        config.hosts.keys().cloned().collect()
    };
    if hosts.is_empty() {
        return Err(Error::message(
            "no hosts configured; run: gh-app auth login",
        ));
    }

    let exe = std::env::current_exe()
        .map_err(|e| Error::message(format!("could not find current executable: {e}")))?;
    let helper = format!(
        "!{} git-credential",
        shell_quote(&exe.display().to_string())
    );
    for hostname in hosts {
        let key = format!("credential.https://{hostname}.helper");
        run_git(&["config", "--global", "--replace-all", &key, ""])?;
        run_git(&["config", "--global", "--add", &key, &helper])?;
        println!("Configured git credential helper for {hostname}");
    }
    Ok(0)
}

fn run_git(args: &[&str]) -> Result<()> {
    let status = std::process::Command::new("git")
        .args(args)
        .status()
        .map_err(|e| Error::message(format!("could not run git: {e}")))?;
    if !status.success() {
        return Err(Error::message(format!("git {} failed", args.join(" "))));
    }
    Ok(())
}

fn shell_quote(value: &str) -> String {
    if value.contains([' ', '\'', '\\']) {
        format!("'{}'", value.replace('\'', "'\\''"))
    } else {
        value.to_string()
    }
}

fn prompt(label: &str, default: Option<&str>) -> Result<String> {
    match default {
        Some(default) => print!("{label} [{default}]: "),
        None => print!("{label}: "),
    }
    io::stdout()
        .flush()
        .map_err(|e| Error::message(format!("could not flush stdout: {e}")))?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| Error::message(format!("could not read input: {e}")))?;
    let value = input.trim();
    if value.is_empty() {
        default
            .map(str::to_string)
            .ok_or_else(|| Error::message(format!("{label} is required")))
    } else {
        Ok(value.to_string())
    }
}

fn select_installation(installations: &[Installation]) -> Result<u64> {
    if let [installation] = installations {
        return Ok(installation.id);
    }
    if installations.is_empty() {
        return Err(Error::message(
            "the app has no installations; install it on GitHub first",
        ));
    }
    if !io::stdin().is_terminal() {
        return Err(Error::message(format!(
            "found {} installations; pass --installation-id",
            installations.len()
        )));
    }
    println!("Multiple installations found:");
    for (index, installation) in installations.iter().enumerate() {
        println!(
            "  {}. {} (installation {})",
            index + 1,
            installation.account.login,
            installation.id
        );
    }
    let value = prompt("Select installation", Some("1"))?;
    let index = value
        .parse::<usize>()
        .map_err(|_| Error::message("installation selection must be a number"))?;
    installations
        .get(index.saturating_sub(1))
        .map(|installation| installation.id)
        .ok_or_else(|| Error::message("installation selection is out of range"))
}
