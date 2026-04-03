#!/bin/sh
#
# Copyright (c) 2006 Junio C Hamano
#

test_description='quoted output'

. ./test-lib.sh

FN='濱野'
GN='純'
HT='	'
DQ='"'

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	mkdir "$FN" &&
	echo initial >"Name" &&
	echo initial >"With SP in it" &&
	echo initial >"$FN $GN" &&
	echo initial >"$FN$GN" &&
	echo initial >"$FN/file" &&
	git add . &&
	git commit -q -m Initial &&

	echo second >"Name" &&
	echo second >"With SP in it" &&
	echo second >"$FN $GN" &&
	echo second >"$FN$GN" &&
	echo second >"$FN/file" &&
	git commit -a -m Second &&

	echo modified >"Name" &&
	echo modified >"With SP in it" &&
	echo modified >"$FN $GN" &&
	echo modified >"$FN$GN" &&
	echo modified >"$FN/file"
'

test_expect_success 'check output from ls-files' '
	git ls-files >current &&
	grep "Name" current &&
	grep "With SP in it" current
'

test_expect_success 'check output from diff-files' '
	git diff --name-only >current &&
	grep "Name" current
'

test_expect_success 'check output from diff-index' '
	git diff --name-only HEAD >current &&
	grep "Name" current
'

test_expect_success 'check output from diff-tree' '
	git diff --name-only HEAD^ HEAD >current &&
	grep "Name" current
'

test_expect_success 'check output from ls-tree' '
	git ls-tree --name-only -r HEAD >current &&
	grep "Name" current
'

test_done
