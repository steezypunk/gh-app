# Validation evidence

Date: 2026-11-02
Environment: Linux x86_64, gh 2.102.0, git 2.47.3, Rust 1.99.1

This file records what was actually executed while validating the design, not
just what was read. The probes live outside the repository so they cannot be
mistaken for production code.

## 1. Real gh through a PATH shim

A temporary `gh` script exported `GH_TOKEN=fake_shim_token` and then
`exec /usr/bin/gh "$@"`, matching the proposed wrapper behavior.

| invocation | observed result |
|---|---|
| `gh auth token` | printed `fake_shim_token`, exit 0 |
| `gh --version` | printed the real gh version, exit 0 |
| `gh help` | printed real gh help |
| `gh auth git-credential get` | printed `username=x-access-token` and the fake token |
| `gh definitely-not-a-command` | exit 1, unchanged |

This confirms that env-token injection plus `exec` preserves argv, output, and
exit codes for the real gh public interface.

## 1.1 Environment token precedence

With an isolated HOME and no gh config, `gh auth token` and
`gh auth git-credential` produced:

| environment | target | result |
|---|---|---|
| `GH_TOKEN=a` | github.com | `a` |
| `GITHUB_TOKEN=b` | github.com | `b` |
| `GH_TOKEN=c GITHUB_TOKEN=d` | github.com | `c` |
| `GH_ENTERPRISE_TOKEN=e` | ghe.example.com | `e` |
| `GITHUB_ENTERPRISE_TOKEN=a` | ghe.example.com | `a` |
| `GH_ENTERPRISE_TOKEN=b GITHUB_ENTERPRISE_TOKEN=c` | ghe.example.com | `b` |
| `GH_TOKEN=f` | ghe.example.com | exit 1, no token |
| `GH_ENTERPRISE_TOKEN=g` | github.com | exit 1, no token |
| `GITHUB_ENTERPRISE_TOKEN=d` | github.com | exit 1, no token |
| `GH_TOKEN=h` | acme.ghe.com | `h` |
| `GH_HOST=ghe.example.com GH_ENTERPRISE_TOKEN=i` | default | `i` |
| `GH_ENTERPRISE_TOKEN=j` | helper for ghe.example.com | `username=x-access-token`, `password=j` |
| none | helper for github.com | exit 1, no output |

This confirms the design's choice of `GH_TOKEN` for github.com and `*.ghe.com`,
and `GH_ENTERPRISE_TOKEN` for GitHub Enterprise Server.

## 1.2 Static Rust exec shim

A small Rust program using `std::os::unix::process::CommandExt::exec` was built
as a static-pie x86_64 musl binary (409 KiB) and run against the real gh:

| invocation | observed result |
|---|---|
| `shim auth token` | `fake_rust_shim_token`, exit 0 |
| `shim --version` | real gh 2.102.0 output, exit 0 |
| `shim auth git-credential get` | `x-access-token` and the fake token |
| `shim nope` | exit 1, unchanged |

This validates the actual planned Rust exec path, not only a shell
approximation.

## 1.3 Symlink multi-call dispatch

A static 417 KiB Rust prototype dispatched on `argv[0]` basename. Three symlinks
were created in a temporary directory: `gh`, `gh-app`, and
`git-credential-gh-app`.

Actual results:

- `gh auth token` returned `fake_multicall_token`, exit 0.
- `gh --version` printed real gh 2.102.0, exit 0.
- `gh nope` returned exit 1.
- `gh-app auth status` entered management mode with the expected arguments.
- `git-credential-gh-app get` returned `x-access-token` and the fake token.

This validates the symlink plus basename dispatch model for all three planned
binary names.

## 2. Real git with the real gh credential helper

With an isolated temporary HOME and global git config:

```text
credential.https://github.com.helper = !/usr/bin/gh auth git-credential
```

`git credential fill` with `GH_TOKEN=fake_git_token` returned:

```text
protocol=https
host=github.com
username=x-access-token
password=fake_git_token
```

