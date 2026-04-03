#!/bin/sh

test_description='git blame basic tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init blame-basic &&
	cd blame-basic &&

	echo "bin: test number 1" >one &&
	git add one &&
	GIT_AUTHOR_NAME=name1 \
	GIT_AUTHOR_EMAIL=email1@test.git \
	git commit -m First &&

	echo "line 1" >file &&
	echo "line 2" >>file &&
	git add file &&
	test_tick &&
	GIT_AUTHOR_NAME="Author A" git commit -m "add file" &&

	echo "line 3" >>file &&
	echo "line 4" >>file &&
	git add file &&
	test_tick &&
	GIT_AUTHOR_NAME="Author B" git commit -m "extend file"
'

test_expect_success 'blame runs on file' '
	cd blame-basic &&
	git blame file >actual &&
	test $(wc -l <actual) -eq 4
'

test_expect_success 'blame -s suppresses author' '
	cd blame-basic &&
	git blame -s file >actual &&
	! grep "Author" actual
'

test_expect_success 'blame --porcelain works' '
	cd blame-basic &&
	git blame --porcelain file >actual &&
	grep "^author Author A" actual &&
	grep "^author Author B" actual
'

test_expect_success 'blame --line-porcelain works' '
	cd blame-basic &&
	git blame --line-porcelain file >actual &&
	grep "^author " actual >authors &&
	test $(wc -l <authors) -eq 4
'

test_expect_success 'blame -e shows email' '
	cd blame-basic &&
	git blame -e one >actual &&
	grep "email1@test.git" actual
'

test_expect_success 'blame --show-email shows email' '
	cd blame-basic &&
	git blame --show-email one >actual &&
	grep "email1@test.git" actual
'

test_expect_success 'blame -l shows full hash' '
	cd blame-basic &&
	git blame -l file >actual &&
	head -1 actual >first_line &&
	sha=$(cat first_line | sed "s/^\([0-9a-f]*\).*/\1/") &&
	test ${#sha} -eq 40
'

test_expect_success 'blame with -L range' '
	cd blame-basic &&
	git blame -L 1,2 file >actual &&
	test $(wc -l <actual) -eq 2
'

test_expect_success 'blame shows correct content' '
	cd blame-basic &&
	git blame file >actual &&
	grep "line 1" actual &&
	grep "line 2" actual &&
	grep "line 3" actual &&
	grep "line 4" actual
'

test_done
