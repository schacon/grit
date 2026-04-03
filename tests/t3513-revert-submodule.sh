#!/bin/sh

test_description='revert basic tests'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo initial >file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	git tag initial &&

	echo changed >file &&
	git add file &&
	test_tick &&
	git commit -m changed &&
	git tag changed &&

	echo more >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m "add file2" &&
	git tag added
'

test_expect_success 'revert undoes a commit' '
	git revert changed &&
	echo initial >expect &&
	test_cmp expect file
'

test_expect_success 'revert with --no-commit stages changes' '
	git reset --hard added &&
	git revert --no-commit added &&
	test_path_is_missing file2 &&
	git revert --abort || git reset --hard
'

test_expect_success 'revert creates proper message' '
	git reset --hard added &&
	git revert added &&
	git log --format=%s --max-count=1 >actual &&
	grep "Revert" actual
'

test_expect_success 'revert detects conflict' '
	git reset --hard added &&
	echo conflict >file &&
	git add file &&
	test_tick &&
	git commit -m conflict &&
	test_must_fail git revert changed
'

test_done