`password_expiry_utc` was also passed through unchanged when a helper returned
it.

The real gh helper accepted `url=https://github.com/owner/repo` and converted it
to `protocol` and `host`. It returned the legacy username/password form even
when the caller sent `capability[]=authtype`, because an env token is not a
refreshable credential. It rejected `protocol=http` and rejected
`host=github.com:443`. The gh-app helper deliberately improves on the port case
by preserving and matching `host:port`.

## 3. Absolute helper versus PATH shim

The same isolated config was tested with a fake `gh` earlier in PATH that wrote
a marker file:

- `!/usr/bin/gh auth git-credential`: the shim was **not** called.
- `!gh auth git-credential`: the shim **was** called.

This directly confirms the design decision that `gh-app auth setup-git` must
rewrite the helper to an absolute gh-app path. A PATH shim alone does not affect
git.

The blank reset was also tested with a global marker helper:

- Host value set to `""` only: the global helper was not called, and git failed
  with no credential.
- Host value set to `""` followed by the real gh helper: the global helper was
  not called, and gh returned `x-access-token` and the fake token.

This confirms that the reset value severs an inherited helper chain before the
gh-app helper is added.

## 4. Real git HTTP credential path

A local HTTP server returned `401` with `WWW-Authenticate: Basic` for every
request. Git was configured with an adaptive helper and ran
`git ls-remote http://127.0.0.1:PORT/repo.git`.

Observed helper request:

```text
capability[]=authtype
authtype=Basic
credential=eC1hY2Nlc3MtdG9rZW46ZmFrZV9odHRwX3Rva2Vu
ephemeral=1
protocol=http
host=127.0.0.1:45187
username=x-access-token
wwwauth[]=Basic realm="test"
```

The local server then received:

```text
Authorization: Basic eC1hY2Nlc3MtdG9rZW46ZmFrZV9odHRwX3Rva2Vu
```

Git still exited 128 because the test server returns 401 on every request, which
is expected. The important result is that git's real HTTP transport advertised
`capability[]=authtype` and accepted the opaque credential response.

Two details matter for implementation:

- The helper must preserve `host` exactly as git sent it, including a port.
  Match configured hosts after stripping a default port, but echo the original.
- The helper must handle a request that already contains `authtype`,
  `credential`, `ephemeral`, and `wwwauth[]`. Unknown fields are ignored.

## 5. `git credential fill` does not advertise authtype

Running `git credential fill` directly in git 2.47.3 sent only:

```text
protocol=https
host=github.com
```

A helper that unconditionally returned the authtype form caused `git credential
fill` to print only `protocol`, `host`, and `username`, with no password. Git
ignored the opaque `credential` field because it had not advertised the
capability.

Implementation rule: emit the authtype form only when the request contains
`capability[]=authtype`. Otherwise emit `username`, `password`, and
`password_expiry_utc`.

## 6. Store and erase

`git credential approve` invoked the helper as `store`. `git credential reject`
invoked it as `erase`. Both received the credential fields on stdin. Returning
exit 0 without output is sufficient and matches the no-op design.

## 6.1 Credential cache and ephemeral

A real git HTTP flow was run twice with `git-credential-cache` first in the
helper chain and a fake helper second. The server returned 401, then 200 with a
minimal advertisement, so git attempted to store the credential.

| helper output | helper `get` calls across two runs |
|---|---|
| authtype plus `ephemeral=1` | 2 |
| authtype without `ephemeral` | 1 |

With `ephemeral=1`, the cache did not serve the credential on the second run.
Without it, the cache served the credential and the helper was not called.
Git's `credential-cache--daemon` source confirms that `store` skips any
credential with `ephemeral` set.

Git's credential core also checks `password_expiry_utc`. An expired credential
is discarded on read, and `git credential approve` refuses to store it. This
supports emitting `password_expiry_utc` for older git clients.

## 6.2 Key permission checks

