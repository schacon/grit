#!/bin/sh
#
# Ported from git/t/t1050-large.sh (subset)

test_description='adding and checking out large blobs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q
'

test_expect_success 'setup large files' '
	printf "%2000000s" X >large1 &&
	cp large1 large2 &&
	cp large1 large3
'

test_expect_success 'add a large file or two' '
	git add large1 large2
'

test_expect_success 'hash-object large file' '
	git hash-object large1
'

test_expect_success 'checkout a large file via cacheinfo' '
	large1=$(git hash-object large1) &&
	git update-index --add --cacheinfo 100644,$large1,another &&
	git checkout -- another &&
	test_cmp large1 another
'

test_expect_success 'diff --stat with large files' '
	test_tick &&
	git commit -q -m initial &&
	echo modified >>large1 &&
	git add large1 &&
	test_tick &&
	git commit -q -m modified &&
	git diff --stat HEAD^ HEAD
'

test_done
