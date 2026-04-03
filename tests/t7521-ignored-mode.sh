#!/bin/sh
test_description='git status --ignored'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo content >tracked &&
	git add tracked &&
	git commit -m "initial" &&
	echo "*.ignored" >.gitignore &&
	git add .gitignore &&
	git commit -m "add gitignore"
'

test_expect_success 'status --porcelain does not show ignored files' '
	cd repo &&
	echo ignored >file.ignored &&
	git status --porcelain >actual &&
	! grep "file.ignored" actual
'

test_expect_success 'status --porcelain --ignored shows ignored files' '
	cd repo &&
	git status --porcelain --ignored >actual &&
	grep "!! file.ignored" actual
'

test_done
