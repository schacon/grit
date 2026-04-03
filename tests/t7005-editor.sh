#!/bin/sh
test_description='GIT_EDITOR, core.editor, and stuff'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo foo >file &&
	git add file &&
	git commit -m "initial"
'

test_expect_success 'commit -m bypasses editor' '
	cd repo &&
	echo bar >>file &&
	git add file &&
	GIT_EDITOR=true git commit -m "message from -m" &&
	git log -n 1 --format=%s >actual &&
	echo "message from -m" >expect &&
	test_cmp expect actual
'

test_done
