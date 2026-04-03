#!/bin/sh

test_description='test env-helper (requires test-tool)'

. ./test-lib.sh

# All tests require test-tool env-helper which is not available in grit.

test_expect_failure 'test-tool env-helper (not available)' '
	false
'

test_done
