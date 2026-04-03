#!/bin/sh

test_description='update-index and add refuse to add beyond symlinks'

. ./test-lib.sh

test_expect_success SYMLINKS 'setup' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "t@t" &&
	>a &&
	mkdir b &&
	ln -s b c &&
	>c/d &&
	git update-index --add a b/d
'

test_expect_failure 'update-index --add beyond symlinks' '
	test_have_prereq SYMLINKS || return 0 &&
	test_must_fail git update-index --add c/d &&
	cat >expect <<-\EOF &&
	a
	b/d
	EOF
	git ls-files >actual &&
	test_cmp expect actual
'

test_expect_failure 'add beyond symlinks' '
	test_have_prereq SYMLINKS || return 0 &&
	test_must_fail git add c/d &&
	cat >expect <<-\EOF &&
	a
	b/d
	EOF
	git ls-files >actual &&
	test_cmp expect actual
'

test_done
