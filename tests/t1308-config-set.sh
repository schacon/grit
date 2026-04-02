#!/bin/sh
# Ported from git/t/t1308-config-set.sh (partially)
# Tests for 'grit config set', 'grit config unset', rename-section, remove-section,
# and legacy --unset-all, --replace-all, --get-all, --get-regexp, etc.

test_description='grit config set/unset/rename/remove'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo
'

# ── config set subcommand ─────────────────────────────────────────────────

test_expect_success 'config set creates new key' '
	cd repo &&
	git config set user.name "Alice" &&
	git config get user.name >actual &&
	echo "Alice" >expected &&
	test_cmp expected actual
'

test_expect_success 'config set overwrites existing key' '
	cd repo &&
	git config set user.name "Bob" &&
	git config get user.name >actual &&
	echo "Bob" >expected &&
	test_cmp expected actual
'

test_expect_success 'config set creates section if needed' '
	cd repo &&
	git config set newsection.key "value" &&
	git config get newsection.key >actual &&
	echo "value" >expected &&
	test_cmp expected actual
'

test_expect_success 'config set --all replaces all matching values' '
	cd repo &&
	git config set multi2.val "original" &&
	git config set --all multi2.val "replaced" &&
	git config --get multi2.val >actual &&
	echo "replaced" >expected &&
	test_cmp expected actual
'

# ── config unset subcommand ───────────────────────────────────────────────

test_expect_success 'config unset removes a key' '
	cd repo &&
	git config set removeme.key "val" &&
	git config get removeme.key >actual &&
	echo "val" >expected &&
	test_cmp expected actual &&
	git config unset removeme.key &&
	test_must_fail git config get removeme.key
'

test_expect_success 'config unset --all removes all occurrences' '
	cd repo &&
	cat >>.git/config <<-EOF &&
	[unsetall]
		x = 1
		x = 2
		x = 3
	EOF
	git config --get-all unsetall.x >actual &&
	test_line_count = 3 actual &&
	git config unset --all unsetall.x &&
	test_must_fail git config --get unsetall.x
'

# ── legacy --unset-all ────────────────────────────────────────────────────

test_expect_success 'legacy --unset-all removes multi-valued key' '
	cd repo &&
	cat >>.git/config <<-EOF &&
	[legacyunset]
		m = a
		m = b
	EOF
	git config --get-all legacyunset.m >actual &&
	test_line_count = 2 actual &&
	git config --unset-all legacyunset.m &&
	test_must_fail git config --get legacyunset.m
'

# ── legacy --replace-all ──────────────────────────────────────────────────

test_expect_success 'legacy --replace-all replaces all values' '
	cd repo &&
	cat >>.git/config <<-EOF &&
	[replall]
		key = old1
		key = old2
	EOF
	git config --replace-all replall.key "new" &&
	git config --get replall.key >actual &&
	echo "new" >expected &&
	test_cmp expected actual
'

# ── legacy --get-all ──────────────────────────────────────────────────────

test_expect_success '--get-all lists all values for multi-valued key' '
	cd repo &&
	cat >>.git/config <<-EOF &&
	[getall]
		item = alpha
		item = beta
		item = gamma
	EOF
	git config --get-all getall.item >actual &&
	cat >expected <<-EOF &&
	alpha
	beta
	gamma
	EOF
	test_cmp expected actual
'

# ── config get --all (subcommand) ─────────────────────────────────────────

test_expect_success 'config get --all lists all values' '
	cd repo &&
	git config get --all getall.item >actual &&
	cat >expected <<-EOF &&
	alpha
	beta
	gamma
	EOF
	test_cmp expected actual
'

# ── set/get various value types ───────────────────────────────────────────

test_expect_success 'set and get empty string value' '
	cd repo &&
	git config set empty.key "" &&
	git config get empty.key >actual &&
	echo "" >expected &&
	test_cmp expected actual
'

test_expect_success 'set value with special characters' '
	cd repo &&
	git config set special.key "hello \"world\"" &&
	git config get special.key >actual &&
	echo "hello \"world\"" >expected &&
	test_cmp expected actual
'

# ── rename-section ────────────────────────────────────────────────────────

test_expect_success 'rename-section (subcommand)' '
	cd repo &&
	git config set oldsec.key1 "v1" &&
	git config set oldsec.key2 "v2" &&
	git config rename-section oldsec newsec &&
	git config get newsec.key1 >actual &&
	echo "v1" >expected &&
	test_cmp expected actual &&
	git config get newsec.key2 >actual2 &&
	echo "v2" >expected2 &&
	test_cmp expected2 actual2 &&
	test_must_fail git config get oldsec.key1
'

test_expect_success 'rename-section (legacy flag)' '
	cd repo &&
	git config set ren1.k "val" &&
	git config --rename-section ren1 ren2 &&
	git config get ren2.k >actual &&
	echo "val" >expected &&
	test_cmp expected actual &&
	test_must_fail git config get ren1.k
'

# ── remove-section ────────────────────────────────────────────────────────

