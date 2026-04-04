#!/bin/sh
#
# Upstream: t9211-scalar-clone.sh
# Requires scalar — ported as test_expect_failure stubs.
#

test_description='test the `scalar clone` subcommand'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- scalar not available in grit ---

test_expect_failure 'set up repository to clone' '
	false
'

test_expect_failure 'creates content in enlistment root' '
	false
'

test_expect_failure 'with spaces' '
	false
'

test_expect_failure 'partial clone if supported by server' '
	false
'

test_expect_failure 'fall back on full clone if partial unsupported' '
	false
'

test_expect_failure 'initializes sparse-checkout by default' '
	false
'

test_expect_failure '--full-clone does not create sparse-checkout' '
	false
'

test_expect_failure '--single-branch clones HEAD only' '
	false
'

test_expect_failure '--no-single-branch clones all branches' '
	false
'

test_expect_failure 'progress with tty' '
	false
'

test_expect_failure 'progress without tty' '
	false
'

test_expect_failure 'scalar clone warns when background maintenance fails' '
	false
'

test_expect_failure 'scalar clone --no-maintenance' '
	false
'

test_expect_failure '`scalar clone --no-src`' '
	false
'

test_done
