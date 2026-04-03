#!/bin/sh

test_description='Test run command (requires test-tool)'

. ./test-lib.sh

# This test requires test-tool run-command which is not available in grit.

test_expect_failure 'run-command (needs test-tool)' '
	false
'

test_done
