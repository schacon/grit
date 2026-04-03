#!/bin/sh

test_description='git update-index for various file types'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'update-index --add adds regular file' '
	echo hello >file1 &&
	git update-index --add file1 &&
	git ls-files --stage file1 >out &&
	grep "100644" out &&
	grep "file1" out
'

test_expect_success 'update-index --add adds multiple files' '
	echo a >file2 &&
	echo b >file3 &&
	git update-index --add file2 file3 &&
	git ls-files --stage >out &&
	grep "file2" out &&
	grep "file3" out
'

test_expect_success 'update-index --remove removes entry' '
	rm file3 &&
	git update-index --remove file3 &&
	git ls-files --stage >out &&
	! grep "file3" out
'

test_expect_success 'update-index --force-remove removes entry' '
	git update-index --force-remove file2 &&
	git ls-files --stage >out &&
	! grep "file2" out
'

test_done
