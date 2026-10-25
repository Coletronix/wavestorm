#!/usr/bin/env bash
# Writes the GitHub release notes for a tag to stdout, from the git history:
#   1. the annotated tag's message (everything after its first line) as the highlights,
#   2. every commit since the previous release tag, newest first,
#   3. the downloads and a link to the full diff.
# Usage: .github/release-notes.sh vX.Y.Z [owner/repo]
set -euo pipefail

tag="$1"
repo="${2:-${GITHUB_REPOSITORY:-Coletronix/wavestorm}}"
version="${tag#v}"
url="https://github.com/$repo"
prev="$(git describe --tags --abbrev=0 --match 'v[0-9]*' "$tag^" 2>/dev/null || true)"

highlights="$(git tag -l --format='%(contents:body)' "$tag" | sed -e '/-----BEGIN PGP SIGNATURE-----/,$d')"
if [ -n "${highlights//[[:space:]]/}" ]; then
  printf '%s\n\n' "$highlights"
fi

if [ -n "$prev" ]; then
  range="$prev..$tag"
  echo "## Changes since $prev"
else
  range="$tag"
  echo "## Changes"
fi
echo
git log --no-merges --format="%H%x09%s" "$range" |
  while IFS=$'\t' read -r hash subject; do
    case "$subject" in "Release v"*) continue ;; esac
    echo "- $subject ([\`${hash:0:7}\`]($url/commit/$hash))"
  done
echo

cat <<EOF
## Downloads

| Platform | File |
|---|---|
| Linux x86_64, any distribution | \`WAVEStorm-$version-x86_64.AppImage\`: make it executable and run it |
| Debian / Ubuntu | \`wavestorm_$version-1_amd64.deb\`: \`sudo apt install ./wavestorm_$version-1_amd64.deb\` |
| Linux x86_64 | \`wavestorm-$version-x86_64-linux.tar.gz\` (binary, desktop entry, icon) or the bare \`wavestorm-$version-x86_64-linux\` binary |
| Windows x86_64 | \`wavestorm-$version-x86_64-windows.zip\` |
| macOS (Apple silicon and Intel) | \`WAVEStorm-$version-macos-universal.dmg\` |

The Windows and macOS builds are not code-signed. On Windows choose *More info > Run anyway*;
on macOS right-click the app and choose *Open* the first time. Verify downloads with
\`sha256sum -c SHA256SUMS.txt --ignore-missing\`.
EOF

if [ -n "$prev" ]; then
  echo
  echo "**Full diff**: $url/compare/$prev...$tag"
fi
