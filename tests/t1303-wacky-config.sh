#!/bin/sh
# Ported from git/t/t1303-wacky-config.sh
# Test wacky input to git config.

test_description='Test wacky input to git config'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	git init repo
'

# Leaving off the newline is intentional!
setup() {
	(printf "[section]\n" &&
	printf "  key = foo") >repo/.git/config
}

# 'check section.key value' verifies that the entry for section.key is 'value'
check() {
	echo "$2" >expected
	(cd repo && git config --get "$1") >actual 2>&1
	test_cmp expected actual
}

test_expect_success 'modify same key' '
	setup &&
	(cd repo && git config section.key bar) &&
	check section.key bar
'

test_expect_success 'add key in same section' '
	setup &&
	(cd repo && git config section.other bar) &&
	check section.key foo &&
	check section.other bar
'

test_expect_success 'add key in different section' '
	setup &&
	(cd repo && git config section2.key bar) &&
	check section.key foo &&
	check section2.key bar
'

test_expect_success 'do not crash on special long config line' '
	LONG_VALUE=$(printf "x%01021dx a" 7) &&
	setup &&
	(cd repo && git config section.key "$LONG_VALUE") &&
	check section.key "$LONG_VALUE"
'

setup_many() {
	setup &&
	# This time we want the newline so that we can tack on more entries.
	echo >>repo/.git/config &&
	# Create 3125 additional entries (total 3126)
	python3 -c "print(\"  key = foo\n\" * 3125, end=\"\")" >>repo/.git/config
}

test_expect_success 'get many entries' '
	setup_many &&
	(cd repo && git config --get-all section.key) >actual &&
	test_line_count = 3126 actual
'

test_expect_success 'get many entries by regex' '
	setup_many &&
	(cd repo && git config --get-regexp "section.key") >actual &&
	test_line_count = 3126 actual
'

test_expect_success 'replace many entries' '
	setup_many &&
	(cd repo && git config --replace-all section.key bar) &&
	check section.key bar
'

test_expect_success 'unset many entries' '
	setup_many &&
	(cd repo && git config --unset-all section.key) &&
	(cd repo && test_must_fail git config section.key)
'

test_done
