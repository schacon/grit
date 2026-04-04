#!/bin/sh
#
# Upstream: t9808-git-p4-chdir.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 relative chdir'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'P4CONFIG and absolute dir clone' '
	false
'

test_expect_failure 'P4CONFIG and relative dir clone' '
	false
'

test_expect_failure 'p4 client root would be relative due to clone --dest' '
	false
'

test_expect_failure 'p4 client root symlink should stay symbolic' '
	false
'

test_done