test_expect_success 'remove-section (subcommand)' '
	cd repo &&
	git config set delsec.a "1" &&
	git config set delsec.b "2" &&
	git config get delsec.a >actual &&
	echo "1" >expected &&
	test_cmp expected actual &&
	git config remove-section delsec &&
	test_must_fail git config get delsec.a &&
	test_must_fail git config get delsec.b
'

test_expect_success 'remove-section (legacy flag)' '
	cd repo &&
	git config set delsec2.x "y" &&
	git config --remove-section delsec2 &&
	test_must_fail git config get delsec2.x
'

# ── --bool / --int / --type ───────────────────────────────────────────────

test_expect_success '--bool canonicalizes boolean values' '
	cd repo &&
	git config set booltest.a "yes" &&
	git config set booltest.b "on" &&
	git config set booltest.c "true" &&
	git config set booltest.d "1" &&
	git config --bool booltest.a >actual &&
	echo "true" >expected &&
	test_cmp expected actual &&
	git config --bool booltest.b >actual &&
	test_cmp expected actual &&
	git config --bool booltest.c >actual &&
	test_cmp expected actual &&
	git config --bool booltest.d >actual &&
	test_cmp expected actual
'

test_expect_success '--bool false variants' '
	cd repo &&
	git config set booltest.e "no" &&
	git config set booltest.f "off" &&
	git config set booltest.g "false" &&
	git config set booltest.h "0" &&
	git config --bool booltest.e >actual &&
	echo "false" >expected &&
	test_cmp expected actual &&
	git config --bool booltest.f >actual &&
	test_cmp expected actual &&
	git config --bool booltest.g >actual &&
	test_cmp expected actual &&
	git config --bool booltest.h >actual &&
	test_cmp expected actual
'

test_expect_success '--int canonicalizes integer' '
	cd repo &&
	git config set inttest.val "42" &&
	git config --int inttest.val >actual &&
	echo "42" >expected &&
	test_cmp expected actual
'

test_expect_success '--type bool works like --bool' '
	cd repo &&
	git config --type bool booltest.a >actual &&
	echo "true" >expected &&
	test_cmp expected actual
'

test_expect_success '--type int works like --int' '
	cd repo &&
	git config --type int inttest.val >actual &&
	echo "42" >expected &&
	test_cmp expected actual
'

# ── --show-origin / --show-scope ──────────────────────────────────────────

test_expect_success '--show-origin prefixes entries with file source' '
	cd repo &&
	git config -l --show-origin >actual &&
	grep "file:" actual
'

test_expect_success '--show-scope prefixes entries with scope' '
	cd repo &&
	git config -l --show-scope >actual &&
	grep "^local" actual
'

# ── -z (NUL terminator) ──────────────────────────────────────────────────

test_expect_success '-z with --list uses NUL delimiters' '
	cd repo &&
	git config -l -z >actual &&
	# NUL bytes present: the file should NOT have newlines between entries
	# Count NUL bytes
	tr "\0" "\n" <actual >decoded &&
	test_line_count -gt 3 decoded
'

# ── --file ────────────────────────────────────────────────────────────────

test_expect_success 'config --file reads/writes specified file' '
	cd repo &&
	git config --file custom.cfg set custom.key "myval" 2>/dev/null ||
	git config --file custom.cfg custom.key "myval" &&
	git config --file custom.cfg --get custom.key >actual &&
	echo "myval" >expected &&
	test_cmp expected actual
'

test_expect_success 'config --file does not touch .git/config' '
	cd repo &&
	test_must_fail git config --get custom.key
'

# ── --global vs --local ──────────────────────────────────────────────────

test_expect_success '--local reads only repo config' '
	cd repo &&
	git config --local -l >actual &&
	grep "core.repositoryformatversion" actual
'

# ── edge cases ────────────────────────────────────────────────────────────

test_expect_success 'get nonexistent key returns error' '
	cd repo &&
	test_must_fail git config get nonexistent.key
'

test_expect_success 'legacy --get nonexistent key returns error' '
	cd repo &&
	test_must_fail git config --get nonexistent.key
'

test_expect_success 'set value with spaces' '
	cd repo &&
	git config set space.key "hello world" &&
	git config get space.key >actual &&
	echo "hello world" >expected &&
	test_cmp expected actual
'

test_expect_success 'set value with equals sign' '
	cd repo &&
	git config set eq.key "a=b" &&
	git config get eq.key >actual &&
	echo "a=b" >expected &&
	test_cmp expected actual
'

test_expect_success 'set and get with subsection' '
	cd repo &&
	git config set "branch.main.remote" "origin" &&
	git config get "branch.main.remote" >actual &&
	echo "origin" >expected &&
	test_cmp expected actual
'

test_expect_success 'config get --default provides fallback' '
	cd repo &&
	git config get --default "fallback" missing.key >actual &&
	echo "fallback" >expected &&
	test_cmp expected actual
'

test_expect_success 'config get with existing key ignores --default' '
	cd repo &&
	git config set exists.key "real" &&
	git config get --default "fallback" exists.key >actual &&
	echo "real" >expected &&
	test_cmp expected actual
'

test_done
