#!/bin/sh

test_description='verify sort functions (requires test-tool)'

. ./test-lib.sh

# This test requires test-tool mergesort which is not available in grit.

test_expect_failure 'DEFINE_LIST_SORT_DEBUG (needs test-tool)' '
	false
'

test_done
