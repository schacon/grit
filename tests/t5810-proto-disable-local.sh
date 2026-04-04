#!/bin/sh
# Ported from git/t/t5810-proto-disable-local.sh
# Tests for protocol.file.allow configuration
#
# Requires protocol.*.allow support. Stubbed.

test_description='protocol disabling for local transport'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	test_create_repo remote &&
	(cd remote && test_commit one)
'

test_expect_success 'clone denied with protocol.file.allow=never' '
	test_must_fail git -c protocol.file.allow=never clone remote clone-denied 2>err &&
	grep -i "not allowed" err
'

test_expect_success 'fetch denied with protocol.file.allow=never' '
	git clone remote clone-for-fetch &&
	test_must_fail git -C clone-for-fetch -c protocol.file.allow=never fetch 2>err &&
	grep -i "not allowed" err
'

test_expect_success 'push denied with protocol.file.allow=never' '
	git clone remote clone-for-push &&
	(cd clone-for-push && test_commit two) &&
	test_must_fail git -C clone-for-push -c protocol.file.allow=never push 2>err &&
	grep -i "not allowed" err
'

test_expect_success 'clone denied with GIT_ALLOW_PROTOCOL excluding file' '
	GIT_ALLOW_PROTOCOL=https test_must_fail git clone remote clone-denied-env 2>err &&
	grep -i "not allowed" err
'

test_done
