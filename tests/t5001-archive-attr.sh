#!/bin/sh

test_description='git archive with various tree structures'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "content" >included.txt &&
	echo "more" >another.txt &&
	mkdir subdir &&
	echo "sub content" >subdir/file.txt &&
	mkdir subdir/deeper &&
	echo "deep content" >subdir/deeper/deep.txt &&
	git add included.txt another.txt subdir &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'archive includes all tracked files' '
	git archive HEAD >all.tar &&
	mkdir -p all-extract &&
	(cd all-extract && tar xf ../all.tar) &&
	test_path_is_file all-extract/included.txt &&
	test_path_is_file all-extract/another.txt &&
	test_path_is_file all-extract/subdir/file.txt &&
	test_path_is_file all-extract/subdir/deeper/deep.txt
'

test_expect_success 'archive with prefix adds directory prefix' '
	git archive --prefix=myproject/ HEAD >prefix.tar &&
	mkdir -p prefix-extract &&
	(cd prefix-extract && tar xf ../prefix.tar) &&
	test_path_is_file prefix-extract/myproject/included.txt &&
	test_path_is_file prefix-extract/myproject/subdir/file.txt
'

test_expect_success 'archive of specific path' '
	git archive HEAD -- subdir >subdir.tar &&
	mkdir -p subdir-extract &&
	(cd subdir-extract && tar xf ../subdir.tar) &&
	test_path_is_file subdir-extract/subdir/file.txt &&
	test_path_is_missing subdir-extract/included.txt
'

test_expect_success 'archive preserves file content' '
	git archive HEAD >content.tar &&
	mkdir -p content-extract &&
	(cd content-extract && tar xf ../content.tar) &&
	test_cmp included.txt content-extract/included.txt &&
	test_cmp subdir/file.txt content-extract/subdir/file.txt
'

test_expect_success 'archive of tree hash works' '
	tree=$(git rev-parse HEAD^{tree}) &&
	git archive $tree >tree.tar &&
	test -s tree.tar
'

test_done
