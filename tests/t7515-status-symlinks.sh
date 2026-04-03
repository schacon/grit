#!/bin/sh
test_description='git status and symlinks'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	mkdir dir &&
	echo x >dir/file1 &&
	echo y >dir/file2 &&
	git add dir &&
	git commit -m initial &&
	git tag initial
'

test_expect_success 'symlink to a directory shows as untracked' '
	cd repo &&
	test_when_finished "rm -f symlink" &&
	ln -s dir symlink &&
	git status --porcelain >actual &&
	grep "?? symlink" actual
'

test_expect_success 'status is clean after removing symlink' '
	cd repo &&
	git status --porcelain >actual &&
	! grep "symlink" actual
'

test_done
