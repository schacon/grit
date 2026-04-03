#!/bin/sh

test_description='Windows named pipes'

. ./test-lib.sh

# Windows-only test, not applicable on Linux/macOS

test_expect_success 'setup' '
	git init
'

test_expect_failure 'Windows named pipe tests (Windows only)' '
	false
'

test_done
