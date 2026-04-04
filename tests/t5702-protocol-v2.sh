#!/bin/sh
# Ported from git/t/t5702-protocol-v2.sh
# Tests for protocol version 2 operations
#
# These tests require protocol v2 server/client negotiation via
# upload-pack / receive-pack, which grit does not yet implement.

test_description='protocol v2 tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'clone with protocol v2' '
	test_create_repo server &&
	(cd server && test_commit one) &&
	GIT_TRACE_PACKET=1 git -c protocol.version=2 clone server client 2>log &&
	grep "version 2" log
'

test_expect_failure 'fetch with protocol v2' '
	GIT_TRACE_PACKET=1 git -C client -c protocol.version=2 fetch 2>log &&
	grep "version 2" log
'

test_expect_failure 'ls-remote with protocol v2' '
	GIT_TRACE_PACKET=1 git -c protocol.version=2 ls-remote server 2>log &&
	grep "version 2" log
'

test_expect_failure 'push with protocol v2' '
	(cd client && test_commit two) &&
	GIT_TRACE_PACKET=1 git -C client -c protocol.version=2 push 2>log &&
	grep "version 2" log
'

test_done
