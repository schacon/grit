#!/bin/sh

test_description='Revision traversal vs grafts and path limiter'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&

	mkdir subdir &&
	echo >fileA fileA &&
	echo >subdir/fileB fileB &&
	git add fileA subdir/fileB &&
	test_tick &&
	git commit -m "Initial in one history." &&
	A0=$(git rev-parse --verify HEAD) &&
	echo "$A0" >../A0 &&

	echo >fileA fileA modified &&
	git add fileA &&
	test_tick &&
	git commit -m "Second in one history." &&
	A1=$(git rev-parse --verify HEAD) &&
	echo "$A1" >../A1 &&

	echo >subdir/fileB fileB modified &&
	git add subdir/fileB &&
	test_tick &&
	git commit -m "Third in one history." &&
	A2=$(git rev-parse --verify HEAD) &&
	echo "$A2" >../A2 &&

	git update-ref -d refs/heads/main &&
	rm -f .git/index &&

	echo >fileA fileA again &&
	echo >subdir/fileB fileB again &&
	git add fileA subdir/fileB &&
	test_tick &&
	git commit -m "Initial in alternate history." &&
	B0=$(git rev-parse --verify HEAD) &&
	echo "$B0" >../B0 &&

	echo >fileA fileA modified in alternate history &&
	git add fileA &&
	test_tick &&
	git commit -m "Second in alternate history." &&
	B1=$(git rev-parse --verify HEAD) &&
	echo "$B1" >../B1 &&

	echo >subdir/fileB fileB modified in alternate history &&
	git add subdir/fileB &&
	test_tick &&
	git commit -m "Third in alternate history." &&
	B2=$(git rev-parse --verify HEAD) &&
	echo "$B2" >../B2
'

test_expect_success 'rev-list without grafts' '
	cd repo &&
	B2=$(cat ../B2) && B1=$(cat ../B1) && B0=$(cat ../B0) &&
	git rev-list $B2 >actual &&
	cat >expect <<-EOF &&
	$B2
	$B1
	$B0
	EOF
	test_cmp expect actual
'

test_expect_success 'rev-list with --count' '
	cd repo &&
	B2=$(cat ../B2) &&
	git rev-list --count $B2 >actual &&
	echo 3 >expect &&
	test_cmp expect actual
'

test_done
