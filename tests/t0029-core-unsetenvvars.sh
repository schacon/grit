#!/bin/sh

test_description='test the Windows-only core.unsetenvvars setting'

. ./test-lib.sh

# This is a Windows-only test (MINGW prereq). Skip on all other platforms.

test_expect_success 'skip on non-Windows' '
	true
'

test_done
