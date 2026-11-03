#!/usr/bin/env bash
# installation.sh - install or update gh-app, and install gh only when missing.
set -euo pipefail

REPO="steezypunk/gh-app"
GH_REPO="cli/cli"

TAG_OVERRIDE=""
EXPLICIT_VERSION=0
INSTALL_DIR="${GH_APP_INSTALL_DIR:-}"
INSTALL_GH=1
FORCE=0
DRY_RUN=0

usage() {
  cat <<'EOF'
Usage: installation.sh [options]

Install or update gh-app. The GitHub CLI is installed only when it is missing.

Options:
  -h, --help             Show this help
  -v, --version VERSION  Install a specific gh-app version (default: latest)
  -d, --dir DIR          Install directory
                         (default: ~/.local/bin, or /usr/local/bin as root)
      --no-gh            Do not install gh when it is missing
  -f, --force            Reinstall even when already up to date
  -n, --dry-run          Show what would happen without changing anything

Environment:
  GH_APP_INSTALL_DIR     Same as --dir

Examples:
  ./installation.sh
  ./installation.sh --version v0.1.1
  ./installation.sh --dir "$HOME/bin"
EOF
}

log() {
  printf '==> %s\n' "$*"
}

warn() {
  printf 'warning: %s\n' "$*" >&2
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    -v|--version)
      [ "$#" -ge 2 ] || die "--version requires a value"
      TAG_OVERRIDE="$2"
      EXPLICIT_VERSION=1
      shift 2
      ;;
    -d|--dir)
      [ "$#" -ge 2 ] || die "--dir requires a value"
      INSTALL_DIR="$2"
      shift 2
      ;;
    --no-gh)
      INSTALL_GH=0
      shift
      ;;
    -f|--force)
      FORCE=1
      shift
      ;;
    -n|--dry-run)
      DRY_RUN=1
      shift
      ;;
    *)
      die "unknown argument: $1 (try --help)"
      ;;
  esac
done

[ "$(uname -s)" = "Linux" ] || die "gh-app supports Linux only"

case "$(uname -m)" in
  x86_64|amd64)
    ARCH="amd64"
    ;;
  aarch64|arm64)
    ARCH="arm64"
    ;;
  *)
    die "unsupported architecture: $(uname -m)"
    ;;
esac

if [ -z "$INSTALL_DIR" ]; then
  if [ "$(id -u)" -eq 0 ]; then
    INSTALL_DIR="/usr/local/bin"
  else
    [ -n "${HOME:-}" ] || die "HOME is not set; pass --dir"
    INSTALL_DIR="${HOME}/.local/bin"
  fi
fi

command -v tar >/dev/null 2>&1 || die "required command not found: tar"
command -v sha256sum >/dev/null 2>&1 || die "required command not found: sha256sum"
command -v install >/dev/null 2>&1 || die "required command not found: install"

if command -v curl >/dev/null 2>&1; then
  DOWNLOADER="curl"
elif command -v wget >/dev/null 2>&1; then
  DOWNLOADER="wget"
else
  die "curl or wget is required"
fi

fetch() {
  local url="$1"
  local dest="$2"
  if [ "$DOWNLOADER" = "curl" ]; then
    curl -fsSL --retry 3 --connect-timeout 15 -o "$dest" "$url"
  else
    wget -qO "$dest" "$url"
  fi
}

latest_tag() {
  local repo="$1"
  local tag=""
  if [ "$DOWNLOADER" = "curl" ]; then
    tag=$(curl -fsSL -o /dev/null -w '%{url_effective}' \
      "https://github.com/${repo}/releases/latest")
  else
    tag=$(wget -qO- "https://api.github.com/repos/${repo}/releases/latest" \
      | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' \
      | head -n1)
  fi
  tag="${tag##*/}"
  [ -n "$tag" ] || die "could not determine the latest release of ${repo}"
  printf '%s\n' "$tag"
}

normalize_tag() {
  case "$1" in
    v*) printf '%s\n' "$1" ;;
    *) printf 'v%s\n' "$1" ;;
  esac
}

