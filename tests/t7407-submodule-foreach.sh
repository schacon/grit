#!/bin/sh
#
# Ported from git/t/t7407-submodule-foreach.sh
# Tests for 'git submodule foreach'
#

test_description='Test "git submodule foreach"'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup a submodule tree' '
	"$REAL_GIT" init upstream &&
	(
		cd upstream &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo file >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" commit -m upstream
	) &&
	"$REAL_GIT" clone upstream super &&
	(
		cd super &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		"$REAL_GIT" config protocol.file.allow always
	) &&
	"$REAL_GIT" clone super submodule &&
	(
		cd submodule &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User"
	) &&
	(
		cd super &&
		"$REAL_GIT" -c protocol.file.allow=always submodule add ../submodule sub1 &&
		"$REAL_GIT" -c protocol.file.allow=always submodule add ../submodule sub2 &&
		"$REAL_GIT" -c protocol.file.allow=always submodule add ../submodule sub3 &&
		"$REAL_GIT" commit -m "submodules" &&
		"$REAL_GIT" submodule init
	)
'

test_expect_success 'test basic "submodule foreach" usage' '
	cd super &&
	git submodule foreach "echo hello" >../actual &&
	grep "hello" ../actual
'

test_expect_success 'test "submodule foreach" visits all submodules' '
	cd super &&
	git submodule foreach "echo visiting" >../actual &&
	count=$(grep -c "visiting" ../actual) &&
	test "$count" -eq 3
'

test_expect_success 'test "submodule foreach" with name variable' '
	cd super &&
	git submodule foreach "echo \$name" >../actual &&
	grep "sub1" ../actual &&
	grep "sub2" ../actual &&
	grep "sub3" ../actual
'

test_expect_success 'test "submodule foreach" with sm_path variable' '
	cd super &&
	git submodule foreach "echo \$sm_path" >../actual &&
	grep "sub1" ../actual &&
	grep "sub2" ../actual &&
	grep "sub3" ../actual
'

test_expect_success 'test "submodule foreach" with toplevel variable' '
	cd super &&
	git submodule foreach "echo \$toplevel" >../actual &&
	grep "super" ../actual
'

test_done
