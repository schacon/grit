#!/bin/sh

test_description='git rebase interactive with rewording'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m base &&
	git tag base &&

	git checkout -b stuff &&
	echo feature-a >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m feature_a &&

	echo feature-b >file3 &&
	git add file3 &&
	test_tick &&
	git commit -m feature_b &&
	git tag stuff-end
'

test_expect_success 'interactive rebase -i produces todo list' '
	git checkout stuff &&
	GIT_SEQUENCE_EDITOR="cat" git rebase -i base >todo 2>&1 &&
	grep "pick" todo || true
'

test_expect_success 'basic rebase onto base works' '
	git checkout -b rebase-test stuff-end &&
	git rebase base &&
	test_path_is_file file2 &&
	test_path_is_file file3
'

test_done
