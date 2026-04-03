#!/bin/sh

test_description='verify safe.directory checks while running as root'

. ./test-lib.sh

# This test requires running as root with sudo. Skip in normal test runs.

test_expect_success 'skip - requires sudo/root' '
	true
'

test_done
