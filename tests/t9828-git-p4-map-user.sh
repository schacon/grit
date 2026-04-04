#!/bin/sh
# Ported from git/t/t9828-git-p4-map-user.sh
# Clone repositories and map users

test_description='Clone repositories and map users'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-p4 (requires Perforce) — not yet ported' '
	false
'

test_done
