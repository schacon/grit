#!/bin/sh

test_description='progress display'

. ./test-lib.sh

# Progress display tests require test-tool progress which is not available in grit.

test_expect_success 'setup' '
	git init
'

test_expect_failure 'progress display tests (requires test-tool progress)' '
	test-tool progress <in 2>stderr
'

test_done
