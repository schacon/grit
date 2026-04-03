#!/bin/sh

test_description='test diff-tree between commits'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup commits' '
	echo bar >foo &&
	git add foo &&
	git commit -m first &&
	git tag first &&
	echo baz >foo &&
	git add foo &&
	git commit -m second &&
	git tag second
'

test_expect_success 'diff-tree between same commit is empty' '
	git diff-tree first first >actual &&
	test_must_be_empty actual
'

test_expect_success 'diff-tree between different commits shows changes' '
	git diff-tree first second >actual &&
	grep "foo" actual
'

test_expect_success 'diff-tree with --name-only' '
	git diff-tree --name-only -r first second >actual &&
	echo foo >expect &&
	test_cmp expect actual
'

test_expect_success 'diff-tree with --name-status' '
	git diff-tree --name-status -r first second >actual &&
	printf "M\tfoo\n" >expect &&
	test_cmp expect actual
'

test_expect_success 'diff-tree with -p shows patch' '
	git diff-tree -p first second >actual &&
	grep "^-bar" actual &&
	grep "^+baz" actual
'

test_expect_success 'diff-tree with --stat' '
	git diff-tree --stat -r first second >actual &&
	grep "foo" actual &&
	grep "1 insertion" actual &&
	grep "1 deletion" actual
'

test_done
