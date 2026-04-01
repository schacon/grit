#!/bin/sh
# Ported from git/t/t4202-log.sh
# Tests for 'grit log'.

test_description='grit log'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with commits' '
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
		git commit -m "second commit" 2>/dev/null &&
	echo "third" >>file.txt &&
	git add file.txt &&
	GIT_AUTHOR_DATE="1000000200 +0000" GIT_COMMITTER_DATE="1000000200 +0000" \
		git commit -m "third commit" 2>/dev/null
'

test_expect_success 'log shows all commits' '
	cd repo &&
	git log --oneline >actual &&
	test "$(wc -l <actual)" -eq 3
'

test_expect_success 'log --oneline shows short hash and subject' '
	cd repo &&
	git log --oneline >actual &&
	head -1 actual >first_line &&
	grep "third commit" first_line
'

test_expect_success 'log -n limits output' '
	cd repo &&
	git log -n 1 --oneline >actual &&
	test "$(wc -l <actual)" -eq 1 &&
	grep "third commit" actual
'

test_expect_success 'log -n 2 shows exactly two' '
	cd repo &&
	git log -n 2 --oneline >actual &&
	test "$(wc -l <actual)" -eq 2
'

test_expect_success 'log --reverse reverses order' '
	cd repo &&
	git log --reverse --oneline >actual &&
	head -1 actual >first_line &&
	grep "first commit" first_line
'

test_expect_success 'log --format=%H shows full hashes' '
	cd repo &&
	git log --format="format:%H" >actual &&
	test "$(wc -l <actual)" -eq 3 &&
	head -1 actual >first_hash &&
	test "$(wc -c <first_hash)" -gt 39
'

test_expect_success 'log --format=%s shows subjects' '
	cd repo &&
	git log --format="format:%s" >actual &&
	head -1 actual >first &&
	echo "third commit" >expected &&
	test_cmp expected first
'

test_expect_success 'log --format=%an shows author name' '
	cd repo &&
	git log -n 1 --format="format:%an" >actual &&
	echo "Test User" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --format=%ae shows author email' '
	cd repo &&
	git log -n 1 --format="format:%ae" >actual &&
	echo "test@test.com" >expected &&
	test_cmp expected actual
'

test_expect_success 'log default format shows Author and Date' '
	cd repo &&
	git log -n 1 >actual &&
	grep "^Author:" actual &&
	grep "^Date:" actual
'

test_expect_success 'log --skip skips commits' '
	cd repo &&
	git log --skip 1 --oneline >actual &&
	test "$(wc -l <actual)" -eq 2 &&
	! grep "third commit" actual
'

test_done
