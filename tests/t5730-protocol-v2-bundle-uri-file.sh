#!/bin/sh
# Ported from git/t/t5730-protocol-v2-bundle-uri-file.sh
# Tests for bundle-uri with file:// transport and protocol v2
#
# Requires bundle-uri protocol capability and file transport. Stubbed.

test_description='bundle-uri with file:// transport (protocol v2)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'clone with bundle-uri over file://' '
	test_create_repo server &&
	(cd server && test_commit one) &&
	GIT_TRACE_PACKET=1 git -c protocol.version=2 -c transfer.bundleURI=true \
		clone file://$(pwd)/server client 2>trace &&
	grep "bundle-uri" trace
'

test_expect_failure 'fetch with bundle-uri over file://' '
	false
'

test_done
