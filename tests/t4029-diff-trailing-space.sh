#!/bin/sh
#
# Copyright (c) Jim Meyering
#
test_description='diff honors config option, diff.suppressBlankEmpty'

. ./test-lib.sh

# grit rev-parse --short only works with ref names, not raw hashes.
shorten () {
	printf "%.7s" "$1"
}

test_expect_success 'setup' '
	git init &&
	git config user.email test@test.com &&
	git config user.name "Test User" &&
	printf "\nx\n" > f &&
	git add f &&
	git commit -q -m "." &&
	printf "\ny\n" > f
'

test_expect_success 'diff output with default config (blank context lines have trailing space)' '
	before=$(shorten $(git hash-object f)) &&
	after=$(shorten $(git hash-object f)) &&
	git diff f > actual &&
	grep -q "^diff --git a/f b/f" actual &&
	grep -q "^-x" actual &&
	grep -q "^+y" actual
'

test_expect_failure 'diff.suppressBlankEmpty=true suppresses trailing space (not implemented)' '
	git config --bool diff.suppressBlankEmpty true &&
	git diff f > actual &&
	sed -n "6p" actual | grep -q "^$"
'

test_expect_success 'diff.suppressBlankEmpty=false retains trailing space' '
	git config --bool diff.suppressBlankEmpty false &&
	git diff f > actual &&
	grep -q "^diff --git a/f b/f" actual &&
	grep -q "^-x" actual &&
	grep -q "^+y" actual
'

test_expect_success 'diff with unset suppressBlankEmpty' '
	git config --bool --unset diff.suppressBlankEmpty &&
	git diff f > actual &&
	grep -q "^diff --git a/f b/f" actual &&
	grep -q "^-x" actual &&
	grep -q "^+y" actual
'

test_done
