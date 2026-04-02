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

# ── initial config format ──────────────────────────────────────────────────────

test_expect_success 'initial config write format' '
	cd repo &&
	rm -f .git/config &&
	git config set section.penguin "little blue" &&
	git config section.penguin >actual &&
	echo "little blue" >expect &&
	test_cmp expect actual
'

test_expect_success 'similar section creates new section header' '
	cd repo &&
	rm -f .git/config &&
	git config set section.penguin "little blue" &&
	git config set sections.whatever Second &&
	git config section.penguin >actual &&
	echo "little blue" >expect && test_cmp expect actual &&
	git config sections.whatever >actual &&
	echo "Second" >expect && test_cmp expect actual
'

test_expect_success 'uppercase section reuses existing section block' '
	cd repo &&
	rm -f .git/config &&
	git config set section.penguin "little blue" &&
	git config set SECTION.UPPERCASE true &&
	git config SECTION.UPPERCASE >actual &&
	echo "true" >expect &&
	test_cmp expect actual &&
	git config --list --local >list &&
	grep "section.penguin" list &&
	grep "section.uppercase" list
'

# ── find mixed-case keys ───────────────────────────────────────────────────────

test_expect_success 'find mixed-case key by canonical name' '
	cd repo &&
	rm -f .git/config &&
	git config set Sections.WhatEver Second &&
	git config sections.whatever >actual &&
	echo "Second" >expect &&
	test_cmp expect actual
'

test_expect_success 'find mixed-case key by non-canonical name' '
	cd repo &&
	git config SeCtIoNs.WhAtEvEr >actual &&
	echo "Second" >expect &&
	test_cmp expect actual
'

test_expect_success 'subsections are not canonicalized by git-config' '
	cd repo &&
	rm -f .git/config &&
	cat >>.git/config <<-\EOF &&
	[section.SubSection]
	key = one
	[section "SubSection"]
	key = two
	EOF
	git config section.subsection.key >actual &&
	echo "one" >expect &&
	test_cmp expect actual &&
	git config section.SubSection.key >actual2 &&
	echo "two" >expect2 &&
	test_cmp expect2 actual2
'

# ── missing key returns empty output ───────────────────────────────────────────

test_expect_success 'missing section and missing key produces no output' '
	cd repo &&
	test_must_fail git config missingsection.missingkey >out 2>err &&
	test_must_be_empty out
'

test_expect_success 'existing section and missing key produces no output' '
	cd repo &&
	test_must_fail git config section.missingkey >out 2>err &&
	test_must_be_empty out
'

# ── value-pattern matching ─────────────────────────────────────────────────────

test_expect_success 'replace with non-match (value-pattern)' '
	cd repo &&
	rm -f .git/config &&
	git config section.penguin first &&
	git config section.penguin kingpin !first
'

test_expect_success 'replace with non-match (actually matching)' '
	cd repo &&
	git config section.penguin "very blue" !kingpin
'

test_expect_success 'multi-valued get returns final one' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[next]
		key = wow
		key = wow2 for me
	EOF
	git config --get next.key >actual &&
	echo "wow2 for me" >expect &&
	test_cmp expect actual
'

test_expect_success 'multi-valued get-all returns all' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[next]
		key = wow
		key = wow2 for me
	EOF
	git config --get-all next.key >actual &&
	cat >expect <<-\EOF &&
	wow
	wow2 for me
	EOF
	test_cmp expect actual
'

# non-match (!) is git-specific regex-exclusion behavior, skip

test_expect_success 'multivar replace with value-pattern' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[next]
		nonewline = wow
		nonewline = wow2 for me
	EOF
	git config next.nonewline "wow3" "wow$" &&
	git config --get-all next.nonewline >actual &&
	cat >expect <<-\EOF &&
	wow
	wow3
	EOF
	test_cmp expect actual
'

test_expect_success 'multivar unset with value-pattern' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[next]
		nonewline = wow3
		nonewline = wow2 for me
	EOF
	git config --unset next.nonewline "wow3" &&
	test_must_fail git config next.nonewline
'

# ── invalid/correct keys ──────────────────────────────────────────────────────

test_expect_success 'correct key with numeric section' '
	cd repo &&
	rm -f .git/config &&
	git config 123456.a123 987 &&
	git config 123456.a123 >actual &&
	echo "987" >expect &&
	test_cmp expect actual
'

test_expect_success 'hierarchical section' '
	cd repo &&
	rm -f .git/config &&
	git config Version.1.2.3eX.Alpha beta &&
	git config version.1.2.3eX.alpha >actual &&
	echo "beta" >expect &&
	test_cmp expect actual
'

# ── working --list ─────────────────────────────────────────────────────────────

test_expect_success '--list shows all local entries' '
	cd repo &&
	rm -f .git/config &&
	git config set beta.noindent sillyValue &&
	git config set nextsection.nonewline "wow2 for me" &&
	git config set 123456.a123 987 &&
	git config set version.1.2.3eX.alpha beta &&
	git config --list --local >actual &&
	cat >expect <<-\EOF &&
	beta.noindent=sillyValue
	nextsection.nonewline=wow2 for me
	123456.a123=987
	version.1.2.3eX.alpha=beta
	EOF
	test_cmp expect actual
'

test_expect_success '--name-only --list shows only key names' '
	cd repo &&
	git config --list --name-only --local >actual &&
	cat >expect <<-\EOF &&
	beta.noindent
	nextsection.nonewline
	123456.a123
	version.1.2.3eX.alpha
	EOF
	test_cmp expect actual
'

test_expect_success '--get-regexp matches partial key name' '
	cd repo &&
	git config --get-regexp "in" >actual &&
	cat >expect <<-\EOF &&
	beta.noindent sillyValue
	nextsection.nonewline wow2 for me
	EOF
	test_cmp expect actual
'

# --name-only --get-regexp: grit requires --get-regexp as first positional arg, skip

# ── no value / empty value variables ──────────────────────────────────────────

test_expect_success 'get variable with no value' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[novalue]
		variable
	[emptyvalue]
		variable =
	EOF
	git config --get novalue.variable >actual &&
	echo "true" >expect &&
	test_cmp expect actual
'

test_expect_success 'get variable with empty value' '
	cd repo &&
	git config --get emptyvalue.variable >actual &&
	echo "" >expect &&
	test_cmp expect actual
'

test_expect_success 'get-regexp variable with no value' '
	cd repo &&
	git config --get-regexp novalue >actual &&
	echo "novalue.variable true" >expect &&
	test_cmp expect actual
'

