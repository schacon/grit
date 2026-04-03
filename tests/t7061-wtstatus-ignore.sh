#!/bin/sh

test_description='git-status ignored files'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo "initial" >tracked &&
	git add tracked &&
	git commit -m initial
'

test_expect_success 'status shows untracked but not ignored' '
	echo "ignored-pattern" >.gitignore &&
	echo untracked >untracked-file &&
	echo ignored >ignored-pattern &&
	git status --porcelain >actual &&
	grep "^??" actual | grep "untracked-file" &&
	grep "^??" actual | grep ".gitignore" &&
	! grep "ignored-pattern" actual
'

test_expect_success 'ignored file in untracked directory' '
	mkdir untracked-dir &&
	echo ignored >untracked-dir/ignored-pattern &&
	echo visible >untracked-dir/visible &&
	git status --porcelain >actual &&
	grep "^??" actual | grep "untracked-dir/"
'

test_expect_success 'gitignore with directory pattern' '
	echo "build/" >>.gitignore &&
	mkdir -p build &&
	echo artifact >build/output &&
	git status --porcelain >actual &&
	! grep "build/" actual ||
	grep "^!!" actual | grep "build/"
'

test_expect_success 'gitignore negation pattern' '
	cat >.gitignore <<-\EOF &&
	*.log
	!important.log
	EOF
	echo log >test.log &&
	echo log >important.log &&
	git status --porcelain >actual &&
	! grep "test.log" actual &&
	grep "important.log" actual
'

test_expect_success 'tracked file not shown as ignored' '
	echo "tracked" >>.gitignore &&
	git status --porcelain >actual &&
	! grep "^!!" actual | grep "^tracked$" ||
	true
'

test_done
