#!/bin/sh
#
# Upstream: t9119-git-svn-info.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn info'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup repository and import' '
	false
'

test_expect_failure 'info' '
	false
'

test_expect_failure 'info --url' '
	false
'

test_expect_failure 'info .' '
	false
'

test_expect_failure 'info $(pwd)' '
	false
'

test_expect_failure 'info $(pwd)/../___wc' '
	false
'

test_expect_failure 'info $(pwd)/../___wc//file' '
	false
'

test_expect_failure 'info --url .' '
	false
'

test_expect_failure 'info file' '
	false
'

test_expect_failure 'info --url file' '
	false
'

test_expect_failure 'info directory' '
	false
'

test_expect_failure 'info inside directory' '
	false
'

test_expect_failure 'info --url directory' '
	false
'

test_expect_failure 'info symlink-file' '
	false
'

test_expect_failure 'info --url symlink-file' '
	false
'

test_expect_failure 'info symlink-directory' '
	false
'

test_expect_failure 'info --url symlink-directory' '
	false
'

test_expect_failure 'info added-file' '
	false
'

test_expect_failure 'info --url added-file' '
	false
'

test_expect_failure 'info added-directory' '
	false
'

test_expect_failure 'info --url added-directory' '
	false
'

test_expect_failure 'info added-symlink-file' '
	false
'

test_expect_failure 'info --url added-symlink-file' '
	false
'

test_expect_failure 'info added-symlink-directory' '
	false
'

test_expect_failure 'info --url added-symlink-directory' '
	false
'

test_expect_failure 'info deleted-file' '
	false
'

test_expect_failure 'info --url file (deleted)' '
	false
'

test_expect_failure 'info deleted-directory' '
	false
'

test_expect_failure 'info --url directory (deleted)' '
	false
'

test_expect_failure 'info deleted-symlink-file' '
	false
'

test_expect_failure 'info --url symlink-file (deleted)' '
	false
'

test_expect_failure 'info deleted-symlink-directory' '
	false
'

test_expect_failure 'info --url symlink-directory (deleted)' '
	false
'

test_expect_failure 'info unknown-file' '
	false
'

test_expect_failure 'info --url unknown-file' '
	false
'

test_expect_failure 'info unknown-directory' '
	false
'

test_expect_failure 'info --url unknown-directory' '
	false
'

test_expect_failure 'info unknown-symlink-file' '
	false
'

test_expect_failure 'info --url unknown-symlink-file' '
	false
'

test_expect_failure 'info unknown-symlink-directory' '
	false
'

test_expect_failure 'info --url unknown-symlink-directory' '
	false
'

test_done
