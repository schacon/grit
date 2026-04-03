#!/bin/sh

test_description='Test diff of symlinks.'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup commits with symlink and regular file' '
	echo content >regular &&
	git add regular &&
	git commit -m "add regular" &&
	git tag c1 &&
	ln -s target link &&
	blob=$(printf "target" | git hash-object -w --stdin) &&
	git update-index --add --cacheinfo 120000,$blob,link &&
	git commit -m "add link" &&
	git tag c2
'

test_expect_success 'diff-tree shows symlink addition' '
	git diff-tree -r --name-only c1 c2 >output &&
	grep "link" output
'

test_expect_success 'diff-tree shows raw mode for symlink' '
	git diff-tree -r c1 c2 >output &&
	grep "120000" output
'

test_expect_success 'diff shows worktree changes' '
	echo "more content" >regular &&
	git diff --stat >output &&
	grep "regular" output
'

test_done
