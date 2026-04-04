#!/bin/sh
#
# Upstream: t9831-git-p4-triggers.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 with server triggers'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'clone with extra info lines from verbose p4 trigger' '
	false
'

test_expect_failure 'import with extra info lines from verbose p4 trigger' '
	false
'

test_expect_failure 'submit description with extra info lines from verbose p4 change trigger' '
	false
'

test_done
