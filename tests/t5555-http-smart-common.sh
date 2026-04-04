#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5555-http-smart-common.sh
# test functionality common to smart fetch & push

test_description='test functionality common to smart fetch & push'
=======
#
# Upstream: t5555-http-smart-common.sh
# Requires HTTP server — stubbed as test_expect_failure.
#

test_description='test functionality common to smart fetch & push (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
>>>>>>> test/batch-EN

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

<<<<<<< HEAD
test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
=======
# --- HTTP smart transport not yet available in grit ---

test_expect_failure 'setup' '
	false
'

test_expect_failure 'git upload-pack --http-backend-info-refs and --advertise-refs are aliased' '
	false
'

test_expect_failure 'git receive-pack --http-backend-info-refs and --advertise-refs are aliased' '
	false
'

test_expect_failure 'git upload-pack --advertise-refs' '
	false
'

test_expect_failure 'git receive-pack --advertise-refs' '
>>>>>>> test/batch-EN
	false
'

test_done
