#!/bin/sh
#
# Upstream: t9824-git-p4-git-lfs.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='Clone repositories and store files in Git LFS'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'Create repo with binary files' '
	false
'

test_expect_failure 'Store files in LFS based on size (>24 bytes)' '
	false
'

test_expect_failure 'Store files in LFS based on size (>25 bytes)' '
	false
'

test_expect_failure 'Store files in LFS based on extension (dat)' '
	false
'

test_expect_failure 'Store files in LFS based on size (>25 bytes) and extension (dat)' '
	false
'

test_expect_failure 'Remove file from repo and store files in LFS based on size (>24 bytes)' '
	false
'

test_expect_failure 'Add .gitattributes and store files in LFS based on size (>24 bytes)' '
	false
'

test_expect_failure 'Add big files to repo and store files in LFS based on compressed size (>28 bytes)' '
	false
'

test_done
