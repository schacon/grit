#!/bin/sh
# Ported from git/t/t5811-proto-disable-git.sh
# Tests for protocol.git.allow configuration

test_description='protocol disabling for git:// transport'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'clone denied with protocol.git.allow=never' '
	test_must_fail git -c protocol.git.allow=never clone git://localhost/repo clone-denied 2>err &&
	grep -i "not allowed" err
'

test_expect_success 'clone denied with GIT_ALLOW_PROTOCOL excluding git' '
	GIT_ALLOW_PROTOCOL=file test_must_fail git clone git://localhost/repo clone-denied-env 2>err &&
	grep -i "not allowed" err
'

test_done
