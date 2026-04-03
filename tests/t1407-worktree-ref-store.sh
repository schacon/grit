#!/bin/sh

test_description='test worktree ref store api (requires test-tool)'

. ./test-lib.sh

# This test requires test-tool ref-store which is not available in grit.

test_expect_failure 'worktree ref-store (needs test-tool)' '
	false
'

test_done
