#!/bin/sh
# Ported from upstream git t7519-status-fsmonitor.sh
# fsmonitor not available, test basic status operations

test_description='status with fsmonitor-like scenarios'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init fsmon-repo &&
	cd fsmon-repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo content >tracked &&
	git add tracked &&
	test_tick &&
	git commit -m initial &&
	sleep 1
'

test_expect_success 'status on clean repo shows branch' '
	cd fsmon-repo &&
	git status >actual &&
	grep "On branch" actual
'

test_expect_success 'status with modified file' '
	cd fsmon-repo &&
	echo modified >tracked &&
	git status >actual &&
	grep "modified" actual
'

test_expect_success 'status with new file' '
	cd fsmon-repo &&
	echo new >untracked &&
	git status >actual &&
	grep "untracked" actual
'

test_expect_success 'status --porcelain' '
	cd fsmon-repo &&
	git status --porcelain >actual &&
	test -s actual
'

test_expect_success 'status --short' '
	cd fsmon-repo &&
	git status --short >actual &&
	test -s actual
'

test_expect_success 'status after commit' '
	cd fsmon-repo &&
	git add tracked &&
	test_tick &&
	git commit -m "update" &&
	git status >actual &&
	grep "untracked" actual
'

test_done
