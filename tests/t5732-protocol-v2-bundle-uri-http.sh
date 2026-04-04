#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5732-protocol-v2-bundle-uri-http.sh
# Test bundle-uri with protocol v2 and 

test_description='Test bundle-uri with protocol v2 and '
=======
#
# Upstream: t5732-protocol-v2-bundle-uri-http.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description="Test bundle-uri with protocol v2 and 'http://' transport (HTTP STUB)"

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
>>>>>>> test/batch-EN

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

<<<<<<< HEAD
test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'protocol (may require server) — not yet ported' '
=======
# --- HTTP transport / bundle-uri not yet available in grit ---

test_expect_failure 'setup' '
	false
'

test_expect_failure 'clone with bundle-uri over http' '
	false
'

test_expect_failure 'fetch with bundle-uri over http' '
>>>>>>> test/batch-EN
	false
'

test_done
