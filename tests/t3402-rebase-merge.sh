#!/bin/sh

test_description='git rebase basic merge test'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

T="A quick brown fox
jumps over the lazy dog."

test_expect_success setup '
	git init -q &&
	for i in 1 2 3 4 5 6 7 8 9 10
	do
		echo "$i $T"
	done >original &&
	git add original &&
	git commit -m "initial" &&
	git branch side &&
	echo "11 $T" >>original &&
	git commit -a -m "main updates a bit." &&

	echo "12 $T" >>original &&
	git commit -a -m "main updates a bit more." &&

	git checkout side &&
	echo "0 $T" >renamed &&
	cat original >>renamed &&
	git add renamed &&
	git commit -a -m "side adds renamed file"
'

test_expect_success 'rebase side onto main' '
	git checkout side &&
	git rebase main &&
	test -f renamed &&
	test -f original
'

test_expect_success 'rebase is on top of main' '
	git rev-parse main >expect_base &&
	git rev-parse HEAD^ >actual_base &&
	test_cmp expect_base actual_base
'

test_expect_success 'side branch has correct content after rebase' '
	git checkout side &&
	test -f renamed &&
	test -f original &&
	grep "12" original
'

test_done
