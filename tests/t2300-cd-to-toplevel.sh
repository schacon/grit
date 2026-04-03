#!/bin/sh

test_description='rev-parse --show-toplevel from subdirectories'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo content >file &&
	git add file &&
	git commit -m initial &&
	mkdir -p sub/dir
'

test_expect_success 'rev-parse --show-toplevel from top' '
	echo "$(pwd)" >expect &&
	git rev-parse --show-toplevel >actual &&
	test_cmp expect actual
'

test_expect_success 'rev-parse --show-toplevel from subdir' '
	echo "$(pwd)" >expect &&
	(
		cd sub/dir &&
		git rev-parse --show-toplevel >../../actual
	) &&
	test_cmp expect actual
'

test_expect_success 'rev-parse --git-dir from top' '
	echo .git >expect &&
	git rev-parse --git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'rev-parse --git-dir from subdir' '
	(
		cd sub/dir &&
		git rev-parse --git-dir >../../actual
	) &&
	echo ../../.git >expect &&
	test_cmp expect actual
'

test_done
