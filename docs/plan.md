# Implementation plan

Status: proposed, not started
Companion docs: `docs/design.md`, `docs/research.md`

## Goal

Ship `gh-app`, a Rust binary that shadows `gh` and provides GitHub App
installation tokens to gh and git, with static Linux builds for x86_64 and
aarch64.

## Definition of done

- `gh-app auth login` captures host, app ID, installation ID, and private key,
  validates them against GitHub, and stores them securely.
- `gh` resolves to `gh-app` through a symlink and behaves normally, with the app
  token injected.
- `gh auth login`, `logout`, `refresh`, and `setup-git` are redirected to
  gh-app's equivalents without breaking gh's flags or help output.
- `git clone`, `fetch`, `pull`, and `push` over HTTPS work through
  `gh-app git-credential`.
- `gh-app auth status` clearly shows the app, installation, token expiry,
  permissions, key, shim, and git helper state, and never leaks secrets.
- Static release artifacts exist for `x86_64-unknown-linux-musl` and
  `aarch64-unknown-linux-musl`.
- Unit, mock-API, git end-to-end, and fake-gh end-to-end tests pass.
- Documentation covers app creation, permissions, install, migration, security,
  and known limitations.

## Phase 1: repository scaffold

Tasks:

- Create `Cargo.toml` with edition 2024 and pinned dependencies.
- Add `src/main.rs` with argv[0] dispatch for `gh`, `gh-app`,
  `git-credential-gh-app`.
- Add `src/cli.rs` with clap subcommands and hidden aliases.
- Add `src/error.rs` with domain errors and a redaction helper.
- Add `src/paths.rs` for config and cache directories.
- Add `src/term.rs` for TTY detection, prompts, and color.
- Add `.github/workflows/ci.yml` with fmt, clippy, test.
- Add `README.md` skeleton and license header check.

Acceptance:

- `cargo run -- --version` works.
- `cargo test` and `cargo clippy -- -D warnings` pass.
- Path tests cover Linux XDG, `GH_APP_CONFIG_DIR`, and Windows fallback.

## M1.5: config and permissions

- `Config` schema with `version`, `settings`, `hosts`, and key source enum.
- TOML load and atomic save.
- Directory and file permission checks.
- Key source validation: exactly one of file, keyring, env, command.
- `gh-app auth list` and an empty `gh-app auth status`.

Acceptance:

- Round-trip test preserves unknown fields.
- 0644 key file is refused with a useful error.
- Symlinked key is refused by default.
- Config version mismatch fails cleanly.

## M2: key, JWT, API, cache

- Key loading from file, env, keyring feature.
- PKCS#1 and PKCS#8 PEM parsing.
- 2048-bit minimum, 4096-bit maximum (ring signing limit).
- Public-key SHA-256 fingerprint.
- `Signer` trait and ring RS256 implementation.
- JWT builder with `iat`, `exp`, `iss`.
- Host normalization and API base derivation.
- ureq client with proxy, timeouts, custom CA, body limit.
- API methods: get app, get installation, list installations, mint token,
  list repositories.
- Token cache JSON, hashed filenames, lock, atomic write, refresh skew.
- Retry policy for 401 clock skew, 429, and 5xx.

Acceptance:

- JWT signature verifies with openssl in a test.
- Mock API returns a token, second invocation uses cache, near-expiry invocation
  mints again.
- 401 triggers one retry, 403 does not retry, 429 retries with backoff.
- Cache file is 0600 and corrupt cache is treated as a miss.

## M3: auth commands

- `auth login` interactive and non-interactive.
- Installation discovery when ID is omitted.
- Key copy into managed path or keyring.
- GET /app, GET /app/installations/{id}, mint, and repository verification.
- Rollback on failure.
- `auth status`, `auth token`, `auth refresh`, `auth logout`, `auth list`.
- `doctor`.

Acceptance:

- Mock login succeeds and writes config plus key with 0600.
- Wrong app ID, bad key, missing installation, and suspended installation all
  fail without leaving partial state.
