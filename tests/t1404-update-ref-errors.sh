#!/bin/sh
# Ported from git/t/t1404-update-ref-errors.sh (harness-compatible subset).

test_description='gust update-ref error handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

C=3333333333333333333333333333333333333333
D=4444444444444444444444444444444444444444
E=5555555555555555555555555555555555555555

test_expect_success 'setup repository' '
	gust init repo &&
	cd repo
'

test_expect_success 'existing loose ref blocks creating deeper ref' '
	cd repo &&
	gust update-ref refs/errors/c "$C" &&
	test_must_fail gust update-ref refs/errors/c/x "$D" &&
	echo "$C" >expect &&
	cat .git/refs/errors/c >actual &&
	test_cmp expect actual
'

test_expect_success 'existing deeper ref blocks creating parent ref' '
	cd repo &&
	gust update-ref refs/errors/d/e "$C" &&
	test_must_fail gust update-ref refs/errors/d "$D" &&
	echo "$C" >expect &&
	cat .git/refs/errors/d/e >actual &&
	test_cmp expect actual
'

test_expect_success 'missing old-value blocks update in --stdin mode' '
	cd repo &&
	echo "update refs/errors/missing $E $D" >stdin &&
	test_must_fail gust update-ref --stdin <stdin &&
	test_path_is_missing .git/refs/errors/missing
'

test_expect_success 'incorrect old-value blocks update in --stdin mode' '
	cd repo &&
	gust update-ref refs/errors/existing "$C" &&
	echo "update refs/errors/existing $E $D" >stdin &&
	test_must_fail gust update-ref --stdin <stdin &&
	echo "$C" >expect &&
	cat .git/refs/errors/existing >actual &&
	test_cmp expect actual
'

test_expect_success 'existing ref blocks create in --stdin mode' '
	cd repo &&
	echo "create refs/errors/existing $E" >stdin &&
	test_must_fail gust update-ref --stdin <stdin &&
	echo "$C" >expect &&
	cat .git/refs/errors/existing >actual &&
	test_cmp expect actual
'

test_expect_success 'incorrect old-value blocks delete in --stdin mode' '
	cd repo &&
	echo "delete refs/errors/existing $D" >stdin &&
	test_must_fail gust update-ref --stdin <stdin &&
	echo "$C" >expect &&
	cat .git/refs/errors/existing >actual &&
	test_cmp expect actual
'

test_done
