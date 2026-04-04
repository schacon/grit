#!/bin/sh
# Ported from git/t/t5700-protocol-v1.sh
# Tests for local transport operations (protocol version negotiation
# tracing is not supported; test basic clone/fetch/push with local transport).

test_description='protocol v1 local transport tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup server repo' '
	test_create_repo server &&
	(cd server && test_commit one)
'

test_expect_success 'clone with local transport' '
	git clone server client &&
	test -f client/one.t &&
	(cd client && git log --oneline >log && grep "one" log)
'

test_expect_success 'fetch with local transport' '
	(cd server && test_commit two) &&
	(cd client && git fetch origin) &&
	(cd client && git log --oneline origin/main >log && grep "two" log)
'

test_expect_success 'push with local transport' '
	(cd client && git merge origin/main && test_commit three && git push origin main) &&
	(cd server && git log --oneline >log && grep "three" log)
'

test_done
