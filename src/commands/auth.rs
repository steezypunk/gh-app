use crate::cli::AuthCommand;
use crate::error::Result;

pub fn run(command: AuthCommand) -> Result<i32> {
    crate::commands::stub_auth(command)
}

pub fn list() -> Result<i32> {
    let path = crate::paths::config_file()?;
    let config = crate::config::io::load(&path)?;
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
