#!/bin/sh
# Ported from git/t/t5404-tracking-branches.sh

test_description='tracking branch update checks for git push'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	echo 1 >file &&
	git add file &&
	git commit -m 1 &&
	git branch b1 &&
	git branch b2 &&
	git clone . aa &&
	(
		cd aa &&
		git checkout -b b1 origin/b1 &&
		echo aa-b1 >>file &&
		git commit -a -m aa-b1 &&
		git checkout -b b2 origin/b2 &&
		echo aa-b2 >>file &&
		git commit -a -m aa-b2 &&
		git checkout main &&
		echo aa-main >>file &&
		git commit -a -m aa-main
	) &&
	git checkout b1 &&
	echo b1 >>file &&
	git commit -a -m b1 &&
	git checkout b2 &&
	echo b2 >>file &&
	git commit -a -m b2 &&
	git checkout main
'

test_expect_success 'push from clone to origin updates main' '
	(
		cd aa &&
		git push origin main
	) &&
	main_in_aa=$(cd aa && git rev-parse main) &&
	main_here=$(git rev-parse main) &&
	test "$main_in_aa" = "$main_here"
'

test_done
