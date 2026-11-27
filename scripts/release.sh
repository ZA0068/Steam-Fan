#!/usr/bin/env bash
# Make a release: bump the version, commit, tag, push. GitHub Actions does the rest:
# the apps for Linux, Windows and macOS go into a GitHub release, and the server image
# is published as ghcr.io/za0068/steam-fan-server:<version> and :latest.
#
#   scripts/release.sh patch     0.1.0 -> 0.1.1   (fixes)
#   scripts/release.sh minor     0.1.0 -> 0.2.0   (new features)
#   scripts/release.sh major     0.1.0 -> 1.0.0
#   scripts/release.sh 0.3.0     exactly this
set -euo pipefail
cd "$(dirname "$0")/.."

bump="${1:-patch}"
cur=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
IFS=. read -r ma mi pa <<<"$cur"
case "$bump" in
    patch) new="$ma.$mi.$((pa + 1))" ;;
    minor) new="$ma.$((mi + 1)).0" ;;
    major) new="$((ma + 1)).0.0" ;;
    [0-9]*.[0-9]*.[0-9]*) new="$bump" ;;
    *) echo "usage: $0 patch|minor|major|X.Y.Z"; exit 1 ;;
esac

if [ -n "$(git status --porcelain)" ]; then
    echo "Commit or stash your changes first:"; git status --short; exit 1
fi
git pull --ff-only
if git rev-parse "v$new" >/dev/null 2>&1; then
    echo "v$new exists already"; exit 1
fi

echo "Steam-Fan $cur -> $new"
cargo test --workspace --quiet
# the workspace version (every crate uses it), and Cargo.lock to match
sed -i.bak "0,/^version = \"$cur\"/s//version = \"$new\"/" Cargo.toml && rm -f Cargo.toml.bak
cargo update --workspace --quiet
git commit -am "Steam-Fan $new"
git tag -a "v$new" -m "Steam-Fan $new"
git push --follow-tags
echo
echo "Pushed v$new. Builds: https://github.com/ZA0068/Steam-Fan/actions"
echo "The release appears at https://github.com/ZA0068/Steam-Fan/releases when they're done (~15 min)."
