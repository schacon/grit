#!/bin/sh
#
# Upstream: t9158-git-svn-mergeinfo.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn mergeinfo propagation'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize source svn repo' '
	false
'

test_expect_failure 'clone svn repo' '
	false
'

test_expect_failure 'change svn:mergeinfo' '
	false
'

test_expect_failure 'verify svn:mergeinfo' '
	false
'

test_expect_failure 'change svn:mergeinfo multiline' '
	false
'

test_expect_failure 'verify svn:mergeinfo multiline' '
	false
'

test_done
