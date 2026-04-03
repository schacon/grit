#!/bin/sh
# Ported from git/t/t6431-merge-criscross.sh
# Tests merge with criss-cross history

test_description='merge-recursive criss-cross test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo with criss-cross history' '
	git init criss &&
	cd criss &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	mkdir data &&
	n=1 &&
	while test $n -le 5
	do
		echo $n > data/$n &&
		n=$(($n+1))
	done &&

	git add data &&
	git commit -m A &&
	git tag A &&

	git checkout -b B A &&
	git rm data/5 &&
	git commit -m B &&

	git checkout -b C A &&
	git rm data/4 &&
	git commit -m C
'

test_expect_success 'merge B and C' '
	cd criss &&
	git checkout B &&
	git merge C -m "merge C into B" &&
	test_path_is_missing data/4 &&
	test_path_is_missing data/5 &&
	test_path_is_file data/1
'

test_expect_success 'reverse merge C and B' '
	cd criss &&
	git checkout C &&
	git merge B -m "merge B into C" &&
	test_path_is_missing data/4 &&
	test_path_is_missing data/5 &&
	test_path_is_file data/1
'

test_done
