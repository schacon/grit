#!/bin/sh
#
# Upstream: t9111-git-svn-use-svnsync-props.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn useSvnsyncProps test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'load svnsync repo' '
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

test_done