A static Rust prototype implemented the planned key open path with `O_NOFOLLOW`
and owner/mode checks. It was built for x86_64 musl and run, and for aarch64
musl and run under qemu-aarch64.

| case | result |
|---|---|
| regular key, mode 0600, owner-owned directory | `ok: mode=600` |
| regular key, mode 0644 | rejected, group/other accessible |
| symlink to a valid key | rejected, `ELOOP` |
| key in a world-writable parent directory | rejected |

The same 0600 and 0644 results were observed on aarch64 under qemu. This
validates the key storage boundary, not only the design text.

## 6.3 Token cache concurrency

A static x86_64 prototype ran eight concurrent processes. Each process took an
exclusive lock, read the cached count, slept, wrote a new count atomically, and
released the lock.

Result:

```text
count=8 mode=600 lock_mode=600
```

No updates were lost, and both the cache file and lock file were mode 0600.
This validates the planned lock plus temp-file-and-rename cache write.

Rust 1.99's standard `File::lock` was sufficient. `fs4` is not needed, which
removes a dependency and simplifies the static build. The lock API requires a
recent Rust, so the project MSRV must be at least the version that stabilized
`File::lock`.

The same multi-process test under qemu-aarch64 failed with `Exec format error`
because the emulator cannot re-exec the aarch64 binary without binfmt
registration. That is a qemu limitation, not a code failure. The single-process
aarch64 key check and JWT/HTTP probes ran successfully.

## 7. Real gh auth command behavior

With an isolated temporary HOME and `GH_TOKEN=fake_git_token`:

- `gh auth token` printed the fake token, exit 0.
- `gh auth status` reported `Failed to log in ... using token (GH_TOKEN)` and
  exit 1, because the token was fake.
- `gh auth status --json hosts` emitted a JSON object with
  `tokenSource: "GH_TOKEN"`, `state: "error"`, and exit 0, preserving gh's
  schema.

With a real installation token, gh's validation path is expected to succeed and
show the app bot as the account. That specific step is blocked in this
environment because there is no GitHub App installation to test with.

## 8. Exact gh setup-git entries

`GH_TOKEN=fake_git_token gh auth setup-git --hostname github.com` in an isolated
HOME produced:

```text
credential.https://github.com.helper
credential.https://github.com.helper !/usr/bin/gh auth git-credential
credential.https://gist.github.com.helper
credential.https://gist.github.com.helper !/usr/bin/gh auth git-credential
```

This confirms the keys and the blank reset value that gh-app must replace.

With `--force` in an isolated HOME:

- `ghe.example.com` produced only `credential.https://ghe.example.com.helper`.
- `acme.ghe.com` produced the host helper and
  `credential.https://gist.acme.ghe.com.helper`.

gh-app deliberately does not configure a gist helper, because GitHub App
installation tokens cannot access gists. For GHES this matches gh. For
`*.ghe.com` it is an intentional difference.

## 9. RS256 JWT

A scratch Rust program parsed a GitHub-style PKCS#1 PEM, converted it to
PKCS#8, signed a JWT signing input with `ring` and `RSA_PKCS1_SHA256`, and wrote
the signature. OpenSSL verified it:

```text
Verified OK
```

A 2048-bit key produced a 256-byte signature and a 447-character JWT.

Additional key variants:

- 4096-bit PKCS#1: signed, 512-byte signature, 788-character JWT, OpenSSL
  verified.
- 2048-bit PKCS#8: signed and OpenSSL verified.
- 1024-bit key: rejected by ring with `KeyRejected("TooSmall")`.

Decoding the JWT confirmed `alg=RS256`, `typ=JWT`, `iss` as a string,
`exp - iat = 600` seconds, and URL-safe base64 without padding.

## 9.1 Mock installation-token endpoint

A Rust probe using `ureq` 3 posted to a local mock endpoint. The observed
request was:

```text
POST /app/installations/987654/access_tokens
Authorization: Bearer eyJhbGciO...
Accept: application/vnd.github+json
X-GitHub-Api-Version: 2026-03-10
User-Agent: gh-app-probe/0.0.0
Body: {"permissions":{"contents":"read"}}
```

