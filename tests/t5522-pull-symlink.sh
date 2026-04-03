#!/bin/sh
# Ported from git/t/t5522-pull-symlink.sh

test_description='pulling from symlinked subdir'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success SYMLINKS setup '
	git init -q &&
	mkdir subdir &&
	echo file >subdir/file &&
	git add subdir/file &&
	git commit -q -m file &&
	git clone -q . clone-repo &&
	(
		cd clone-repo &&
		git config receive.denyCurrentBranch warn
	) &&
	git config receive.denyCurrentBranch warn
'

test_expect_success SYMLINKS 'pulling from real subdir' '
	echo real >subdir/file &&
	git add subdir/file &&
	git commit -m real &&
	(
		cd clone-repo/subdir/ &&
		git pull &&
		test real = $(cat file)
	)
'

test_done
