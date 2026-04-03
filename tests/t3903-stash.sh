#!/bin/sh
#
# Copyright (c) 2007 Johannes E Schindelin
#

test_description='Test git stash'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'stash some dirty working directory' '
	echo 1 >file &&
	git add file &&
	echo unrelated >other-file &&
	git add other-file &&
	test_tick &&
	git commit -m initial &&
	echo 2 >file &&
	git add file &&
	echo 3 >file &&
	test_tick &&
	git stash &&
	git diff-files --quiet &&
	git diff-index --cached --quiet HEAD
'

test_expect_success 'applying bogus stash does nothing' '
	test_must_fail git stash apply stash@{1} &&
	echo 1 >expect &&
	test_cmp expect file
'

test_expect_success 'apply does not need clean working directory' '
	echo 4 >other-file &&
	git stash apply &&
	echo 3 >expect &&
	test_cmp expect file
'

test_expect_success 'apply does not clobber working directory changes' '
	git reset --hard &&
	echo 4 >file &&
	test_must_fail git stash apply &&
	echo 4 >expect &&
	test_cmp expect file
'

test_expect_success 'unstashing in a subdirectory' '
	git reset --hard HEAD &&
	mkdir subdir &&
	(
		cd subdir &&
		git stash apply
	)
'

test_expect_success 'stash drop complains of extra options' '
	test_must_fail git stash drop --foo
'

test_expect_success 'stash branch' '
	git reset --hard &&
	echo second-content >file &&
	git add file &&
	test_tick &&
	git commit -m second &&
	echo bar >file &&
	git stash &&
	git stash branch stashbranch &&
	test bar = $(cat file)
'

test_expect_success 'stash show' '
	git reset --hard &&
	git checkout main &&
	echo show-test >file &&
	git stash &&
	git stash show >output 2>&1 &&
	test -s output
'

test_expect_success 'stash list' '
	git stash list >output &&
	test_line_count -ge 1 output
'

test_expect_success 'stash clear' '
	git stash clear &&
	test 0 = $(git stash list | wc -l)
'

test_expect_success 'stash push with pathspec' '
	git reset --hard &&
	echo change1 >file &&
	echo change2 >other-file &&
	git stash push -- file &&
	test change2 = "$(cat other-file)" &&
	git checkout -- other-file &&
	git stash pop &&
	test change1 = "$(cat file)"
'

test_expect_success 'stash with message' '
	git reset --hard &&
	echo msg-test >file &&
	git stash push -m "custom message" &&
	git stash list | grep "custom message"
'

test_expect_success 'stash apply and pop' '
	git reset --hard &&
	echo poptest >file &&
	git stash &&
	git stash apply &&
	test poptest = "$(cat file)" &&
	git reset --hard &&
	git stash pop &&
	test poptest = "$(cat file)"
'

test_expect_success 'stash multiple times and list them' '
	git stash clear &&
	git reset --hard &&
	echo first-stash >file &&
	git stash &&
	echo second-stash >file &&
	git stash &&
	test 2 = $(git stash list | wc -l)
'

test_expect_success 'stash drop removes correct stash' '
	git stash drop stash@{0} &&
	test 1 = $(git stash list | wc -l) &&
	git stash pop &&
	test first-stash = "$(cat file)"
'

test_done
