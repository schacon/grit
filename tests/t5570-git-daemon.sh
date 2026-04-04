#!/bin/sh
#
# Upstream: t5570-git-daemon.sh
# Requires git-daemon — stubbed as test_expect_failure.
#

test_description='test fetching over git protocol (DAEMON STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- git daemon not yet available in grit ---

test_expect_failure 'daemon rejects invalid --init-timeout values' '
	false
'

test_expect_failure 'daemon rejects invalid --timeout values' '
	false
'

test_expect_failure 'daemon rejects invalid --max-connections values' '
	false
'

test_expect_failure 'setup repository' '
	false
'

test_expect_failure 'create git-accessible bare repository' '
	false
'

test_expect_failure 'clone via git protocol' '
	false
'

test_expect_failure 'fetch changes via git protocol' '
	false
'

test_expect_failure 'push via git protocol' '
	false
'

test_done
