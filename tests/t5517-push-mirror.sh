#!/bin/sh
# Ported from git/t/t5517-push-mirror.sh
# Simplified: tests basic push functionality (--mirror not yet supported)

test_description='pushing branches and tags'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'push creates new branches in bare repo' '
	git init -q &&
	echo one >foo && git add foo && git commit -m one &&
	git init --bare mirror.git &&
	git remote add mirror mirror.git &&
	git push mirror main &&
	main_local=$(git show-ref -s --verify refs/heads/main) &&
	main_remote=$(git --git-dir=mirror.git show-ref -s --verify refs/heads/main) &&
	test "$main_local" = "$main_remote"
'

test_expect_success 'push updates existing branches' '
	echo two >foo && git add foo && git commit -m two &&
	git push mirror main &&
	main_local=$(git show-ref -s --verify refs/heads/main) &&
	main_remote=$(git --git-dir=mirror.git show-ref -s --verify refs/heads/main) &&
	test "$main_local" = "$main_remote"
'

test_expect_success 'push --tags sends tags' '
	git tag -f tmain main &&
	git push --tags mirror &&
	main_local=$(git show-ref -s --verify refs/tags/tmain) &&
	main_remote=$(git --git-dir=mirror.git show-ref -s --verify refs/tags/tmain) &&
	test "$main_local" = "$main_remote"
'

test_expect_success 'push --force updates existing tags' '
	echo three >foo && git add foo && git commit -m three &&
	git tag -f tmain main &&
	git push --force --tags mirror &&
	main_local=$(git show-ref -s --verify refs/tags/tmain) &&
	main_remote=$(git --git-dir=mirror.git show-ref -s --verify refs/tags/tmain) &&
	test "$main_local" = "$main_remote"
'

test_done
