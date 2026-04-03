#!/bin/sh
test_description='git rev-list should notice bad commits'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo "foo" >foo && git add foo && git commit -m "first commit" &&
	echo "bar" >bar && git add bar && git commit -m "second commit" &&
	echo "baz" >baz && git add baz && git commit -m "third commit" &&
	echo "foo again" >>foo && git add foo && git commit -m "fourth commit"
'

test_expect_success 'verify number of revisions' '
	cd repo &&
	revs=$(git rev-list --all | wc -l) &&
	test $revs -eq 4
'

test_done
