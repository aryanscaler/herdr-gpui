#!/bin/sh
# Installs the latest stable Sheprd release on Linux through the system
# package manager (APT, DNF, zypper or pacman), served at
# https://sheprd.org/install.sh:
#
#   curl -fsSL https://sheprd.org/install.sh | sh
#
# It downloads the package for this machine from GitHub Releases, checks it
# against the release's published SHA-256, and installs it with sudo. The package
# includes upstream herdr-core but never replaces a personal Herdr daemon.
# The package manager owns the result (package name herdr-gpui is retained for
# compatibility), so later updates come from the app's App Updates panel.
set -eu

repo=sheprd-labs/sheprd

fail() {
  printf 'sheprd install: %s\n' "$1" >&2
  exit 1
}

case $(uname -s) in
  Linux) ;;
  Darwin) fail 'on macOS, run: brew install sheprd-labs/tap/sheprd' ;;
  *) fail "unsupported system $(uname -s); see https://github.com/$repo#install" ;;
esac

case $(uname -m) in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *) fail "no Linux package for $(uname -m); see https://github.com/$repo#from-source" ;;
esac

# The package manager on PATH, not a distribution name, decides the format.
if command -v apt-get >/dev/null 2>&1; then
  manager=apt
elif command -v dnf >/dev/null 2>&1; then
  manager=dnf
elif command -v zypper >/dev/null 2>&1; then
  manager=zypper
elif command -v pacman >/dev/null 2>&1; then
  manager=pacman
else
  fail "no APT, DNF, zypper or pacman found; download a tarball from https://github.com/$repo/releases or use Nix: nix run github:$repo"
fi

case $manager in
  apt) ext=deb ;;
  dnf | zypper) ext=rpm ;;
  pacman) ext=pkg.tar.zst ;;
esac

command -v curl >/dev/null 2>&1 || fail 'curl is required'
command -v sha256sum >/dev/null 2>&1 || fail 'sha256sum is required'

# The latest release is always the newest stable one; betas are prereleases.
latest=$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest") ||
  fail 'could not reach GitHub Releases'
version=${latest##*/v}
case $version in
  [1-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9].[1-9]*) ;;
  *) fail "unexpected latest release URL: $latest" ;;
esac

file=Sheprd-$version-$arch-unknown-linux-gnu.$ext
base=https://github.com/$repo/releases/download/v$version
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

printf 'Downloading %s\n' "$file"
curl -fsSL --proto '=https' -o "$tmp/$file" "$base/$file" || fail "download failed: $base/$file"
curl -fsSL --proto '=https' -o "$tmp/$file.sha256" "$base/$file.sha256" || fail "download failed: $base/$file.sha256"
(cd "$tmp" && sha256sum -c --status "$file.sha256") || fail "checksum mismatch for $file"

sudo=
if [ "$(id -u)" -ne 0 ]; then
  command -v sudo >/dev/null 2>&1 || fail 'run as root or install sudo'
  sudo=sudo
fi

# APT reads the package as its sandbox user, which cannot enter the private
# temporary directory; let it read the verified file there.
chmod 755 "$tmp"
chmod 644 "$tmp/$file"

case $manager in
  apt) $sudo apt-get install -y "$tmp/$file" ;;
  dnf) $sudo dnf install -y "$tmp/$file" ;;
  # The release RPM is unsigned, which zypper refuses even in non-interactive
  # mode; the verified checksum above is what authenticates these bytes.
  zypper) $sudo zypper install -y --allow-unsigned-rpm "$tmp/$file" ;;
  pacman) $sudo pacman -U --noconfirm "$tmp/$file" ;;
esac

printf 'Installed Sheprd %s. Start it from your applications menu or run: sheprd\n' "$version"
