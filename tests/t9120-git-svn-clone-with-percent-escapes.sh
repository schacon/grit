#!/bin/sh
#
# Upstream: t9120-git-svn-clone-with-percent-escapes.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn clone with percent escapes'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup svnrepo' '
	false
'

test_expect_failure 'test clone with percent escapes' '
	false
'

test_expect_failure 'svn checkout with percent escapes' '
	false
'

test_expect_failure 'svn checkout with space' '
	false
'

test_expect_failure 'test clone trunk with percent escapes and minimize-url' '
	false
'

test_expect_failure 'test clone trunk with percent escapes' '
	false
'

test_expect_failure 'test clone --stdlayout with percent escapes' '
	false
'

test_expect_failure 'test clone -s with unescaped space' '
	false
'

test_done