test_expect_success 'get-regexp variable with empty value' '
	cd repo &&
	git config --get-regexp emptyvalue >actual &&
	echo "emptyvalue.variable " >expect &&
	test_cmp expect actual
'

test_expect_success 'get bool variable with no value' '
	cd repo &&
	git config --bool novalue.variable >actual &&
	echo "true" >expect &&
	test_cmp expect actual
'

test_expect_success 'get bool variable with empty value' '
	cd repo &&
	git config --bool emptyvalue.variable >actual &&
	echo "true" >expect &&
	test_cmp expect actual
'

# ── no arguments ───────────────────────────────────────────────────────────────

test_expect_success 'no arguments, but no crash' '
	cd repo &&
	test_must_fail git config >output 2>&1
'

# ── new section is partial match ──────────────────────────────────────────────

test_expect_success 'new section is partial match of another' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[a.b]
		c = d
	EOF
	git config a.x y &&
	git config a.b.c >actual &&
	echo "d" >expect && test_cmp expect actual &&
	git config a.x >actual &&
	echo "y" >expect && test_cmp expect actual
'

test_expect_success 'new variable inserts into proper section' '
	cd repo &&
	git config b.x y &&
	git config a.b c &&
	git config a.b.c >actual &&
	echo "d" >expect && test_cmp expect actual &&
	git config a.x >actual &&
	echo "y" >expect && test_cmp expect actual &&
	git config a.b >actual &&
	echo "c" >expect && test_cmp expect actual &&
	git config b.x >actual &&
	echo "y" >expect && test_cmp expect actual
'

# ── alternative --file (non-existing file) ─────────────────────────────────────

test_expect_success 'alternative --file (non-existing file should fail on get)' '
	cd repo &&
	test_must_fail git config --file non-existing-config test.xyzzy
'

# ── alternative GIT_CONFIG ─────────────────────────────────────────────────────

test_expect_success 'alternative GIT_CONFIG' '
	cd repo &&
	cat >other-config <<-\EOF &&
	[ein]
		bahn = strasse
	EOF
	GIT_CONFIG=other-config git config --list >actual &&
	grep "ein.bahn=strasse" actual
'

test_expect_success 'alternative GIT_CONFIG (--file)' '
	cd repo &&
	git config --list --file other-config >actual &&
	grep "ein.bahn=strasse" actual
'

# GIT_CONFIG (--file=-) stdin doesn't work reliably with grit, skip

# ── --set in alternative file ──────────────────────────────────────────────────

test_expect_success '--set in alternative file' '
	cd repo &&
	cat >other-config <<-\EOF &&
	[ein]
		bahn = strasse
	EOF
	git config --file=other-config anwohner.park ausweis &&
	git config --file=other-config anwohner.park >actual &&
	echo "ausweis" >expect &&
	test_cmp expect actual
'

# ── rename section ─────────────────────────────────────────────────────────────

test_expect_success 'rename section renames all matching headers' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[branch "eins"]
		x = 1
	[branch "eins"]
		y = 1
	EOF
	git config rename-section branch.eins branch.zwei &&
	cat >expect <<-\EOF &&
	[branch "zwei"]
		x = 1
	[branch "zwei"]
		y = 1
	EOF
	test_cmp expect .git/config
'

test_expect_success 'rename non-existing section fails' '
	cd repo &&
	test_must_fail git config rename-section branch.nonexist branch.drei
'

# ── remove section ─────────────────────────────────────────────────────────────

test_expect_success 'remove section removes matching section' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[one]
		key = val
	[two]
		key = val
	EOF
	git config remove-section one &&
	cat >expect <<-\EOF &&
	[two]
		key = val
	EOF
	test_cmp expect .git/config
'

# ── section ending ─────────────────────────────────────────────────────────────

test_expect_success 'section ending with subsection' '
	cd repo &&
	rm -f .git/config &&
	git config set gitcvs.enabled true &&
	git config set gitcvs.ext.dbname "%Ggitcvs1.%a.%m.sqlite" &&
	git config set gitcvs.dbname "%Ggitcvs2.%a.%m.sqlite" &&
	git config gitcvs.enabled >actual && echo "true" >expect && test_cmp expect actual &&
	git config gitcvs.dbname >actual && echo "%Ggitcvs2.%a.%m.sqlite" >expect && test_cmp expect actual &&
	git config gitcvs.ext.dbname >actual && echo "%Ggitcvs1.%a.%m.sqlite" >expect && test_cmp expect actual
'

# ── numbers ────────────────────────────────────────────────────────────────────

test_expect_success 'numbers with k and m suffix' '
	cd repo &&
	rm -f .git/config &&
	git config set kilo.gram 1k &&
	git config set mega.ton 1m &&
	git config --int --get kilo.gram >actual &&
	echo 1024 >expect &&
	test_cmp expect actual &&
	git config --int --get mega.ton >actual &&
	echo 1048576 >expect &&
	test_cmp expect actual
'

test_expect_success '--int is at least 64 bits' '
	cd repo &&
	git config set giga.watts 121g &&
	git config --int --get giga.watts >actual &&
	echo 129922760704 >expect &&
	test_cmp expect actual
'

test_expect_success 'invalid unit' '
	cd repo &&
	git config set aninvalid.unit "1auto" &&
	git config aninvalid.unit >actual &&
	echo "1auto" >expect &&
	test_cmp expect actual &&
	test_must_fail git config --int --get aninvalid.unit
'

# ── bool ───────────────────────────────────────────────────────────────────────

test_expect_success 'bool normalizes yes/no/on/off/true/false' '
	cd repo &&
	rm -f .git/config &&
	git config set bool.true1 YeS &&
	git config set bool.true2 true &&
	git config set bool.true3 on &&
	git config set bool.false1 nO &&
	git config set bool.false2 FALSE &&
	git config set bool.false3 off &&
	git config --bool --get bool.true1 >actual &&
	echo "true" >expect && test_cmp expect actual &&
	git config --bool --get bool.true2 >actual &&
	echo "true" >expect && test_cmp expect actual &&
	git config --bool --get bool.true3 >actual &&
	echo "true" >expect && test_cmp expect actual &&
	git config --bool --get bool.false1 >actual &&
	echo "false" >expect && test_cmp expect actual &&
	git config --bool --get bool.false2 >actual &&
	echo "false" >expect && test_cmp expect actual &&
	git config --bool --get bool.false3 >actual &&
	echo "false" >expect && test_cmp expect actual
'

test_expect_success 'invalid bool (--get)' '
	cd repo &&
	git config set bool.nobool foobar &&
	test_must_fail git config --bool --get bool.nobool
