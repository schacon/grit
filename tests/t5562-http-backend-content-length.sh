#!/bin/sh
#
# Upstream: t5562-http-backend-content-length.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='test git-http-backend respects CONTENT_LENGTH'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not available in grit ---

test_expect_failure 'setup' '
	false
'

test_expect_failure 'setup, compression related' '
	false
'

test_expect_failure 'fetch plain' '
	false
'

test_expect_failure 'fetch plain truncated' '
	false
'

test_expect_failure 'fetch plain empty' '
	false
'

test_expect_failure 'fetch gzipped' '
	false
'

test_expect_failure 'fetch gzipped truncated' '
	false
'

test_expect_failure 'fetch gzipped empty' '
	false
'

test_expect_failure 'push plain' '
	false
'

test_expect_failure 'push plain truncated' '
	false
'

test_expect_failure 'push plain empty' '
	false
'

test_expect_failure 'push gzipped' '
	false
'

test_expect_failure 'push gzipped truncated' '
	false
'

test_expect_failure 'push gzipped empty' '
	false
'

test_expect_failure 'CONTENT_LENGTH overflow ssite_t' '
	false
'

test_expect_failure 'empty CONTENT_LENGTH' '
	false
'

test_done
