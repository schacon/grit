#!/bin/sh
test_description='object name disambiguation'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo content >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'full SHA resolves' '
	OID=$(git rev-parse HEAD) &&
	git rev-parse "$OID" >actual &&
	echo "$OID" >expect &&
	test_cmp expect actual
'

test_expect_success 'short SHA resolves unambiguously' '
	OID=$(git rev-parse HEAD) &&
	SHORT=$(echo "$OID" | cut -c1-12) &&
	git rev-parse "$SHORT" >actual &&
	echo "$OID" >expect &&
	test_cmp expect actual
'

test_expect_failure 'ambiguous short SHA reports error' '
	git init --bare blob.prefix &&
	(
		cd blob.prefix &&
		echo brocdnra | git hash-object -w --stdin &&
		echo brigddsv | git hash-object -w --stdin
	) &&
	test_must_fail git -C blob.prefix rev-parse dead 2>err &&
	grep -i "ambiguous" err
'

test_expect_success 'disambiguate with ^{type}' '
	OID=$(git rev-parse HEAD) &&
	git rev-parse "$OID^{commit}" >actual &&
	echo "$OID" >expect &&
	test_cmp expect actual
'

test_expect_success 'HEAD^{tree} resolves' '
	TREE=$(git rev-parse HEAD^{tree}) &&
	test -n "$TREE" &&
	echo "$TREE" | grep "^[0-9a-f]\{40\}$"
'

test_expect_success 'tag disambiguation' '
	git tag v1.0 HEAD &&
	git rev-parse v1.0 >actual &&
	git rev-parse HEAD >expect &&
	test_cmp expect actual
'

test_expect_success '--short produces abbreviated output' '
	OID=$(git rev-parse HEAD) &&
	git rev-parse --short HEAD >actual &&
	SHORT=$(cat actual) &&
	test ${#SHORT} -lt ${#OID}
'

test_expect_success '--verify accepts valid ref' '
	git rev-parse --verify HEAD >actual &&
	test_line_count = 1 actual
'

test_expect_success '--verify rejects invalid ref' '
	test_must_fail git rev-parse --verify nonexistent 2>err &&
	test -s err
'

test_done
