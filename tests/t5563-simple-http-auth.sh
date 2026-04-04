#!/bin/sh
#
# Upstream: t5563-simple-http-auth.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description='test http auth header and credential helper interop (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

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
	false
'

test_done
