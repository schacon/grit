#!/bin/sh

test_description='git status ignored modes'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup initial commit and ignore file' '
	cat >.gitignore <<-\EOF &&
	*.ign
	ignored_dir/
	EOF
	git add . &&
	git commit -m "Initial commit"
'

test_expect_success 'ignored files are not shown in normal status' '
	test_when_finished "git clean -fdx" &&
	mkdir -p ignored dir/ignored &&
	touch ignored/ignored_1.ign ignored/ignored_2.ign \
		dir/ignored/ignored_1.ign dir/ignored/ignored_2.ign &&
	git status --porcelain >output &&
	! grep "ignored_1.ign" output &&
	! grep "ignored_2.ign" output
'

test_expect_success 'ignored directory is not shown in normal status' '
	test_when_finished "git clean -fdx" &&
	mkdir ignored_dir &&
	touch ignored_dir/ignored_1 ignored_dir/ignored_2 &&
	git status --porcelain >output &&
	! grep "ignored_dir" output
'

test_expect_success 'status --ignored shows ignored items' '
	test_when_finished "git clean -fdx" &&
	mkdir -p ignored dir/ignored &&
	touch ignored/ignored_1.ign dir/ignored/ignored_1.ign &&
	git status --ignored >output &&
	grep "ignored" output
'

test_done
