#!/bin/sh

test_description='Test handling of overwriting untracked files'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	test_commit init &&

	git branch stable &&
	git branch work &&

	git checkout work &&
	test_commit foo &&

	git checkout stable
'

test_expect_success 'reset --hard will nuke untracked files/dirs' '
	mkdir foo.t &&
	echo precious >foo.t/file &&
	printf "%s" foo >expect &&

	git reset --hard work &&

	# check that untracked directory foo.t/ was nuked
	test_path_is_file foo.t &&
	test_cmp expect foo.t
'

test_expect_success 'checkout between branches preserves untracked files in other paths' '
	git checkout stable &&
	echo untracked >untracked_file &&
	git checkout work &&
	test_path_is_file untracked_file &&
	echo untracked >expect &&
	test_cmp expect untracked_file
'

test_done
