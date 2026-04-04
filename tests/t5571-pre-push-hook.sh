#!/bin/sh
# Ported from git/t/t5571-pre-push-hook.sh
# Tests pre-push hooks

test_description='check pre-push hooks'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_hook pre-push <<-\EOF &&
	cat >actual
	EOF

	git init --bare repo1 &&
	git remote add parent1 repo1 &&
	test_commit one &&

	git push parent1 HEAD:main &&
	test -f actual &&
	# grit sends local ref as refs/heads/main (not HEAD)
	grep "refs/heads/main" actual
'

test_expect_success 'push with failing hook' '
	test_hook pre-push <<-\EOF &&
	cat >actual &&
	exit 1
	EOF

	test_commit two &&

	test_when_finished "rm -f actual" &&
	test_must_fail git push parent1 HEAD &&
	test -f actual
'

# grit does not support --no-verify for push
test_expect_success '--no-verify bypasses hook' '
	git push --no-verify parent1 HEAD &&
	test_path_is_missing actual
'

test_expect_success 'push with hook capturing args' '
	test_hook --setup pre-push <<-\EOF &&
	echo "$1" >actual
	echo "$2" >>actual
	cat >>actual
	EOF

	test_commit three &&
	git push parent1 main &&
	grep "parent1" actual &&
	grep "repo1" actual
'

test_expect_success 'push delete with hook' '
	test_hook --setup pre-push <<-\EOF &&
	cat >actual
	EOF
	git push parent1 :refs/heads/main &&
	test -f actual
'

test_done
