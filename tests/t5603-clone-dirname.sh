#!/bin/sh

test_description='check output directory names used by git-clone'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo content >file &&
	git add file &&
	git commit -m initial
'

# Test that cloning produces the right directory name
test_expect_success 'clone into default directory' '
	git clone . my-repo &&
	test_path_is_dir my-repo
'

test_expect_success 'clone with trailing slash' '
	rm -rf my-repo &&
	mkdir src &&
	git clone --bare . src/repo.git &&
	git clone src/repo.git my-repo &&
	test_path_is_dir my-repo
'

test_expect_success 'clone strips .git suffix for directory name' '
	rm -rf repo &&
	git clone src/repo.git &&
	test_path_is_dir repo
'

test_expect_success 'clone bare adds .git suffix' '
	rm -rf repo.git &&
	git clone --bare . repo.git &&
	test_path_is_dir repo.git
'

test_done
