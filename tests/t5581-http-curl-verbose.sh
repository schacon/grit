#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5581-http-curl-verbose.sh
# test GIT_CURL_VERBOSE

test_description='test GIT_CURL_VERBOSE'
=======
#
# Upstream: t5581-http-curl-verbose.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description='test GIT_CURL_VERBOSE (HTTP STUB)'

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

test_expect_failure 'setup repository' '
	false
'

test_expect_failure 'failure in git-upload-pack is shown' '
>>>>>>> test/batch-EN
	false
'

test_done
