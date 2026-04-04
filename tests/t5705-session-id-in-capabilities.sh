#!/bin/sh
# Ported from git/t/t5705-session-id-in-capabilities.sh
# Tests for session-id in protocol capabilities
#
# Requires protocol v2 server support. Stubbed.

test_description='session-id in capabilities'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'session IDs in v2 fetch' '
	test_create_repo server &&
	(cd server && test_commit one) &&
	git clone server client &&
	GIT_TRACE_PACKET=1 git -C client -c protocol.version=2 fetch 2>log &&
	grep "session-id" log
'

test_expect_success 'session IDs in v0 fetch' '
	GIT_TRACE_PACKET=1 git -C client fetch 2>log &&
	grep "session-id" log
'

test_done
