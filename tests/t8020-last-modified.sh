#!/bin/sh

test_description='last-modified tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init last-mod &&
	cd last-mod &&
	echo 1 >file &&
	git add file &&
	test_tick &&
	git commit -m 1 &&
	git tag t1 &&
	mkdir a &&
	echo 2 >a/file &&
	git add a/file &&
	test_tick &&
	git commit -m 2 &&
	git tag t2 &&
	mkdir a/b &&
	echo 3 >a/b/file &&
	git add a/b/file &&
	test_tick &&
	git commit -m 3 &&
	git tag t3
'

test_expect_success 'last-modified shows output' '
	cd last-mod &&
	git last-modified >output &&
	test -s output
'

test_expect_success 'last-modified lists entries' '
	cd last-mod &&
	git last-modified >output &&
	grep "file" output &&
	grep "a" output
'

test_done
