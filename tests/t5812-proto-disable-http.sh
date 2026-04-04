#!/bin/sh
# Ported from git/t/t5812-proto-disable-http.sh
# Tests for protocol disabling with HTTP transport

test_description='test disabling of git-over-http in clone/fetch'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'clone denied with protocol.http.allow=never' '
	test_must_fail git -c protocol.http.allow=never clone http://localhost/repo clone-http 2>err &&
	grep -i "not allowed" err
'

test_expect_success 'clone denied with protocol.https.allow=never' '
	test_must_fail git -c protocol.https.allow=never clone https://localhost/repo clone-https 2>err &&
	grep -i "not allowed" err
'

test_expect_success 'http transport respects GIT_ALLOW_PROTOCOL' '
	GIT_ALLOW_PROTOCOL=file test_must_fail git clone http://localhost/repo clone-env 2>err &&
	grep -i "not allowed" err
'

test_expect_success 'https transport respects GIT_ALLOW_PROTOCOL' '
	GIT_ALLOW_PROTOCOL=file test_must_fail git clone https://localhost/repo clone-env2 2>err &&
	grep -i "not allowed" err
'

test_done
