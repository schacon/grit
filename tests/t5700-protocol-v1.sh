#!/bin/sh
# Ported from git/t/t5700-protocol-v1.sh
# Tests for protocol version 1 negotiation
#
# These tests require upload-pack/receive-pack server support
# which grit does not yet implement. Stubbed with test_expect_failure.

test_description='protocol version 1 tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'clone with protocol v1' '
	test_create_repo server &&
	(cd server && test_commit one) &&
	GIT_TRACE_PACKET=1 git clone --protocol=version=1 server client 2>log &&
	grep "version 1" log
'

test_expect_failure 'fetch with protocol v1' '
	(cd server && test_commit two) &&
	GIT_TRACE_PACKET=1 git -C client fetch --protocol=version=1 2>log &&
	grep "version 1" log
'

test_expect_failure 'push with protocol v1' '
	(cd client && test_commit three) &&
	GIT_TRACE_PACKET=1 git -C client push --protocol=version=1 2>log &&
	grep "version 1" log
'

test_done
