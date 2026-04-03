#!/bin/sh

test_description='detect unwritable repository and fail correctly'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q
'

test_expect_success 'setup' '
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	>file &&
	git add file &&
	git commit -m initial &&
	echo content >file &&
	git add file
'

test_expect_success 'write-tree should notice unwritable repository' '
	test_when_finished "chmod 775 .git/objects .git/objects/??" &&
	chmod a-w .git/objects .git/objects/?? &&
	test_must_fail git write-tree 2>out.write-tree
'

test_expect_success 'commit should notice unwritable repository' '
	test_when_finished "chmod 775 .git/objects .git/objects/??" &&
	chmod a-w .git/objects .git/objects/?? &&
	test_must_fail git commit -m second 2>out.commit
'

test_expect_success 'add should notice unwritable repository' '
	test_when_finished "chmod 775 .git/objects .git/objects/??" &&
	echo b >file &&
	chmod a-w .git/objects .git/objects/?? &&
	test_must_fail git add file 2>out.add
'

test_done
