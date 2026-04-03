#!/bin/sh

test_description='git rm basic tests'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	mkdir -p sub/dir &&
	echo a >a &&
	echo b >b &&
	echo c >c &&
	echo d >sub/d &&
	echo e >sub/dir/e &&
	git add -A &&
	test_tick &&
	git commit -m files
'

test_expect_success 'rm removes file from index and worktree' '
	git rm a &&
	test_path_is_missing a &&
	test_must_fail git ls-files --error-unmatch a
'

test_expect_success 'rm --cached removes from index but keeps file' '
	echo new-b >b &&
	git add b &&
	git rm --cached b &&
	test_path_is_file b &&
	test_must_fail git ls-files --error-unmatch b
'

test_expect_success 'rm -r removes directory recursively' '
	git reset --hard &&
	git rm -r sub &&
	test_path_is_missing sub/d &&
	test_path_is_missing sub/dir/e
'

test_expect_success 'rm with --dry-run does not remove' '
	git reset --hard &&
	git rm --dry-run c >output &&
	test_path_is_file c &&
	git ls-files --error-unmatch c
'

test_expect_success 'rm --quiet suppresses output' '
	git reset --hard &&
	git rm --quiet a 2>err &&
	test_must_be_empty err
'

test_expect_success 'rm --ignore-unmatch exits zero for missing file' '
	git rm --ignore-unmatch nonexistent
'

test_expect_success 'rm nonexistent file fails' '
	test_must_fail git rm nonexistent
'

test_done
