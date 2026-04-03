#!/bin/sh

test_description='Test revision walking api (requires test-tool)'

. ./test-lib.sh

# This test requires test-tool revision-walking which is not available in grit.

test_expect_failure 'revision walking can be done twice (needs test-tool)' '
	false
'

test_done
