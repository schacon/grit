#!/bin/sh

test_description='stash basic operation tests'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo initial >file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	git tag initial
'

test_expect_success 'stash saves and restores changes' '
	git reset --hard initial &&
	git stash clear &&
	echo modified >file &&
	git stash &&
	echo initial >expect &&
	test_cmp expect file &&
	git stash pop &&
	echo modified >expect &&
	test_cmp expect file
'

test_expect_success 'stash list shows entries' '
	git reset --hard initial &&
	git stash clear &&
	echo change1 >file &&
	git stash &&
	git stash list >actual &&
	test_line_count = 1 actual &&
	git stash drop
'

test_expect_success 'multiple stashes stack correctly' '
	git reset --hard initial &&
	git stash clear &&
	echo change1 >file &&
	git stash push -m "first stash" &&
	echo change2 >file &&
	git stash push -m "second stash" &&
	git stash list >actual &&
	test_line_count = 2 actual &&
	git stash pop &&
	echo change2 >expect &&
	test_cmp expect file &&
	git reset --hard initial &&
	git stash pop &&
	echo change1 >expect &&
	test_cmp expect file
'

test_expect_success 'stash apply keeps stash entry' '
	git reset --hard initial &&
	git stash clear &&
	echo change >file &&
	git stash &&
	git stash apply &&
	git stash list >actual &&
	test_line_count = 1 actual &&
	git stash drop
'

test_expect_success 'stash drop removes entry' '
	git reset --hard initial &&
	git stash clear &&
	echo change >file &&
	git stash &&
	git stash drop &&
	git stash list >actual &&
	test_must_be_empty actual
'

test_expect_success 'stash clear removes all entries' '
	git reset --hard initial &&
	echo c1 >file &&
	git stash &&
	echo c2 >file &&
	git stash &&
	git stash clear &&
	git stash list >actual &&
	test_must_be_empty actual
'

test_expect_success 'stash with -k keeps index' '
	git reset --hard initial &&
	git stash clear &&
	echo staged >file &&
	git add file &&
	echo unstaged >file &&
	git stash push -k &&
	echo staged >expect &&
	test_cmp expect file &&
	git reset --hard initial &&
	git stash pop
'

test_done
