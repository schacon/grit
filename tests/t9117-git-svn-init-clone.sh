#!/bin/sh
#
# Upstream: t9117-git-svn-init-clone.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn init/clone tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup svnrepo' '
	false
'

test_expect_failure 'basic clone' '
	false
'

test_expect_failure 'clone to target directory' '
	false
'

test_expect_failure 'clone with --stdlayout' '
	false
'

test_expect_failure 'clone to target directory with --stdlayout' '
	false
'

test_expect_failure 'init without -s/-T/-b/-t does not warn' '
	false
'

test_expect_failure 'clone without -s/-T/-b/-t does not warn' '
	false
'

test_expect_failure 'init with -s/-T/-b/-t assumes --prefix=origin/' '
	false
'

test_expect_failure 'clone with -s/-T/-b/-t assumes --prefix=origin/' '
	false
'

test_expect_failure 'init with -s/-T/-b/-t and --prefix "" still works' '
	false
'

test_expect_failure 'clone with -s/-T/-b/-t and --prefix "" still works' '
	false
'

test_expect_failure 'init with -T as a full url works' '
	false
'

test_done