# ver_gt A B returns success when A is newer than B.
ver_gt() {
  local a="${1#v}"
  local b="${2#v}"
  a="${a%%-*}"
  b="${b%%-*}"
  local IFS=.
  local -a av bv
  read -r -a av <<< "$a"
  read -r -a bv <<< "$b"
  local i ai bi
  for i in 0 1 2; do
    ai="${av[i]:-0}"
    bi="${bv[i]:-0}"
    if (( 10#$ai > 10#$bi )); then
      return 0
    fi
    if (( 10#$ai < 10#$bi )); then
      return 1
    fi
  done
  return 1
}

installed_version() {
  local bin="${INSTALL_DIR}/gh-app"
  [ -x "$bin" ] || return 1
  "$bin" version 2>/dev/null | awk 'NR == 1 { print $2 }'
}

install_gh_app() {
  local tag="$1"
  local ver="${tag#v}"
  local asset="gh-app-${tag}-linux-${ARCH}.tar.gz"
  local base="https://github.com/${REPO}/releases/download/${tag}"

  if [ "$DRY_RUN" -eq 1 ]; then
    log "would install gh-app ${ver} to ${INSTALL_DIR}/gh-app"
    return 0
  fi

  log "downloading gh-app ${ver} (linux-${ARCH})"
  fetch "${base}/${asset}" "${WORKDIR}/${asset}"
  fetch "${base}/${asset}.sha256" "${WORKDIR}/${asset}.sha256"
  ( cd "$WORKDIR" && sha256sum -c "${asset}.sha256" >/dev/null ) \
    || die "checksum verification failed for ${asset}"

  tar -xzf "${WORKDIR}/${asset}" -C "$WORKDIR"
  [ -f "${WORKDIR}/gh-app" ] || die "archive did not contain a gh-app binary"

  install -m 0755 "${WORKDIR}/gh-app" "${INSTALL_DIR}/.gh-app.new"
  mv -f "${INSTALL_DIR}/.gh-app.new" "${INSTALL_DIR}/gh-app"
}

install_gh() {
  local tag ver asset checksums base src
  tag=$(latest_tag "$GH_REPO")
  ver="${tag#v}"
  asset="gh_${ver}_linux_${ARCH}.tar.gz"
  checksums="gh_${ver}_checksums.txt"
  base="https://github.com/${GH_REPO}/releases/download/${tag}"

  if [ "$DRY_RUN" -eq 1 ]; then
    log "would install GitHub CLI ${ver} to ${INSTALL_DIR}/gh"
    return 0
  fi

  log "installing GitHub CLI ${ver} (linux-${ARCH})"
  fetch "${base}/${asset}" "${WORKDIR}/${asset}"
  fetch "${base}/${checksums}" "${WORKDIR}/${checksums}"
  awk -v a="$asset" '$2 == a || $2 == "*" a { print }' \
    "${WORKDIR}/${checksums}" > "${WORKDIR}/gh.sha256"
  [ -s "${WORKDIR}/gh.sha256" ] || die "could not find a checksum for ${asset}"
  ( cd "$WORKDIR" && sha256sum -c gh.sha256 >/dev/null ) \
    || die "checksum verification failed for ${asset}"

  tar -xzf "${WORKDIR}/${asset}" -C "$WORKDIR"
  src="${WORKDIR}/gh_${ver}_linux_${ARCH}/bin/gh"
  [ -f "$src" ] || die "archive did not contain a gh binary"

  install -m 0755 "$src" "${INSTALL_DIR}/.gh.new"
  mv -f "${INSTALL_DIR}/.gh.new" "${INSTALL_DIR}/gh"
}

if [ "$DRY_RUN" -eq 0 ]; then
  mkdir -p "$INSTALL_DIR" 2>/dev/null \
    || die "cannot create install directory: ${INSTALL_DIR}"
  [ -w "$INSTALL_DIR" ] \
    || die "no write permission for ${INSTALL_DIR}; re-run as root or pass --dir"
fi

WORKDIR=$(mktemp -d "${TMPDIR:-/tmp}/gh-app-install.XXXXXX")
cleanup() {
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

if [ "$EXPLICIT_VERSION" -eq 1 ]; then
  target_tag=$(normalize_tag "$TAG_OVERRIDE")
else
  target_tag=$(latest_tag "$REPO")
fi

current=$(installed_version || true)

install_app=1
if [ -n "$current" ] && [ "$FORCE" -eq 0 ]; then
  if [ "${current#v}" = "${target_tag#v}" ]; then
    log "gh-app ${current} is already installed at ${INSTALL_DIR}/gh-app"
    install_app=0
  elif [ "$EXPLICIT_VERSION" -eq 0 ] && ver_gt "$current" "$target_tag"; then
    log "installed gh-app ${current} is newer than ${target_tag#v}, keeping it"
    install_app=0
  else
    log "updating gh-app ${current} -> ${target_tag#v}"
  fi
elif [ -n "$current" ]; then
  log "reinstalling gh-app ${target_tag#v}"
else
  log "installing gh-app ${target_tag#v}"
fi

if [ "$install_app" -eq 1 ]; then
  install_gh_app "$target_tag"
fi

if [ "$INSTALL_GH" -eq 1 ]; then
  if command -v gh >/dev/null 2>&1; then
    log "GitHub CLI already installed: $(command -v gh)"
  else
    install_gh
  fi
else
  if ! command -v gh >/dev/null 2>&1; then
    warn "gh is not installed; gh-app passthrough needs it."
    warn "Omit --no-gh or install gh from https://github.com/cli/cli"
  fi
  log "skipping GitHub CLI installation (--no-gh)"
fi

if [ "$DRY_RUN" -eq 1 ]; then
  log "dry run complete, nothing changed"
  exit 0
fi

installed=$("$INSTALL_DIR/gh-app" version 2>/dev/null | awk 'NR == 1 { print $2 }') \
  || die "installed gh-app did not run"
[ -n "$installed" ] || die "could not read the installed gh-app version"

on_path=0
case ":${PATH:-}:" in
  *":${INSTALL_DIR}:"*)
    on_path=1
    ;;
  *)
    on_path=0
    ;;
esac

printf '\n'
log "gh-app ${installed} installed at ${INSTALL_DIR}/gh-app"

if [ "$on_path" -eq 0 ]; then
  warn "${INSTALL_DIR} is not on your PATH"
  printf '    export PATH="%s:$PATH"\n' "$INSTALL_DIR"
fi

cat <<EOF

Use gh-app for GitHub CLI commands:
  gh-app pr list
  gh-app pr create --title "..." --body "..."
  gh-app issue list
  gh-app api repos/steezypunk/gh-app

This installer did not install or change a gh shim. Use gh-app explicitly.
If you want plain gh to use the app token, run:
  gh-app setup

For credentials, see the Quick start section in README.md.
EOF
