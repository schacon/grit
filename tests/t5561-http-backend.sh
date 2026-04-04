#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5561-http-backend.sh
# test git-http-backend

test_description='test git-http-backend'
=======
#
# Upstream: t5561-http-backend.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description='test git-http-backend (HTTP STUB)'

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

test_expect_failure 'server request log matches test results' '
>>>>>>> test/batch-EN
	false
'

test_done
