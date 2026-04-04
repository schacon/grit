#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5549-fetch-push-http.sh
# fetch/push functionality using the HTTP protocol

test_description='fetch/push functionality using the HTTP protocol'
=======
#
# Upstream: t5549-fetch-push-http.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description='fetch/push functionality using the HTTP protocol (HTTP STUB)'

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

test_expect_failure 'push without negotiation' '
	false
'

test_expect_failure 'push with negotiation' '
	false
'

test_expect_failure 'push with negotiation proceeds anyway even if negotiation fails' '
>>>>>>> test/batch-EN
	false
'

test_done
