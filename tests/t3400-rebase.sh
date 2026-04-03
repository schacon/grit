#!/bin/sh
#
# Copyright (c) 2005 Amos Waterland
#

test_description='git rebase assorted tests

This test runs git rebase and checks that the author information is not lost
among other things.
'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

GIT_AUTHOR_NAME="author@name"
GIT_AUTHOR_EMAIL="bogus@email@address"
export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL

test_expect_success 'prepare repository with topic branches' '
	echo First >A &&
	git add A &&
	test_tick &&
	git commit -m "Add A." &&
	git tag first &&

	git checkout -b force-3way &&
	echo Dummy >Y &&
	git update-index --add Y &&
	git commit -m "Add Y." &&

	git checkout -b filemove &&
	git reset --soft main &&
	mkdir D &&
	git mv A D/A &&
	git commit -m "Move A." &&

	git checkout -b my-topic-branch main &&
	echo Second >B &&
	git add B &&
	test_tick &&
	git commit -m "Add B." &&
	git tag second &&

	git checkout -f main &&
	echo Third >>A &&
	git update-index A &&
	git commit -m "Modify A." &&

	git checkout -b side my-topic-branch &&
	echo Side >>C &&
	git add C &&
	git commit -m "Add C" &&

	git checkout -f my-topic-branch &&
	git tag topic
'

test_expect_success 'rebase against main' '
	git reset --hard HEAD &&
	git rebase main
'

test_expect_success 'the rebase operation should not have destroyed author information' '
	! (git log | grep "Author:" | grep "<>")
'

test_expect_success 'rebase from ambiguous branch name' '
	git checkout -b topic side &&
	git rebase main
'

test_expect_success 'rebase sets ORIG_HEAD to pre-rebase state' '
	git checkout -b orig-head topic &&
	pre="$(git rev-parse --verify HEAD)" &&
	git rebase main &&
	orig="$(git rev-parse --verify ORIG_HEAD)" &&
	test "$pre" = "$orig"
'

test_expect_success 'rebase produces linear history' '
	git checkout my-topic-branch &&
	git reset --hard second &&
	git rebase main &&
	git log --oneline HEAD >commits &&
	main_count=$(git log --oneline main | wc -l | tr -d " ") &&
	head_count=$(wc -l <commits | tr -d " ") &&
	test "$head_count" -gt "$main_count"
'

test_done
