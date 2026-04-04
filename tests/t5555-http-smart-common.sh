#!/bin/sh
#
# Upstream: t5555-http-smart-common.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='test functionality common to smart fetch & push'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not available in grit ---

test_expect_failure 'setup' '
	false
'

test_expect_failure 'git upload-pack --http-backend-info-refs and --advertise-refs are aliased' '
	false
'

test_expect_failure 'git receive-pack --http-backend-info-refs and --advertise-refs are aliased' '
	false
'

test_expect_failure 'git upload-pack --advertise-refs' '
	false
'

test_expect_failure 'git upload-pack --advertise-refs: v0' '
	false
'

test_expect_failure 'git receive-pack --advertise-refs: v0' '
	false
'

test_expect_failure 'git upload-pack --advertise-refs: v1' '
	false
'

test_expect_failure 'git receive-pack --advertise-refs: v1' '
	false
'

test_expect_failure 'git upload-pack --advertise-refs: v2' '
	false
'

test_expect_failure 'git receive-pack --advertise-refs: v2' '
	false
'

test_done
