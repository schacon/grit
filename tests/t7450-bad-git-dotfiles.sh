#!/bin/sh

test_description='check handling of .git dotfiles'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup' '
	$REAL_GIT init dotfiles-repo &&
	cd dotfiles-repo &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	echo content >file &&
	$REAL_GIT add file &&
	$REAL_GIT commit -m initial
'

test_expect_success 'grit handles .gitignore' '
	cd dotfiles-repo &&
	echo "*.ignored" >.gitignore &&
	$REAL_GIT add .gitignore &&
	$REAL_GIT commit -m "add gitignore" &&
	touch should-be-ignored.ignored &&
	touch should-not-be-ignored.txt &&
	git status --porcelain >output &&
	! grep "should-be-ignored" output &&
	grep "should-not-be-ignored" output
'

test_expect_success 'grit handles .gitattributes' '
	cd dotfiles-repo &&
	echo "*.bin binary" >.gitattributes &&
	$REAL_GIT add .gitattributes &&
	$REAL_GIT commit -m "add gitattributes" &&
	git cat-file -t HEAD >output &&
	echo commit >expect &&
	test_cmp expect output
'

test_expect_success 'grit reads repo with nested directories' '
	cd dotfiles-repo &&
	mkdir -p deep/nested/dir &&
	echo deep >deep/nested/dir/file &&
	$REAL_GIT add deep &&
	$REAL_GIT commit -m "nested dirs" &&
	git ls-tree -r HEAD >output &&
	grep "deep/nested/dir/file" output
'

test_done
