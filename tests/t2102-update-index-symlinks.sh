#!/bin/sh

test_description='git update-index symlink handling'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'update-index --add a regular file' '
	echo content >regular &&
	git update-index --add regular &&
	git ls-files --stage regular >out &&
	grep "^100644" out
'

test_expect_success 'update-index --add a symlink' '
	ln -s regular symlink &&
	git update-index --add symlink &&
	git ls-files --stage symlink >out &&
	grep "symlink$" out
'

test_expect_success 'update-index with --index-info' '
	blob=$(echo hello | git hash-object -t blob -w --stdin) &&
	echo "100644 $blob	newfile" | git update-index --index-info &&
	git ls-files --stage newfile >out &&
	grep "newfile$" out &&
	grep "^100644" out
'

test_done
