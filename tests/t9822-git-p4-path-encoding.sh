#!/bin/sh
#
# Upstream: t9822-git-p4-path-encoding.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='Clone repositories with non ASCII paths'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'Create a repo containing iso8859-1 encoded paths' '
	false
'

test_expect_failure 'Clone auto-detects depot with iso8859-1 paths' '
	false
'

test_expect_failure 'Clone repo containing iso8859-1 encoded paths with git-p4.pathEncoding' '
	false
'

test_expect_failure 'Delete iso8859-1 encoded paths and clone' '
	false
'

test_done
