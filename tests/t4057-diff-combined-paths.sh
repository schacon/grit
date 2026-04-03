#!/bin/sh

test_description='combined diff show only paths that are different to all parents'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo
'

test_expect_success 'trivial merge setup' '
	cd repo &&
	for i in $(test_seq 1 9)
	do
		echo $i >$i.txt &&
		git add $i.txt || return 1
	done &&
	git commit -m "init" &&
	git checkout -b side &&
	for i in $(test_seq 2 9)
	do
		echo $i/2 >>$i.txt || return 1
	done &&
	git commit -a -m "side 2-9" &&
	git checkout main &&
	echo 1/2 >1.txt &&
	git commit -a -m "main 1" &&
	git merge side
'

test_expect_success 'diff-tree shows changed files in merge' '
	cd repo &&
	git diff-tree --name-only HEAD >actual &&
	test -s actual
'

test_expect_success 'diff-tree -r lists all changed files recursively' '
	cd repo &&
	git diff-tree -r --name-only HEAD HEAD^ >actual &&
	test -s actual
'

test_done
