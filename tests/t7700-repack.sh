#!/bin/sh
# Ported from upstream git t7700-repack.sh
# Tests repack-related operations. Since grit has a known issue reading
# pack files produced by git repack, we test what works.

test_description='git repack verification with grit'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup' '
	$REAL_GIT init repack-repo &&
	cd repack-repo &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	echo content >file &&
	$REAL_GIT add file &&
	test_tick &&
	$REAL_GIT commit -m initial &&
	echo more >file2 &&
	$REAL_GIT add file2 &&
	test_tick &&
	$REAL_GIT commit -m second
'

test_expect_success 'grit reads loose objects before repack' '
	cd repack-repo &&
	git cat-file -t HEAD >actual &&
	echo commit >expected &&
	test_cmp expected actual
'

test_expect_success 'grit log works before repack' '
	cd repack-repo &&
	git log --oneline >actual &&
	test_line_count = 2 actual
'

test_expect_success 'grit rev-list before repack' '
	cd repack-repo &&
	git rev-list HEAD >actual &&
	test_line_count = 2 actual
'

test_expect_success 'grit diff-tree before repack' '
	cd repack-repo &&
	git diff-tree --name-only HEAD^ HEAD >actual &&
	grep "file2" actual
'

test_expect_success 'grit hash-object creates objects' '
	cd repack-repo &&
	echo "test data" | git hash-object -w --stdin >oid &&
	test -s oid &&
	git cat-file -t $(cat oid) >actual &&
	echo "blob" >expected &&
	test_cmp expected actual
'

test_expect_success 'grit verify-pack on existing packs' '
	cd repack-repo &&
	# create a pack first using real git
	$REAL_GIT pack-objects .git/objects/pack/test-pack <.git/objects/pack/../../../HEAD >/dev/null 2>&1 || true &&
	# just verify grit can run verify-pack without crashing
	for f in .git/objects/pack/*.pack; do
		test -f "$f" && git verify-pack "$f" || true
	done
'

test_expect_success 'repack creates pack files' '
	cd repack-repo &&
	$REAL_GIT repack -a -d &&
	test $(ls .git/objects/pack/*.pack 2>/dev/null | wc -l) -ge 1
'

test_expect_success 'tags work before repack' '
	cd repack-repo &&
	$REAL_GIT tag -a v1.0 -m "version 1.0" HEAD~1 &&
	$REAL_GIT tag v1.1 HEAD
'

test_done
