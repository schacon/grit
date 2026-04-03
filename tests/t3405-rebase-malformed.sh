#!/bin/sh

test_description='rebase should handle arbitrary git message'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success setup '
	git init -q &&

	>file1 &&
	>file2 &&
	git add file1 file2 &&
	test_tick &&
	git commit -m "Initial commit" &&

	git checkout -b multi-line-subject &&
	cat >file2 <<-\MSGEOF &&
	This is an example of a commit log message
	that does not  conform to git commit convention.

	It has two paragraphs, but its first paragraph is not friendly
	to oneline summary format.
	MSGEOF
	git add file2 &&
	test_tick &&
	git commit -m "This is an example of a commit log message
that does not  conform to git commit convention.

It has two paragraphs, but its first paragraph is not friendly
to oneline summary format." &&

	git cat-file commit HEAD | sed -e "1,/^\$/d" >F0 &&

	git checkout main &&

	echo One >file1 &&
	test_tick &&
	git add file1 &&
	git commit -m "Second commit"
'

test_expect_success 'rebase commit with multi-line subject' '
	git checkout multi-line-subject &&
	git rebase main &&
	git cat-file commit HEAD | sed -e "1,/^\$/d" >F1 &&
	test_cmp F0 F1
'

test_done
