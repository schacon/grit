#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5559-http-fetch-smart-http2.sh
# http-fetch-smart-http2

test_description='http-fetch-smart-http2'
=======
#
# Upstream: t5559-http-fetch-smart-http2.sh
# Requires HTTP/2 server — stubbed as test_expect_failure.
# This is essentially t5551 re-run with HTTP/2.
#

test_description='test smart fetching over http via http-backend with HTTP/2 (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
>>>>>>> test/batch-EN

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

<<<<<<< HEAD
test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
=======
# --- HTTP/2 transport not yet available in grit ---

test_expect_failure 'setup repository' '
	false
'

test_expect_failure 'clone http/2 repository' '
	false
'

test_expect_failure 'fetch changes via http/2' '
>>>>>>> test/batch-EN
	false
'

test_done
