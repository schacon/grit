#!/bin/sh

test_description='test describe'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&

	echo one >file && git add file &&
	test_tick && git commit -m initial &&

	echo two >file && git add file &&
	test_tick && git commit -m second &&

	echo three >file && git add file &&
	test_tick && git commit -m third &&

	git tag -a -m "annotated A" A &&

	echo four >file && git add file &&
	test_tick && git commit -m fourth &&
	git tag c &&

	echo five >file && git add file &&
	test_tick && git commit -m fifth
'

test_expect_success 'describe HEAD with annotated tag' '
	cd repo &&
	git describe HEAD >raw &&
	# should be A-N-gHASH format
	grep "^A-[0-9]*-g[0-9a-f]*$" raw
'

test_expect_success 'describe exact match' '
	cd repo &&
	git describe --exact-match A >actual &&
	echo A >expect &&
	test_cmp expect actual
'

test_expect_success 'describe --tags uses lightweight tags' '
	cd repo &&
	git describe --tags HEAD >raw &&
	grep "^c-[0-9]*-g[0-9a-f]*$" raw
'

test_expect_success 'describe --tags --exact-match on lightweight tag' '
	cd repo &&
	git describe --tags --exact-match c^ >actual &&
	echo A >expect &&
	test_cmp expect actual
'

test_expect_success 'describe --exact-match failure on non-tagged commit' '
	cd repo &&
	test_must_fail git describe --exact-match HEAD 2>err
'

test_expect_success 'describe --long always shows distance' '
	cd repo &&
	git describe --long A >raw &&
	grep "^A-0-g[0-9a-f]*$" raw
'

test_expect_success 'describe --abbrev controls hash length' '
	cd repo &&
	git describe --abbrev=4 HEAD >raw &&
	grep "^A-[0-9]*-g[0-9a-f]\{4,5\}$" raw
'

test_expect_success 'describe --always falls back to commit hash' '
	cd repo &&
	git describe --always --match=no-such-tag >actual &&
	test -s actual
'

test_expect_success 'describe --match filters tags' '
	cd repo &&
	git tag -a -m "version 1" v1.0 HEAD^ &&
	git describe --match="v*" HEAD >raw &&
	grep "^v1.0-[0-9]*-g[0-9a-f]*$" raw
'

test_expect_success 'describe --first-parent' '
	cd repo &&
	git describe --first-parent HEAD >raw &&
	test -s raw
'

test_expect_success 'describe complains about missing object' '
	cd repo &&
	test_must_fail git describe $ZERO_OID 2>err
'

test_expect_success 'describe --exact-match does not show --always fallback' '
	cd repo &&
	test_must_fail git describe --exact-match --always
'

test_expect_success 'describe with --candidates' '
	cd repo &&
	git describe --candidates=1 HEAD >raw &&
	test -s raw
'

test_done