The mock returned a token response and the probe parsed `token`, `expires_at`,
`permissions`, and `repository_selection` successfully. This is synthetic
evidence for the request shape and response parsing. The real GitHub API was not
called because there are no app credentials in this environment.

## 9.2 Dependency and binary probe

A release build with `ureq` 3, rustls, `ring`, `rsa`, `serde`, and `base64`
compiled successfully on x86_64 Linux. The binary was 3.2 MiB. `cargo tree`
showed rustls using `ring` and did not show `aws-lc-rs`.

This supports the choice of `ureq` plus `ring` for simpler static musl builds.

## 9.3 Static musl builds

A scratch-only Rust 1.99.1 toolchain and musl targets were installed under the
scratch directory. The dependency probe was built for both architectures:

| target | result |
|---|---|
| `x86_64-unknown-linux-musl` | static-pie ELF, 3.2 MiB, executed successfully |
| `aarch64-unknown-linux-musl` | static aarch64 ELF, 3.1 MiB, executed under qemu-aarch64 |

The x86_64 build used a locally extracted musl 1.2.6 GCC toolchain. The aarch64
build used the `aarch64-linux-musl-cross` GCC 11.2.1 toolchain from musl.cc.
The aarch64 binary was executed with a locally extracted static `qemu-aarch64`
5.2.0 and printed `ok`.

This proves the dependency set and build flags are viable for both requested
architectures. It does not replace a CI build of the actual gh-app source.

## 9.4 Static mint probe

The JWT-plus-HTTP probe was built statically and exercised:

- `x86_64-unknown-linux-musl`: static-pie, 3.2 MiB, built and ran against the
  local mock token endpoint. It produced the expected POST, Bearer JWT, headers,
  and parsed the token response.
- `aarch64-unknown-linux-musl`: static aarch64, 3.6 MiB, built successfully.
  Executed under qemu-aarch64 against the same mock endpoint and produced the
  same request and response.

## 9.5 Real GitHub endpoint with a fake app

A locally signed JWT with `iss=123456` was sent to the real
`POST https://api.github.com/app/installations/1/access_tokens` endpoint. GitHub
returned HTTP 401:

```json
{"message":"A JSON web token could not be decoded","documentation_url":"https://docs.github.com/rest","status":"401"}
```

This is consistent with app ID 123456 having no public key on GitHub. Without a
real app it cannot be distinguished from another JWT rejection, although the
local signature verified with OpenSSL. The call confirms the endpoint path, TLS,
Bearer transport, API version header, and GitHub's error shape, but it does not
prove that a real app's JWT is accepted. That check needs a real app ID and
private key and remains blocked.

## 9.6 Optional keyring feature

The `keyring` 4.2 crate with its default `v1` feature compiled as a static
x86_64 musl binary, 4.6 MiB. On this headless host, `Entry::new` returned:

```text
entry-error: No default store has been set, so cannot search or create entries
```

This confirms two things:

- The optional keyring dependency does not break the static musl build.
- Linux keyring storage needs a working Secret Service. If it is unavailable,
  `--keyring` must fail clearly and file storage remains the fallback.

## 9.7 Proxy support

`ureq` was configured with `Proxy::try_from_env` and pointed at a local proxy
through `HTTP_PROXY`. The proxy observed `CONNECT example.test:80`, then the
tunneled `GET /path`, and returned a marker body. The probe exited 0 with
`proxy_found=true`.

This confirms environment proxy support. It also shows that ureq uses CONNECT
for proxied requests, so an HTTP proxy must permit CONNECT to the target host
and port.

## 9.8 Custom CA

A local test CA and leaf certificate for `localhost` were created. The same
probe was run against one local HTTPS server in two trust configurations:

| trust configuration | result |
|---|---|
| bundled webpki roots only | rejected, `UnknownIssuer` |
| custom root supplied with `RootCerts::new_with_certs` | HTTP 200, `tls-ok` |

