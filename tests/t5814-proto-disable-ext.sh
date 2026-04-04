#!/bin/sh
# Ported from git/t/t5814-proto-disable-ext.sh
# Tests for protocol.ext.allow configuration

test_description='protocol disabling for ext:: transport'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'clone denied with protocol.ext.allow=never' '
	test_must_fail git -c protocol.ext.allow=never clone "ext::git %s /repo" clone-denied 2>err &&
	grep -i "not allowed" err
'

test_expect_success 'clone denied with GIT_ALLOW_PROTOCOL excluding ext' '
	GIT_ALLOW_PROTOCOL=file test_must_fail git clone "ext::git %s /repo" clone-denied-env 2>err &&
	grep -i "not allowed" err
'

test_done
