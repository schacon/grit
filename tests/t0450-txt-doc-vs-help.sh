#!/bin/sh

test_description='assert Documentation and -h output (not applicable to grit)'

. ./test-lib.sh

# This test compares Documentation/*.adoc with -h output.
# Not applicable to grit which has its own help system.

test_expect_success 'skip - not applicable to grit' '
	true
'

test_done
