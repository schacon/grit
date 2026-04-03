#!/bin/sh
#
# Ported from git/t/t5616-partial-clone.sh
# Tests for git partial clone
# Note: grit does not support --filter or partial clones yet
#

test_description='git partial clone'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup normal src repo' '
	"$REAL_GIT" init src &&
	(
		cd src &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		for n in 1 2 3 4; do
			echo "This is file: $n" >file.$n.txt &&
			"$REAL_GIT" add file.$n.txt &&
			"$REAL_GIT" commit -m "file $n" || return 1
		done
	)
'

test_expect_success 'basic clone of src' '
	git clone src clone1 &&
	test -f clone1/file.1.txt &&
	test -f clone1/file.4.txt
'

test_expect_failure 'partial clone with --filter=blob:none' '
	git clone --filter=blob:none src clone-partial &&
	test -d clone-partial/.git
'

test_done
