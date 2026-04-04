#!/bin/sh
#
# Upstream: t9827-git-p4-change-filetype.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 support for file type change'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'create files' '
	false
'

test_expect_failure 'change file to symbolic link' '
	false
'

test_expect_failure 'change symbolic link to file' '
	false
'

test_done
