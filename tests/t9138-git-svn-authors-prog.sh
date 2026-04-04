#!/bin/sh
#
# Upstream: t9138-git-svn-authors-prog.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn authors prog tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'svn-authors setup' '
	false
'

test_expect_failure 'setup svnrepo' '
	false
'

test_expect_failure 'import authors with prog and file' '
	false
'

test_expect_failure 'imported 6 revisions successfully' '
	false
'

test_expect_failure 'authors-prog ran correctly' '
	false
'

test_expect_failure 'authors-file overrode authors-prog' '
	false
'

test_expect_failure 'authors-prog imported user without email' '
	false
'

test_expect_failure 'imported without authors-prog and authors-file' '
	false
'

test_expect_failure 'authors-prog handled special characters in username' '
	false
'

test_done
