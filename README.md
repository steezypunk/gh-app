# gh-app

`gh-app` is a Rust CLI that lets the GitHub CLI (`gh`) and `git` use GitHub App
installation credentials instead of a personal access token.

This repository currently contains the completed design and the first
implementation milestone: configuration paths, config schema, key permission
checks, and CLI scaffolding.

## Documentation

- [Design](docs/design.md)
- [Research notes](docs/research.md)
- [Validation evidence](docs/validation.md)
- [Implementation plan](docs/plan.md)

## Status

Implemented:

- multi-call dispatch skeleton for `gh`, `gh-app`, and `git-credential-gh-app`
- XDG and `GH_APP_*` path resolution
- TOML config schema with unknown-field preservation
- config load/save with 0600 atomic writes
- private key permission checks with `O_NOFOLLOW`
- PKCS#1/PKCS#8 key loading and RS256 JWT signing
- GitHub API client with proxy and custom CA support
- installation-token minting with a locked, 0600 token cache
- `gh-app version`
- `gh-app auth list`
- `gh-app auth login`, `status`, `token`, `refresh`, `logout`, and `setup-git`
- interactive login prompts and OS keyring storage
- `gh` passthrough with installation-token injection
- `git-credential-gh-app` helper with authtype and legacy output
- `gh-app setup` shim installation and git configuration
- `gh-app doctor` basic config and key checks

Not implemented yet:

- published release artifacts (the release workflow is present but not run)

## Build

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Package a built static binary:

```sh
./scripts/package.sh v0.1.0 x86_64-unknown-linux-musl
```

## Quick start

1. Create a GitHub App owned by the account or organization you want the bot to
   act for. Grant at least:
   - `Metadata: read` (mandatory)
   - `Contents: read/write` for HTTPS clone and push
   - `Pull requests: write` for PR commands
   - `Issues: write`, `Actions: write`, `Checks: write` as needed
   - `Administration: write` only if the bot must create or delete repositories

2. Install the app and note the app ID, installation ID, and downloaded PEM key.

3. Run:

```sh
gh-app auth login \
  --hostname github.com \
  --app-id 123456 \
  --installation-id 987654 \
  --private-key ~/Downloads/my-app.private-key.pem
```

4. Install the shim and configure git:

```sh
gh-app setup
gh-app doctor
```

5. Verify:

```sh
gh auth token
printf 'protocol=https\nhost=github.com\n\n' | git credential fill
```

## Storage

| data | location | mode |
|---|---|---|
| config | `~/.config/gh-app/config.toml` | 0600 |
| private key | `~/.config/gh-app/keys/<host>-<app_id>.pem` | 0600 |
| token cache | `~/.cache/gh-app/tokens/` | 0600 |

Set `GH_APP_CONFIG_DIR` or `GH_APP_CACHE_DIR` to override. On Windows the
defaults are under `%APPDATA%` and `%LOCALAPPDATA%`.

## Environment variables

| variable | purpose |
|---|---|
| `GH_APP_CONFIG_DIR` | config directory override |
| `GH_APP_CACHE_DIR` | token cache directory override |
| `GH_APP_REAL_GH` | path to the real gh binary |
| `GH_APP_HOST` | target hostname override |
| `GH_APP_APP_ID` | app ID for non-interactive login |
| `GH_APP_INSTALLATION_ID` | installation ID for non-interactive login |
| `GH_APP_PRIVATE_KEY` | PEM contents for login or signer source |
| `GH_APP_PRIVATE_KEY_PATH` | PEM path for login |
| `GH_APP_API_URL` | API base override |
| `GH_APP_CA_BUNDLE` | additional PEM CA bundle |
| `GH_APP_STRICT` | fail instead of passing through when no app credential matches |
| `GH_APP_FALLBACK` | pass through to gh if token minting fails |
| `GH_APP_NO_CACHE` | disable the token cache |

## Troubleshooting

- `gh` still uses a PAT: an existing git helper may be an absolute path such as
  `/usr/bin/gh`. Run `gh-app auth setup-git` and `gh-app doctor`.
- `gh` shim not active: ensure the shim directory precedes the real gh in
  `PATH`, then run `hash -r`.
- API 401 near a clock boundary: check NTP. gh-app retries once using the
  GitHub `Date` header.
- API 403: check the app installation permissions and repository selection.
- API 404 on an installation: verify the installation ID belongs to the app.
- GHES TLS errors: pass `--cacert` or set `GH_APP_CA_BUNDLE`.
- `gh repo clone` uses SSH: run `gh config set git_protocol https`, or clone
  with an HTTPS URL.
- Long-running command fails after an hour: installation tokens expire after
  one hour. Re-run the command.
- Non-interactive login without args: provide `--app-id`,
  `--installation-id`, and `--private-key`, or the matching environment
  variables.

## Bot identity limitations

Installation tokens act as the app installation, not a human user. Gists,
Codespaces, Copilot, personal SSH/GPG key management, `gh api user`, and
personal-account repository creation are not supported. `@me` filters may
resolve to the bot.

## Security

See [SECURITY.md](SECURITY.md) for the threat model, key and cache controls,
rotation, and residual risks.

## License

MIT
