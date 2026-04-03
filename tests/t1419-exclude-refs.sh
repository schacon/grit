#!/bin/sh

test_description='test exclude_patterns functionality (requires test-tool)'

. ./test-lib.sh

# This test requires test-tool ref-store which is not available in grit.

test_expect_failure 'exclude-refs (needs test-tool ref-store)' '
	false
'

test_done