This validates the planned `--cacert` and `GH_APP_CA_BUNDLE` path for
enterprise TLS inspection and private GHES certificate authorities.

## 10. Constraints and blocked checks

These acceptance paths were not executed:

- Real `gh` commands with a valid GitHub App installation token.
- Real GitHub App JWT acceptance and installation-token minting.
- OS keyring storage. The environment is headless and likely has no Secret
  Service.
- Windows and macOS.
- Token expiry during a long-running gh command.
- Multi-host target inference.

These are release-gate items for CI or a machine with the proper credentials and
runtime environment.

## 11. What changed because of the evidence

- The credential helper must branch on `capability[]=authtype`. Unconditional
  authtype output breaks `git credential fill`.
- The helper must preserve `host:port` from the request.
- The helper must tolerate requests that already contain `authtype`,
  `credential`, `ephemeral`, and `wwwauth[]`.
- Absolute helper paths are mandatory. This was proven both ways.
- `gh auth status` remains a pass-through, including its exit code when a token
  is invalid.
- `ring` signing is limited to 2048 through 4096 bit RSA keys.
- `GH_TOKEN` and `GH_ENTERPRISE_TOKEN` selection matches the design, including
  the GHES and `*.ghe.com` split.
- `ephemeral=1` is effective against `git-credential-cache` in a real HTTP flow.
- The M1 gh-app source itself builds as static x86_64 and aarch64 binaries and
  runs on both architectures. CI still owns release packaging and smoke tests.
- The static aarch64 JWT-plus-HTTP probe also ran under qemu-aarch64 against the
  mock endpoint.

## 12. Acceptance trace

| requirement | evidence | status |
|---|---|---|
| Shadow gh with app token | real gh shim test | verified |
| Rust exec shim | static x86_64 shim against real gh | verified |
| Symlink multi-call dispatch | static 417 KiB prototype, three symlinks | verified |
| Preserve gh exit codes | unknown command and version tests | verified |
| Git uses gh helper with env token | real git fill test | verified |
| Absolute helper path needed | marker test both ways | verified |
| Blank reset severs helper chain | global marker helper test | verified |
| Authtype credential output | real git HTTP 401 test | verified |
| Legacy credential output | real git fill test | verified |
| store/erase no-op | real git approve/reject test | verified |
| setup-git keys | real gh setup-git test | verified |
| GHES and ghe.com setup keys | real gh --force tests | verified |
| JWT signing | openssl verification | verified |
| JWT claims and key variants | 2048/4096 PKCS#1, 2048 PKCS#8, 1024 rejection | verified |
| Token env precedence | GH_TOKEN/GITHUB_TOKEN/GH_ENTERPRISE_TOKEN tests | verified |
| Git cache ephemeral | real HTTP two-run comparison | verified |
| Token request shape | local mock endpoint | synthetic |
| Real GitHub JWT endpoint | fake app returned expected 401 | verified error path |
| Real installation token | no credentials available | blocked |
| Static x86_64 binary | static-pie built and executed | verified |
| Static aarch64 binary | static ELF built and run under qemu | verified |
| Static mint probe | x86_64 and aarch64 ran against mock | verified |
| Key permission checks | x86_64 and aarch64 static prototype plus M1 code | verified |
| Token cache locking | eight-process static prototype | verified |
| Optional keyring feature | static build succeeds, headless store unavailable | verified |
| HTTP proxy support | local CONNECT proxy test | verified |
| Custom CA bundle | local CA accepted, default roots rejected | verified |
| M1 config and paths | 21 unit tests plus 8 CLI tests | verified |
| M1 static binaries | 1.1 MiB x86_64, 877 KiB aarch64, both executed | verified |
| Key storage and permission model | file storage verified; no OS keyring on this host | partial |
| Non-interactive auth login | mock API flow, key copy, config save | verified |
| Token cache reuse and refresh | mock API mint-count assertions | verified |
| Auth status text and JSON | mock flow and no-token assertion | verified |
| Auth logout cleanup | key and config removal test | verified |
| gh passthrough token injection | fake real gh test | verified |
| git credential helper | legacy and authtype integration tests | verified |
| auth setup-git | temp git config test | verified |
| setup shim install | temp PATH and symlink test | verified |
| Release workflows | present, not run in this environment | configured |
| 429 retry | local mock returns 429 then 201 | verified |
| Strict mode | no-config strict failure test | verified |
| No-cache mode | two token runs mint twice | verified |
| Remote host parsing | https, ssh, and git@ forms | verified |
| Real git HTTPS through setup-git | 401 server received expected Basic auth | verified |
| Real git HTTP with host:port | helper matched and 401 retry succeeded | verified |
| Final archive extraction | both binaries run, auth login/list/doctor pass | verified |
| Clean-container x86_64 smoke | scratch image ran version/list/doctor | verified |
| Real gh 2.102 through shim | version/help/exit/token injection pass | verified |
| Live GitHub API error path | GET /app and token POST returned 401 | verified |
| Final-binary concurrency | eight processes minted once | verified |
| Static dependency closure | no NEEDED entries or interpreter | verified |
| Release-binary JWT signature | x86_64 and aarch64 verified by OpenSSL | verified |
| Final-binary env precedence | GH_TOKEN, GITHUB_TOKEN, enterprise, force | verified |
| Final-binary CONNECT proxy | proxy saw expected host and origin minted | verified |
| Final-binary custom CA | custom root accepted, default roots reject | verified |
| Final-binary retry policy | 429, 500, and 401 clock skew retried once | verified |
| Final-binary refresh skew | near-expiry cached token reminted | verified |
| gh auth login/logout redirect | internal commands ran, real gh skipped | verified |
| gh auth refresh/setup-git redirect | internal commands and -h mapping pass | verified |
| Logout git helper cleanup | removed by default, kept with flag | verified |
| Interactive login PTY | prompts and installation selection | verified |
| Keyring headless failure | clear error, no partial config | verified |
| Aarch64 auth flow | login and list under qemu | verified |

