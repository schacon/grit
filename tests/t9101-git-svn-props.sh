#!/bin/sh
#
# Upstream: t9101-git-svn-props.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn property tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'checkout working copy from svn' '
	false
'

test_expect_failure 'setup some commits to svn' '
	false
'

test_expect_failure 'initialize git svn' '
	false
'

test_expect_failure 'fetch revisions from svn' '
	false
'

test_expect_failure 'raw $Id$ found in kw.c' '
	false
'

test_expect_failure 'fetch and pull latest from svn and checkout a new wc' '
	false
'

test_expect_failure 'Set CRLF on cr files' '
	false
'

test_expect_failure 'fetch and pull latest from svn' '
	false
'

test_expect_failure 'CRLF + $Id$' '
	false
'

test_expect_failure 'CRLF + $Id$ (no newline)' '
	false
'

test_expect_failure 'test show-ignore' '
	false
'

test_expect_failure 'test create-ignore' '
	false
'

test_expect_failure 'test propget' '
	false
'

test_expect_failure 'test proplist' '
	false
'

test_expect_failure '$name' '
	false
'

test_expect_failure 'propset CR on crlf files' '
	false
'

test_expect_failure 'Comparing $i' '
	false
'

test_done
