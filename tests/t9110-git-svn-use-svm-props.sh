#!/bin/sh
#
# Upstream: t9110-git-svn-use-svm-props.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn useSvmProps test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'load svm repo' '
	false
'

test_expect_failure 'verify metadata for /bar' '
	false
'

test_expect_failure 'verify metadata for /dir/a/b/c/d/e' '
	false
'

test_expect_failure 'verify metadata for /dir' '
	false
'

test_expect_failure 'find commit based on SVN revision number' '
	false
'

test_expect_failure 'empty rebase' '
	false
'

test_done
