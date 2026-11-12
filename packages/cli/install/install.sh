#!/bin/sh
# Installs the tsgpu CLI on macOS or Linux into Bun's bin directory:
#
#   curl -fsSL https://raw.githubusercontent.com/ByteTheDev/TSGPUI/main/packages/cli/install/install.sh | sh
#
# Environment variables:
#   TSGPU_VERSION  install this release tag, for example 0.3.0 (default: the latest release)
#   TSGPU_URL      install the archive at this URL instead (a mirror, or a local test server)
#   BUN_INSTALL    Bun's home directory (default: ~/.bun)
#
# The archive is unpacked to $BUN_INSTALL/install/tsgpu and a launcher is written to
# $BUN_INSTALL/bin/tsgpu. The launcher runs the CLI with Bun when present, otherwise Node.js 20+.

set -eu

REPO_URL='https://github.com/ByteTheDev/TSGPUI'
ARCHIVE='tsgpu-cli.tgz'

say() { printf 'tsgpu: %s\n' "$*"; }
fail() { printf 'tsgpu: error: %s\n' "$*" >&2; exit 1; }

require_runtime() {
  if command -v bun >/dev/null 2>&1; then return 0; fi
  if command -v node >/dev/null 2>&1 && node -e 'process.exit(Number(process.versions.node.split(".")[0]) < 20 ? 1 : 0)'; then
    return 0
  fi
  fail 'tsgpu needs Bun or Node.js 20 or newer on PATH'
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

main() {
  case "$(uname -s)" in
    Darwin|Linux) ;;
    *) fail "unsupported OS '$(uname -s)'; on Windows run install.ps1 instead" ;;
  esac
  command -v curl >/dev/null 2>&1 || fail 'curl is required'
  command -v tar >/dev/null 2>&1 || fail 'tar is required'
  require_runtime

  if [ -n "${TSGPU_URL:-}" ]; then
    url="$TSGPU_URL"
  elif [ -n "${TSGPU_VERSION:-}" ]; then
    url="$REPO_URL/releases/download/v${TSGPU_VERSION#v}/$ARCHIVE"
  else
    url="$REPO_URL/releases/latest/download/$ARCHIVE"
  fi

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  say "downloading $url"
  curl -fsSL "$url" -o "$tmp/$ARCHIVE" || fail "download failed: $url"
  curl -fsSL "$url.sha256" -o "$tmp/$ARCHIVE.sha256" || fail "checksum download failed: $url.sha256"

  expected="$(awk '{print $1}' "$tmp/$ARCHIVE.sha256")"
  actual="$(sha256_of "$tmp/$ARCHIVE")"
  if [ "$actual" != "$expected" ]; then
    fail "checksum mismatch for $ARCHIVE (expected $expected, got $actual); nothing was installed"
  fi

  mkdir -p "$tmp/extract"
  tar -xzf "$tmp/$ARCHIVE" -C "$tmp/extract" --strip-components 1 || fail 'could not unpack the archive'
  [ -f "$tmp/extract/dist/tsgpu.mjs" ] || fail 'archive does not contain dist/tsgpu.mjs'

  bun_home="${BUN_INSTALL:-$HOME/.bun}"
  pkg_dir="$bun_home/install/tsgpu"
  bin_dir="$bun_home/bin"
  mkdir -p "$bun_home/install" "$bin_dir"
  rm -rf "$pkg_dir"
  mv "$tmp/extract" "$pkg_dir"

  # The launcher finds the CLI relative to itself, so the install can move with $BUN_INSTALL.
  cat > "$bin_dir/tsgpu.tmp" <<'LAUNCHER'
#!/bin/sh
# tsgpu launcher written by the TS-GPU installer. Run install.sh again to update it.
here="$(cd "$(dirname "$0")" && pwd)"
cli="$here/../install/tsgpu/dist/tsgpu.mjs"
if command -v bun >/dev/null 2>&1; then exec bun "$cli" "$@"; fi
exec node "$cli" "$@"
LAUNCHER
  chmod 755 "$bin_dir/tsgpu.tmp"
  mv "$bin_dir/tsgpu.tmp" "$bin_dir/tsgpu"

  say "installed $("$bin_dir/tsgpu" --version) to $pkg_dir"
  case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *)
      say "$bin_dir is not on your PATH. Add this line to your shell profile, then open a new terminal:"
      printf '  export PATH="%s:$PATH"\n' "$bin_dir"
      ;;
  esac
  say "next: run 'tsgpu doctor' to check the toolchain"
}

main "$@"
