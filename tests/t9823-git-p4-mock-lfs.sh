#!/bin/sh
#
# Upstream: t9823-git-p4-mock-lfs.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='Clone repositories and store files in Mock LFS'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'Create repo with binary files' '
	false
'

test_expect_failure 'Store files in Mock LFS based on size (>24 bytes)' '
	false
'

test_expect_failure 'Store files in Mock LFS based on extension (dat)' '
	false
'

test_expect_failure 'Store files in Mock LFS based on extension (dat) and use git p4 sync and no client spec' '
	false
'

test_expect_failure 'Remove file from repo and store files in Mock LFS based on size (>24 bytes)' '
	false
'

test_expect_failure 'Run git p4 submit in repo configured with large file system' '
	false
'

test_done
