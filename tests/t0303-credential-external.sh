#!/bin/sh

test_description='external credential helper tests'

. ./test-lib.sh

# External credential helper tests require a specific helper to be configured
# via GIT_TEST_CREDENTIAL_HELPER. Not applicable without one.

test_expect_success 'setup' '
	git init
'

test_expect_failure 'external credential helper (requires GIT_TEST_CREDENTIAL_HELPER)' '
	test -n "$GIT_TEST_CREDENTIAL_HELPER"
'

test_done
