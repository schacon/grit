#!/bin/sh

test_description='sparse checkout scope tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo "initial" >a &&
	mkdir -p sub1 sub2 &&
	echo "sub1" >sub1/file &&
	echo "sub2" >sub2/file &&
	git add a sub1 sub2 &&
	git commit -m "initial commit"
'

test_expect_success 'sparse-checkout init enables sparse checkout' '
	git sparse-checkout init &&
	git config core.sparseCheckout >actual &&
	grep "true" actual
'

test_expect_success 'sparse-checkout set limits directories in working tree' '
	git sparse-checkout set sub1 &&
	test_path_is_file a &&
	test_path_is_file sub1/file &&
	test_path_is_missing sub2/file
'

test_expect_success 'sparse-checkout list shows current patterns' '
	git sparse-checkout set sub1 &&
	git sparse-checkout list >actual &&
	grep "sub1" actual
'

test_expect_success 'sparse-checkout set updates sparse-checkout file' '
	git sparse-checkout set sub1 sub2 &&
	cat .git/info/sparse-checkout >actual &&
	grep "sub1" actual &&
	grep "sub2" actual
'

test_expect_success 'sparse-checkout disable turns off sparse checkout' '
	git sparse-checkout disable &&
	test_path_is_file a &&
	test_path_is_file sub1/file &&
	test_path_is_file sub2/file
'

test_done
