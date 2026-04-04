#!/bin/sh
# Ported from git/t/t9117-git-svn-init-clone.sh
# All tests require Subversion and are marked test_expect_failure.

test_description='git svn init/clone tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'setup svnrepo (not ported - requires SVN infrastructure)' '
	false
'

test_expect_failure 'basic clone (requires SVN)' '
	false
'

test_expect_failure 'clone to target directory (requires SVN)' '
	false
'

test_expect_failure 'clone with --stdlayout (requires SVN)' '
	false
'

test_expect_failure 'clone to target directory with --stdlayout (requires SVN)' '
	false
'

test_expect_failure 'init without -s/-T/-b/-t does not warn (requires SVN)' '
	false
'

test_expect_failure 'clone without -s/-T/-b/-t does not warn (requires SVN)' '
	false
'

test_expect_failure 'init with -s/-T/-b/-t assumes --prefix=origin/ (requires SVN)' '
	false
'

test_expect_failure 'clone with -s/-T/-b/-t assumes --prefix=origin/ (requires SVN)' '
	false
'

test_expect_failure 'init with -s/-T/-b/-t and --prefix "" still works (requires SVN)' '
	false
'

test_expect_failure 'clone with -s/-T/-b/-t and --prefix "" still works (requires SVN)' '
	false
'

test_expect_failure 'init with -T as a full url works (requires SVN)' '
	false
'

test_done
