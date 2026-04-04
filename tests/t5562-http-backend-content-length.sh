#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5562-http-backend-content-length.sh
# test git-http-backend respects CONTENT_LENGTH

test_description='test git-http-backend respects CONTENT_LENGTH'
=======
#
# Upstream: t5562-http-backend-content-length.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description='test git-http-backend respects CONTENT_LENGTH (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
>>>>>>> test/batch-EN

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

<<<<<<< HEAD
test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
=======
# --- HTTP transport not yet available in grit ---

test_expect_failure 'setup' '
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

test_expect_failure 'push plain' '
	false
'

test_expect_failure 'push plain truncated' '
>>>>>>> test/batch-EN
	false
'

test_done
