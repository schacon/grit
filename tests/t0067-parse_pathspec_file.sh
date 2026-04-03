#!/bin/sh

test_description='Test parse_pathspec_file() (requires test-tool)'

. ./test-lib.sh

# This test requires test-tool parse-pathspec-file which is not available in grit.

test_expect_failure 'parse-pathspec-file (needs test-tool)' '
	false
'

test_done
