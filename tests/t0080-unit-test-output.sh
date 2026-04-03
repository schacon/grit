#!/bin/sh

test_description='Test the output of the unit test framework'

. ./test-lib.sh

# These tests require test-tool which is not available in grit.

test_expect_success 'setup' '
	git init
'

test_expect_failure 'TAP output from unit tests (requires test-tool)' '
	test-tool example-tap 2>&1 >actual
'

test_done
