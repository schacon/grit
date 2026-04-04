#!/bin/sh
#
# Upstream: t9820-git-p4-editor-handling.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 handling of EDITOR'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'EDITOR with options' '
	false
'

test_done
