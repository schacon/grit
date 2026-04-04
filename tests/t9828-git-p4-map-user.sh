#!/bin/sh
#
# Upstream: t9828-git-p4-map-user.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='Clone repositories and map users'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'Create a repo with different users' '
	false
'

test_expect_failure 'Clone repo root path with all history' '
	false
'

test_done