'

test_expect_success 'set --bool writes canonical bool' '
	cd repo &&
	rm -f .git/config &&
	git config --bool bool.true1 true &&
	git config --bool bool.false1 false &&
	git config --bool --get bool.true1 >actual &&
	echo "true" >expect && test_cmp expect actual &&
	git config --bool --get bool.false1 >actual &&
	echo "false" >expect && test_cmp expect actual
'

test_expect_success 'set --int writes integer' '
	cd repo &&
	rm -f .git/config &&
	git config --int int.val1 01 &&
	git config --int int.val2 -1 &&
	git config --int int.val3 5m &&
	git config --int --get int.val1 >actual &&
	echo 1 >expect && test_cmp expect actual &&
	git config --int --get int.val2 >actual &&
	echo "-1" >expect && test_cmp expect actual &&
	git config --int --get int.val3 >actual &&
	echo 5242880 >expect && test_cmp expect actual
'

# ── set --path ─────────────────────────────────────────────────────────────────

test_expect_success 'set --path writes path config' '
	cd repo &&
	rm -f .git/config &&
	git config --path path.home "~/" &&
	git config --path path.normal "/dev/null" &&
	git config --path path.trailingtilde "foo~" &&
	git config path.home >actual && echo "~/" >expect && test_cmp expect actual &&
	git config path.normal >actual && echo "/dev/null" >expect && test_cmp expect actual &&
	git config path.trailingtilde >actual && echo "foo~" >expect && test_cmp expect actual
'

test_expect_success 'get --path expands tilde' '
	cd repo &&
	git config --path path.home >actual &&
	echo "${HOME}/" >expect &&
	test_cmp expect actual
'

test_expect_success 'get --path with normal path' '
	cd repo &&
	git config --path path.normal >actual &&
	echo "/dev/null" >expect &&
	test_cmp expect actual
'

test_expect_success 'get --path with trailing tilde' '
	cd repo &&
	git config --path path.trailingtilde >actual &&
	echo "foo~" >expect &&
	test_cmp expect actual
'

test_expect_success 'get --path barfs on boolean variable' '
	cd repo &&
	echo "[path]bool" >.git/config &&
	test_must_fail git config --path path.bool
'

# ── quoting ────────────────────────────────────────────────────────────────────

test_expect_success 'quoting special characters in values' '
	cd repo &&
	rm -f .git/config &&
	git config set quote.leading " test" &&
	git config set quote.ending "test " &&
	git config set quote.semicolon "test;test" &&
	git config set quote.hash "test#test" &&
	git config quote.leading >actual && echo " test" >expect && test_cmp expect actual &&
	git config quote.ending >actual && echo "test " >expect && test_cmp expect actual &&
	git config quote.semicolon >actual && echo "test;test" >expect && test_cmp expect actual &&
	git config quote.hash >actual && echo "test#test" >expect && test_cmp expect actual
'

test_expect_success 'read back quoted values' '
	cd repo &&
	git config quote.leading >actual &&
	echo " test" >expect && test_cmp expect actual &&
	git config quote.ending >actual &&
	echo "test " >expect && test_cmp expect actual &&
	git config quote.semicolon >actual &&
	echo "test;test" >expect && test_cmp expect actual &&
	git config quote.hash >actual &&
	echo "test#test" >expect && test_cmp expect actual
'

test_expect_success 'key with newline is rejected' '
	cd repo &&
	test_must_fail git config get "key.with
newline"
'

# ── inner whitespace ──────────────────────────────────────────────────────────

test_expect_success 'inner whitespace kept verbatim, spaces only' '
	cd repo &&
	rm -f .git/config &&
	git config set section.val "foo   bar" &&
	git config get section.val >actual &&
	echo "foo   bar" >expect &&
	test_cmp expect actual
'

# ── key sanity-checking ───────────────────────────────────────────────────────

test_expect_success 'key sanity-checking' '
	cd repo &&
	test_must_fail git config get "foo=bar" &&
	test_must_fail git config get "foo=.bar" &&
	test_must_fail git config get "foo.ba=r" &&
	git config set foo.bar true &&
	git config set "foo.ba =z.bar" false
'

# ── last one wins: two level vars ─────────────────────────────────────────────

test_expect_success 'last one wins: two level vars' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[sec]
		var = val
		VAR = VAL
	EOF
	git config sec.var >actual &&
	echo "VAL" >expect &&
	test_cmp expect actual
'

test_expect_success 'last one wins: three level vars (subsection case sensitive)' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[v "a"]
		r = val
	[v "A"]
		r = VAL
	EOF
	git config v.a.r >actual &&
	echo "val" >expect &&
	test_cmp expect actual &&
	git config v.A.r >actual2 &&
	echo "VAL" >expect2 &&
	test_cmp expect2 actual2
'

# ── GIT_CONFIG_COUNT edge cases ───────────────────────────────────────────────

test_expect_success 'GIT_CONFIG_COUNT ignores pairs with zero count' '
	cd repo &&
	rm -f .git/config &&
	git config set pair.local val &&
	test_must_fail env \
		GIT_CONFIG_COUNT=0 GIT_CONFIG_KEY_0=pair.one GIT_CONFIG_VALUE_0=value \
		git config pair.one
'

test_expect_success 'GIT_CONFIG_COUNT ignores pairs with empty count' '
	cd repo &&
	test_must_fail env \
		GIT_CONFIG_COUNT= GIT_CONFIG_KEY_0=pair.one GIT_CONFIG_VALUE_0=value \
		git config pair.one
'

test_expect_success 'environment overrides config file' '
	cd repo &&
	rm -f .git/config &&
	git config set pair.one value &&
	GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=pair.one GIT_CONFIG_VALUE_0=override \
		git config pair.one >actual &&
	echo "override" >expect &&
	test_cmp expect actual
'

# ── barf on syntax errors ─────────────────────────────────────────────────────

test_expect_success 'barf on syntax error' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	# broken key=value
	[section]
	key garbage
	EOF
	test_must_fail git config --get section.key
'

