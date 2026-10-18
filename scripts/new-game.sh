#!/usr/bin/env bash
# Turn a copy of this template into a new game.
#
#   scripts/new-game.sh <name> <Title> [destination]
#
#   name   crate/package/bin slug: orb-dash        (also the underscore form)
#   Title  human-readable title: "Orb Dash"
#   dest   default: ../../<name>  (a sibling of the template in Games/repame)
#
# The copy keeps the template's relative path back to Documents/Repos, so the
# [patch.crates-io] escape hatch at the bottom of Cargo.toml keeps working.
set -euo pipefail

name="${1:-}"
title="${2:-}"
if [[ -z "$name" || -z "$title" ]]; then
    echo "usage: $0 <crate-slug> <Game Title> [dest]" >&2
    exit 1
fi
underscore="${name//-/_}"
template_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dest="${3:-$(dirname "$template_dir")/$name}"

if [[ -e "$dest" ]]; then
    echo "destination already exists: $dest" >&2
    exit 1
fi

# Copy without build output: the template's own target/ is gigabytes of
# dep artifacts, and a new game must start from a clean tree anyway.
mkdir -p "$dest"
tar -C "$template_dir" --exclude=./target --exclude=./dist -cf - . | tar -C "$dest" -xf -
cd "$dest"

# A single-word slug makes the lib and bin share an output filename, and cargo
# then refuses to tell trunk which artifact to ship. Give the lib a suffix in
# that case; a hyphenated slug already separates them.
if [[ "$underscore" == "$name" ]]; then
    underscore="${name}_lib"
fi

# Every placeholder occurrence: crate, bin, lib, log prefixes, cloud key,
# shortcut ids, window title, page title.
find . -type f \
    \( -name "*.rs" -o -name "*.toml" -o -name "*.html" -o -name "*.json" -o -name "*.md" \) \
    -exec sed -i "s/web-game/$name/g; s/web_game/$underscore/g; s/Web Game/$title/g" {} +

echo "created $dest"
echo "next:"
echo "  cd $dest"
echo "  cargo run                      # desktop"
echo "  cargo fmt && cargo clippy      # keep it clean"
echo "  trunk build --release          # dist/ for the portal"
