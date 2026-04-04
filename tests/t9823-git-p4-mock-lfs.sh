#!/bin/sh
# Ported from git/t/t9823-git-p4-mock-lfs.sh
# Clone repositories and store files in Mock LFS

test_description='Clone repositories and store files in Mock LFS'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-p4 (requires Perforce) — not yet ported' '
	false
'

test_done
