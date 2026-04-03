#!/bin/sh

test_description='Test compatObjectFormat (not applicable)'

. ./test-lib.sh

# This test requires RUST prereq and complex multi-hash setup.
# Not applicable to grit.

test_expect_success 'skip - not applicable to grit' '
	true
'

test_done
