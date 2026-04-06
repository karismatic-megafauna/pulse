#!/usr/bin/env bash
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# ── Bird names (A–Z) ───────────────────────────────────────────────────────
BIRD_NAMES=(
    "Ambitious Albatross"       # 0.1.0
    "Boisterous Budgie"         # 0.2.0
    "Cunning Cormorant"         # 0.3.0
    "Daring Dove"               # 0.4.0
    "Eager Eagle"               # 0.5.0
    "Fearless Falcon"           # 0.6.0
    "Gallant Goldfinch"         # 0.7.0
    "Hearty Heron"              # 0.8.0
    "Intrepid Ibis"             # 0.9.0
    "Jovial Jay"                # 0.10.0
    "Keen Kingfisher"           # 0.11.0
    "Lively Lark"               # 0.12.0
    "Mighty Magpie"             # 0.13.0
    "Noble Nightingale"         # 0.14.0
    "Outgoing Osprey"           # 0.15.0
    "Plucky Pelican"            # 0.16.0
    "Quick Quail"               # 0.17.0
    "Resolute Robin"            # 0.18.0
    "Spirited Sparrow"          # 0.19.0
    "Tenacious Tern"            # 0.20.0
    "Undaunted Umbrellabird"    # 0.21.0
    "Valiant Vulture"           # 0.22.0
    "Witty Woodpecker"          # 0.23.0
    "Xenial Xenops"             # 0.24.0
    "Yearning Yellowhammer"     # 0.25.0
    "Zealous Zebrafinch"        # 0.26.0
)

# ── Safety check ────────────────────────────────────────────────────────────
if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "WARNING: You have uncommitted changes."
    echo ""
    git status --short
    echo ""
    read -rp "Continue anyway? [y/N] " ans
    [[ "$ans" =~ ^[Yy]$ ]] || { echo "Aborted."; exit 1; }
fi

# ── Parse current version ──────────────────────────────────────────────────
current_version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml)
current_minor=$(echo "$current_version" | cut -d. -f2)
echo "Current version: v${current_version} — ${BIRD_NAMES[$((current_minor - 1))]}"

# ── Compute next version ───────────────────────────────────────────────────
next_minor=$((current_minor + 1))
if (( next_minor > 26 )); then
    echo "ERROR: Out of bird names! Time to go 1.0."
    exit 1
fi
next_version="0.${next_minor}.0"
next_name="${BIRD_NAMES[$((next_minor - 1))]}"
echo "Next version:    v${next_version} — ${next_name}"
echo ""

# ── Bootstrap: tag current version if no tags exist ─────────────────────────
if ! git tag -l 'v*' | grep -q .; then
    echo "No release tags found. Tagging current HEAD as v${current_version}..."
    git tag "v${current_version}"
fi

last_tag=$(git tag --sort=-v:refname | grep '^v' | head -1)

# ── Show commit history since last release ──────────────────────────────────
echo "=== COMMITS SINCE ${last_tag} ==="
echo ""
git log --oneline --no-merges "${last_tag}..HEAD" || echo "(no commits)"
echo ""
echo "========================================"
echo ""
echo "Review these changes. If running with Claude Code, ask Claude"
echo "to propose README updates based on this history."
echo ""

# ── README review ───────────────────────────────────────────────────────────
read -rp "Open README.md in \$EDITOR? [Y/s to skip] " ans
if [[ ! "$ans" =~ ^[Ss]$ ]]; then
    ${EDITOR:-vim} README.md
fi

# ── Apply version bumps ────────────────────────────────────────────────────
echo ""
echo "Bumping version to v${next_version} — ${next_name}..."

sed -i '' "s/^version = \"${current_version}\"/version = \"${next_version}\"/" Cargo.toml
sed -i '' "s/^const VERSION_NAME: &str = \".*\";/const VERSION_NAME: \&str = \"${next_name}\";/" src/app.rs

echo "  Updated Cargo.toml"
echo "  Updated src/app.rs"

# ── Build & install ─────────────────────────────────────────────────────────
echo ""
echo "Building and installing..."
if ! cargo install --path .; then
    echo "ERROR: Build failed. Reverting version bumps."
    git checkout Cargo.toml src/app.rs
    exit 1
fi

# ── Commit & tag ────────────────────────────────────────────────────────────
echo ""
echo "Committing release..."
git add Cargo.toml Cargo.lock src/app.rs README.md
git commit --no-gpg-sign -m "$(cat <<EOF
release: v${next_version} — ${next_name}
EOF
)"
git tag "v${next_version}"

echo ""
echo "=== Released v${next_version} — ${next_name} ==="
echo ""
echo "Don't forget to push:"
echo "  git push && git push --tags"
