#!/bin/sh
# Ported from git/t/t1300-config.sh
# Tests for 'grit config'.

test_description='grit config'

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

# ── Whitespace handling ────────────────────────────────────────────────────────

test_expect_success 'setup whitespace config file' '
	cd repo &&
	cat >.git/wscfg <<-EOF
	[ws]
		trailing = rock
		internal = big	blue
		quoted = "hello world "
		annotated = big blue	# to be discarded
		annotatedq = "big blue"# discard this too
	EOF
'

test_expect_success 'trailing whitespace stripped from unquoted value' '
	cd repo &&
	git config --file .git/wscfg ws.trailing >actual &&
	echo "rock" >expected &&
	test_cmp expected actual
'

test_expect_success 'internal whitespace preserved' '
	cd repo &&
	git config --file .git/wscfg ws.internal >actual &&
	printf "big\tblue\n" >expected &&
	test_cmp expected actual
'

test_expect_success 'trailing whitespace preserved inside quotes' '
	cd repo &&
	git config --file .git/wscfg ws.quoted >actual &&
	echo "hello world " >expected &&
	test_cmp expected actual
'

test_expect_success 'inline comment stripped from unquoted value' '
	cd repo &&
	git config --file .git/wscfg ws.annotated >actual &&
	echo "big blue" >expected &&
	test_cmp expected actual
'

test_expect_success 'inline comment stripped after closing quote' '
	cd repo &&
	git config --file .git/wscfg ws.annotatedq >actual &&
	echo "big blue" >expected &&
	test_cmp expected actual
'

# ── multivar: manual writes and --get-all ─────────────────────────────────────

test_expect_success 'setup multivar config file' '
	cd repo &&
	printf "[mv]\n\tkey = first\n\tkey = second\n\tkey = third\n" >.git/mvcfg
'

test_expect_success '--get-all returns all values for multivar key' '
	cd repo &&
	git config --file .git/mvcfg --get-all mv.key >actual &&
	printf "first\nsecond\nthird\n" >expected &&
	test_cmp expected actual
'

test_expect_success '--replace-all updates value in single-entry file' '
	cd repo &&
	printf "[rv]\n\tkey = original\n" >.git/rvcfg &&
	git config --file .git/rvcfg --replace-all rv.key updated &&
	git config --file .git/rvcfg rv.key >actual &&
	echo "updated" >expected &&
	test_cmp expected actual
'

test_expect_success 'plain get returns last value for multivar' '
	cd repo &&
	cat >.git/mv2cfg <<-\EOF
	[multi]
		key = alpha
		key = beta
	EOF
	git config --file .git/mv2cfg multi.key >actual &&
	echo "beta" >expected &&
	test_cmp expected actual
'

# ── --get-regexp ───────────────────────────────────────────────────────────────

test_expect_success 'setup regexp config file' '
	cd repo &&
	cat >.git/recfg <<-\EOF
	[alpha]
		foo = one
		bar = two
	[beta]
		foo = three
		baz = four
	EOF
'

test_expect_success '--get-regexp returns matching key value pairs' '
	cd repo &&
	git config --file .git/recfg --get-regexp foo >actual &&
	printf "alpha.foo one\nbeta.foo three\n" >expected &&
	test_cmp expected actual
'

test_expect_success '--name-only --get-regexp returns only key names' '
	cd repo &&
	git config --file .git/recfg --name-only --get-regexp foo >actual &&
	printf "alpha.foo\nbeta.foo\n" >expected &&
	test_cmp expected actual
'

test_expect_success '--get-regexp with no match exits non-zero' '
	cd repo &&
	! git config --file .git/recfg --get-regexp nonexistent
'

test_expect_success '--get-regexp matches partial key name' '
	cd repo &&
	git config --file .git/recfg --get-regexp ba >actual &&
	printf "alpha.bar two\nbeta.baz four\n" >expected &&
	test_cmp expected actual
'

# ── section and key case sensitivity ──────────────────────────────────────────

test_expect_success 'section name is case-insensitive on read' '
	cd repo &&
	git config --file .git/recfg ALPHA.FOO >actual &&
	echo "one" >expected &&
	test_cmp expected actual
'

test_expect_success 'key name is case-insensitive' '
	cd repo &&
	git config --file .git/recfg alpha.FOO >actual &&
	echo "one" >expected &&
	test_cmp expected actual
'

test_expect_success 'subsection name is case-sensitive' '
	cd repo &&
	cat >.git/subscfg <<-\EOF
	[remote "origin"]
		url = https://example.com
	[remote "Origin"]
		url = https://other.com
	EOF
	git config --file .git/subscfg remote.origin.url >actual &&
	echo "https://example.com" >expected &&
	test_cmp expected actual &&
	git config --file .git/subscfg remote.Origin.url >actual2 &&
	echo "https://other.com" >expected2 &&
	test_cmp expected2 actual2
'

test_expect_success '--list normalizes key names to lowercase' '
	cd repo &&
	cat >.git/normcfg <<-\EOF
	[Section]
		CamelKey = value
	EOF
	git config --file .git/normcfg --list >actual &&
	echo "section.camelkey=value" >expected &&
	test_cmp expected actual
