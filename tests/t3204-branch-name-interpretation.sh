#!/bin/sh
# Test branch name edge cases (slashes, dots, special chars)

test_description='grit branch name edge cases and check-ref-format'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo with initial commit' '
	grit init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo "init" >file.txt &&
	grit add file.txt &&
	grit commit -m "initial"
'

test_expect_success 'branch with slash in name' '
	cd repo &&
	grit branch feature/test &&
	grit branch >output &&
	grep "feature/test" output
'

test_expect_success 'branch with dots in name' '
	cd repo &&
	grit branch v1.0.0-rc &&
	grit branch >output &&
	grep "v1.0.0-rc" output
'

test_expect_success 'branch with multiple slashes' '
	cd repo &&
	grit branch feature/sub/deep &&
	grit branch >output &&
	grep "feature/sub/deep" output
'

test_expect_success 'checkout branch with slash' '
	cd repo &&
	grit checkout feature/test &&
	grit symbolic-ref HEAD >actual &&
	echo refs/heads/feature/test >expect &&
	test_cmp expect actual
'

test_expect_success 'checkout branch with dots' '
	cd repo &&
	grit checkout v1.0.0-rc &&
	grit symbolic-ref HEAD >actual &&
	echo refs/heads/v1.0.0-rc >expect &&
	test_cmp expect actual
'

test_expect_success 'check-ref-format accepts valid names' '
	grit check-ref-format refs/heads/valid &&
	grit check-ref-format refs/heads/feature/test &&
	grit check-ref-format refs/heads/v1.0 &&
	grit check-ref-format refs/heads/name-with-dashes &&
	grit check-ref-format refs/heads/name_with_underscores
'

test_expect_success 'check-ref-format rejects double dot' '
	test_must_fail grit check-ref-format refs/heads/bad..name
'

test_expect_success 'check-ref-format rejects .lock suffix' '
	test_must_fail grit check-ref-format refs/heads/bad.lock
'

test_expect_success 'check-ref-format rejects space' '
	test_must_fail grit check-ref-format "refs/heads/bad name"
'

test_expect_success 'check-ref-format rejects @{' '
	test_must_fail grit check-ref-format "refs/heads/bad@{name"
'

test_expect_success 'check-ref-format rejects trailing dot' '
	test_must_fail grit check-ref-format "refs/heads/bad."
'

test_expect_success 'check-ref-format rejects trailing slash' '
	test_must_fail grit check-ref-format "refs/heads/bad/"
'

test_expect_success 'check-ref-format --branch resolves branch name' '
	grit check-ref-format --branch master >actual &&
	echo master >expect &&
	test_cmp expect actual
'

test_expect_success 'delete branch with slash' '
	cd repo &&
	grit checkout master &&
	grit branch -d feature/test &&
	grit branch >output &&
	! grep "feature/test" output
'

test_expect_success 'delete branch with dots' '
	cd repo &&
	grit branch -d v1.0.0-rc &&
	grit branch >output &&
	! grep "v1.0.0-rc" output
'

test_done
