#!/bin/sh

test_description='Test ls-files with various directory structures'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'ls-files with deep directory structures' '
	mkdir -p a/b/c/d &&
	echo content >a/b/c/d/file &&
	git add a/b/c/d/file &&
	git ls-files >out &&
	grep "a/b/c/d/file" out
'

test_expect_success 'ls-files with many files in same directory' '
	for i in $(test_seq 1 20); do
		echo "file $i" >file_$i
	done &&
	git add file_* &&
	git ls-files >out &&
	test_line_count -ge 20 out
'

test_expect_success 'ls-files --stage with many files' '
	git ls-files --stage >out &&
	test_line_count -ge 20 out &&
	grep "^100644" out
'

test_expect_success 'update-index --index-info with many entries' '
	blob=$(echo test | git hash-object -t blob -w --stdin) &&
	for i in $(test_seq 1 10); do
		echo "100644 $blob	batch_$i"
	done | git update-index --index-info &&
	git ls-files >out &&
	grep "batch_1" out &&
	grep "batch_10" out
'

test_done