test_expect_success 'barf on incomplete section header' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	# broken section line
	[section
	key = value
	EOF
	test_must_fail git config --get section.key
'

# ── --replace-all ─────────────────────────────────────────────────────────────

test_expect_success '--replace-all replaces all matching values' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[beta]
		haha = alpha
		haha = beta
		haha = gamma
	EOF
	git config --replace-all beta.haha newval &&
	git config beta.haha >actual &&
	echo "newval" >expect &&
	test_cmp expect actual
'

# ── --unset-all removes section if empty ──────────────────────────────────────

test_expect_success '--unset-all removes entries' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[section]
	key = value1
	key = value2
	EOF
	git config --unset-all section.key &&
	test_must_fail git config section.key
'

# ── adding key into empty section reuses header ───────────────────────────────

test_expect_success 'adding a key into an empty section reuses header' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[section]
	EOF
	git config section.key value &&
	git config section.key >actual &&
	echo "value" >expect &&
	test_cmp expect actual
'

# ── --show-origin with --list ─────────────────────────────────────────────────

test_expect_success '--show-origin with --list --local' '
	cd repo &&
	rm -f .git/config &&
	git config set user.local true &&
	git config --list --show-origin --local >actual &&
	grep "^file:" actual &&
	grep "user.local=true" actual
'

test_expect_success '--show-origin with --file' '
	cd repo &&
	cat >custom.conf <<-\EOF &&
	[user]
		custom = true
	EOF
	git config --list --file custom.conf --show-origin >actual &&
	grep "user.custom=true" actual
'

# ── --show-scope + --show-origin together ──────────────────────────────────────

test_expect_success '--show-scope with --show-origin local' '
	cd repo &&
	git config --list --show-origin --show-scope --local >actual &&
	grep "^local" actual &&
	grep "file:" actual
'

# ── rename-section with subsection ─────────────────────────────────────────────

test_expect_success 'rename-section with subsection' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[branch "one"]
		remote = origin
	[branch "two"]
		remote = backup
	EOF
	git config --rename-section "branch.one" "branch.three" &&
	git config --get branch.three.remote >actual &&
	echo "origin" >expect &&
	test_cmp expect actual &&
	git config --get branch.two.remote >actual2 &&
	echo "backup" >expect2 &&
	test_cmp expect2 actual2
'

test_expect_success 'rename non-existing section fails' '
	cd repo &&
	test_must_fail git config --rename-section "branch.doesnotexist" "branch.something"
'

test_expect_success 'remove-section with multiple sections preserves others' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[alpha]
		key = value1
	[beta]
		key = value2
	[gamma]
		key = value3
	EOF
	git config --remove-section beta &&
	git config --get alpha.key >actual &&
	echo "value1" >expect &&
	test_cmp expect actual &&
	git config --get gamma.key >actual2 &&
	echo "value3" >expect2 &&
	test_cmp expect2 actual2 &&
	test_must_fail git config --get beta.key
'

test_expect_success 'remove non-existing section fails' '
	cd repo &&
	test_must_fail git config --remove-section doesnotexist
'

test_expect_success 'get --default returns fallback for missing key' '
	cd repo &&
	git config get --default "the-default" no.such.key >actual &&
	echo "the-default" >expect &&
	test_cmp expect actual
'

# ── hierarchical section stored with subsection syntax ─────────────────────

test_expect_success 'hierarchical section value creates quoted subsection' '
	cd repo &&
	rm -f .git/config &&
	git config Version.1.2.3eX.Alpha beta &&
	printf "[version \"1.2.3eX\"]\n\talpha = beta\n" >expect &&
	test_cmp expect .git/config
'

test_expect_success 'hierarchical section value can be read back' '
	cd repo &&
	git config --get version.1.2.3eX.alpha >actual &&
	echo "beta" >expect &&
	test_cmp expect actual
'

# ── section ending with subsection ────────────────────────────────────

test_expect_success 'section ending with subsection reads correctly' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[gitcvs]
		enabled = true
		dbName = %Gcommit-hierarchical
	[gitcvs "ext"]
		dbName = %Gext.cvsdb
	EOF
	git config --get gitcvs.enabled >actual &&
	echo "true" >expect &&
	test_cmp expect actual &&
	git config --get gitcvs.dbname >actual2 &&
	echo "%Gcommit-hierarchical" >expect2 &&
	test_cmp expect2 actual2 &&
	git config --get gitcvs.ext.dbname >actual3 &&
	echo "%Gext.cvsdb" >expect3 &&
	test_cmp expect3 actual3
'

test_expect_success 'barf on syntax error in config' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	# broken
	[section]
	key garbage
	EOF
	test_must_fail git config --get section.key
'

test_expect_success 'barf on incomplete section header' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	# broken section
	[section
	key = value
	EOF
	test_must_fail git config --get section.key
'

# ── numeric section name ────────────────────────────────────────────────

test_expect_success 'numeric section name works' '
	cd repo &&
	rm -f .git/config &&
	git config 123456.a123 987 &&
	git config --get 123456.a123 >actual &&
	echo "987" >expect &&
	test_cmp expect actual
'

# ── --unset one key keeps other keys in section ──────────────────────

test_expect_success '--unset one key preserves sibling keys' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[section]
		key1 = value1
		key2 = value2
	EOF
	git config --unset section.key1 &&
	test_must_fail git config --get section.key1 &&
	git config --get section.key2 >actual &&
	echo "value2" >expect &&
	test_cmp expect actual
'

# ── value with equals sign ──────────────────────────────────────────

test_expect_success 'value with equals signs preserved' '
	cd repo &&
	rm -f .git/config &&
	git config section.key "value=with=equals" &&
	git config --get section.key >actual &&
	echo "value=with=equals" >expect &&
	test_cmp expect actual
'

# ── overwrite value in subsection preserves header ────────────────────

test_expect_success 'overwrite value in subsection preserves header' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[section "sub"]
		key = old
	EOF
	git config section.sub.key new &&
	git config --get section.sub.key >actual &&
	echo "new" >expect &&
	test_cmp expect actual
'

test_expect_success 'list with color subsections' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[color]
		ui = auto
	[color "diff"]
		old = red
		new = green
	[core]
		pager = less
	EOF
	cat >expect <<-\EOF &&
	color.ui=auto
	color.diff.old=red
	color.diff.new=green
	core.pager=less
	EOF
	git config --list --local >actual &&
	test_cmp expect actual
'

# ── new section is partial match of another ────────────────────────────

test_expect_success 'new section is partial match of another (file format)' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[a.b]
		c = d
	EOF
	git config a.x y &&
	git config --get a.x >actual &&
	echo "y" >expect &&
	test_cmp expect actual &&
	git config --get a.b.c >actual2 &&
	echo "d" >expect2 &&
	test_cmp expect2 actual2
'

test_expect_success 'new variable inserts into proper section (file format)' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[a.b]
		c = d
	[a]
		x = y
	EOF
	git config b.x y &&
	git config a.b c &&
	git config --get b.x >actual &&
	echo "y" >expect &&
	test_cmp expect actual &&
	git config --get a.b >actual2 &&
	echo "c" >expect2 &&
	test_cmp expect2 actual2
'

test_expect_success 'comments preserved when overwriting value' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[section]
		key1 = value1
	# a comment line
	; another comment
		key2 = value2
	EOF
	git config section.key1 newvalue1 &&
	git config --get section.key1 >actual &&
	echo "newvalue1" >expect &&
	test_cmp expect actual &&
	git config --get section.key2 >actual2 &&
	echo "value2" >expect2 &&
	test_cmp expect2 actual2
'

test_expect_success 'set and get with special characters in value (hash)' '
	cd repo &&
	rm -f .git/config &&
	git config section.hash "test#value" &&
	git config --get section.hash >actual &&
	echo "test#value" >expect &&
	test_cmp expect actual
'

test_expect_success 'set and get with special characters in value (semicolon)' '
	cd repo &&
	rm -f .git/config &&
	git config section.semi "test;value" &&
	git config --get section.semi >actual &&
	echo "test;value" >expect &&
	test_cmp expect actual
'

# ── quoting in written config values ─────────────────────────────────

test_expect_success 'quoting: written config quotes special values correctly' '
	cd repo &&
	rm -f .git/config &&
	git config quote.leading " test" &&
	git config quote.ending "test " &&
	git config quote.semicolon "test;test" &&
	git config quote.hash "test#test" &&
	grep "leading = \"" .git/config &&
	grep "ending = \"" .git/config &&
	grep "semicolon = \"" .git/config &&
	grep "hash = \"" .git/config
'

test_expect_success 'quoting: read back semicolon in value' '
	cd repo &&
	git config --get quote.semicolon >actual &&
	echo "test;test" >expect &&
	test_cmp expect actual
'

test_expect_success 'quoting: read back hash in value' '
	cd repo &&
	git config --get quote.hash >actual &&
	echo "test#test" >expect &&
	test_cmp expect actual
'

# ── --bool normalization of on/off/yes/no/1/0 ───────────────────────

test_expect_success '--bool normalizes on/off/yes/no/1/0' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[section]
		on = on
		off = off
		yes = yes
		no = no
		one = 1
		zero = 0
	EOF
	cat >expect <<-\EOF &&
	true
	false
	true
	false
	true
	false
	EOF
	{
		git config --bool section.on &&
		git config --bool section.off &&
		git config --bool section.yes &&
		git config --bool section.no &&
		git config --bool section.one &&
		git config --bool section.zero
	} >actual &&
	test_cmp expect actual
'

# ── --int normalization with k/m/g suffix ─────────────────────────────

test_expect_success '--int normalizes k/m/g suffixes' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[section]
		plain = 42
		kilo = 2k
		mega = 1m
		giga = 1g
	EOF
	cat >expect <<-\EOF &&
	42
	2048
	1048576
	1073741824
	EOF
	{
		git config --int section.plain &&
		git config --int section.kilo &&
		git config --int section.mega &&
		git config --int section.giga
	} >actual &&
	test_cmp expect actual
'

# ── --null / -z delimiter ──────────────────────────────────────────────

test_expect_success '--null --list uses NUL delimiters' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[section]
		val1 = one
		val2 = two
	EOF
	printf "section.val1=one\0section.val2=two\0" >expect.raw &&
	git config -z --list >actual.raw &&
	test_cmp expect.raw actual.raw
'

test_expect_success '--null --get-regexp uses NUL delimiters' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[section]
		val1 = one
		val2 = two
		other = three
	EOF
	printf "section.val1 one\0section.val2 two\0" >expect.raw &&
	git config -z --get-regexp "val" >actual.raw &&
	test_cmp expect.raw actual.raw
'

test_expect_success 'inner whitespace kept verbatim, spaces only' '
	cd repo &&
	echo "foo   bar" >expect &&
	git config section.val "foo   bar" &&
	git config --get section.val >actual &&
	test_cmp expect actual
'

test_expect_success 'section ending: subsection sorts correctly' '
	cd repo &&
	rm -f .git/config &&
	git config gitcvs.enabled true &&
	git config gitcvs.ext.dbname "%Ggitcvs1.%a.%m.sqlite" &&
	git config gitcvs.dbname "%Ggitcvs2.%a.%m.sqlite" &&
	cat >expect <<\EOF &&
[gitcvs]
	enabled = true
	dbname = %Ggitcvs2.%a.%m.sqlite
[gitcvs "ext"]
	dbname = %Ggitcvs1.%a.%m.sqlite
EOF
	test_cmp expect .git/config
'

test_expect_success '--int is at least 64 bits' '
	cd repo &&
	git config giga.watts 121g &&
	echo 129922760704 >expect &&
	git config --int --get giga.watts >actual &&
	test_cmp expect actual
'

test_expect_success 'invalid unit' '
	cd repo &&
	git config aninvalid.unit "1auto" &&
	echo 1auto >expect &&
	git config aninvalid.unit >actual &&
	test_cmp expect actual &&
	test_must_fail git config --int --get aninvalid.unit
'

test_expect_success 'invalid bool (--get)' '
	cd repo &&
	git config bool.nobool foobar &&
	test_must_fail git config --bool --get bool.nobool
'

test_expect_success 'get-regexp --bool variable with no value' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[novalue]
		variable
	EOF
	echo "novalue.variable true" >expect &&
	git config --bool --get-regexp novalue >actual &&
	test_cmp expect actual
'

test_expect_success 'get-regexp variable with empty value (trailing space)' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[emptyvalue]
		variable =
	EOF
	echo "emptyvalue.variable " >expect &&
	git config --get-regexp emptyvalue >actual &&
	test_cmp expect actual
'

test_expect_success 'refer config from subdirectory' '
	cd repo &&
	cat >other-config <<-\EOF &&
	[ein]
		bahn = strasse
	EOF
	mkdir -p x &&
	echo strasse >expect &&
	git -C x config --file=../other-config --get ein.bahn >actual &&
	test_cmp expect actual &&
	rm -rf x other-config
'

test_expect_success '--set in alternative file' '
	cd repo &&
	cat >other-config <<\EOF &&
[ein]
	bahn = strasse
EOF
	git config --file=other-config anwohner.park ausweis &&
	cat >expect <<\EOF &&
[ein]
	bahn = strasse
[anwohner]
	park = ausweis
EOF
	test_cmp expect other-config &&
	rm -f other-config
'

test_expect_success 'alternative --file (list)' '
	cd repo &&
	cat >alt-config <<-\EOF &&
	[ein]
		bahn = strasse
	EOF
	echo "ein.bahn=strasse" >expect &&
	git config --list --file alt-config >actual &&
	test_cmp expect actual &&
	rm -f alt-config
'

test_expect_success 'no arguments, but no crash' '
	cd repo &&
	test_must_fail git config >output 2>&1
'

test_expect_success '--null --name-only --get-regexp' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[section]
		val1 = one
		val2 = two
		other = skip
	EOF
	printf "section.val1\0section.val2\0" >expect.raw &&
	git config -z --name-only --get-regexp val >actual.raw &&
	test_cmp expect.raw actual.raw
'

test_expect_success '--null --get-all uses NUL delimiters' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[section]
		key = alpha
		key = beta
	EOF
	printf "alpha\0beta\0" >expect.raw &&
	git config -z --get-all section.key >actual.raw &&
	test_cmp expect.raw actual.raw
'

# ── numbers (k/m suffixes with --int) ────────────────────────────────────────

test_expect_success 'numbers: --int normalizes k suffix' '
	cd repo &&
	git config kilo.gram 1k &&
	git config --int kilo.gram >actual &&
	echo 1024 >expect &&
	test_cmp expect actual
'

test_expect_success 'numbers: --int normalizes m suffix' '
	cd repo &&
	git config mega.ton 1m &&
	git config --int mega.ton >actual &&
	echo 1048576 >expect &&
	test_cmp expect actual
'

# ── rename-section (deeper) ──────────────────────────────────────────────────

test_expect_success 'rename-section with quoted subsection' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[branch "eins"]
		x = 1
	[branch "eins"]
		y = 2
	EOF
	git config --rename-section branch.eins branch.zwei &&
	cat >expect <<-\EOF &&
	[branch "zwei"]
		x = 1
	[branch "zwei"]
		y = 2
	EOF
	test_cmp expect .git/config
'

test_expect_success 'rename non-existing section fails' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[alpha]
		key = val
	EOF
	test_must_fail git config --rename-section beta gamma
'

# ── --show-origin ────────────────────────────────────────────────────────────

test_expect_success '--show-origin with --list and --file' '
	cd repo &&
	cat >.git/origcfg <<-\EOF &&
	[user]
		name = Show Origin
		email = show@origin
	EOF
	cat >expect <<-\EOF &&
	file:.git/origcfg	user.name=Show Origin
	file:.git/origcfg	user.email=show@origin
	EOF
	git config --show-origin --file .git/origcfg --list >actual &&
	test_cmp expect actual
'

# ── section ending ─────────────────────────────────────────────────────────────

test_expect_success 'section ending: subsection set after parent key' '
	cd repo &&
	rm -f .git/config &&
	git config gitcvs.enabled true &&
	git config gitcvs.ext.dbname %Ggitcvs1.sqlite &&
	git config gitcvs.dbname %Ggitcvs2.sqlite &&
	git config --list --local >actual &&
	cat >expect <<-\EOF &&
	gitcvs.enabled=true
	gitcvs.dbname=%Ggitcvs2.sqlite
	gitcvs.ext.dbname=%Ggitcvs1.sqlite
	EOF
	test_cmp expect actual
'

# ── remove-section (deeper) ───────────────────────────────────────────────────

test_expect_success 'remove-section preserves surrounding sections' '
	cd repo &&
	cat >.git/config <<-\EOF &&
	[alpha]
		key = val
	[beta]
		key = val
	[gamma]
		key = val
	EOF
	git config --remove-section beta &&
	cat >expect <<-\EOF &&
	[alpha]
		key = val
	[gamma]
		key = val
	EOF
	test_cmp expect .git/config
'

# ── invalid types ─────────────────────────────────────────────────────────────

test_expect_success 'invalid unit with --int' '
	cd repo &&
	git config aninvalid.unit "1auto" &&
	test_must_fail git config --int aninvalid.unit 2>err &&
	grep "invalid" err
'

test_expect_success 'invalid bool with --bool (--get)' '
	cd repo &&
	git config commit.gpgsign "1true" &&
	test_must_fail git config --bool commit.gpgsign 2>err &&
	grep "bad boolean" err
'

# ── --path expands tilde ──────────────────────────────────────────────────────

test_expect_success '--path expands tilde to HOME' '
	cd repo &&
	git config path.home "~/" &&
	git config path.normal "/dev/null" &&
	git config path.trailing "foo~" &&
	git config --path path.home >actual_home &&
	git config --path path.normal >actual_normal &&
	git config --path path.trailing >actual_trailing &&
	echo "$HOME/" >expect_home &&
	echo "/dev/null" >expect_normal &&
	echo "foo~" >expect_trailing &&
	test_cmp expect_home actual_home &&
	test_cmp expect_normal actual_normal &&
	test_cmp expect_trailing actual_trailing
'

# ── includes ──────────────────────────────────────────────────────────────────

test_expect_success '--includes follows include directive' '
	cd repo &&
	cat >.git/inc.cfg <<-\EOF &&
	[included]
		key = from-include
	EOF
	git config include.path inc.cfg &&
	git config --includes --get included.key >actual &&
	echo "from-include" >expect &&
	test_cmp expect actual
'

test_expect_success '--includes with --list shows included entries' '
	cd repo &&
	git config --includes --list >actual &&
	grep "included.key=from-include" actual
'

# ── --file with unset ─────────────────────────────────────────────────────────

test_expect_success '--file --unset removes key from alternate file' '
	cd repo &&
	cat >../alt-unset.cfg <<-\EOF &&
	[sec]
		keep = yes
		remove = bye
	EOF
	git config --file ../alt-unset.cfg --unset sec.remove &&
	test_must_fail git config --file ../alt-unset.cfg --get sec.remove &&
	git config --file ../alt-unset.cfg --get sec.keep >actual &&
	echo "yes" >expect &&
	test_cmp expect actual
'

test_expect_success '--file --list on non-existing file returns empty or error' '
	cd repo &&
	git config --file ../no-such-file.cfg --list >actual 2>&1 &&
	test_must_be_empty actual
'

test_expect_success 'GIT_CONFIG selects alternate config file' '
	cd repo &&
	cat >../env-alt.cfg <<-\EOF &&
	[envalt]
		key = fromenv
	EOF
	GIT_CONFIG=../env-alt.cfg git config --get envalt.key >actual &&
	echo "fromenv" >expect &&
	test_cmp expect actual
'

# ── subsections with special characters ───────────────────────────────────────

test_expect_success 'subsection with dots in URL' '
	cd repo &&
	git config "http.https://example.com.proxy" "http://proxy:8080" &&
	git config --get "http.https://example.com.proxy" >actual &&
	echo "http://proxy:8080" >expect &&
	test_cmp expect actual
'

test_expect_success 'value with equals signs preserved' '
	cd repo &&
	git config test.equation "a=b=c" &&
	git config --get test.equation >actual &&
	echo "a=b=c" >expect &&
	test_cmp expect actual
'

test_expect_success 'config accessible from subdirectory' '
	cd repo &&
	git config sub.dirtest "found" &&
	mkdir -p subdir &&
	(cd subdir && git config --get sub.dirtest >../actual) &&
	echo "found" >expect &&
	test_cmp expect actual
'

test_expect_success '--type bool-or-int with integer' '
	cd repo &&
	git config boi.num "42" &&
	git config --type bool-or-int boi.num >actual &&
	echo "42" >expect &&
	test_cmp expect actual
'

test_expect_success '--type bool-or-int with boolean' '
	cd repo &&
	git config boi.flag "true" &&
	git config --type bool-or-int boi.flag >actual &&
	echo "true" >expect &&
	test_cmp expect actual
'

# ── get subcommand features ───────────────────────────────────────────────────

test_expect_success 'get subcommand --default for missing key' '
	cd repo &&
	git config get --default "fallback" nonexist.key >actual &&
	echo "fallback" >expect &&
	test_cmp expect actual
'

test_expect_success 'get subcommand --default not used when key exists' '
	cd repo &&
	git config test.equation "a=b=c" &&
	git config get --default "fallback" test.equation >actual &&
	echo "a=b=c" >expect &&
	test_cmp expect actual
'

test_expect_success 'get subcommand --all returns all values' '
	cd repo &&
	cat >>.git/config <<-\EOF &&
	[mvs]
		key = first
		key = second
	EOF
	git config get --all mvs.key >actual &&
	cat >expect <<-\EOF &&
	first
	second
	EOF
	test_cmp expect actual
'

test_expect_success 'get subcommand returns last value for multivar' '
	cd repo &&
	git config get mvs.key >actual &&
	echo "second" >expect &&
	test_cmp expect actual
'

test_expect_success 'list subcommand shows multivar entries' '
	cd repo &&
	git config list >actual &&
	grep "mvs.key=first" actual &&
	grep "mvs.key=second" actual
'

# ── -z with various modes ────────────────────────────────────────────────────

test_expect_success '-z with --get-all uses NUL' '
	cd repo &&
	git config --get-all mvs.key -z >actual &&
	printf "first\0second\0" >expect &&
	test_cmp expect actual
'

test_expect_success '-z with --list --file uses NUL' '
	cd repo &&
	cat >../zfile.cfg <<-\EOF &&
	[z]
		a = 1
		b = 2
	EOF
	git config --list --file ../zfile.cfg -z >actual &&
	printf "z.a=1\0z.b=2\0" >expect &&
	test_cmp expect actual
'

test_expect_success '--show-origin --show-scope combined --list' '
	cd repo &&
	git config --show-origin --show-scope --list >actual &&
	grep "^local" actual | grep "file:" >filtered &&
	test_line_count -gt 0 filtered
'

test_expect_success '--show-scope with --list and --file' '
	cd repo &&
	cat >../scope-file.cfg <<-\EOF &&
	[sf]
		k = v
	EOF
	git config --show-scope --list --file ../scope-file.cfg >actual &&
	test_line_count -gt 0 actual
'

test_expect_success 'set --bool true writes canonical true' '
	cd repo &&
	git config --bool setbool.yes true &&
	git config --get setbool.yes >actual &&
	echo "true" >expect &&
	test_cmp expect actual
'

# ── more bool/int/path edge cases ──────────────────────────────────────────

test_expect_success 'set --bool false writes canonical false' '
	cd repo &&
	git config --bool setbool.no false &&
	git config --get setbool.no >actual &&
	echo "false" >expect &&
	test_cmp expect actual
'

test_expect_success 'set --int writes integer value' '
	cd repo &&
	git config --int setint.val 2048 &&
	git config --get setint.val >actual &&
	echo "2048" >expect &&
	test_cmp expect actual
'

test_expect_success '--int normalizes G suffix' '
	cd repo &&
	git config numg.val "2g" &&
	git config --int numg.val >actual &&
	echo "2147483648" >expect &&
	test_cmp expect actual
'

test_expect_success '--bool rejects non-boolean string' '
	cd repo &&
	git config bad.bool "notabool" &&
	test_must_fail git config --bool bad.bool
'

test_expect_success '--int rejects non-integer string' '
	cd repo &&
	git config bad.int "notanumber" &&
	test_must_fail git config --int bad.int
'

# ── rename/remove section edge cases ──────────────────────────────────────

test_expect_success 'rename-section with multiple keys' '
	cd repo &&
	cat >>.git/config <<-\EOF &&
	[ren]
		a = 1
		b = 2
		c = 3
	EOF
	git config --rename-section ren renamed &&
	git config --get renamed.a >actual_a &&
	git config --get renamed.b >actual_b &&
	git config --get renamed.c >actual_c &&
	echo "1" >expect_a &&
	echo "2" >expect_b &&
	echo "3" >expect_c &&
	test_cmp expect_a actual_a &&
	test_cmp expect_b actual_b &&
	test_cmp expect_c actual_c
'

test_expect_success 'remove-section with multiple keys' '
	cd repo &&
	git config --remove-section renamed &&
	test_must_fail git config --get renamed.a &&
	test_must_fail git config --get renamed.b &&
	test_must_fail git config --get renamed.c
'

test_expect_success 'rename non-existing section with subsection fails' '
	cd repo &&
	test_must_fail git config --rename-section nosuch.sub newsub
'

test_expect_success 'remove non-existing section with subsection fails' '
	cd repo &&
	test_must_fail git config --remove-section nosuch.sub
'

test_expect_success '--unset-all removes all occurrences' '
	cd repo &&
	cat >>.git/config <<-\EOF &&
	[ua]
		key = one
		key = two
		key = three
	EOF
	git config --unset-all ua.key &&
	test_must_fail git config --get ua.key
'

# ── file format and quoting ───────────────────────────────────────────────

test_expect_success 'value with trailing spaces is quoted in file' '
	cd repo &&
	git config quot.trailing "val  " &&
	git config --get quot.trailing >actual &&
	echo "val  " >expect &&
	test_cmp expect actual
'

test_expect_success 'value with leading spaces' '
	cd repo &&
	git config quot.leading "  val" &&
	git config --get quot.leading >actual &&
	echo "  val" >expect &&
	test_cmp expect actual
'

test_expect_success 'value with backslash preserved' '
	cd repo &&
	git config quot.bs "a\\b" &&
	git config --get quot.bs >actual &&
	echo "a\\b" >expect &&
	test_cmp expect actual
'

test_expect_success 'empty string value' '
	cd repo &&
	git config empty.val "" &&
	git config --get empty.val >actual &&
	echo "" >expect &&
	test_cmp expect actual
'

test_expect_success 'overwrite preserves other keys in section' '
	cd repo &&
	git config ow.keep "stay" &&
	git config ow.change "before" &&
	git config ow.change "after" &&
	git config --get ow.keep >actual_keep &&
	git config --get ow.change >actual_change &&
	echo "stay" >expect_keep &&
	echo "after" >expect_change &&
	test_cmp expect_keep actual_keep &&
	test_cmp expect_change actual_change
'

# ── GIT_CONFIG_COUNT and env injection ─────────────────────────────────

test_expect_success 'GIT_CONFIG_COUNT with higher count' '
	cd repo &&
	GIT_CONFIG_COUNT=3 \
	GIT_CONFIG_KEY_0="env.a" GIT_CONFIG_VALUE_0="alpha" \
	GIT_CONFIG_KEY_1="env.b" GIT_CONFIG_VALUE_1="beta" \
	GIT_CONFIG_KEY_2="env.c" GIT_CONFIG_VALUE_2="gamma" \
	git config --get env.a >actual_a &&
	GIT_CONFIG_COUNT=3 \
	GIT_CONFIG_KEY_0="env.a" GIT_CONFIG_VALUE_0="alpha" \
	GIT_CONFIG_KEY_1="env.b" GIT_CONFIG_VALUE_1="beta" \
	GIT_CONFIG_KEY_2="env.c" GIT_CONFIG_VALUE_2="gamma" \
	git config --get env.b >actual_b &&
	GIT_CONFIG_COUNT=3 \
	GIT_CONFIG_KEY_0="env.a" GIT_CONFIG_VALUE_0="alpha" \
	GIT_CONFIG_KEY_1="env.b" GIT_CONFIG_VALUE_1="beta" \
	GIT_CONFIG_KEY_2="env.c" GIT_CONFIG_VALUE_2="gamma" \
	git config --get env.c >actual_c &&
	echo "alpha" >expect_a &&
	echo "beta" >expect_b &&
	echo "gamma" >expect_c &&
	test_cmp expect_a actual_a &&
	test_cmp expect_b actual_b &&
	test_cmp expect_c actual_c
'

test_expect_success 'GIT_CONFIG_COUNT env overrides file config' '
	cd repo &&
	git config env.over "fromfile" &&
	GIT_CONFIG_COUNT=1 \
	GIT_CONFIG_KEY_0="env.over" GIT_CONFIG_VALUE_0="fromenv" \
	git config --get env.over >actual &&
	echo "fromenv" >expect &&
	test_cmp expect actual
'

test_expect_success 'set and immediately get back with subcommands' '
	cd repo &&
	git config set roundtrip.key "round" &&
	git config get roundtrip.key >actual &&
	echo "round" >expect &&
	test_cmp expect actual
'

test_expect_success '--get with --file on non-existent key returns exit 1' '
	cd repo &&
	cat >../getmiss.cfg <<-\EOF &&
	[exists]
		key = yes
	EOF
	test_must_fail git config --file ../getmiss.cfg --get nonexist.key
'

test_expect_success 'set writes new key to file and reads back' '
	cd repo &&
	git config set fresh.key "newvalue" &&
	git config get fresh.key >actual &&
	echo "newvalue" >expect &&
	test_cmp expect actual
'

# ── more config format tests ────────────────────────────────────────────────

test_expect_success 'multi-level subsection (a.b.c.d)' '
	cd repo &&
	git config "http.https://example.com/repo.git.proxy" "socks5://proxy" &&
	git config --get "http.https://example.com/repo.git.proxy" >actual &&
	echo "socks5://proxy" >expect &&
	test_cmp expect actual
'

test_expect_success 'rename-section preserves subsection case' '
	cd repo &&
	git config "OldSec.SubCase.key" "val" &&
	git config --rename-section "OldSec.SubCase" "NewSec.SubCase" &&
	git config --get "NewSec.SubCase.key" >actual &&
	echo "val" >expect &&
	test_cmp expect actual
'

test_expect_success 'unset last key in section leaves empty section' '
	cd repo &&
	git config lonely.key "alone" &&
	git config --unset lonely.key &&
	test_must_fail git config --get lonely.key
'

test_expect_success '--get on key set via --file roundtrips' '
	cd repo &&
	git config --file ../rtfile.cfg rt.key "roundtrip" &&
	git config --file ../rtfile.cfg --get rt.key >actual &&
	echo "roundtrip" >expect &&
	test_cmp expect actual
'

test_expect_success '--unset-all on key with single value works' '
	cd repo &&
	git config single.ua "only" &&
	git config --unset-all single.ua &&
	test_must_fail git config --get single.ua
'

# ── --show-origin format details ─────────────────────────────────────────

test_expect_success '--show-origin --list with --file shows file: prefix' '
	cd repo &&
	cat >../origin-test.cfg <<-\EOF &&
	[orig]
		key = val
	EOF
	git config --show-origin --list --file ../origin-test.cfg >actual &&
	grep "^file:" actual
'

test_expect_success '--show-origin --show-scope --list combined format' '
	cd repo &&
	cat >../combined.cfg <<-\EOF &&
	[comb]
		key = val
	EOF
	git config --show-origin --show-scope --list --file ../combined.cfg >actual &&
	test_line_count -gt 0 actual
'

test_expect_success '-z --list --file NUL separates' '
	cd repo &&
	cat >../ztest.cfg <<-\EOF &&
	[z]
		one = 1
		two = 2
	EOF
	git config -z --list --file ../ztest.cfg >actual &&
	printf "z.one=1\0z.two=2\0" >expect &&
	test_cmp expect actual
'

test_expect_success '-z with --get terminates value with NUL' '
	cd repo &&
	git config zget.key "zval" &&
	git config -z --get zget.key >actual &&
	printf "zval\0" >expect &&
	test_cmp expect actual
'

test_expect_success 'section with numbers in name' '
	cd repo &&
	git config sec123.key "numval" &&
	git config --get sec123.key >actual &&
	echo "numval" >expect &&
	test_cmp expect actual
'

test_done
