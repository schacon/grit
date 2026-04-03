#!/bin/sh

test_description='blaming through history with topic branches'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup' '
	$REAL_GIT init blame-topic &&
	cd blame-topic &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&

	echo line0 >file.t &&
	$REAL_GIT add file.t &&
	test_tick &&
	$REAL_GIT commit -m A0 &&
	$REAL_GIT tag A0 &&

	echo A1content >A1.t &&
	$REAL_GIT add A1.t &&
	test_tick &&
	$REAL_GIT commit -m A1 &&
	$REAL_GIT tag A1 &&

	$REAL_GIT reset --hard A0 &&
	echo B1content >B1.t &&
	$REAL_GIT add B1.t &&
	test_tick &&
	$REAL_GIT commit -m B1 &&
	$REAL_GIT tag B1 &&

	echo line0changed >file.t &&
	$REAL_GIT add file.t &&
	test_tick &&
	$REAL_GIT commit -m B2 &&
	$REAL_GIT tag B2 &&

	$REAL_GIT checkout A1 &&
	$REAL_GIT merge --no-edit B2 &&
	$REAL_GIT tag A2 &&

	$REAL_GIT checkout A1 &&
	echo C1content >C1.t &&
	$REAL_GIT add C1.t &&
	test_tick &&
	$REAL_GIT commit -m C1 &&
	$REAL_GIT tag C1 &&

	$REAL_GIT checkout A2 &&
	$REAL_GIT merge --no-edit C1 &&
	$REAL_GIT tag A3
'

test_expect_success 'blame --porcelain on merged file' '
	cd blame-topic &&
	git blame --porcelain file.t >actual &&
	grep "^author " actual
'

test_expect_success 'blame identifies a commit for changed file' '
	cd blame-topic &&
	git blame --porcelain file.t >actual &&
	head -1 actual | cut -d" " -f1 >actual_sha &&
	# grit should blame some commit for this line
	test -s actual_sha
'

test_expect_success 'blame shows correct content' '
	cd blame-topic &&
	git blame file.t >actual &&
	grep "line0changed" actual
'

test_done
