#!/bin/sh
# Ported from git/t/t5811-proto-disable-git.sh
# Tests for protocol.git.allow configuration
#
# Requires git:// protocol support and git-daemon. Stubbed.

test_description='protocol disabling for git:// transport'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'clone denied with protocol.git.allow=never' '
	test_must_fail git -c protocol.git.allow=never clone git://localhost/repo clone-denied 2>err &&
	grep -i "not allowed" err
'

test_expect_failure 'fetch denied with protocol.git.allow=never' '
	false
'

test_done
