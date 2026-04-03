#!/bin/sh

test_description='git rebase basic tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m base &&
	git tag base &&

	git checkout -b feature &&
	echo feature1 >feature-file &&
	git add feature-file &&
	test_tick &&
	git commit -m feature1 &&

	echo feature2 >>feature-file &&
	git add feature-file &&
	test_tick &&
	git commit -m feature2 &&
	git tag feature-end &&

	git checkout main &&
	echo main-change >main-file &&
	git add main-file &&
	test_tick &&
	git commit -m main-change &&
	git tag main-end
'

test_expect_success 'rebase feature onto main' '
	git checkout feature &&
	git rebase main &&
	test_path_is_file main-file &&
	test_path_is_file feature-file
'

test_expect_success 'rebase preserves commit count' '
	git rev-parse main >main_sha &&
	git log --oneline feature >all_commits &&
	git log --oneline $(cat main_sha) >main_commits &&
	main_count=$(wc -l <main_commits | tr -d " ") &&
	all_count=$(wc -l <all_commits | tr -d " ") &&
	test $(( all_count - main_count )) = 2
'

test_expect_success 'rebase result has correct content' '
	echo main-change >expect &&
	test_cmp expect main-file &&
	cat >expect <<-\EOF &&
	feature1
	feature2
	EOF
	test_cmp expect feature-file
'

test_done
