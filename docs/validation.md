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

## 3. Absolute helper versus PATH shim

The same isolated config was tested with a fake `gh` earlier in PATH that wrote
a marker file:

- `!/usr/bin/gh auth git-credential`: the shim was **not** called.
- `!gh auth git-credential`: the shim **was** called.

This directly confirms the design decision that `gh-app auth setup-git` must
rewrite the helper to an absolute gh-app path. A PATH shim alone does not affect
git.

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

## 7. Real gh auth command behavior

With an isolated temporary HOME and `GH_TOKEN=fake_git_token`:

- `gh auth token` printed the fake token, exit 0.
- `gh auth status` reported `Failed to log in ... using token (GH_TOKEN)` and
  exit 1, because the token was fake.

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

## 9. RS256 JWT

A scratch Rust program parsed a GitHub-style PKCS#1 PEM, converted it to
PKCS#8, signed a JWT signing input with `ring` and `RSA_PKCS1_SHA256`, and wrote
the signature. OpenSSL verified it:

```text
Verified OK
```

A 2048-bit key produced a 256-byte signature and a 447-character JWT.

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

## 10. Constraints and blocked checks

These acceptance paths were not executed:

- Real `gh` commands with a valid GitHub App installation token.
- Real GitHub App JWT acceptance and installation-token minting.
- Static `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` builds.
  `RUSTUP_HOME` is root-owned and read-only, the musl target download failed,
  and there is no `musl-gcc`, aarch64 cross compiler, `cross`, `zig`, Docker, or
  passwordless sudo in this environment.
- OS keyring storage. The environment is headless and likely has no Secret
  Service.
- Windows and macOS.
- Token expiry during a long-running gh command.
- Multi-host target inference.

These are release-gate items for CI or a machine with the proper toolchains. The
static build claim is a design target, not a locally verified artifact.

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
- Static musl builds remain a CI verification item, not a completed check.

## 12. Acceptance trace

| requirement | evidence | status |
|---|---|---|
| Shadow gh with app token | real gh shim test | verified |
| Preserve gh exit codes | unknown command and version tests | verified |
| Git uses gh helper with env token | real git fill test | verified |
| Absolute helper path needed | marker test both ways | verified |
| Authtype credential output | real git HTTP 401 test | verified |
| Legacy credential output | real git fill test | verified |
| store/erase no-op | real git approve/reject test | verified |
| setup-git keys | real gh setup-git test | verified |
| JWT signing | openssl verification | verified |
| Token request shape | local mock endpoint | synthetic |
| Real installation token | no credentials available | blocked |
| Static x86_64/aarch64 binaries | no toolchains available | blocked |
| Key storage and permission model | design only | not implemented |
| gh-app auth login UX | design only | not implemented |
