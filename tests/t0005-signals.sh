#!/bin/sh

test_description='signals work as we expect'

. ./test-lib.sh

# All tests in this file require test-tool sigchain which is not
# available in grit. Mark them as expected failures.

test_expect_failure 'sigchain works (needs test-tool)' '
	false
'

test_done
