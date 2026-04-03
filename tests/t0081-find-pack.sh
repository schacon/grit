#!/bin/sh

test_description='test find-pack (requires test-tool)'

. ./test-lib.sh

# This test requires test-tool find-pack which is not available in grit.

test_expect_failure 'find-pack (needs test-tool)' '
	false
'

test_done
