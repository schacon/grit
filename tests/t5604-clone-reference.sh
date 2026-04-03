#!/bin/sh

test_description='test clone --reference'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

commit_in () {
	(
		cd "$1" &&
		echo "$2" >"$2" &&
		git add "$2" &&
		git commit -m "$2"
	)
}

test_expect_success 'preparing first repository' '
	test_create_repo A &&
	commit_in A file1
'

test_expect_success 'preparing second repository' '
	git clone A B &&
	commit_in B file2 &&
	git -C B repack -ad &&
	git -C B prune
'

test_expect_failure 'cloning with reference (-l -s)' '
	git clone -l -s --reference B A C
'

test_expect_failure 'existence of info/alternates' '
	test_line_count = 2 C/.git/objects/info/alternates
'

test_expect_success 'cloning without reference' '
	rm -rf D &&
	git clone A D
'

test_expect_success 'cloned repo has objects' '
	git -C D log --oneline >actual &&
	test_line_count = 1 actual
'

test_done
