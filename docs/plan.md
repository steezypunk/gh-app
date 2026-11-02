# Implementation plan

Status: core product, interactive UX, docs, and v0.1.0 release complete; arm64 container emulation remains unvalidated
Companion docs: `docs/design.md`, `docs/research.md`, `docs/validation.md`

## Current progress

- M0 design and research: complete.
- M0.5 interface, crypto, packaging, proxy, and TLS validation: complete.
- M1 scaffold, paths, config, permissions, `auth list`, `version`, and basic
  `doctor`: complete.
- M2 key loading, JWT signing, API client, token minting, and token cache:
  complete, including 401 clock-skew retry and 429/5xx backoff.
- M3 `auth login`, `status`, `token`, `refresh`, `logout`, and non-interactive
  installation discovery: complete, including interactive prompts, installation
  selection, and keyring storage.
- M4 real `gh` discovery, token injection, auth redirection, and shim setup:
  complete, including strict/fallback/no-cache switches and git remote host
  inference.
- M4.5 git credential helper and `auth setup-git`: complete.
- M4.6 install, security, troubleshooting, and bot-limit docs: complete.
- Release workflows ran successfully and `v0.1.0` is published with static
  x86_64 and aarch64 archives plus SHA256 files.

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

## M1: repository scaffold (completed)

Tasks:

- Create `Cargo.toml` with edition 2024 and pinned dependencies.
- Add `src/main.rs` with argv[0] dispatch for `gh`, `gh-app`,
  `git-credential-gh-app`.
- Add `src/cli.rs` with clap subcommands and hidden aliases.
- Add `src/error.rs` with domain errors. Secret redaction is deferred until
  tokens and keys are read.
- Add `src/paths.rs` for config and cache directories.
- Add `src/term.rs` for TTY detection, prompts, and color. Deferred until a
  prompt-driven command lands.
- Add `.github/workflows/ci.yml` with fmt, clippy, test.
- Add `README.md` skeleton and license header check.

Acceptance:

- `cargo run -- --version` works.
- `cargo test` and `cargo clippy -- -D warnings` pass.
- Path tests cover Linux XDG and `GH_APP_CONFIG_DIR`. Windows platform
  fallbacks are compiled conditionally but were not executed in this
  environment.

Result:

- `cargo test`: 21 unit tests and 8 CLI tests pass.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass.
- Static x86_64 musl binary is 1.1 MiB and runs.
- Static aarch64 musl binary is 877 KiB and runs under qemu-aarch64.

## M0.5: design validation (completed)

- Real gh shim test with `GH_TOKEN` and `exec`.
- Static Rust exec shim test against real gh.
- Symlink multi-call dispatch test for `gh`, `gh-app`, and
  `git-credential-gh-app`.
- GH_TOKEN, GITHUB_TOKEN, GH_ENTERPRISE_TOKEN, GH_HOST precedence tests.
- Real git credential fill with the real gh helper.
- Absolute versus relative helper path test.
- Real git HTTP 401 test for `capability[]=authtype`.
- Real git HTTP cache test with and without `ephemeral=1`.
- Real git approve/reject test for store/erase.
- Blank reset versus global helper chain test.
- Key permission prototype: 0600 accepted, 0644, symlink, and world-writable
  parent rejected on x86_64 and aarch64.
- Token cache concurrency: eight processes, all updates retained, cache and lock
  files mode 0600.
- Optional keyring feature compiles static; headless runtime reports no default
  store.
- HTTP proxy from environment and custom CA bundle validated against local
  servers.
- RS256 JWT verified for 2048/4096 PKCS#1, 2048 PKCS#8, and 1024 rejection.
- Mock installation-token endpoint test with ureq.
- Real GitHub token endpoint error shape with a fake app.
- Dependency stack compiled and checked for `aws-lc-rs`.
- Static x86_64 musl build executed and ran; static aarch64 musl build completed
  and ran under qemu-aarch64.

Acceptance: evidence recorded in `docs/validation.md`. At that point, real
installation-token checks and CI builds of the actual source were still release
gates; both were exercised later.

## M1.5: config and permissions (completed)

