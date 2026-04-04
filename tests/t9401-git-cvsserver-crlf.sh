#!/bin/sh
#
# Upstream: t9401-git-cvsserver-crlf.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git-cvsserver -kb modes

tests -kb mode for binary files when accessing a git
repository using cvs CLI client via git-cvsserver server'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- CVS not available in grit ---

test_expect_failure 'setup' '
	false
'

test_expect_failure 'cvs co (default crlf)' '
	false
'

test_expect_failure 'cvs co (allbinary)' '
	false
'

test_expect_failure 'cvs co (use attributes/allbinary)' '
	false
'

test_expect_failure 'cvs co (use attributes)' '
	false
'

test_expect_failure 'adding files' '
	false
'

test_expect_failure 'updating' '
	false
'

test_expect_failure 'cvs co (use attributes/guess)' '
	false
'

test_expect_failure 'setup multi-line files' '
	false
'

test_expect_failure 'cvs co (guess)' '
	false
'

test_expect_failure 'cvs co another copy (guess)' '
	false
'

test_expect_failure 'cvs status - sticky options' '
	false
'

test_expect_failure 'add text (guess)' '
	false
'

test_expect_failure 'add bin (guess)' '
	false
'

test_expect_failure 'remove files (guess)' '
	false
'

test_expect_failure 'cvs ci (guess)' '
	false
'

test_expect_failure 'update subdir of other copy (guess)' '
	false
'

test_expect_failure 'update/merge full other copy (guess)' '
	false
'

test_done
