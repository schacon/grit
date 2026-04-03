#!/bin/sh

test_description='git clean interactive mode (basic)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Interactive clean requires terminal interaction which is hard to test.
# Test the non-interactive parts of clean that relate to the interactive test.

test_expect_success 'setup' '
	git init clean-interactive &&
	cd clean-interactive &&
	git config clean.requireForce no &&
	mkdir -p src docs &&
	echo code >src/main.c &&
	echo readme >docs/README &&
	git add . &&
	git commit -m initial
'

test_expect_success 'git clean -n shows what would be removed' '
	cd clean-interactive &&
	touch untracked1 untracked2 &&
	mkdir extra-dir &&
	touch extra-dir/file &&
	git clean -n >output &&
	grep "untracked1" output &&
	grep "untracked2" output
'

test_expect_success 'git clean -n -d shows directories too' '
	cd clean-interactive &&
	git clean -n -d >output &&
	grep "extra-dir" output
'

test_expect_success 'git clean on specific path' '
	cd clean-interactive &&
	mkdir -p subdir &&
	touch subdir/untracked &&
	touch other-untracked &&
	git clean subdir/ &&
	test_path_is_missing subdir/untracked &&
	test_path_is_file other-untracked &&
	rm -f other-untracked
'

test_expect_success 'git clean -d removes untracked directories' '
	cd clean-interactive &&
	mkdir -p newdir &&
	touch newdir/file &&
	git clean -d &&
	test_path_is_missing newdir
'

test_done
