#!/bin/sh

test_description='test git clone to cleanup after failure'

. ./test-lib.sh

test_expect_success 'clone of non-existent source should fail' '
	test_must_fail git clone foo bar
'

test_expect_success 'failed clone should not leave a directory' '
	test_path_is_missing bar
'

test_expect_success 'create a repo to clone' '
	test_create_repo foo
'

test_expect_success 'create objects in repo for later corruption' '
	(
		cd foo &&
		echo content >file &&
		git add file &&
		git commit -m "add file"
	)
'

test_expect_success 'clone should work now that source exists' '
	git clone foo bar
'

test_expect_success 'successful clone must leave the directory' '
	test_path_is_dir bar
'

test_done