## 13. M1 scaffold validation

The first implementation milestone was built and exercised:

- `cargo test`: 21 unit tests and 8 CLI integration tests pass.
- `cargo clippy --all-targets -- -D warnings` passes.
- `cargo fmt --check` passes.
- The real `gh-app` source built as a static-pie x86_64 musl binary, 1.1 MiB,
  and ran `version`, `auth list`, and `doctor`.
- The real `gh-app` source built as a static aarch64 musl binary, 877 KiB, and
  ran `version` under qemu-aarch64.
- Config round-trip preserved unknown top-level, settings, and host fields.
- A 0600 key was accepted; 0644, symlink, and world-writable parent cases were
  rejected.
- `auth list` rendered a configured host, `doctor` validated the key, and the
  not-yet-implemented commands failed with explicit messages.

This validates the scaffold, config boundary, and packaging, not the GitHub App
authentication flow.

## 14. M2/M3 auth flow validation

The key, JWT, API, cache, and auth command layer was implemented and tested:

- `cargo test`: 34 unit tests and 10 CLI integration tests pass.
- A mock GitHub API verifies `GET /app`, `GET /app/installations/{id}`,
  `POST /app/installations/{id}/access_tokens`, and
  `GET /installation/repositories`.
- Login mints once, stores the token cache at 0600, and prints the app,
  installation, account, key fingerprint, token expiry, and repository count.
- Two `auth token` runs reuse the cached token; the mock mint count stays at 1.
- `auth refresh` mints again and increments the count to 2.
- `auth status --verify` mints again and increments the count to 3.
- `auth status --json` contains only the masked token, not the raw token.
- `auth logout --yes` removes the managed key and host config.
- Static M2 binaries built: 3.5 MiB x86_64 and 2.9 MiB aarch64, both ran
  `version`. The x86_64 binary is static-pie; the aarch64 binary is static.

