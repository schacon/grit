#!/bin/sh
#
# Ported from git/t/t0600-reffiles-backend.sh (subset)

test_description='Test reffiles backend'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q
'

test_expect_success 'setup' '
	git commit --allow-empty -m Initial &&
	git rev-parse HEAD >C_oid &&
	git commit --allow-empty -m Second &&
	git rev-parse HEAD >D_oid &&
	git commit --allow-empty -m Third &&
	git rev-parse HEAD >E_oid
'

test_expect_success 'empty directory should not fool for-each-ref' '
	C=$(cat C_oid) &&
	prefix=refs/e-for-each-ref &&
	git update-ref $prefix/foo $C &&
	git for-each-ref $prefix >expected &&
	git pack-refs --all &&
	mkdir -p .git/$prefix/foo/bar/baz &&
	git for-each-ref $prefix >actual &&
	test_cmp expected actual
'

test_done
