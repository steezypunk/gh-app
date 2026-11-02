# Security

`gh-app` handles a long-lived GitHub App private key and short-lived
installation tokens. Treat the config directory as sensitive.

## Asset model

| asset | lifetime | impact if leaked |
|---|---|---|
| private key | until revoked on GitHub | persistent app authentication; can mint tokens for every installation |
| config | persistent | host, app ID, installation ID, key path; not enough to authenticate alone |
| token cache | at most 1 hour | installation access within granted permissions |
| token in process environment | life of the gh process | same as the cached token |
| token in a git cache | helper-dependent | stale or valid token until expiry |

## Key storage

The default key location is:

```text
~/.config/gh-app/keys/<host>-<app_id>.pem
```

Controls:

- config directory mode 0700
- key file mode 0600
- key opened with `O_NOFOLLOW`; symlinks are refused
- owner must match the current user
- group or other permission bits are refused
- world-writable parent directories are refused
- keys copied into the managed path are written with a same-directory
  temporary file and rename

The OS keyring can be used with `--keyring` when a Secret Service, Keychain, or
Credential Manager backend is available. On headless Linux, keyring storage is
expected to fail clearly; file storage remains the default.

CI and ephemeral runners can use `GH_APP_PRIVATE_KEY` or `--private-key -`.
This keeps the key out of argv and the config directory, but the key still
exists in the process environment or stdin.

## Token cache

Tokens are cached at:

```text
~/.cache/gh-app/tokens/
```

Each file is mode 0600 and contains a bearer token with an expiry. Cached
tokens are refreshed five minutes before expiry. `GH_APP_NO_CACHE=1` or
`settings.cache_tokens = false` disables disk caching.

The cache never contains the private key. It is deleted on logout.

## Git credential handling

- The helper returns `x-access-token` and the installation token.
- When git advertises `capability[]=authtype`, the helper returns an opaque
  `credential=` value and `ephemeral=1` so caching helpers do not store it.
- Older git receives `password_expiry_utc`.
- `store` and `erase` are no-ops. Git cannot persist or erase app credentials
  through this helper.
- `auth setup-git` writes a blank reset followed by an absolute helper command,
  severing inherited helpers for that host.

## Environment exposure

The installation token is passed to real gh through `GH_TOKEN` or
`GH_ENTERPRISE_TOKEN`. On Linux, same-user processes and root can read
`/proc/<pid>/environ`. The alternative, writing the token into gh's config,
would persist it and is worse.

## Redaction and logging

- Full tokens are printed only by explicit `auth token` commands.
- Status output masks tokens as `ghs_******`.
- API errors include the status, message, and request ID, but not authorization
  headers.
- No telemetry is sent. The only network destination is the configured GitHub
  API.

## Rotation and revocation

`gh-app auth logout` removes local config, managed key, and cache entries. It
does not revoke the private key on GitHub.

If a key is lost or exposed:

1. Delete the key in the GitHub App settings.
2. Generate a new key.
3. Run `gh-app auth login --force` with the new key.

GitHub allows multiple active app keys for staged rotation.

## Residual risks

- Malware running as the same user can read the key file or ptrace the process.
- Backups may include the config directory. Exclude it or use the keyring.
- Root can read everything. Full-disk encryption protects data at rest only.
- A stolen private key remains valid until revoked.
- Installation tokens are bot credentials, not user credentials.

## Reporting

Open a private security advisory or contact the maintainer directly for
vulnerabilities. Do not include private keys or live tokens in reports.
