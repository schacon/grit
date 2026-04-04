#!/bin/sh
#
# Upstream: t5540-http-push-webdav.sh
# Requires HTTP/WebDAV server — stubbed as test_expect_failure.
# NOTE: grit already has t5540-fetch-push-edge-cases.sh (different test).
#

test_description='test WebDAV http-push (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP/WebDAV transport not yet available in grit ---

test_expect_failure 'setup remote repository' '
	false
'

test_expect_failure 'create password-protected repository' '
	false
'

test_expect_failure 'clone remote repository' '
	false
'

test_expect_failure 'push to remote repository with packed refs' '
	false
'

test_expect_failure 'push already up-to-date' '
	false
'

test_expect_failure 'create and delete remote branch' '
	false
'

test_expect_failure 'push to password-protected repository' '
	false
'

test_done
