#!/usr/bin/env bash
# Upstream t6110-rev-list-sparse.sh history (first-parent + boundary range).
set -euo pipefail
wt="$1"
cd "$wt"
git config user.email "t6110@test.local"
git config user.name "t6110"
git commit --allow-empty -qm A
git commit --allow-empty -qm B
git commit --allow-empty -qm C
git branch side HEAD~1
git checkout side
git commit --allow-empty -qm D
git commit --allow-empty -qm E
git merge -m M main
git checkout side
