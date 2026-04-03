#!/bin/sh

test_description='reftable basics'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not yet fully implement the reftable backend.
# Basic init with --ref-format=reftable succeeds but does not create
# reftable directory structures.

test_expect_success 'init: reftable flag accepted' '
	rm -rf repo &&
	git init --ref-format=reftable repo
'

test_expect_failure 'init: creates reftable directory structure' '
	rm -rf repo &&
	git init --ref-format=reftable repo &&
	test_path_is_dir repo/.git/reftable &&
	test_path_is_file repo/.git/reftable/tables.list
'

test_expect_success 'init: reinitializing reftable backend succeeds' '
	rm -rf repo &&
	git init --ref-format=reftable repo &&
	git init --ref-format=reftable repo
'

test_expect_failure 'init: reinitializing files with reftable backend fails' '
	rm -rf repo &&
	git init --ref-format=files repo &&
	test_must_fail git init --ref-format=reftable repo
'

test_expect_success 'clone: can clone reftable repository' '
	rm -rf repo clone &&
	git init --ref-format=reftable repo &&
	(
		cd repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo content >file &&
		git add file &&
		git commit -m initial
	) &&
	git clone repo clone &&
	test_path_is_file clone/file
'

test_expect_failure 'show-ref-format reports reftable' '
	rm -rf repo &&
	git init --ref-format=reftable repo &&
	echo reftable >expect &&
	git -C repo rev-parse --show-ref-format >actual &&
	test_cmp expect actual
'

test_expect_success 'ref transaction: basic update-ref' '
	rm -rf repo &&
	git init --ref-format=reftable repo &&
	(
		cd repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo content >file &&
		git add file &&
		git commit -m initial &&
		HEAD=$(git rev-parse HEAD) &&
		git update-ref refs/heads/new-branch "$HEAD" &&
		git rev-parse refs/heads/new-branch >actual &&
		echo "$HEAD" >expect &&
		test_cmp expect actual
	)
'

test_expect_success 'ref transaction: delete ref' '
	rm -rf repo &&
	git init --ref-format=reftable repo &&
	(
		cd repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo content >file &&
		git add file &&
		git commit -m initial &&
		git branch to-delete &&
		git update-ref -d refs/heads/to-delete &&
		test_must_fail git rev-parse refs/heads/to-delete
	)
'

test_expect_success 'ref transaction: symbolic ref' '
	rm -rf repo &&
	git init --ref-format=reftable repo &&
	(
		cd repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo content >file &&
		git add file &&
		git commit -m initial &&
		git symbolic-ref refs/heads/sym refs/heads/master &&
		echo refs/heads/master >expect &&
		git symbolic-ref refs/heads/sym >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'show-ref lists refs in reftable repo' '
	rm -rf repo &&
	git init --ref-format=reftable repo &&
	(
		cd repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo content >file &&
		git add file &&
		git commit -m initial &&
		git tag v1.0 &&
		git show-ref >actual &&
		grep "refs/heads/" actual &&
		grep "refs/tags/v1.0" actual
	)
'

test_expect_success 'for-each-ref works in reftable repo' '
	rm -rf repo &&
	git init --ref-format=reftable repo &&
	(
		cd repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo content >file &&
		git add file &&
		git commit -m initial &&
		git for-each-ref --format="%(refname)" >actual &&
		grep "refs/heads/" actual
	)
'

test_done
