#!/bin/sh

test_description='git rebase across mode change'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	mkdir DS &&
	>DS/whatever &&
	git add DS &&
	test_tick &&
	git commit -m base &&

	git branch side1 &&
	git branch side2 &&

	git checkout side2 &&
	>unrelated &&
	git add unrelated &&
	test_tick &&
	git commit -m commit1 &&

	echo content >>unrelated &&
	test_tick &&
	git commit -am commit2
'

test_expect_success 'rebase non-conflicting changes' '
	git checkout -b rebase-test side2 &&
	git rebase side1
'

test_done
