#!/bin/sh
#
# Upstream: t9130-git-svn-authors-file.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn authors file tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup svnrepo' '
	false
'

test_expect_failure 'start import with incomplete authors file' '
	false
'

test_expect_failure 'imported 2 revisions successfully' '
	false
'

test_expect_failure 'continues to import once authors have been added' '
	false
'

test_expect_failure 'authors-file against globs' '
	false
'

test_expect_failure 'fetch fails on ee' '
	false
'

test_expect_failure 'failure happened without negative side effects' '
	false
'

test_expect_failure 'fetch continues after authors-file is fixed' '
	false
'

test_expect_failure 'fresh clone with svn.authors-file in config' '
	false
'

test_expect_failure 'authors-file imported user without email' '
	false
'

test_done
