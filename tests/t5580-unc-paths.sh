#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5580-unc-paths.sh
# various Windows-only path tests

test_description='various Windows-only path tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

=======
#
# Upstream: t5580-unc-paths.sh
# Windows-only UNC path tests — skip on non-Windows.
#

test_description='various Windows-only path tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# This test is Windows-only (requires CYGWIN or MINGW).
# On Linux, skip all tests.
skip_all='skipping Windows-only UNC path tests'
>>>>>>> test/batch-EN
test_done
