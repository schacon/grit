#!/bin/sh
#
# Upstream: t9803-git-p4-shell-metachars.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 transparency to shell metachars in filenames'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'shell metachars in filenames' '
	false
'

test_expect_failure 'deleting with shell metachars' '
	false
'

test_expect_failure 'branch with shell char' '
	false
'

test_done
