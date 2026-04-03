#!/bin/sh

test_description='basic git merge-index tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup diverging branches' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	test_write_lines 1 2 3 4 5 6 7 8 9 10 >file &&
	git add file &&
	git commit -m base &&
	git tag base &&
	sed s/2/two/ <file >tmp &&
	mv tmp file &&
	git add file &&
	git commit -m two &&
	git tag two &&
	git checkout -b other HEAD^ &&
	sed s/10/ten/ <file >tmp &&
	mv tmp file &&
	git add file &&
	git commit -m ten &&
	git tag ten
'

test_expect_success 'three-way read-tree creates unmerged entries' '
	cd repo &&
	BASE=$(git rev-parse base) &&
	TEN=$(git rev-parse ten) &&
	TWO=$(git rev-parse two) &&
	git read-tree -m $BASE $TEN $TWO &&
	git ls-files -u >unmerged &&
	test -s unmerged
'

test_expect_success 'merge-index with merge program' '
	cd repo &&
	BASE=$(git rev-parse base) &&
	TEN=$(git rev-parse ten) &&
	TWO=$(git rev-parse two) &&
	git read-tree -m $BASE $TEN $TWO &&
	git merge-index git-merge-one-file -a 2>err || true
'

test_done
