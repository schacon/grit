#!/bin/sh
# Ported from git/t/t5704-protocol-violations.sh
# Tests for protocol violation handling
#
# Requires server-side protocol handling. Stubbed.

test_description='protocol violation handling'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'extra lines at end of v2 ls-refs response' '
	test_create_repo server &&
	(cd server && test_commit one) &&
	test_must_fail git ls-remote server
'

test_expect_success 'extra lines at end of v2 fetch response' '
	false
'

test_done
