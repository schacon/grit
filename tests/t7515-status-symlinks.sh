#!/bin/sh

test_description='git status and symlinks'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	mkdir dir &&
	echo x >dir/file1 &&
	echo y >dir/file2 &&
	git add dir &&
	git commit -m initial &&
	git tag initial
'

test_expect_success 'symlink to a directory shows as untracked' '
	test_when_finished "rm -f symlink" &&
	ln -s dir symlink &&
	git status --porcelain >actual &&
	grep "?? symlink" actual
'

test_done
