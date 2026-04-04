#!/bin/sh
# Ported from git/t/t9826-git-p4-keep-empty-commits.sh
# Clone repositories and keep empty commits

test_description='Clone repositories and keep empty commits'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-p4 (requires Perforce) — not yet ported' '
	false
'

test_done
