#!/bin/sh

test_description='merging with file additions'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup (initial)' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	touch file &&
	git add . &&
	git commit -m initial &&
	git tag initial
'

test_expect_success 'create branches with different files' '
	cd repo &&
	git checkout -b feature &&
	echo feature >feature-file &&
	git add feature-file &&
	git commit -m "add feature file" &&
	git checkout main &&
	echo main >main-file &&
	git add main-file &&
	git commit -m "add main file"
'

test_expect_success 'merge branches with non-overlapping files' '
	cd repo &&
	git merge feature &&
	test_path_is_file feature-file &&
	test_path_is_file main-file
'

test_expect_success 'setup large simple rename' '
	cd repo &&
	git reset --hard initial &&
	i=1 &&
	while test $i -le 20; do
		echo "content $i" >file-$i || return 1
		i=$(($i + 1))
	done &&
	git add . &&
	git commit -m create-files &&

	git branch simple-change &&
	git checkout -b simple-add &&
	echo extra >extra-file &&
	git add extra-file &&
	git commit -m add-extra &&

	git checkout simple-change &&
	echo change >>file &&
	git add file &&
	git commit -m simple-change
'

test_expect_success 'merge with many files succeeds' '
	cd repo &&
	git checkout simple-add &&
	git merge simple-change
'

test_done
