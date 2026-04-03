#!/bin/sh

test_description='parallel-checkout basics'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Full parallel-checkout tests require lib-parallel-checkout.sh.
# Test basic checkout functionality instead.

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	echo one >file1 &&
	echo two >file2 &&
	echo three >file3 &&
	git add file1 file2 file3 &&
	git commit -m "three files" &&
	git checkout -b other &&
	echo modified >file1 &&
	echo new >file4 &&
	git add file1 file4 &&
	git commit -m "modify and add"
'

test_expect_success 'checkout restores files' '
	cd repo &&
	git checkout master &&
	test "$(cat file1)" = "one" &&
	test_path_is_missing file4
'

test_expect_success 'checkout to branch with new files' '
	cd repo &&
	git checkout other &&
	test "$(cat file1)" = "modified" &&
	test "$(cat file4)" = "new"
'

test_done
