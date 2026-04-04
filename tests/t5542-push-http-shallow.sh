#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5542-push-http-shallow.sh
# push from/to a shallow clone over http

test_description='push from/to a shallow clone over http'
=======
#
# Upstream: t5542-push-http-shallow.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
# NOTE: grit already has t5542-push-advanced.sh (different test).
#

test_description='push from/to a shallow clone over http (HTTP STUB)'

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

test_expect_failure 'push to shallow repo via http' '
	false
'

test_expect_failure 'push from shallow repo via http' '
>>>>>>> test/batch-EN
	false
'

test_done
