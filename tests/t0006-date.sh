#!/bin/sh
# Test date parsing and formatting in commits

test_description='grit date parsing and formatting'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo' '
	grit init repo &&
	cd repo &&
	git config user.name "Test Author" &&
	git config user.email "author@example.com"
'

test_expect_success 'commit with ISO 8601 date' '
	cd repo &&
	echo "a" >a.txt &&
	grit add a.txt &&
	GIT_AUTHOR_DATE="2005-04-07T22:13:13" \
	GIT_COMMITTER_DATE="2005-04-07T22:13:13" \
	grit commit -m "iso date" &&
	grit rev-parse HEAD >iso_oid
'

test_expect_success 'ISO date is preserved in commit object' '
	cd repo &&
	grit cat-file -p $(cat iso_oid) >commit_obj &&
	grep "author.*2005-04-07" commit_obj
'

test_expect_success 'commit with epoch+timezone date' '
	cd repo &&
	echo "b" >b.txt &&
	grit add b.txt &&
	GIT_AUTHOR_DATE="1112911993 +0200" \
	GIT_COMMITTER_DATE="1112911993 +0200" \
	grit commit -m "epoch date" &&
	grit rev-parse HEAD >epoch_oid
'

test_expect_success 'epoch date is preserved in commit object' '
	cd repo &&
	grit cat-file -p $(cat epoch_oid) >commit_obj &&
	grep "author.*1112911993 +0200" commit_obj
'

test_expect_success 'commit with RFC 2822 date' '
	cd repo &&
	echo "c" >c.txt &&
	grit add c.txt &&
	GIT_AUTHOR_DATE="Thu, 7 Apr 2005 22:13:13 +0200" \
	GIT_COMMITTER_DATE="Thu, 7 Apr 2005 22:13:13 +0200" \
	grit commit -m "rfc2822 date" &&
	grit rev-parse HEAD >rfc_oid
'

test_expect_success 'RFC 2822 date is preserved in commit object' '
	cd repo &&
	grit cat-file -p $(cat rfc_oid) >commit_obj &&
	grep "author.*Thu, 7 Apr 2005" commit_obj
'

test_expect_success 'log --format=%an shows author name' '
	cd repo &&
	grit log --format="%an" >actual &&
	head -1 actual >first &&
	echo "Test Author" >expect &&
	test_cmp expect first
'

test_expect_success 'log --format=%ae shows author email' '
	cd repo &&
	grit log --format="%ae" >actual &&
	head -1 actual >first &&
	echo "author@example.com" >expect &&
	test_cmp expect first
'

test_expect_success 'log --format=%H shows full hash' '
	cd repo &&
	grit log -n1 --format="%H" >actual &&
	test "$(wc -c <actual | tr -d " ")" -ge 40
'

test_expect_success 'log --format=%h shows abbreviated hash' '
	cd repo &&
	grit log -n1 --format="%h" >actual &&
	len=$(wc -c <actual | tr -d " ") &&
	test "$len" -ge 7 &&
	test "$len" -le 41
'

test_expect_success 'log --format=%s shows subject' '
	cd repo &&
	grit log -n1 --format="%s" >actual &&
	echo "rfc2822 date" >expect &&
	test_cmp expect actual
'

test_expect_success 'different author and committer dates' '
	cd repo &&
	echo "d" >d.txt &&
	grit add d.txt &&
	GIT_AUTHOR_DATE="1000000000 +0000" \
	GIT_COMMITTER_DATE="1100000000 +0000" \
	grit commit -m "split dates" &&
	grit cat-file -p HEAD >commit_obj &&
	grep "^author.*1000000000" commit_obj &&
	grep "^committer.*1100000000" commit_obj
'

test_done
