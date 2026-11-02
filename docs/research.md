# Research notes

Date: 2026-11-02
Environment: Linux x86_64, gh 2.102.0, git 2.47.3, Rust 1.99.1

This file records the facts that drive the design. Numbers and URLs were checked
on the date above.

## 1. gh can be shadowed, but absolute paths bypass the shim

On Unix, gh looks up its own state in `~/.config/gh` (`hosts.yml` is mode 0600).
It does not care what executable name it was started from. A symlink named `gh`
that points at `gh-app` is therefore a practical way to shadow it for normal
PATH lookups.

The important exception found during testing:

```text
$ git config --global --get-regexp '^credential'
credential.https://github.com.helper
credential.https://github.com.helper !/usr/bin/gh auth git-credential
credential.https://gist.github.com.helper
credential.https://gist.github.com.helper !/usr/bin/gh auth git-credential
```

`gh auth setup-git` stores an absolute path to the real gh. Git will keep
calling `/usr/bin/gh` even after a `gh` shim is placed earlier in PATH. The
installer must rewrite the git helper configuration, not just install the shim.

## 2. gh's credential helper honors GH_TOKEN

gh 2.102 prints an installation-token style credential when `GH_TOKEN` is set:

```text
$ printf 'protocol=https\nhost=github.com\n\n' | GH_TOKEN=fake_token_123 gh auth git-credential get
protocol=https
host=github.com
username=x-access-token
password=fake_token_123
```

Without an environment token it printed the user's stored PAT instead. This
means the wrapper can either delegate credential generation to gh with
`GH_TOKEN` set, or implement the helper directly. Direct implementation is
preferred so the wrapper can mark short-lived credentials as ephemeral.

## 3. gh environment token precedence

From `gh help environment`:

- `GH_TOKEN`, `GITHUB_TOKEN`: used for github.com and subdomains of `ghe.com`.
- `GH_ENTERPRISE_TOKEN`, `GITHUB_ENTERPRISE_TOKEN`: used for GitHub Enterprise
  Server hosts.
- `GH_HOST`: overrides the hostname only when the target cannot be inferred
  from the repository or arguments.

Also verified:

```text
$ GH_TOKEN=fake_token_123 gh auth token
fake_token_123
```

`gh auth status` validates an environment token and reports it as `(GH_TOKEN)`.
It uses the GraphQL `viewer` query for the login name and an HTTP request to the
API root for scope headers. Installation tokens can use GraphQL and the root
endpoint, so `gh auth status` should work, showing the app bot as the account.

## 4. gh itself knows about expiring credentials

Modern gh's credential helper implements the git 2.46+ `authtype` capability:

- Reads `capability[]=authtype` from the request.
- Returns `capability[]=authtype`, `authtype=Basic`, base64 `credential=...`,
  and `ephemeral=1` for refreshable credentials.
- `ephemeral=1` tells caching helpers not to persist a token that will go stale.

The wrapper's installation token is always short lived, so its helper should do
the same. For older git, it should return ordinary `username`/`password` fields
and a `password_expiry_utc` value.

gh treats `store` and `erase` as no-ops so git cannot log the user out or
persist a second copy. The wrapper should match that behavior.

## 5. GitHub App JWT and installation token

From GitHub Docs:

- JWT algorithm: RS256.
- Claims: `iat` (recommended 60 seconds in the past), `exp` (no more than 10
  minutes in the future), `iss` (application ID or client ID). Client ID is the
  documented recommendation, but application ID is accepted.
- Token request: `POST /app/installations/{installation_id}/access_tokens`
  with `Authorization: Bearer <JWT>`.
- Installation tokens expire after one hour.
- The request body may include `repositories` or `repository_ids` (up to 500)
  and a `permissions` object to downscope the token.
- Many REST endpoints do not accept installation tokens. GraphQL is supported
  when the app has the required permissions.
- HTTP Git access uses the installation token as the password, with
  `x-access-token` as the username.

## 6. Installation token format is changing

GitHub announced a staged rollout starting 2026-04-27 of a stateless
`ghs_APPID_JWT` token format. Code must not assume a fixed length or parse the
token. The wrapper should only check that the value is non-empty and starts with
`ghs_` for display purposes, never for validation.

## 7. Private keys

- GitHub App PEM downloads are PKCS#1 `RSA PRIVATE KEY` format.
- Keys do not expire. An app can have up to 25 active keys for rotation.
- GitHub's own guidance calls the private key the single most valuable secret
  for an app and recommends a key vault or sign-only service.
- There is a public fingerprint procedure:
  `openssl rsa -in key.pem -pubout -outform DER | openssl sha256 -binary | openssl base64`

The wrapper should support PKCS#1 and PKCS#8 PEM, check key size, and display the
SHA-256 public-key fingerprint at login so the user can compare it with the
GitHub UI.

## 8. Bot identity limitations

An installation token acts as the app installation, not as a human user. Things
that need a user identity or user-owned resources will not work, for example:

- gists (GitHub Apps cannot access gists)
- codespaces
- copilot
- managing the user's SSH or GPG keys
- `gh api user`
- forks or repositories created under a personal account

