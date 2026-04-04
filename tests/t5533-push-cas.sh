#!/bin/sh
# Ported from git/t/t5533-push-cas.sh
# Tests compare & swap push force/delete safety

test_description='compare & swap push force/delete safety'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

setup_srcdst_basic () {
	rm -fr src dst &&
	git clone --no-local . src &&
	git clone --no-local src dst &&
	(
		cd src && git checkout HEAD^0
	)
}

test_expect_success setup '
	git init -q &&
	test_commit A &&
	test_commit B &&
	test_commit C
'

# grit --force-with-lease is a boolean flag only, not --force-with-lease=ref:val
test_expect_failure 'push to update (protected)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		test_commit D &&
		test_must_fail git push --force-with-lease=main:main origin main 2>err &&
		grep "stale info" err
	) &&
	git ls-remote . refs/heads/main >expect &&
	git ls-remote src refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success 'push to update (protected, forced)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		test_commit D &&
		git push --force --force-with-lease origin main
	) &&
	git ls-remote dst refs/heads/main >expect &&
	git ls-remote src refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success 'push to update (allowed)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		test_commit E &&
		git push --force-with-lease origin main
	) &&
	git ls-remote dst refs/heads/main >expect &&
	git ls-remote src refs/heads/main >actual &&
	test_cmp expect actual
'

# grit --force-with-lease does not support =ref:val syntax
test_expect_success 'push to update (allowed, tracking)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		test_commit E &&
		git push --force-with-lease=main origin main
	) &&
	git ls-remote dst refs/heads/main >expect &&
	git ls-remote src refs/heads/main >actual &&
	test_cmp expect actual
'

# grit --force-with-lease does not properly protect against stale delete
test_expect_failure 'push to delete (protected)' '
	setup_srcdst_basic &&
	git ls-remote src refs/heads/main >expect &&
	(
		cd dst &&
		test_must_fail git push --force-with-lease --delete origin main
	) &&
	git ls-remote src refs/heads/main >actual &&
	test_cmp expect actual
'

# grit --force-with-lease + --delete combination
test_expect_failure 'push to delete (allowed)' '
	setup_srcdst_basic &&
	(
		cd dst &&
		git fetch &&
		git push --force-with-lease --delete origin main
	) &&
	git ls-remote src refs/heads/main >actual &&
	test_must_be_empty actual
'

test_done
