#!/bin/sh

test_description='test diff-tree between trees'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'diff-tree between different trees shows changes' '
	echo bar >foo &&
	git add foo &&
	good_tree=$(git write-tree) &&
	echo baz >foo &&
	git add foo &&
	other_tree=$(git write-tree) &&
	git diff-tree $good_tree $other_tree >actual &&
	grep "foo" actual
'

test_expect_success 'diff-tree between same tree is empty' '
	echo bar >foo &&
	git add foo &&
	tree=$(git write-tree) &&
	git diff-tree $tree $tree >actual &&
	test_must_be_empty actual
'

test_done