Repository operations, pull requests, issues, actions, releases, and API calls
covered by the app's permissions should work.

GitHub's docs say some REST endpoints do not accept installation tokens, so
compatibility is permission and endpoint dependent. The design should surface
clear 403 errors and document the limitation instead of pretending to be a PAT.

## 9. Storage conventions

gh uses `~/.config/gh` on Linux and macOS, and stores `hosts.yml` with mode
0600. The wrapper should follow XDG on Unix rather than inventing a dot-directory
in the home directory.

## 10. Rust crypto and cross-compile proof

A scratch crate proved the following on this host:

- Parse a PKCS#1 GitHub-style PEM with the `rsa` crate.
- Convert it to PKCS#8 DER.
- Sign a JWT signing input with `ring` using `RSA_PKCS1_SHA256`.
- Verify the signature with `openssl dgst -sha256 -verify`: `Verified OK`.

`ring` is attractive because it is already needed by rustls and has good musl
support. `ureq` 3 uses `ring` as its default rustls provider and is a blocking
HTTP client, avoiding tokio. `reqwest` 0.13's default rustls feature uses
`aws-lc-rs`, which complicates static musl cross builds.

Decision: use `ring` for RSA signing, `rsa` only for PEM parsing and PKCS#1 to
PKCS#8 conversion, and `ureq` 3 with rustls/ring for HTTP. Implement JWT
encoding directly, which is small and auditable.

## 11. Additional real-interface checks

The full evidence is in `docs/validation.md`. The most important results:

- A temporary `gh` shim that exports `GH_TOKEN` and `exec`s `/usr/bin/gh`
  preserved auth token output, version output, help, and exit codes.
- A static Rust binary using `CommandExt::exec` reproduced the same behavior
  against the real gh.
- Real git with the real gh helper returned `x-access-token` and the env token.
- An absolute git helper bypassed a PATH shim. A relative helper was resolved
  through PATH. This proves gh-app must rewrite the helper path.
- Git's real HTTP transport sent `capability[]=authtype` and accepted an opaque
  `credential=` response, producing `Authorization: Basic ...`.
- `git credential fill` did not send the capability. A helper that emitted the
  authtype form unconditionally lost the password. The helper must branch.
- `git credential approve` calls the helper as `store`, and `reject` calls it
  as `erase`, both with credential fields on stdin.
- A blank host-level helper value severed a global helper chain in real git.
- `ephemeral=1` prevented `git-credential-cache` from serving the credential on
  a second real HTTP run. Without it, the cache served the credential.
- `GH_TOKEN` takes precedence over `GITHUB_TOKEN`, is not used for GHES, and is
  used for `*.ghe.com`. `GH_ENTERPRISE_TOKEN` is required for GHES.
- gh configures a gist helper for `*.ghe.com` but not for GHES.
- The selected ureq 3 plus rustls plus ring dependency stack compiled, and did
  not pull `aws-lc-rs`.
- Static musl builds were demonstrated with scratch-only Rust targets and musl
  GCC toolchains: x86_64 built and executed, aarch64 built and executed under
  qemu-aarch64. CI must still build the actual gh-app source.
- A static x86_64 JWT plus HTTP probe ran against the mock endpoint. A static
  aarch64 probe also ran under qemu-aarch64.
- The real GitHub token endpoint returned 401 `A JSON web token could not be
  decoded` for a fake app ID, which is expected because no public key exists for
  that app. Real app acceptance remains blocked.
- A static key-check prototype rejected 0644, symlinks, and world-writable
  parents on both x86_64 and aarch64.
- A static token-cache prototype ran eight concurrent processes without lost
  updates and produced 0600 cache and lock files. Standard `File::lock` replaced
  the planned `fs4` dependency.
- A static 417 KiB multi-call prototype dispatched correctly through symlinks
  named `gh`, `gh-app`, and `git-credential-gh-app`.
- A probe using the optional `keyring` 4.2 feature compiled static at 4.6 MiB
  but reported no default store on the headless host, confirming file fallback
  is required.
- `ureq` honored proxy environment variables and tunneled through CONNECT.
- A custom CA root was accepted while the same local certificate failed against
  the bundled webpki roots, validating the enterprise CA path.

## 12. Sources

- gh credential helper source:
  https://github.com/cli/cli/blob/trunk/pkg/cmd/auth/gitcredential/helper.go
- gh setup-git source:
  https://github.com/cli/cli/blob/trunk/pkg/cmd/auth/setupgit/setupgit.go
- gh helper config source:
  https://github.com/cli/cli/blob/trunk/pkg/cmd/auth/shared/gitcredentials/helper_config.go
- gh host mapping source:
  https://github.com/cli/cli/blob/trunk/internal/ghinstance/host.go
- GitHub App installation auth:
  https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-as-a-github-app-installation
- Generating installation tokens:
  https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app
- Generating a JWT:
  https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-json-web-token-jwt-for-a-github-app
- Managing private keys:
  https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/managing-private-keys-for-github-apps
- GitHub App rate limits:
  https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/rate-limits-for-github-apps
- git credential protocol:
  https://git-scm.com/docs/git-credential
