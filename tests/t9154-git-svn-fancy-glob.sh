#!/bin/sh
#
# Upstream: t9154-git-svn-fancy-glob.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn fancy glob test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'load svn repo' '
	false
'

test_expect_failure 'add red branch' '
	false
'

test_expect_failure 'add gre branch' '
	false
'

test_expect_failure 'add green branch' '
	false
'

test_expect_failure 'add all branches' '
	false
'

test_done