'

# ── --type flag ────────────────────────────────────────────────────────────────

test_expect_success '--type bool normalizes true values' '
	cd repo &&
	git config core.flag yes &&
	git config --type bool core.flag >actual &&
	echo "true" >expected &&
	test_cmp expected actual
'

test_expect_success '--type int normalizes integer values' '
	cd repo &&
	git config pack.windowmemory 1k &&
	git config --type int pack.windowmemory >actual &&
	echo "1024" >expected &&
	test_cmp expected actual
'

test_expect_success '--type path expands tilde' '
	cd repo &&
	git config mykey.homepath "~/somedir" &&
	git config --type path mykey.homepath >actual &&
	echo "${HOME}/somedir" >expected &&
	test_cmp expected actual
'

test_expect_success '--bool with invalid value exits with error' '
	cd repo &&
	git config bad.bool "not-a-bool" &&
	! git config --bool bad.bool
'

test_expect_success '--int with invalid value exits with error' '
	cd repo &&
	git config bad.int "not-an-int" &&
	! git config --int bad.int
'

# ── --show-origin ──────────────────────────────────────────────────────────────

test_expect_success '--show-origin --list prefixes entries with file path' '
	cd repo &&
	git config --file .git/recfg --show-origin --list >actual &&
	grep "^file:" actual &&
	grep "alpha.foo=one" actual
'

test_expect_success '--show-origin --list shows correct file for alternate file' '
	cd repo &&
	git config --file .git/normcfg --show-origin --list >actual &&
	grep "^file:" actual &&
	grep "section.camelkey=value$" actual
'

# ── --show-scope ───────────────────────────────────────────────────────────────

test_expect_success '--show-scope --list with --file shows scope' '
	cd repo &&
	git config --file .git/normcfg --show-scope --list >actual &&
	grep "section.camelkey=value" actual
'

test_expect_success '--show-scope --list local config shows "local" scope' '
	cd repo &&
	git config --list --show-scope --local >actual &&
	grep "^local$(printf "\t")" actual
'

# ── -z (NUL delimiter) ────────────────────────────────────────────────────────

test_expect_success '-z --get terminates value with NUL' '
	cd repo &&
	git config -z --file .git/recfg alpha.foo >actual_raw &&
	printf "one\0" >expected_raw &&
	test_cmp expected_raw actual_raw
'

test_expect_success '-z --list separates entries with NUL' '
	cd repo &&
	git config -z --file .git/normcfg --list >actual_raw &&
	printf "section.camelkey=value\0" >expected_raw &&
	test_cmp expected_raw actual_raw
'

# ── error handling ─────────────────────────────────────────────────────────────

test_expect_success 'key without section returns error' '
	cd repo &&
	! git config .badkey
'

test_expect_success 'key with empty variable name returns error' '
	cd repo &&
	! git config section.
'

test_expect_success '--unset non-existent key returns non-zero exit' '
	cd repo &&
	! git config --file .git/recfg --unset alpha.nosuchkey
'

test_expect_success '--unset removes single-value key from file' '
	cd repo &&
	printf "[utest]\n\tkey = todelete\n" >.git/mvcfg2 &&
	git config --file .git/mvcfg2 --unset utest.key &&
	! git config --file .git/mvcfg2 utest.key
'

# ── --file flag ────────────────────────────────────────────────────────────────

test_expect_success '--file reads from alternate file' '
	cd repo &&
	cat >.git/altcfg <<-\EOF
	[custom]
		setting = altvalue
	EOF
	git config --file .git/altcfg custom.setting >actual &&
	echo "altvalue" >expected &&
	test_cmp expected actual
'

test_expect_success '--file writes to alternate file' '
	cd repo &&
	git config --file .git/altcfg custom.newkey "written" &&
	git config --file .git/altcfg custom.newkey >actual &&
	echo "written" >expected &&
	test_cmp expected actual
'

test_expect_success '--file with multi-section config lists all sections' '
	cd repo &&
	cat >.git/multicfg <<-\EOF
	[aaa]
		x = 1
	[bbb]
		y = 2
	EOF
	git config --file .git/multicfg --list >actual &&
	printf "aaa.x=1\nbbb.y=2\n" >expected &&
	test_cmp expected actual
'

# ── environment variable overrides ────────────────────────────────────────────

test_expect_success 'GIT_CONFIG_COUNT injects key-value pairs' '
	cd repo &&
	GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=env.inject GIT_CONFIG_VALUE_0=injected \
		git config --get env.inject >actual &&
	echo "injected" >expected &&
	test_cmp expected actual
'

test_expect_success 'GIT_CONFIG_COUNT injects multiple pairs' '
	cd repo &&
	GIT_CONFIG_COUNT=2 \
	GIT_CONFIG_KEY_0=env.one GIT_CONFIG_VALUE_0=first \
	GIT_CONFIG_KEY_1=env.two GIT_CONFIG_VALUE_1=second \
		git config --get-regexp env >actual &&
	printf "env.one first\nenv.two second\n" >expected &&
	test_cmp expected actual
'

test_done
