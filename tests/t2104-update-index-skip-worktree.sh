#!/bin/sh
#
# Ported from git/t/t2104-update-index-skip-worktree.sh
# Tests basic skip-worktree / no-skip-worktree flag setting.
# Note: --show-index-version and ls-files -t status prefix tests
# are omitted (not implemented in grit).

test_description='skip-worktree bit test'

. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_success 'setup' '
	mkdir sub &&
	touch ./1 ./2 sub/1 sub/2 &&
	git add 1 2 sub/1 sub/2
'

test_expect_success 'update-index --skip-worktree' '
	git update-index --skip-worktree 1 sub/1
'

test_expect_success 'update-index --no-skip-worktree' '
	git update-index --no-skip-worktree 1 sub/1
'

test_done
