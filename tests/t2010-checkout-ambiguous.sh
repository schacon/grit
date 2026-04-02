#!/bin/sh
# Test ambiguous ref resolution during checkout (branch vs tag vs file)

test_description='grit checkout ambiguous ref resolution'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo with branch, tag, and file of same name' '
	grit init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo "initial content" >file.txt &&
	grit add file.txt &&
	grit commit -m "initial" &&
	grit branch ambiguous &&
	grit tag ambiguous-tag
'

test_expect_success 'checkout prefers branch over tag when both exist' '
	cd repo &&
	grit checkout master &&
	grit tag ambiguous &&
	grit checkout ambiguous 2>stderr &&
	grit symbolic-ref HEAD >actual &&
	echo refs/heads/ambiguous >expect &&
	test_cmp expect actual
'

test_expect_success 'checkout warns about ambiguous refname' '
	cd repo &&
	grit checkout master &&
	grit checkout ambiguous 2>stderr &&
	grep -i "ambiguous" stderr
'

test_expect_success 'checkout tag detaches HEAD' '
	cd repo &&
	grit checkout master &&
	grit checkout ambiguous-tag 2>stderr &&
	test_must_fail grit symbolic-ref HEAD 2>/dev/null &&
	grep -i "detached" stderr
'

test_expect_success 'checkout -- restores file from index' '
	cd repo &&
	grit checkout master &&
	echo "modified" >file.txt &&
	grit checkout -- file.txt &&
	echo "initial content" >expect &&
	test_cmp expect file.txt
'

test_expect_success 'checkout branch when file of same name exists' '
	cd repo &&
	grit checkout master &&
	echo "data" >ambiguous &&
	grit add ambiguous &&
	grit commit -m "add file named ambiguous" &&
	grit checkout ambiguous 2>stderr &&
	grit symbolic-ref HEAD >actual &&
	echo refs/heads/ambiguous >expect &&
	test_cmp expect actual
'

test_expect_success 'checkout -- disambiguates to file path' '
	cd repo &&
	grit checkout master &&
	echo "original" >somefile &&
	grit add somefile &&
	grit commit -m "add somefile" &&
	echo "changed" >somefile &&
	grit checkout -- somefile &&
	echo "original" >expect &&
	test_cmp expect somefile
'

test_done