- `auth status --json` contains no token and no key contents.
- `auth logout --yes` removes config, key, cache, and git entries.

## M4: gh shadowing

- Real gh discovery with `GH_APP_REAL_GH`, saved path, PATH scan.
- Inode/canonical-path recursion guard.
- `setup` symlink creation and PATH order check.
- Target host inference from args, env, remote, and default.
- Environment token injection and exec.
- Intercept `gh auth login/logout/refresh/setup-git/switch`.
- Pass through `gh auth status/token`.
- Static command fast path.
- `GH_APP_STRICT`, `GH_APP_FALLBACK`, `GH_APP_FORCE`, `GH_APP_NO_CACHE`.

Acceptance:

- Fake real gh script receives the correct token variable and args.
- `gh --version` does not touch the network.
- `gh auth setup-git` invokes gh-app's setup, not real gh's.
- A second `gh` invocation uses the cached token.
- Recursion guard errors instead of looping.

## M4.5: git credential helper

- Parse `get`, `store`, `erase`.
- Parse `url=`, `protocol`, `host`, `path`, `username`, `capability[]`.
- Host match against config.
- Token from cache or mint.
- Output old and authtype forms.
- `ephemeral=1` for git 2.46+.
- `password_expiry_utc` for older git.
- `setup-git` writes blank reset plus absolute helper command.
- Logout cleanup.

Acceptance:

- `git credential fill` returns `x-access-token` and the token.
- Capability request returns `ephemeral=1` and base64 credential.
- Unknown host falls through with no output.
- `credential.https://github.com.helper` is exactly the expected command.
- No token appears in stderr or logs.

## M4.6: docs

- README: install, create app, permissions, login, setup, verify, migrate from
  PAT, uninstall.
- SECURITY.md: threat model, key storage, reporting.
- docs/design.md and docs/research.md kept current.
- Troubleshooting: 401 clock skew, 403 permissions, 404 installation, GHES CA,
  PATH order, `git_protocol=ssh`, long-running commands.
- Document bot identity limitations.

## M5: release

- Release profile: LTO, strip, panic abort, codegen units 1.
- Cross build both musl targets.
- Smoke test artifacts in a clean container.
- Generate SHA256SUMS.
- Add shell completions.
- Tag and publish.

## Test matrix

| area | unit | mock API | git e2e | fake gh e2e | real API |
|---|---|---|---|---|---|
| paths and config | yes | | | | |
| key parsing and permissions | yes | | | | |
| JWT and fingerprint | yes | | | | |
| host mapping | yes | yes | | | |
| login validation | | yes | | | optional |
| token cache and refresh | yes | yes | | | |
| credential protocol | yes | | yes | | |
| setup-git | | | yes | | |
| shim and injection | yes | | | yes | |
| auth command redirection | | | | yes | |
| cross build | | | | | |

## Risks and mitigations

| risk | mitigation |
|---|---|
| gh changes auth internals | rely on stable env token behavior and our own helper, not gh internals |
| git config bypasses shim | rewrite the absolute helper path during setup |
| token expires in long commands | mint immediately before exec and document one-hour limit |
| private key leakage | 0600, owner checks, keyring option, zeroize, redaction, no argv secrets |
| keyring unavailable on headless Linux | file storage default, explicit keyring failure |
| static musl cross build of ring | use cross or native arm runner with C toolchain |
| installation tokens unsupported by some endpoints | clear 403 handling and documented limitations |
| two hosts sharing GH_TOKEN | infer target host or require GH_APP_HOST |
| stale PAT fallback | fail closed on mint errors unless GH_APP_FALLBACK=1 |
| user runs absolute /usr/bin/gh | document and provide doctor PATH check |

## Decisions to confirm

1. Default key storage: file 0600 with keyring opt-in.
2. Explicit GH_TOKEN wins over the app token unless GH_APP_FORCE=1.
3. One app installation per host in v1.
4. `gh auth status` and `gh auth token` pass through to real gh.
5. Linux x86_64 and aarch64 only for release artifacts.
6. No gist helper.
7. `setup` does not modify shell startup files.
