#!/bin/sh
#
# Upstream: t5540-http-push-webdav.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='test WebDAV http-push

This test runs various sanity checks on http-push.'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not available in grit ---

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

test_expect_failure 'push to remote repository with unpacked refs' '
	false
'

test_expect_failure 'http-push fetches unpacked objects' '
	false
'

test_expect_failure 'http-push fetches packed objects' '
	false
'

test_expect_failure 'create and delete remote branch' '
	false
'

test_expect_failure 'non-force push fails if not up to date' '
	false
'

test_expect_failure 'MKCOL sends directory names with trailing slashes' '
	false
'

test_expect_failure 'PUT and MOVE sends object to URLs with SHA-1 hash suffix' '
	false
'

test_expect_failure 'push to password-protected repository (user in URL)' '
	false
'

test_expect_failure 'user was prompted only once for password' '
	false
'

test_expect_failure 'push to password-protected repository (no user in URL)' '
	false
'

test_expect_failure 'push to password-protected repository (netrc)' '
	false
'

test_done
