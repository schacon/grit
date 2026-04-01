#!/bin/sh
# Ported from git/t/t1300-config.sh
# Tests for 'gust config'.

test_description='gust config'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	git init repo
'

test_expect_success 'set and get a value (legacy)' '
	cd repo &&
	git config user.name "Test User" &&
	git config user.name >actual &&
	echo "Test User" >expected &&
	test_cmp expected actual
'

test_expect_success 'set and get a value (subcommand)' '
	cd repo &&
	git config set user.email "test@example.com" &&
	git config get user.email >actual &&
	echo "test@example.com" >expected &&
	test_cmp expected actual
'

test_expect_success 'overwrite existing value' '
	cd repo &&
	git config user.name "New Name" &&
	git config user.name >actual &&
	echo "New Name" >expected &&
	test_cmp expected actual
'

test_expect_success 'mixed case section' '
	cd repo &&
	git config Section.Movie BadPhysics &&
	git config Section.Movie >actual &&
	echo "BadPhysics" >expected &&
	test_cmp expected actual
'

test_expect_success 'uppercase section reuses existing section block' '
	cd repo &&
	git config SECTION.UPPERCASE true &&
	git config SECTION.UPPERCASE >actual &&
	echo "true" >expected &&
	test_cmp expected actual
'

test_expect_success 'get non-existent key returns exit 1' '
	cd repo &&
	! git config nonexistent.key
'

test_expect_success 'unset a key (legacy)' '
	cd repo &&
	git config extra.key "temp" &&
	git config extra.key >actual &&
	echo "temp" >expected &&
	test_cmp expected actual &&
	git config --unset extra.key &&
	! git config extra.key
'

test_expect_success 'unset a key (subcommand)' '
	cd repo &&
	git config set extra.key2 "temp2" &&
	git config unset extra.key2 &&
	! git config get extra.key2
'

test_expect_success 'list all config entries' '
	cd repo &&
	git config --list >actual &&
	grep "user.name=New Name" actual &&
	grep "user.email=test@example.com" actual
'

test_expect_success 'list local config only' '
	cd repo &&
	git config --list --local >actual &&
	grep "user.name=New Name" actual
'

test_expect_success '--bool normalizes boolean values' '
	cd repo &&
	git config core.flag yes &&
	git config --bool core.flag >actual &&
	echo "true" >expected &&
	test_cmp expected actual
'

test_expect_success '--bool normalizes false' '
	cd repo &&
	git config core.flag2 off &&
	git config --bool core.flag2 >actual &&
	echo "false" >expected &&
	test_cmp expected actual
'

test_expect_success '--int normalizes integer values' '
	cd repo &&
	git config pack.windowmemory 1k &&
	git config --int pack.windowmemory >actual &&
	echo "1024" >expected &&
	test_cmp expected actual
'

test_expect_success '--int with M suffix' '
	cd repo &&
	git config pack.bigsize 2m &&
	git config --int pack.bigsize >actual &&
	echo "2097152" >expected &&
	test_cmp expected actual
'

test_expect_success 'remove-section removes entire section (legacy)' '
	cd repo &&
	git config extra2.a one &&
	git config extra2.b two &&
	git config --remove-section extra2 &&
	! git config extra2.a &&
	! git config extra2.b
'

test_expect_success 'remove-section via subcommand' '
	cd repo &&
	git config extra3.a one &&
	git config extra3.b two &&
	git config remove-section extra3 &&
	! git config extra3.a
'

test_expect_success 'rename-section (legacy)' '
	cd repo &&
	git config oldsec.key val &&
	git config --rename-section oldsec newsec &&
	! git config oldsec.key &&
	git config newsec.key >actual &&
	echo "val" >expected &&
	test_cmp expected actual
'

test_expect_success 'rename-section via subcommand' '
	cd repo &&
	git config old2.key val2 &&
	git config rename-section old2 new2 &&
	git config new2.key >actual &&
	echo "val2" >expected &&
	test_cmp expected actual
'

test_expect_success 'subsection (remote.origin.url)' '
	cd repo &&
	git config "remote.origin.url" "https://example.com/repo.git" &&
	git config remote.origin.url >actual &&
	echo "https://example.com/repo.git" >expected &&
	test_cmp expected actual
'

test_expect_success 'config value with special chars (hash, semicolon)' '
	cd repo &&
	git config section.comment "value # not a comment" &&
	git config section.comment >actual &&
	echo "value # not a comment" >expected &&
	test_cmp expected actual
'

test_expect_success '--show-scope shows scope' '
	cd repo &&
	git config --list --show-scope --local >actual &&
	grep "^local" actual
'

test_expect_success '--name-only shows only key names' '
	cd repo &&
	git config --list --name-only --local >actual &&
	grep "^user.name$" actual &&
	! grep "=" actual
'

test_expect_success '-z uses NUL as delimiter' '
	cd repo &&
	git config -z --list --local >actual &&
	tr "\0" "\n" <actual >decoded &&
	grep "user.name=New Name" decoded
'

test_expect_success 'multiple values for same key (get --all)' '
	cd repo &&
	git config multi.key first &&
	printf "\tkey = second\n" >>.git/config &&
	git config get --all multi.key >actual &&
	echo "first" >expected &&
	echo "second" >>expected &&
	test_cmp expected actual
'

test_done
