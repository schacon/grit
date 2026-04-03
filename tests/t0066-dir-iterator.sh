#!/bin/sh

test_description='Test directory iterator (requires test-tool)'

. ./test-lib.sh

# This test requires test-tool dir-iterator which is not available in grit.

test_expect_failure 'dir-iterator (needs test-tool)' '
	false
'

test_done
