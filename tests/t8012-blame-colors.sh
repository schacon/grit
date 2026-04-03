#!/bin/sh

test_description='git blame color output (basic)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init blame-colors &&
	cd blame-colors &&
	echo "line 1" >hello.c &&
	echo "line 2" >>hello.c &&
	echo "line 3" >>hello.c &&
	git add hello.c &&
	GIT_AUTHOR_NAME="Author F" git commit -m "first" &&

	echo "line 4" >>hello.c &&
	echo "line 5" >>hello.c &&
	git add hello.c &&
	GIT_AUTHOR_NAME="Author H" git commit -m "second" &&

	echo "line 6" >>hello.c &&
	git add hello.c &&
	GIT_AUTHOR_NAME="Author F" git commit -m "third"
'

test_expect_success 'blame works on multi-author file' '
	cd blame-colors &&
	git blame hello.c >output &&
	grep "Author F" output &&
	grep "Author H" output
'

test_expect_success 'blame --porcelain on multi-author file' '
	cd blame-colors &&
	git blame --porcelain hello.c >output &&
	grep "^author Author F" output &&
	grep "^author Author H" output
'

test_expect_success 'blame --line-porcelain counts correct lines' '
	cd blame-colors &&
	git blame --line-porcelain hello.c >output &&
	grep "^author " output >authors &&
	test $(wc -l <authors) -eq 6
'

test_done
