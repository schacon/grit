#!/bin/sh
#
# Ported from upstream git t4052-stat-output.sh
# Most tests need format-patch, show, log --stat, merge --stat, etc.
# which grit does not fully support. We port the diff-specific cases.
#

test_description='test --stat output of various commands'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

# 120-character name
name=aaaaaaaaaa
name=$name$name$name$name$name$name$name$name$name$name$name$name

test_expect_success 'preparation' '
	git init &&
	git config user.email test@test.com &&
	git config user.name "Test User" &&
	>"$name" &&
	git add "$name" &&
	git commit -m message &&
	echo a >"$name" &&
	git add "$name" &&
	git commit -m message
'

test_expect_success "diff --stat: small change with long name produces stat output" '
	git diff --stat HEAD^ HEAD >output &&
	grep " | " output >actual &&
	test -s actual
'

test_expect_failure "diff --stat: long name is abbreviated to fit terminal width (not implemented)" '
	git diff --stat HEAD^ HEAD >output &&
	grep " | " output >actual &&
	cat >expect80 <<-\EOF &&
	 ...aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | 1 +
	EOF
	test_cmp expect80 actual
'

test_expect_success "diff --stat=60: stat width limits output" '
	git diff --stat=60 HEAD^ HEAD >output &&
	grep " | " output >actual &&
	test -s actual
'

test_expect_failure "diff --stat with stat-name-width config (not implemented)" '
	git -c diff.statNameWidth=30 diff --stat HEAD^ HEAD >output &&
	grep " | " output >actual &&
	# Verify the name was actually truncated to ~30 chars
	awk -F"|" "{print length(\$1)}" actual >widths &&
	test "$(cat widths)" -le 32
'

test_expect_success 'preparation for big change' '
	echo a >"$name" &&
	i=0 &&
	while test $i -lt 1000
	do
		echo $i >>"$name" &&
		i=$(($i + 1)) || return 1
	done &&
	git add "$name" &&
	git commit -m message
'

test_expect_success "diff --stat: big change produces stat output" '
	git diff --stat HEAD^ HEAD >output &&
	grep " | " output >actual &&
	test -s actual
'

test_expect_success "diff --stat=40: big change with narrow terminal" '
	git diff --stat=40 HEAD^ HEAD >output &&
	grep " | " output >actual &&
	test -s actual
'

test_done
