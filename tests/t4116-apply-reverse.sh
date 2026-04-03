#!/bin/sh

test_description='git apply in reverse'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success setup '
	test_write_lines a b c d e f g h i j k l m n >file1 &&
	git add file1 &&
	git commit -m initial &&
	git tag initial &&
	test_write_lines a b c g h i J K L m o n p q >file1 &&
	git commit -a -m second &&
	git tag second &&
	git diff initial second >patch
'

test_expect_success 'apply in forward' '
	git reset --hard initial &&
	git apply patch &&
	test_cmp file1 file1
'

test_expect_success 'apply in reverse' '
	git reset --hard second &&
	git apply --reverse patch &&
	git diff initial >diff &&
	test_must_be_empty diff
'

test_expect_success 'reversing a whitespace introduction' '
	git reset --hard initial &&
	sed "s/a/a /" < file1 > file1.new &&
	mv file1.new file1 &&
	git diff > ws-patch &&
	git checkout -- file1 &&
	echo "a " > file1.new &&
	test_write_lines b c d e f g h i j k l m n >> file1.new &&
	mv file1.new file1 &&
	git apply --reverse ws-patch
'

test_done
