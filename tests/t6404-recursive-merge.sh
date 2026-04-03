#!/bin/sh

test_description='Test merge without common ancestors'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup tests' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&

	echo 1 >a1 &&
	git add a1 &&
	GIT_AUTHOR_DATE="2006-12-12 23:00:00" git commit -m 1 &&

	git checkout -b A main &&
	echo A >a1 &&
	git add a1 &&
	GIT_AUTHOR_DATE="2006-12-12 23:00:01" git commit -m A &&

	git checkout -b B main &&
	echo B >a1 &&
	git add a1 &&
	GIT_AUTHOR_DATE="2006-12-12 23:00:02" git commit -m B &&

	git checkout -b D A &&
	git rev-parse B >.git/MERGE_HEAD &&
	echo D >a1 &&
	git update-index a1 &&
	GIT_AUTHOR_DATE="2006-12-12 23:00:03" git commit -m D &&

	git symbolic-ref HEAD refs/heads/other &&
	echo 2 >a1 &&
	GIT_AUTHOR_DATE="2006-12-12 23:00:04" git commit -m 2 &&

	git checkout -b C &&
	echo C >a1 &&
	git add a1 &&
	GIT_AUTHOR_DATE="2006-12-12 23:00:05" git commit -m C &&

	git checkout -b E C &&
	git rev-parse B >.git/MERGE_HEAD &&
	echo E >a1 &&
	git update-index a1 &&
	GIT_AUTHOR_DATE="2006-12-12 23:00:06" git commit -m E &&

	git checkout -b G E &&
	git rev-parse A >.git/MERGE_HEAD &&
	echo G >a1 &&
	git update-index a1 &&
	GIT_AUTHOR_DATE="2006-12-12 23:00:07" git commit -m G &&

	git checkout -b F D &&
	git rev-parse C >.git/MERGE_HEAD &&
	echo F >a1 &&
	git update-index a1 &&
	GIT_AUTHOR_DATE="2006-12-12 23:00:08" git commit -m F
'

test_expect_success 'combined merge conflicts' '
	cd repo &&
	test_must_fail git merge -m final G
'

test_expect_success 'result contains a conflict' '
	cd repo &&
	grep "^<<<<<<< " a1 &&
	grep "^=======" a1 &&
	grep "^>>>>>>> " a1 &&
	grep "F" a1 &&
	grep "G" a1
'

test_expect_success 'refuse to merge binary files' '
	cd repo &&
	git reset --hard &&
	git checkout F &&
	printf "\0" >binary-file &&
	git add binary-file &&
	test_tick &&
	git commit -m binary &&
	git checkout G &&
	printf "\0\0" >binary-file &&
	git add binary-file &&
	test_tick &&
	git commit -m binary2 &&
	test_must_fail git merge F
'

test_done
