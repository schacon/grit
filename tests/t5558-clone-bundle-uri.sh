#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5558-clone-bundle-uri.sh
# test fetching bundles with --bundle-uri

test_description='test fetching bundles with --bundle-uri'
=======
#
# Upstream: t5558-clone-bundle-uri.sh
# Requires HTTP server for most tests — stubbed as test_expect_failure.
# Some local bundle tests exist upstream but bundle-uri is not yet in grit.
#

test_description='test fetching bundles with --bundle-uri (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
>>>>>>> test/batch-EN

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

<<<<<<< HEAD
test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
=======
# --- bundle-uri / HTTP transport not yet available in grit ---

test_expect_failure 'fail to clone from non-existent file' '
	false
'

test_expect_failure 'fail to clone from non-bundle file' '
	false
'

test_expect_failure 'create bundle' '
	false
'

test_expect_failure 'clone with path bundle' '
	false
'

test_expect_failure 'clone with bundle that has bad header' '
	false
'

test_expect_failure 'clone with file:// bundle' '
	false
'

test_expect_failure 'clone with http:// bundle' '
>>>>>>> test/batch-EN
	false
'

test_done
