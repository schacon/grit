#!/bin/sh
#
# Upstream: t9109-git-svn-multi-glob.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn globbing refspecs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'test refspec globbing' '
	false
'

test_expect_failure 'test left-hand-side only globbing' '
	false
'

test_expect_failure 'test another branch' '
	false
'

test_expect_failure 'prepare test disallow multiple globs' '
	false
'

test_expect_failure 'test disallow multiple globs' '
	false
'

test_done
