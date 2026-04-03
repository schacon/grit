#!/bin/sh

test_description='git reset basic operations'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# The upstream test tests reset --patch which requires interactive input.
# We test basic reset functionality instead.

test_expect_success 'setup' '
	git init reset-repo &&
	cd reset-repo &&
	mkdir dir &&
	echo parent >dir/foo &&
	echo dummy >bar &&
	git add dir bar &&
	test_tick &&
	git commit -m initial &&
	git tag initial &&

	echo head >dir/foo &&
	git add dir/foo &&
	test_tick &&
	git commit -m second &&
	git tag second
'

test_expect_success 'reset --soft moves HEAD only' '
	cd reset-repo &&
	git reset --soft initial &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse initial)" &&
	git diff --cached --name-only >staged &&
	grep "dir/foo" staged
'

test_expect_success 'reset --mixed (default) resets index' '
	cd reset-repo &&
	git reset second &&
	git reset --mixed initial &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse initial)" &&
	git diff --name-only >unstaged &&
	grep "dir/foo" unstaged
'

test_expect_success 'reset --hard resets index and working tree' '
	cd reset-repo &&
	git reset --hard second &&
	echo head >expect &&
	test_cmp expect dir/foo &&
	git reset --hard initial &&
	echo parent >expect &&
	test_cmp expect dir/foo
'

test_expect_success 'reset to specific file' '
	cd reset-repo &&
	git reset --hard second &&
	echo changed >dir/foo &&
	git add dir/foo &&
	git reset -- dir/foo &&
	git diff --cached --name-only >staged &&
	test_must_be_empty staged
'

test_done
