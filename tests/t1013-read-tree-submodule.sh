#!/bin/sh

test_description='read-tree can handle submodules'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo content >file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	"$REAL_GIT" init sub &&
	(cd sub && echo x >x && "$REAL_GIT" add x && "$REAL_GIT" commit -m "sub init") &&
	git add sub &&
	test_tick &&
	git commit -m "add submodule" &&
	git branch with-sub
'

test_expect_success 'read-tree with submodule entry does not crash' '
	cd repo &&
	git read-tree HEAD
'

test_done
