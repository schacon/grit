#!/bin/sh

test_description='git status porcelain output'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo x >file_x &&
	echo y >file_y &&
	echo z >file_z &&
	mkdir dir1 &&
	echo a >dir1/file_a &&
	echo b >dir1/file_b
'

test_expect_success 'before initial commit, only untracked shown' '
	git status --porcelain >actual &&
	grep "^??" actual | grep "file_x" &&
	grep "^??" actual | grep "file_y" &&
	grep "^??" actual | grep "file_z" &&
	grep "^??" actual | grep "dir1/"
'

test_expect_success 'after adding files, staged shown' '
	git add file_x file_y file_z dir1 &&
	git status --porcelain >actual &&
	grep "^A " actual | grep "file_x" &&
	grep "^A " actual | grep "dir1/file_a"
'

test_expect_success 'after commit, clean status' '
	git commit -m initial &&
	git status --porcelain >actual &&
	! grep "^[AMDRC]" actual
'

test_expect_success 'modified file shown correctly' '
	echo xx >>file_x &&
	git status --porcelain >actual &&
	grep "^ M\|^.M" actual | grep "file_x"
'

test_expect_success 'deleted file shown correctly' '
	rm file_z &&
	git status --porcelain >actual &&
	grep "^ D\|^.D" actual | grep "file_z"
'

test_expect_success 'staged modification shown' '
	echo yy >>file_y &&
	git add file_y &&
	git status --porcelain >actual &&
	grep "^M " actual | grep "file_y"
'

test_expect_success 'branch info with -b flag' '
	git status --porcelain -b >actual &&
	grep "^## " actual
'

test_expect_success 'status with -z uses NUL terminator' '
	git status --porcelain -z >actual &&
	# NUL-terminated output should exist
	test -s actual
'

test_done
