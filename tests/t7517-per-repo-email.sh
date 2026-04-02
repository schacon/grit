#!/bin/sh
# Test per-repo user.email/user.name configuration

test_description='grit per-repo user.email and user.name config'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo' '
	grit init repo &&
	cd repo
'

test_expect_success 'set per-repo user.email' '
	cd repo &&
	grit config user.email "local@example.com" &&
	grit config user.email >actual &&
	echo "local@example.com" >expect &&
	test_cmp expect actual
'

test_expect_success 'set per-repo user.name' '
	cd repo &&
	grit config user.name "Local User" &&
	grit config user.name >actual &&
	echo "Local User" >expect &&
	test_cmp expect actual
'

test_expect_success 'config is written to .git/config' '
	cd repo &&
	grep "local@example.com" .git/config &&
	grep "Local User" .git/config
'

test_expect_success 'commit uses per-repo identity' '
	cd repo &&
	echo "content" >file.txt &&
	grit add file.txt &&
	grit commit -m "test commit" &&
	grit log --format="%ae" >actual_email &&
	echo "local@example.com" >expect &&
	test_cmp expect actual_email &&
	grit log --format="%an" >actual_name &&
	echo "Local User" >expect &&
	test_cmp expect actual_name
'

test_expect_success 'different repos can have different identities' '
	grit init repo2 &&
	cd repo2 &&
	grit config user.email "other@example.com" &&
	grit config user.name "Other User" &&
	echo "data" >file.txt &&
	grit add file.txt &&
	grit commit -m "other commit" &&
	grit log --format="%ae" >actual_email &&
	echo "other@example.com" >expect &&
	test_cmp expect actual_email &&
	grit log --format="%an" >actual_name &&
	echo "Other User" >expect &&
	test_cmp expect actual_name
'

test_expect_success 'changing config updates subsequent commits' '
	cd repo &&
	grit config user.email "updated@example.com" &&
	grit config user.name "Updated User" &&
	echo "more" >>file.txt &&
	grit add file.txt &&
	grit commit -m "updated identity commit" &&
	grit log -n1 --format="%ae" >actual_email &&
	echo "updated@example.com" >expect &&
	test_cmp expect actual_email
'

test_expect_success 'commit without user config fails' '
	grit init no-config &&
	cd no-config &&
	echo "x" >f.txt &&
	grit add f.txt &&
	test_must_fail grit commit -m "should fail" 2>stderr &&
	grep -i "tell me who you are\|user.email\|user.name" stderr
'

test_done
