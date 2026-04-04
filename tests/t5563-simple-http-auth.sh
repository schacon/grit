#!/bin/sh
#
# Upstream: t5563-simple-http-auth.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='test http auth header and credential helper interop'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not available in grit ---

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

test_expect_failure 'access using basic proactive auth' '
	false
'

test_expect_failure 'access using auto proactive auth with basic default' '
	false
'

test_expect_failure 'access using auto proactive auth with authtype from credential helper' '
	false
'

test_expect_failure 'access using basic auth with extra challenges' '
	false
'

test_expect_failure 'access using basic auth mixed-case wwwauth header name' '
	false
'

test_expect_failure 'access using basic auth with wwwauth header continuations' '
	false
'

test_expect_failure 'access using basic auth with wwwauth header empty continuations' '
	false
'

test_expect_failure 'access using basic auth with wwwauth header mixed continuations' '
	false
'

test_expect_failure 'access using bearer auth' '
	false
'

test_expect_failure 'access using bearer auth with invalid credentials' '
	false
'

test_expect_failure 'clone with bearer auth and probe_rpc' '
	false
'

test_expect_failure 'access using three-legged auth' '
	false
'

test_done
