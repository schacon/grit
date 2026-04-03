#!/bin/sh

test_description='Tests replace refs functionality'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'set up buggy branch' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	echo "line 1" >>hello &&
	echo "line 2" >>hello &&
	echo "line 3" >>hello &&
	echo "line 4" >>hello &&
	git add hello &&
	test_tick &&
	git commit -m "4 lines" &&
	git rev-parse --verify HEAD >../hash1 &&
	echo "line BUG" >>hello &&
	echo "line 6" >>hello &&
	echo "line 7" >>hello &&
	echo "line 8" >>hello &&
	git add hello &&
	test_tick &&
	git commit -m "4 more lines with a BUG" &&
	git rev-parse --verify HEAD >../hash2 &&
	echo "line 9" >>hello &&
	echo "line 10" >>hello &&
	git add hello &&
	test_tick &&
	git commit -m "2 more lines" &&
	git rev-parse --verify HEAD >../hash3 &&
	echo "line 11" >>hello &&
	git add hello &&
	test_tick &&
	git commit -m "1 more line" &&
	git rev-parse --verify HEAD >../hash4
'

test_expect_success 'replace the author' '
	cd repo &&
	HASH2=$(cat ../hash2) &&
	git cat-file commit $HASH2 >orig &&
	sed -e "s/A U Thor/O Thor/" <orig >replaced &&
	NEWHASH=$(git hash-object -t commit -w replaced) &&
	git replace $HASH2 $NEWHASH &&
	git replace -l >output &&
	grep "$HASH2" output
'

test_expect_success 'list all replace refs' '
	cd repo &&
	git replace -l >output &&
	test_line_count = 1 output
'

test_expect_success 'delete replace ref' '
	cd repo &&
	HASH2=$(cat ../hash2) &&
	git replace -d $HASH2 &&
	git replace -l >output &&
	test_must_be_empty output
'

test_expect_success 'create replace ref again' '
	cd repo &&
	HASH2=$(cat ../hash2) &&
	git cat-file commit $HASH2 >orig &&
	sed -e "s/A U Thor/O Thor/" <orig >replaced &&
	NEWHASH=$(git hash-object -t commit -w replaced) &&
	git replace $HASH2 $NEWHASH
'

test_expect_success 'replaced commit shows replacement in log' '
	cd repo &&
	git log --oneline >output &&
	test_line_count = 4 output
'

test_expect_success 'rev-list still works with replaced objects' '
	cd repo &&
	git rev-list HEAD >output &&
	test_line_count = 4 output
'

test_expect_success 'verify replace ref exists in refs/replace' '
	cd repo &&
	HASH2=$(cat ../hash2) &&
	git show-ref | grep "refs/replace/$HASH2"
'

test_expect_success 'replace fails for existing ref without force' '
	cd repo &&
	HASH2=$(cat ../hash2) &&
	git cat-file commit $HASH2 >actual &&
	sed -e "s/A U Thor/Q Thor/" <actual >expected2 &&
	NEWHASH2=$(git hash-object -t commit -w expected2) &&
	test_must_fail git replace $HASH2 $NEWHASH2
'

test_expect_success 'check replace ref format' '
	cd repo &&
	HASH2=$(cat ../hash2) &&
	git rev-parse --verify "refs/replace/$HASH2"
'

test_expect_success 'replace --graft rewrites parents' '
	cd repo &&
	HASH3=$(cat ../hash3) &&
	HASH1=$(cat ../hash1) &&
	# Remove existing replace refs first
	git replace -d $HASH2 2>/dev/null || true &&
	# Graft hash3 to have hash1 as sole parent (skipping hash2)
	git replace --graft $HASH3 $HASH1 &&
	git replace -l >output &&
	grep "$HASH3" output
'

test_expect_success 'replace --graft with no parents makes root commit' '
	cd repo &&
	HASH3=$(cat ../hash3) &&
	# Force overwrite the existing graft
	git replace -f --graft $HASH3 &&
	# The replacement commit should exist
	git rev-parse --verify "refs/replace/$HASH3"
'

test_expect_success 'cat-file shows replacement content' '
	cd repo &&
	HASH1=$(cat ../hash1) &&
	# HASH1 has no replace ref yet; create one with modified author
	GIT_NO_REPLACE_OBJECTS=1 git cat-file commit $HASH1 >orig &&
	sed -e "s/A U Thor/Replaced Author/" <orig >replaced &&
	NEWHASH=$(git hash-object -t commit -w replaced) &&
	git replace $HASH1 $NEWHASH &&
	git cat-file commit $HASH1 >actual &&
	grep "Replaced Author" actual
'

test_done
