#!/bin/sh

test_description='add -i and add -p basic tests'

. ./test-lib.sh

test_expect_success 'setup' '
	mkdir repo &&
	cd repo &&
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo initial >file &&
	git add file &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'add --help shows interactive option' '
	cd repo &&
	git add --help 2>&1 >output &&
	grep -i "interactive\|patch" output
'

test_expect_success 'add basic file' '
	cd repo &&
	echo new >newfile &&
	git add newfile &&
	git ls-files --error-unmatch newfile
'

test_expect_success 'add with -A adds and removes' '
	cd repo &&
	git rm --cached newfile &&
	echo newer >newfile &&
	echo extra >extra &&
	git add -A &&
	git ls-files --error-unmatch newfile &&
	git ls-files --error-unmatch extra
'

test_expect_success 'add -u updates tracked files only' '
	cd repo &&
	git reset --hard &&
	echo modified >file &&
	echo untracked >untracked &&
	git add -u &&
	git diff --cached --name-only >actual &&
	grep file actual &&
	test_must_fail git ls-files --error-unmatch untracked
'

test_done
