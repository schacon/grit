#!/bin/sh

test_description='test various @{X} syntax combinations together'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit supports HEAD, HEAD^, HEAD~N, HEAD^{type}, tag, HEAD:path
# grit does NOT support: @{N}, @{-N}, HEAD@{N}, or local branch -u

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo one >file &&
	git add file &&
	git commit -m "main-one" &&
	git tag main-one &&
	echo two >file &&
	git add file &&
	git commit -m "main-two" &&
	git tag main-two &&
	git checkout -b new-branch &&
	echo three >file &&
	git add file &&
	git commit -m "new-one" &&
	git tag new-one &&
	echo four >file &&
	git add file &&
	git commit -m "new-two" &&
	git tag new-two
'

test_expect_success 'HEAD resolves' '
	git rev-parse HEAD >actual &&
	git rev-parse new-two >expect &&
	test_cmp expect actual
'

test_expect_success 'HEAD ref is symbolic' '
	echo refs/heads/new-branch >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'HEAD^ works' '
	git rev-parse HEAD^ >actual &&
	git rev-parse new-one >expect &&
	test_cmp expect actual
'

test_expect_success 'HEAD~2 works' '
	git rev-parse HEAD~2 >actual &&
	git rev-parse main-two >expect &&
	test_cmp expect actual
'

test_expect_success 'HEAD^{tree} works' '
	git rev-parse HEAD^{tree} >actual &&
	test -n "$(cat actual)"
'

test_expect_success 'HEAD^{commit} works' '
	git rev-parse HEAD^{commit} >actual &&
	git rev-parse HEAD >expect &&
	test_cmp expect actual
'

test_expect_success 'tag resolves correctly' '
	git rev-parse main-one >actual &&
	test -n "$(cat actual)"
'

test_expect_success 'HEAD:file resolves to blob' '
	echo four >expect &&
	git cat-file -p HEAD:file >actual &&
	test_cmp expect actual
'

test_expect_success 'create and access path with @' '
	echo content >normal &&
	echo content >"fun@ny" &&
	git add normal "fun@ny" &&
	git commit -m "funny path" &&
	git cat-file -p HEAD:normal >actual &&
	echo content >expect &&
	test_cmp expect actual
'

test_expect_success 'switch to main and verify HEAD' '
	git checkout main &&
	git rev-parse HEAD >actual &&
	git rev-parse main-two >expect &&
	test_cmp expect actual
'

test_expect_success '@{1} shows previous reflog entry' '
	git rev-parse "@{1}" >actual 2>&1 &&
	test -n "$(cat actual)"
'

test_expect_failure '@{-1} refers to previous branch' '
	git rev-parse --symbolic-full-name "@{-1}" >actual &&
	echo refs/heads/new-branch >expect &&
	test_cmp expect actual
'

test_expect_success 'HEAD@{1} shows previous HEAD' '
	git rev-parse "HEAD@{1}" >actual 2>&1 &&
	test -n "$(cat actual)"
'

test_done
