#!/bin/sh

test_description='add with pathspec tests'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo A >fileA.t &&
	echo B >fileB.t &&
	echo C >fileC.t &&
	echo D >fileD.t &&
	git add fileA.t fileB.t fileC.t fileD.t &&
	test_tick &&
	git commit -m "Files" &&
	git tag checkpoint
'

test_expect_success 'add specific files with pathspec' '
	git reset --hard checkpoint &&
	echo A2 >fileA.t &&
	echo B2 >fileB.t &&
	git add fileA.t &&
	git status --porcelain >actual &&
	grep "^M  fileA.t" actual &&
	grep "^ M fileB.t" actual
'

test_expect_success 'add all with dot' '
	git reset --hard checkpoint &&
	echo A2 >fileA.t &&
	echo B2 >fileB.t &&
	git add . &&
	git diff --cached --name-only >actual &&
	grep fileA.t actual &&
	grep fileB.t actual
'

test_expect_success 'add with directory pathspec' '
	git reset --hard checkpoint &&
	mkdir -p subdir &&
	echo sub >subdir/file.t &&
	git add subdir &&
	git ls-files --error-unmatch subdir/file.t
'

test_expect_success 'add with glob pathspec' '
	git reset --hard checkpoint &&
	echo A2 >fileA.t &&
	echo B2 >fileB.t &&
	echo C2 >fileC.t &&
	git add "file?.t" &&
	git diff --cached --name-only >actual &&
	test_line_count = 3 actual
'

test_done
