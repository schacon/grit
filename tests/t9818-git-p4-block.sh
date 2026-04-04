#!/bin/sh
#
# Upstream: t9818-git-p4-block.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 fetching changes in multiple blocks'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'Create group with limited maxrows' '
	false
'

test_expect_failure 'Create a repo with many changes' '
	false
'

test_expect_failure 'Default user cannot fetch changes' '
	false
'

test_expect_failure 'Clone the repo' '
	false
'

test_expect_failure 'All files are present' '
	false
'

test_expect_failure 'file.txt is correct' '
	false
'

test_expect_failure 'Correct number of commits' '
	false
'

test_expect_failure 'Previous version of file.txt is correct' '
	false
'

test_expect_failure 'Add some more files' '
	false
'

test_expect_failure 'Syncing files' '
	false
'

test_expect_failure 'Create a repo with multiple depot paths' '
	false
'

test_expect_failure 'Clone repo with multiple depot paths' '
	false
'

test_expect_failure 'Clone repo with self-sizing block size' '
	false
'

test_done
