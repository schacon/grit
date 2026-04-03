#!/bin/sh
test_description='git status with renames'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo content >original &&
	git add original &&
	git commit -m "initial"
'

test_expect_success 'status after mv shows rename or delete+add' '
	cd repo &&
	git mv original renamed &&
	git status --porcelain >actual &&
	test -s actual
'

test_expect_success 'status --short shows changes' '
	cd repo &&
	git status -s >actual &&
	test -s actual
'

test_done
