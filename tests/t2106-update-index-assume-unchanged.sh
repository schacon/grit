#!/bin/sh

test_description='git update-index --assume-unchanged test.
'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	: >file &&
	git add file &&
	git commit -m initial &&
	git branch other &&
	echo upstream >file &&
	git add file &&
	git commit -m upstream
'

test_expect_success 'assume-unchanged bit can be set and unset' '
	git update-index --assume-unchanged file &&
	git ls-files -t file >out &&
	echo "h file" >expect &&
	test_cmp expect out &&
	git update-index --no-assume-unchanged file &&
	git ls-files -t file >out &&
	echo "H file" >expect &&
	test_cmp expect out
'

# The next test requires checkout to detect dirty files
# which may not be fully implemented in grit
test_expect_failure 'do not switch branches with dirty file' '
	git reset --hard &&
	git checkout other &&
	echo dirt >file &&
	git update-index --assume-unchanged file &&
	test_must_fail git checkout - 2>err &&
	test_grep overwritten err
'

test_done
