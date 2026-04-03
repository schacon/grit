#!/bin/sh
# Ported from upstream git t7900-maintenance.sh

test_description='git maintenance operations, verified with grit'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup' '
	$REAL_GIT init maint-repo &&
	cd maint-repo &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	echo content >file &&
	$REAL_GIT add file &&
	test_tick &&
	$REAL_GIT commit -m initial
'

test_expect_success 'grit reads objects before maintenance' '
	cd maint-repo &&
	git cat-file -t HEAD >actual &&
	echo commit >expected &&
	test_cmp expected actual
'

test_expect_success 'grit log before maintenance' '
	cd maint-repo &&
	git log --oneline >actual &&
	test_line_count = 1 actual
'

test_expect_success 'grit rev-parse before maintenance' '
	cd maint-repo &&
	git rev-parse HEAD >actual &&
	test -s actual
'

test_expect_success 'grit cat-file on tree' '
	cd maint-repo &&
	git cat-file -p HEAD >actual &&
	grep "^tree " actual &&
	grep "^author " actual
'

test_expect_success 'add more commits' '
	cd maint-repo &&
	echo more >file2 &&
	$REAL_GIT add file2 &&
	test_tick &&
	$REAL_GIT commit -m second &&
	git log --oneline >actual &&
	test_line_count = 2 actual
'

test_expect_success 'grit rev-list works' '
	cd maint-repo &&
	git rev-list HEAD >actual &&
	test_line_count = 2 actual
'

test_expect_success 'grit diff-tree works' '
	cd maint-repo &&
	git diff-tree --name-only HEAD^ HEAD >actual &&
	grep "file2" actual
'

test_done
