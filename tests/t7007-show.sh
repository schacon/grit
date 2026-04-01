#!/bin/sh
# Ported from git/t/t7007-show.sh
# Tests for 'grit show'.

test_description='grit show'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with commits and tags' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo "first" >file.txt &&
	git add file.txt &&
	GIT_AUTHOR_DATE="1000000000 +0000" GIT_COMMITTER_DATE="1000000000 +0000" \
		git commit -m "first commit" 2>/dev/null &&
	echo "second" >>file.txt &&
	git add file.txt &&
	GIT_AUTHOR_DATE="1000000100 +0000" GIT_COMMITTER_DATE="1000000100 +0000" \
		git commit -m "second commit" 2>/dev/null
'

test_expect_success 'show HEAD shows commit header' '
	cd repo &&
	git show >actual &&
	grep "^commit " actual &&
	grep "^Author:" actual &&
	grep "^Date:" actual &&
	grep "second commit" actual
'

test_expect_success 'show HEAD shows diff' '
	cd repo &&
	git show >actual &&
	grep "^diff --git" actual
'

test_expect_success 'show <commit> shows that commit' '
	cd repo &&
	FIRST=$(git log --format="%H" | tail -1) &&
	git show "$FIRST" >actual &&
	grep "first commit" actual
'

test_expect_success 'show --oneline shows short hash and subject' '
	cd repo &&
	git show --oneline >actual &&
	head -1 actual >first_line &&
	grep "second commit" first_line &&
	test "$(wc -w <first_line)" -ge 2
'

test_expect_success 'show --quiet suppresses diff' '
	cd repo &&
	git show --quiet >actual &&
	grep "^commit " actual &&
	! grep "^diff --git" actual
'

test_expect_success 'show shows blob contents' '
	cd repo &&
	BLOB=$(git rev-parse HEAD:file.txt) &&
	git show "$BLOB" >actual &&
	grep "first" actual
'

test_expect_success 'show shows tree listing' '
	cd repo &&
	TREE=$(git rev-parse HEAD^{tree}) &&
	git show "$TREE" >actual &&
	grep "file.txt" actual
'

test_expect_success 'show annotated tag shows tag then commit' '
	cd repo &&
	GIT_COMMITTER_DATE="1000000200 +0000" \
		git tag -a v1.0 -m "version 1.0" &&
	git show v1.0 >actual &&
	grep "tag v1.0" actual &&
	grep "version 1.0" actual &&
	grep "^commit " actual
'

test_expect_success 'show lightweight tag shows the commit' '
	cd repo &&
	git tag v0.9 &&
	git show v0.9 >actual &&
	grep "^commit " actual
'

test_expect_success 'show --format=%s shows subject' '
	cd repo &&
	git show --format="format:%s" >actual &&
	head -1 actual >first &&
	echo "second commit" >expected &&
	test_cmp expected first
'

test_expect_success 'show first commit has no diff header parent (root commit diff)' '
	cd repo &&
	FIRST=$(git log --format="%H" | tail -1) &&
	git show "$FIRST" >actual &&
	grep "^diff --git" actual &&
	grep "first commit" actual
'

test_done
