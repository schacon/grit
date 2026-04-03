#!/bin/sh
test_description='git rev-list trivial path optimization test'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_success setup '
	echo Hello >a &&
	mkdir d &&
	echo World >d/f &&
	echo World >d/z &&
	git add a d &&
	test_tick &&
	git commit -m "Initial commit" &&
	git rev-parse --verify HEAD &&
	git tag initial
'

test_expect_success 'further setup' '
	git checkout -b side &&
	echo Irrelevant >c &&
	echo Irrelevant >d/f &&
	git add c d/f &&
	test_tick &&
	git commit -m "Side makes an irrelevant commit" &&
	echo "More Irrelevancy" >c &&
	git add c &&
	test_tick &&
	git commit -m "Side makes another irrelevant commit" &&
	echo Bye >a &&
	git add a &&
	test_tick &&
	git commit -m "Side touches a" &&
	echo "Yet more Irrelevancy" >c &&
	git add c &&
	test_tick &&
	git commit -m "Side makes yet another irrelevant commit" &&
	git checkout main &&
	echo Another >b &&
	echo Munged >d/z &&
	git add b d/z &&
	test_tick &&
	git commit -m "Main touches b" &&
	git merge side &&
	echo Touched >b &&
	git add b &&
	test_tick &&
	git commit -m "Main touches b again"
'

test_expect_success 'rev-list counts commits' '
	test $(git rev-list HEAD | wc -l) = 8
'

test_done
