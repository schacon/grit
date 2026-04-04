#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5564-http-proxy.sh
# test fetching through http proxy

test_description='test fetching through http proxy'
=======
#
# Upstream: t5564-http-proxy.sh
# Requires HTTP proxy server — stubbed as test_expect_failure.
#

test_description='test fetching through http proxy (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
>>>>>>> test/batch-EN

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

<<<<<<< HEAD
test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
=======
# --- HTTP proxy transport not yet available in grit ---

test_expect_failure 'setup repository' '
	false
'

test_expect_failure 'proxy requires password' '
	false
'

test_expect_failure 'clone through proxy with auth' '
	false
'

test_expect_failure 'clone can prompt for proxy password' '
>>>>>>> test/batch-EN
	false
'

test_done
