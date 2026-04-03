#!/bin/sh
#
# Copyright (C) 2005 Rene Scharfe
#

test_description='git archive test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "file content" >file.txt &&
	mkdir subdir &&
	echo "subdir content" >subdir/file2.txt &&
	git add file.txt subdir/file2.txt &&
	test_tick &&
	git commit -m initial &&
	git tag v1.0
'

test_expect_success 'archive HEAD as tar' '
	git archive HEAD >archive.tar &&
	test -s archive.tar
'

test_expect_success 'archive by commit hash' '
	commit=$(git rev-parse HEAD) &&
	git archive $commit >commit-archive.tar &&
	test -s commit-archive.tar
'

test_expect_success 'archive contains correct files' '
	mkdir -p extract &&
	(cd extract && tar xf ../archive.tar) &&
	test_cmp file.txt extract/file.txt &&
	test_cmp subdir/file2.txt extract/subdir/file2.txt
'

test_expect_success 'archive with prefix' '
	git archive --prefix=project/ HEAD >prefix-archive.tar &&
	mkdir -p prefix-extract &&
	(cd prefix-extract && tar xf ../prefix-archive.tar) &&
	test_cmp file.txt prefix-extract/project/file.txt
'

test_expect_success 'archive specific path' '
	git archive HEAD -- file.txt >path-archive.tar &&
	mkdir -p path-extract &&
	(cd path-extract && tar xf ../path-archive.tar) &&
	test_cmp file.txt path-extract/file.txt &&
	test_path_is_missing path-extract/subdir
'

test_expect_success 'archive tree-ish works with tree hash' '
	tree=$(git rev-parse HEAD^{tree}) &&
	git archive $tree >tree-archive.tar &&
	test -s tree-archive.tar
'

test_expect_success 'archive --format=tar is default' '
	git archive --format=tar HEAD >format-archive.tar &&
	test_cmp archive.tar format-archive.tar
'

test_expect_success 'archive --format=zip produces zip' '
	git archive --format=zip HEAD >archive.zip &&
	test -s archive.zip
'

test_done
