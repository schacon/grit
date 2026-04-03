#!/bin/sh

test_description='Test criss-cross merge setup'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'prepare repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	test_write_lines 1 2 3 4 5 6 7 8 9 >file &&
	git add file &&
	git commit -m "Initial commit"
'

test_expect_success 'create divergent branches' '
	cd repo &&
	git branch A &&
	git branch B &&
	git checkout A &&
	test_write_lines 1 2 3 4 5 6 7 "8 changed in A" 9 >file &&
	git add file &&
	git commit -m "A changes line 8" &&
	git checkout B &&
	test_write_lines 1 2 "3 changed in B" 4 5 6 7 8 9 >file &&
	git add file &&
	git commit -m "B changes line 3"
'

test_expect_success 'simple merge succeeds' '
	cd repo &&
	git checkout A &&
	git merge B &&
	grep "3 changed in B" file &&
	grep "8 changed in A" file
'

test_done
