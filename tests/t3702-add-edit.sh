#!/bin/sh

test_description='add basic tests'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo initial >file &&
	git add file &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'add new file' '
	echo new >newfile &&
	git add newfile &&
	git ls-files --error-unmatch newfile
'

test_expect_success 'add modified file' '
	echo modified >file &&
	git add file &&
	git diff --cached --name-only >actual &&
	grep file actual
'

test_expect_success 'add multiple files' '
	echo a >fileA &&
	echo b >fileB &&
	git add fileA fileB &&
	git ls-files --error-unmatch fileA &&
	git ls-files --error-unmatch fileB
'

test_expect_success 'add with dot adds all' '
	echo c >fileC &&
	git add . &&
	git ls-files --error-unmatch fileC
'

test_expect_success 'add -n is dry run' '
	echo d >fileD &&
	git add -n fileD &&
	test_must_fail git ls-files --error-unmatch fileD
'

test_expect_success 'add respects .gitignore' '
	echo "*.ignored" >.gitignore &&
	echo data >test.ignored &&
	test_must_fail git add test.ignored 2>err &&
	test_must_fail git ls-files --error-unmatch test.ignored &&
	rm .gitignore test.ignored
'

test_done
