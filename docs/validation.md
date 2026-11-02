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

This is expected because app ID 123456 has no public key on GitHub. It confirms
the endpoint path, TLS, Bearer transport, API version header, and GitHub's error
shape, but it does not prove that a real app's JWT is accepted. That check needs
a real app ID and private key and remains blocked.

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
- Static musl builds work for x86_64 and aarch64 in a scratch toolchain; CI must
  still build and smoke-test the actual gh-app source.
- The static aarch64 JWT-plus-HTTP probe also ran under qemu-aarch64 against the
  mock endpoint.

## 12. Acceptance trace

| requirement | evidence | status |
|---|---|---|
| Shadow gh with app token | real gh shim test | verified |
| Rust exec shim | static x86_64 shim against real gh | verified |
| Preserve gh exit codes | unknown command and version tests | verified |
| Git uses gh helper with env token | real git fill test | verified |
| Absolute helper path needed | marker test both ways | verified |
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
| Key storage and permission model | design only | not implemented |
| gh-app auth login UX | design only | not implemented |
