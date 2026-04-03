#!/bin/sh

test_description='behavior of diff when reading objects'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with multiple files' '
	git init repo &&
	cd repo &&
	echo a >a &&
	echo b >b &&
	git add a b &&
	git commit -m "first" &&
	echo c >a &&
	echo d >b &&
	git commit -a -m "second"
'

test_expect_success 'diff HEAD~1 HEAD shows both files' '
	cd repo &&
	git diff HEAD~1 HEAD >actual &&
	grep "a/a" actual &&
	grep "b/b" actual
'

test_expect_success 'diff --name-only HEAD~1 HEAD lists files' '
	cd repo &&
	git diff --name-only HEAD~1 HEAD >actual &&
	grep "^a$" actual &&
	grep "^b$" actual
'

test_expect_success 'diff --stat HEAD~1 HEAD shows stats' '
	cd repo &&
	git diff --stat HEAD~1 HEAD >actual &&
	grep "a " actual &&
	grep "b " actual
'

test_done
