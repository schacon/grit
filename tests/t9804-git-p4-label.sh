#!/bin/sh
#
# Upstream: t9804-git-p4-label.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 label tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'basic p4 labels' '
	false
'

test_expect_failure 'two labels on the same changelist' '
	false
'

test_done
