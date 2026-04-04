#!/bin/sh
#
# Upstream: t9200-git-cvsexportcommit.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='Test export of commits to CVS'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- CVS not available in grit ---

test_expect_failure 'git setup' '
	false
'

test_expect_failure 'New file' '
	false
'

test_expect_failure 'Remove two files, add two and update two' '
	false
'

test_expect_failure 'Remove only binary files' '
	false
'

test_expect_failure 'Remove only a text file' '
	false
'

test_expect_failure 'New file with spaces in file name' '
	false
'

test_expect_failure 'Update file with spaces in file name' '
	false
'

test_expect_failure 'File with non-ascii file name' '
	false
'

test_expect_failure 'Mismatching patch should fail' '
	false
'

test_expect_failure 'Retain execute bit' '
	false
'

test_expect_failure '-w option should work with relative GIT_DIR' '
	false
'

test_expect_failure 'check files before directories' '
	false
'

test_expect_failure 're-commit a removed filename which remains in CVS attic' '
	false
'

test_expect_failure 'commit a file with leading spaces in the name' '
	false
'

test_expect_failure 'use the same checkout for Git and CVS' '
	false
'

test_done
