#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5557-http-get.sh
# test downloading a file by URL

test_description='test downloading a file by URL'
=======
#
# Upstream: t5557-http-get.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description='test downloading a file by URL (HTTP STUB)'

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

test_expect_failure 'get by URL: 404' '
	false
'

test_expect_failure 'get by URL: 200' '
>>>>>>> test/batch-EN
	false
'

test_done
