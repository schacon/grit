#!/bin/sh
test_description='grit pager configuration

Tests GIT_PAGER, core.pager, PAGER environment variables, and
their interaction. Also tests pager behavior when stdout is not a tty.'

. ./test-lib.sh

# ── Setup ────────────────────────────────────────────────────────────────────

test_expect_success 'setup: create repo with commits' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	for i in 1 2 3 4 5; do
		echo "content $i" >file$i.txt &&
		git add file$i.txt &&
		git commit -m "commit $i" || return 1
	done
'

# ── GIT_PAGER ────────────────────────────────────────────────────────────────

test_expect_success 'GIT_PAGER=cat allows normal output' '
	cd repo &&
	GIT_PAGER=cat git log --oneline >out &&
	test_line_count = 5 out
'

test_expect_success 'GIT_PAGER set does not break piped output' '
	cd repo &&
	GIT_PAGER="head -2" git log --oneline >out &&
	test -s out
'

test_expect_success 'GIT_PAGER ignored when stdout is not a tty' '
	cd repo &&
	GIT_PAGER="false" git log --oneline >out &&
	test_line_count = 5 out
'

# ── core.pager ───────────────────────────────────────────────────────────────

test_expect_success 'core.pager can be set' '
	cd repo &&
	git config core.pager cat &&
	git config --get core.pager >out &&
	grep "cat" out
'

test_expect_success 'core.pager=cat works with log' '
	cd repo &&
	git config core.pager cat &&
	git log --oneline >out &&
	test_line_count = 5 out
'

test_expect_success 'GIT_PAGER overrides core.pager' '
	cd repo &&
	git config core.pager "head -1" &&
	GIT_PAGER=cat git log --oneline >out &&
	test_line_count = 5 out
'

test_expect_success 'unsetting core.pager reverts to default' '
	cd repo &&
	git config --unset core.pager &&
	test_must_fail git config --get core.pager
'

# ── PAGER ────────────────────────────────────────────────────────────────────

test_expect_success 'PAGER=cat allows normal output' '
	cd repo &&
	PAGER=cat git log --oneline >out &&
	test -s out
'

test_expect_success 'GIT_PAGER overrides PAGER' '
	cd repo &&
	GIT_PAGER=cat PAGER="head -1" git log --oneline >out &&
	test_line_count = 5 out
'

# ── Non-TTY behavior (piped stdout) ─────────────────────────────────────────

test_expect_success 'log output works when piped' '
	cd repo &&
	git log --oneline | cat >out &&
	test_line_count = 5 out
'

test_expect_success 'status works when piped' '
	cd repo &&
	git status --porcelain | cat >out
'

test_expect_success 'diff works when piped' '
	cd repo &&
	echo "changed" >file1.txt &&
	git diff | cat >out &&
	test -s out &&
	git checkout -- file1.txt
'

test_expect_success 'branch list works when piped' '
	cd repo &&
	git branch | cat >out &&
	grep "master" out
'

# ── Various commands respect pager setting ───────────────────────────────────

test_expect_success 'log respects GIT_PAGER' '
	cd repo &&
	GIT_PAGER=cat git log --oneline >out &&
	test -s out
'

test_expect_success 'diff respects GIT_PAGER' '
	cd repo &&
	echo "change" >file1.txt &&
	GIT_PAGER=cat git diff >out &&
	test -s out &&
	git checkout -- file1.txt
'

test_expect_success 'show respects GIT_PAGER' '
	cd repo &&
	GIT_PAGER=cat git show --oneline HEAD >out &&
	test -s out
'

# ── Edge cases ───────────────────────────────────────────────────────────────

test_expect_success 'empty pager string uses default' '
	cd repo &&
	GIT_PAGER="" git log --oneline >out &&
	test -s out
'

test_expect_success 'core.pager with empty value' '
	cd repo &&
	git config core.pager "" &&
	git log --oneline >out &&
	test -s out &&
	git config --unset core.pager
'

test_expect_success 'pager does not affect porcelain status' '
	cd repo &&
	GIT_PAGER="head -1" git status --porcelain >out
'

test_expect_success 'pager does not affect rev-parse' '
	cd repo &&
	GIT_PAGER="head -1" git rev-parse HEAD >out &&
	test -s out
'

test_done
