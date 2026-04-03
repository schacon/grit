#!/bin/sh

test_description='read-tree can handle submodules'

. ./test-lib.sh

# Submodule tests require lib-submodule-update.sh infrastructure
# which is not available in the grit test harness.

test_expect_success 'setup' '
	git init
'

test_expect_failure 'read-tree submodule switch (requires lib-submodule-update)' '
	false
'

test_done
