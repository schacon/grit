#!/bin/sh

test_description='blame on files created by combining other files'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup split file case' '
	git init blame-split &&
	cd blame-split &&

	test_seq 1000 1010 >one &&
	test_seq 2000 2010 >two &&
	git add one two &&
	test_tick &&
	git commit -m base &&
	git tag base &&

	sed "6s/^/modified /" <one >one.tmp &&
	mv one.tmp one &&
	sed "6s/^/modified /" <two >two.tmp &&
	mv two.tmp two &&
	git add -u &&
	test_tick &&
	git commit -m modified &&
	git tag modified &&

	cat one two >combined &&
	git add combined &&
	git rm one two &&
	test_tick &&
	git commit -m combined &&
	git tag combined
'

test_expect_success 'blame combined file works' '
	cd blame-split &&
	git blame combined >output &&
	test $(wc -l <output) -eq 22
'

test_expect_success 'blame --porcelain combined file works' '
	cd blame-split &&
	git blame --porcelain combined >output &&
	grep "^author " output
'

test_expect_success 'blame --line-porcelain combined file works' '
	cd blame-split &&
	git blame --line-porcelain combined >output &&
	grep "^author " output >authors &&
	test $(wc -l <authors) -eq 22
'

test_done
