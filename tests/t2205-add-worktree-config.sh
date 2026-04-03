#!/bin/sh

test_description='directory traversal respects user config

This tests that git add works with manually configured worktrees and
various directory structures.'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'add files from current directory' '
	echo content >tracked &&
	git add tracked &&
	git ls-files --stage tracked >out &&
	grep "tracked" out
'

test_expect_success 'add files from subdirectory' '
	mkdir -p sub/dir &&
	echo subcontent >sub/dir/file &&
	git add sub/dir/file &&
	git ls-files --stage sub/dir/file >out &&
	grep "sub/dir/file" out
'

test_expect_success 'add respects .gitignore' '
	echo "*.ignored" >.gitignore &&
	echo content >test.ignored &&
	test_must_fail git add test.ignored &&
	git ls-files --stage >out &&
	! grep "test.ignored" out
'

test_expect_success 'add with --force overrides .gitignore' '
	git add --force test.ignored &&
	git ls-files --stage >out &&
	grep "test.ignored" out
'

test_done
