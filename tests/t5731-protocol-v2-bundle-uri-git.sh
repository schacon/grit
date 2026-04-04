#!/bin/sh
# Ported from git/t/t5731-protocol-v2-bundle-uri-git.sh
# Tests for bundle-uri with git:// transport and protocol v2
#
# Requires git-daemon and bundle-uri protocol. Stubbed.

test_description='bundle-uri with git:// transport (protocol v2)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'clone with bundle-uri over git://' '
	test_create_repo server &&
	(cd server && test_commit one) &&
	git -c protocol.version=2 clone git://localhost/server client
'

test_expect_failure 'fetch with bundle-uri over git://' '
	(cd server && test_commit two) &&
	git -C client -c protocol.version=2 fetch
'

test_done
