#!/bin/sh
#
# Upstream: t5570-git-daemon.sh
# Requires git-daemon — ported as test_expect_failure stubs.
#

test_description='test fetching over git protocol'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- git-daemon transport not available in grit, but flag validation works ---

test_expect_success 'daemon rejects invalid --init-timeout values' '
	for arg in "3a" "-3"
	do
		test_must_fail git daemon --init-timeout="$arg" 2>err &&
		test_grep "invalid init-timeout ${SQ}$arg${SQ}, expecting a non-negative integer" err ||
		return 1
	done
'

test_expect_success 'daemon rejects invalid --timeout values' '
	for arg in "3a" "-3"
	do
		test_must_fail git daemon --timeout="$arg" 2>err &&
		test_grep "invalid timeout ${SQ}$arg${SQ}, expecting a non-negative integer" err ||
		return 1
	done
'

test_expect_success 'daemon rejects invalid --max-connections values' '
	arg=3a &&
	test_must_fail git daemon --max-connections=3a 2>err &&
	test_grep "invalid max-connections ${SQ}$arg${SQ}, expecting an integer" err
'

test_expect_failure 'setup repository' '
	false
'

test_expect_failure 'create git-accessible bare repository' '
	false
'

test_expect_failure 'clone git repository' '
	false
'

test_expect_failure 'fetch changes via git protocol' '
	false
'

test_expect_failure 'no-op fetch -v stderr is as expected' '
	false
'

test_expect_failure 'no-op fetch without "-v" is quiet' '
	false
'

test_expect_failure 'remote detects correct HEAD' '
	false
'

test_expect_failure 'prepare pack objects' '
	false
'

test_expect_failure 'fetch notices corrupt pack' '
	false
'

test_expect_failure 'fetch notices corrupt idx' '
	false
'

test_expect_failure 'client refuses to ask for repo with newline' '
	false
'

test_expect_failure 'clone non-existent' '
	false
'

test_expect_failure 'push disabled' '
	false
'

test_expect_failure 'read access denied' '
	false
'

test_expect_failure 'not exported' '
	false
'

test_expect_failure 'clone non-existent' '
	false
'

test_expect_failure 'push disabled' '
	false
'

test_expect_failure 'read access denied' '
	false
'

test_expect_failure 'not exported' '
	false
'

test_expect_failure 'access repo via interpolated hostname' '
	false
'

test_expect_failure 'hostname cannot break out of directory' '
	false
'

test_expect_failure 'hostname interpolation works after LF-stripping' '
	false
'

test_done
