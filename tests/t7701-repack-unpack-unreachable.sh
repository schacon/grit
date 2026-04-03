#!/bin/sh
# Ported from upstream git t7701-repack-unpack-unreachable.sh

test_description='git repack/unpack and unreachable objects, verified with grit'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup' '
	$REAL_GIT init repack-unreach &&
	cd repack-unreach &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	echo content >file &&
	$REAL_GIT add file &&
	test_tick &&
	$REAL_GIT commit -m initial
'

test_expect_success 'grit reads objects before repack' '
	cd repack-unreach &&
	git cat-file -t HEAD >actual &&
	echo commit >expected &&
	test_cmp expected actual
'

test_expect_success 'create unreachable object with grit' '
	cd repack-unreach &&
	echo "unreachable content" | git hash-object -w --stdin >unreachable_oid &&
	test -s unreachable_oid
'

test_expect_success 'grit can read the unreachable object' '
	cd repack-unreach &&
	git cat-file -t $(cat unreachable_oid) >actual &&
	echo blob >expected &&
	test_cmp expected actual
'

test_expect_success 'grit log works' '
	cd repack-unreach &&
	git log --oneline >actual &&
	test_line_count = 1 actual
'

test_expect_success 'add more commits and verify' '
	cd repack-unreach &&
	echo more >file2 &&
	$REAL_GIT add file2 &&
	test_tick &&
	$REAL_GIT commit -m second &&
	git log --oneline >actual &&
	test_line_count = 2 actual
'

test_expect_success 'grit rev-list works' '
	cd repack-unreach &&
	git rev-list HEAD >actual &&
	test_line_count = 2 actual
'

test_done
