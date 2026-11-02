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
- `gh-app version`
- `gh-app auth list`
- `gh-app doctor` basic config and key checks

Not implemented yet:

- GitHub App JWT signing and installation-token minting
- `auth login`, `status`, `token`, `refresh`, `logout`, `setup-git`
- real `gh` passthrough and git credential helper
- `setup` and release artifacts

## Build

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Security

Private keys are expected at
`~/.config/gh-app/keys/<host>-<app_id>.pem`, mode 0600, in a directory that is
not group or world writable. Symlinked keys are refused. See
[the design](docs/design.md) for the full security model.

## License

MIT