At this milestone, interactive prompts, keyring storage, retry behavior, and
git helper/config cleanup were not yet covered. Later sections cover them.

## 15. M4/M4.5 and final packaging validation

- `cargo test`: 42 unit tests and 23 CLI integration tests pass.
- A fake real gh receives `GH_TOKEN=ghs_test_token` from the shim, proving token
  injection and exec behavior.
- An explicit non-empty `GH_TOKEN` is preserved unchanged; an empty `GH_TOKEN`
  falls back to the app token.
- Static `--version` bypasses token minting.
- `git-credential get` returns the legacy username/password form with
  `password_expiry_utc`.
- A capability request returns `authtype=Basic`, a base64 credential, and
  `ephemeral=1`.
- `auth setup-git` writes the blank reset and an absolute helper command.
- Helper paths are single-quoted, including spaces, quotes, `$`, and `#`.
- `setup` creates the `gh` symlink and checks PATH ordering.
- A local mock returns 429 with `Retry-After: 0`, then 201; the client retries
  and succeeds.
- A local mock returns 500 once, then 201; the client retries and succeeds.
- `GH_APP_NO_CACHE=1` causes two token runs to mint twice.
- `GH_APP_STRICT` fails when no host is configured.
- `GH_APP_FALLBACK=1` executes the real gh when token minting fails.
- Remote host parsing covers HTTPS, `ssh://`, and `git@host:path` forms.
- `GH_REPO` host prefixes and empty or malformed repository values are handled.
- Keyring support compiles into the static binary; headless runtime reports a
  clear no-default-store error.
- The final static binaries are 5.8 MiB x86_64 and 4.8 MiB aarch64. Both run
  `gh-app version`; the aarch64 binary runs under qemu-aarch64.
- CI and release workflows for both musl targets are present but were not run in
  this environment.
- Local packaging produced x86_64 and aarch64 tarballs plus SHA256 files under
  `dist/`; both checksums verified and each archive contains the `gh-app`
  binary.

Not covered: real GitHub App installation acceptance and GitHub-hosted release
publication.

## 16. Final end-to-end interface validation

Additional checks were run with the actual built binary:

- Real git HTTPS flow: `auth setup-git --hostname localhost` wrote the blank
  reset and the absolute `!<exe> git-credential` command. `git ls-remote`
  reached a local TLS server that returned 401; the configured helper minted
  `ghs_actual_helper_token` and git retried with
  `Authorization: Basic base64(x-access-token:ghs_actual_helper_token)`. The
  server was reached through a local CONNECT proxy so the URL stayed
  `https://localhost`, matching the helper config that setup-git writes.
- Real git HTTP flow with `host:port`: the same helper command was supplied
  explicitly with `GH_APP_ALLOW_HTTP=1`. Git sent `host=localhost:PORT`; the
  helper matched the configured `localhost` after stripping the port, minted
  once, and git retried with the expected Basic authorization.
- Git does not apply the host-only `credential.https://host.helper` key to a
  `https://host:port` URL. Custom-port GHES users must add a port-specific
  helper entry manually; the helper itself handles the port. This caveat is in
  the README troubleshooting section.
- Interactive login through a PTY: host default, app ID, key path, and a
  two-installation selection menu. The selected installation 456 was written to
  config and the managed key was created.
- `--keyring` on this headless host failed clearly with
  `No default store has been set`, exit 1, and no partial config.
- The aarch64 static binary completed `auth login --no-verify` and `auth list`
  under qemu-aarch64, including key copy and config creation.
- Both final release archives were extracted and checked from `dist/`. The
  x86_64 binary is static-pie and the aarch64 binary is static. `auth login
  --no-verify`, `auth list`, and `doctor --offline` passed for the extracted
  x86_64 artifact and for the extracted aarch64 artifact under qemu-aarch64,
  each with an isolated config, cache, and HOME.
- Final binaries removed both host helper values on `auth logout --yes` and
  preserved both with `--keep-git-config`. This passed for x86_64 directly and
  for aarch64 under qemu, with an isolated git config.
