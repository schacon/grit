#!/bin/sh
#
# Upstream: t9142-git-svn-shallow-clone.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn shallow clone'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup test repository' '
	false
'

test_expect_failure 'clone trunk with "-r HEAD"' '
	false
'

test_done
