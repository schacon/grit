#!/bin/sh

test_description='Basic subproject-like functionality (nested repos)'

. ./test-lib.sh

test_expect_success 'setup: create project with files' '
	git init -q &&
	echo content >Makefile &&
	echo data >README &&
	git add Makefile README &&
	git commit -m "Initial commit" &&
	git branch save
'

test_expect_success 'add more files and commit' '
	echo more >extra &&
	git add extra &&
	git commit -m "Added extra file"
'

test_expect_success 'diff-tree between commits shows changes' '
	git diff-tree HEAD^ HEAD >output &&
	test -s output
'

test_expect_success 'ls-files shows all tracked files' '
	git ls-files >actual &&
	cat >expect <<-\EOF &&
	Makefile
	README
	extra
	EOF
	test_cmp expect actual
'

test_expect_success 'checkout previous branch' '
	git checkout save &&
	git ls-files >actual &&
	cat >expect <<-\EOF &&
	Makefile
	README
	EOF
	test_cmp expect actual
'

test_done
