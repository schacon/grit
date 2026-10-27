#!/usr/bin/env bash
# Compare snapshot revisions without changing the source or target path between builds.
set -euo pipefail

if [ "$#" -ne 4 ]; then
    echo "usage: $0 BEFORE AFTER FIXTURE NEW_OUTPUT_DIRECTORY" >&2
    exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
before="$(git -C "$repo_root" rev-parse "$1^{commit}")"
after="$(git -C "$repo_root" rev-parse "$2^{commit}")"
fixture="$(cd "$3" && pwd)"
output="$(realpath -m "$4")"

# Overlaying archives is sufficient for these revisions only when no files disappear.
if [ -n "$(git -C "$repo_root" diff --name-only --diff-filter=D "$before" "$after")" ]; then
    echo "this helper requires revisions with no deleted files" >&2
    exit 2
fi
mkdir "$output"
mkdir "$output/source" "$output/before" "$output/after"
source_dir="$output/source"

git -C "$fixture" rev-parse HEAD > "$output/fixture-head.txt"
git --version > "$output/git-version.txt"
printf '%s\n%s\n' "$before" "$after" > "$output/revisions.txt"

for label in before after; do
    revision="$before"
    if [ "$label" = after ]; then revision="$after"; fi
    git -C "$repo_root" archive "$revision" | tar -x -C "$source_dir"
    # Archives retain commit timestamps. Refresh sources so Cargo cannot reuse the
    # previous revision merely because its build artifacts are newer than both archives.
    find "$source_dir" -path "$source_dir/target" -prune -o -type f -exec touch {} +
    (
        cd "$source_dir"
        rustc --version > "$output/rust-version.txt"
        cargo build --release -p grit-cli -p grit-utils
    ) > "$output/$label-build.log" 2>&1
    cp "$source_dir/target/release/grit" "$source_dir/target/release/grit-bench" "$output/$label/"
done
sha256sum "$output/before/grit" "$output/after/grit" > "$output/binary-hashes.txt"
if cmp -s "$output/before/grit" "$output/after/grit"; then
    echo "identical executables; check the selected revisions and build logs" >&2
    exit 1
fi

cd "$fixture"
hyperfine --shell=none --warmup 20 --runs 300 \
    --export-json "$output/hyperfine-forward.json" \
    "'$output/before/grit' branch --json" "'$output/after/grit' branch --json" \
    "git branch -vv"
hyperfine --shell=none --warmup 20 --runs 300 \
    --export-json "$output/hyperfine-reverse.json" \
    "git branch -vv" "'$output/after/grit' branch --json" \
    "'$output/before/grit' branch --json"
