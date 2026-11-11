#!/usr/bin/env bash
# Keep the required PR gate running while skipping native jobs for non-code edits.
# Documentation-only images are excluded by name so any new asset is treated
# as a build input until proven otherwise.
# The website workflow only publishes site/, which no build or test reads; the
# zizmor job still audits it on every run.
#
# Prints `code=` for the native jobs, `windows_package=` for the optimized
# Windows packaging and installer check, which is too slow to run for every
# change and covers only what the Windows release artifacts are made from, and
# `website=` for the Python-only sheprd.org and docs checks. A website-only
# change runs those and none of the native jobs.
set -euo pipefail

if [[ "$EVENT" == workflow_dispatch ]] ||
   [[ ! "$BASE" =~ ^[0-9a-f]{40}$ ]] ||
   ! git cat-file -e "$BASE^{commit}" 2>/dev/null; then
  printf 'code=true\nwindows_package=true\nwebsite=true\n'
  exit 0
fi

# PRs use their merge base; pushes compare the complete pushed range.
if [[ "$EVENT" == pull_request ]]; then
  BASE="$(git merge-base "$BASE" "$HEAD_SHA")"
fi

# Prints true when any of the pathspecs changed in the range.
changed() {
  if git diff --quiet "$BASE" "$HEAD_SHA" -- "$@"; then
    printf 'false'
  else
    local status=$?
    # A Git error must fail detection, never silently skip required checks.
    [[ "$status" == 1 ]] || exit "$status"
    printf 'true'
  fi
}

code=$(changed \
  crates assets .cargo scripts .github/workflows \
  Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml .rustfmt.toml \
  clippy.toml .clippy.toml justfile mise.toml cliff.toml \
  ':(exclude)**/*.md' \
  ':(exclude)assets/social-preview-*' \
  ':(exclude)assets/icons/sheprd-icon-badge.*' \
  ':(exclude).github/workflows/site.yml')
windows_package=$(changed \
  scripts/release/package-windows.py scripts/release/windows-installer.py \
  scripts/release/sheprd.iss scripts/release/installer site/install.ps1 \
  crates/sheprd-launcher crates/sheprd-gpui/src/lib.rs crates/sheprd-gpui/src/main.rs \
  crates/sheprd-gpui/build.rs crates/sheprd-gpui/windows_resource.rs \
  assets/icons/Sheprd.ico assets/icons/Sheprd-worktree.ico assets/icons/windows \
  .github/workflows/ci.yml)
# The docs link into READMEs, docs/ and the example config, so any Markdown or
# build input can break a page; the check takes seconds.
website=$(changed website site docs ':(glob)**/*.md' .github/workflows/ci.yml)
[[ "$code" == true ]] && website=true
printf 'code=%s\nwindows_package=%s\nwebsite=%s\n' "$code" "$windows_package" "$website"