- `Config` schema with `version`, `settings`, `hosts`, and key source enum.
- TOML load and atomic save.
- Directory and file permission checks.
- Key source validation: exactly one of file, keyring, env, command.
- `gh-app auth list` is implemented. `gh-app auth status` remains in M3.

Acceptance:

- Round-trip test preserves unknown fields.
- 0644 key file is refused with a useful error.
- Symlinked key is refused by default.
- Config version mismatch fails cleanly.

Result: all acceptance checks are covered by unit tests, including unknown-field
preservation and key permission rejection.

## M2: key, JWT, API, cache (core complete)

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

Result:

- Key loading, fingerprints, JWT encoding, API calls, token minting, cache
  locking, and atomic 0600 writes are implemented.
- Tests cover PKCS#1/PKCS#8, 1024-bit rejection, JWT claims and signature
  length, cache key stability, scope hashing, and token masking.
- Mock API integration confirms login minting, cache reuse, refresh, status
  verification, and cache file mode.
- Clock-skew, 429, and 5xx retry logic is implemented and unit tested.

## M3: auth commands (core complete)

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

Result:

- Non-interactive login with explicit app ID, installation ID, key path, API
  URL, and `--no-verify` works.
- Key copy, 0600 enforcement, config save, token cache, status text/JSON,
  token output, refresh, and logout are implemented.
- Mock API test covers the full login-to-logout flow.
- Interactive prompts, installation selection, keyring, and `auth setup-git`
  are implemented.

## M4: gh shadowing (core complete)

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

Result:

- Real gh discovery, PATH scanning, self-inode skipping, static-command fast
  path, token injection, auth command redirection, and shim installation are
  implemented.
- Tests use a fake real gh and assert token injection and exit behavior.
- `gh --version` does not touch the network.
- `GH_APP_STRICT`, `GH_APP_FALLBACK`, `GH_APP_FORCE`, and `GH_APP_NO_CACHE` are
  implemented.
- Target host inference covers explicit flags, `GH_HOST`, configured git
  remotes, and the default host.

## M4.5: git credential helper (complete)

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
- `git credential fill` without `capability[]=authtype` receives the legacy
  username/password form.
- A real git HTTP 401 flow receives the authtype form and sends Basic auth.
- No token appears in stderr or logs.

Result:

- The helper parses `get`, `store`, `erase`, `url=`, `host`, `protocol`, and
  `capability[]`.
- Legacy and authtype outputs are implemented, including `ephemeral=1` and
  `password_expiry_utc`.
- Integration tests assert both output forms, and `auth setup-git` writes the
  blank reset plus absolute helper command.

## M4.6: docs (complete)

- README: install, create app, permissions, login, setup, verify, migrate from
  PAT, uninstall.
- SECURITY.md: threat model, key storage, reporting.
- docs/design.md and docs/research.md kept current.
- Troubleshooting: 401 clock skew, 403 permissions, 404 installation, GHES CA,
  PATH order, `git_protocol=ssh`, long-running commands.
- Document bot identity limitations.

Result: README quick start, permissions, storage, environment variables,
troubleshooting, bot limitations, and SECURITY.md are present.

## M5: release (v0.1.0 published)

- Release profile: LTO, strip, panic abort, codegen units 1.
- Cross build both musl targets.
- Smoke test artifacts in a clean container (x86_64 scratch image passed; the
  aarch64 image cannot execute because the container host has no binfmt).
- Generate per-target SHA256 files.
- Add shell completions (optional, not implemented).
- Tag and publish.

CI and release workflows for both musl targets ran successfully for tag
`v0.1.0`. Local static x86_64 (5.8 MiB) and aarch64 (4.8 MiB) builds run, and
`scripts/package.sh` produced portable, checksum-verified tarballs for both.
Both archives were extracted and exercised on their target architecture. The
GitHub release was published at
<https://github.com/steezypunk/gh-app/releases/tag/v0.1.0>, and its four assets
(two archives and two checksums) were downloaded and verified.

Cross-build note: scratch-only Rust targets and musl GCC toolchains were
installed under the scratch directory. The actual gh-app source was built for
both targets after the final fixes, then executed (`version` on both, and
`auth login` plus `auth list` on aarch64 under qemu-aarch64). The x86_64 binary
is static-pie; the aarch64 binary is static.

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
