#!/bin/sh

test_description='diff algorithm influence on merge'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup' '
	$REAL_GIT init diff-algo-merge &&
	cd diff-algo-merge &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&

	cat >file.c <<-\EOF &&
	int f(int x) {
	  return x + 1;
	}

	int g(int y) {
	  return y * 2;
	}
	EOF
	$REAL_GIT add file.c &&
	$REAL_GIT commit -m c0 &&
	$REAL_GIT tag c0 &&

	cat >file.c <<-\EOF &&
	int f(int x) {
	  return x + 2;
	}

	int g(int y) {
	  return y * 2;
	}
	EOF
	$REAL_GIT add file.c &&
	$REAL_GIT commit -m c1 &&
	$REAL_GIT tag c1 &&

	$REAL_GIT reset --hard c0 &&
	cat >file.c <<-\EOF &&
	int f(int x) {
	  return x + 1;
	}

	int g(int y) {
	  return y * 3;
	}
	EOF
	$REAL_GIT add file.c &&
	$REAL_GIT commit -m c2 &&
	$REAL_GIT tag c2
'

test_expect_success 'grit reads repo with merge-friendly history' '
	cd diff-algo-merge &&
	git log --oneline >output &&
	test $(wc -l <output) -ge 2
'

test_expect_success 'grit diff between branches' '
	cd diff-algo-merge &&
	git diff c0 c1 >output &&
	grep "return x" output
'

test_expect_success 'merge and verify with grit' '
	cd diff-algo-merge &&
	$REAL_GIT checkout c1 &&
	$REAL_GIT merge --no-edit c2 &&
	git cat-file commit HEAD >output &&
	grep "parent" output
'

test_done
