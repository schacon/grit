#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5563-simple-http-auth.sh
# test http auth header and credential helper interop

test_description='test http auth header and credential helper interop'
=======
#
# Upstream: t5563-simple-http-auth.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description='test http auth header and credential helper interop (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
>>>>>>> test/batch-EN

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

<<<<<<< HEAD
test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
=======
# --- HTTP auth not yet available in grit ---

test_expect_failure 'setup_credential_helper' '
	false
'

test_expect_failure 'setup repository' '
	false
'

test_expect_failure 'access using basic auth' '
	false
'

test_expect_failure 'access using basic auth via authtype' '
	false
'

test_expect_failure 'access using basic auth invalid credentials' '
>>>>>>> test/batch-EN
	false
'

test_done