- A scratch container containing only the x86_64 artifact ran `version`, then
  `auth list` and `doctor --offline` against a baked isolated config. The
  aarch64 image built but could not execute because the container host has no
  arm64 binfmt emulation; the same artifact passed under host qemu-aarch64.
- The final static sizes are 5.8 MiB (x86_64) and 4.8 MiB (aarch64).

## 17. Deeper final-binary validation

The following checks used the shipped x86_64 binary, gh 2.102.0, git 2.47.3,
and the final musl artifacts:

- Real gh through the shim: `gh auth token` received and printed the mock
  installation token, a second call reused the cache with one total mint,
  `gh --version` and `gh help` passed through without minting, and an unknown
  real-gh command preserved exit code 1.
- Real git credential protocol: `git credential fill` returned
  `x-access-token` and the minted password from the helper; an unknown host
  produced no credentials; plain HTTP produced no credentials unless
  `GH_APP_ALLOW_HTTP=1`; `git credential approve` and `reject` were no-op
  successes.
- Permission boundaries with the final binary: a 0644 key, symlinked key, and
  0644 config were each rejected with clear errors and no token output.
- Cache single-flight: eight concurrent `auth token` processes all succeeded
  with the same token and one mock mint. The cache file was mode 0600.
- Live GitHub TLS: normal `auth login` against `api.github.com` with a fake app
  returned HTTP 401 `A JSON web token could not be decoded` and left no partial
  config. A no-verify login followed by `auth token` reached the real
  installation-token endpoint and also returned that 401.
- Static linkage: the x86_64 artifact is static-pie with zero `NEEDED` entries
  and `ldd` reports statically linked. The aarch64 artifact has no dynamic
  section, no interpreter, and `ldd` reports not a dynamic executable.
- Release-binary JWT signing: both final binaries minted against a mock endpoint
  that captured the Bearer header. Their JWTs had `alg=RS256`, `typ=JWT`,
  `iss=123`, `iat` about 60 seconds in the past, and `exp-iat=600`. Both
  signatures verified with `openssl dgst -sha256 -verify`; the aarch64 binary
  ran under qemu-aarch64.
- Environment precedence with the final shim: explicit non-empty `GH_TOKEN` is
  preserved, empty `GH_TOKEN` falls back to the app token, `GH_APP_FORCE=1`
  overrides an explicit `GH_TOKEN`, `GITHUB_TOKEN` does not block the app token,
  and explicit `GH_ENTERPRISE_TOKEN` is preserved for a GHES host while its
  absence injects the app token.
- Network path with the final binary: a local CONNECT proxy saw
  `example.invalid:80` and tunneled the request to a local origin, which minted
  the token. A local CA supplied through `ca_bundle` was accepted, while the
  same HTTPS endpoint without the bundle failed with
  `invalid peer certificate: UnknownIssuer`.
- Retry policy with the final binary: a mock endpoint returned 429, 500, or 401
  once and then 201. Each case succeeded after exactly two POSTs. For the 401
  case the server `Date` was two hours ahead, and the retried JWT `iat` shifted
  to match that clock. A token expiring inside the 300-second skew was reminted
  on the second `auth token` call.
- `gh auth login` and `gh auth logout` through the shim ran the internal
  commands and never executed the fake real gh. Login wrote the host config,
  logout removed it, and logout removed the host git helper entries by default
  while `--keep-git-config` preserved them.
- `gh auth setup-git -h github.com` mapped `-h` to `--hostname`, wrote the helper
  config, and did not run the fake real gh. `gh auth refresh` minted through the
  internal command, and `gh auth switch` failed with the documented unsupported
  message instead of invoking real gh.
- Permanent regressions added for unknown host, HTTP blocking, 0644 config, and
  0644 key handling, plus `GH_APP_FORCE`, `GITHUB_TOKEN`, and GHES enterprise
  token precedence, logout helper cleanup, and `gh auth` login, logout, refresh,
  setup-git, and switch redirection.
