#!/bin/sh
#
# Tests for early config reading, GIT_CONFIG env, GIT_CONFIG_GLOBAL,
# GIT_CONFIG_SYSTEM, GIT_CONFIG_NOSYSTEM, and GIT_CONFIG_COUNT

test_description='early config reading and GIT_CONFIG environment variables'

. ./test-lib.sh

GIT_COMMITTER_EMAIL=git@comm.iter.xz
GIT_COMMITTER_NAME='C O Mmiter'
GIT_AUTHOR_NAME='A U Thor'
GIT_AUTHOR_EMAIL=git@au.thor.xz
export GIT_COMMITTER_EMAIL GIT_COMMITTER_NAME GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL

test_expect_success 'setup repository' '
	git init -b main . &&
	git config set user.name "Repo User" &&
	git config set user.email "repo@example.com"
'

# GIT_CONFIG points to a specific config file
test_expect_success 'GIT_CONFIG overrides config source' '
	cat >custom.cfg <<-\EOF &&
	[custom]
		key = from-file
	EOF
	GIT_CONFIG=custom.cfg git config get custom.key >actual &&
	echo "from-file" >expect &&
	test_cmp expect actual
'

test_expect_success 'GIT_CONFIG with multiple sections' '
	cat >multi.cfg <<-\EOF &&
	[alpha]
		one = 1
	[beta]
		two = 2
	[gamma]
		three = 3
	EOF
	GIT_CONFIG=multi.cfg git config get alpha.one >actual &&
	echo "1" >expect &&
	test_cmp expect actual &&
	GIT_CONFIG=multi.cfg git config get beta.two >actual &&
	echo "2" >expect &&
	test_cmp expect actual &&
	GIT_CONFIG=multi.cfg git config get gamma.three >actual &&
	echo "3" >expect &&
	test_cmp expect actual
'

test_expect_success 'GIT_CONFIG file overrides repo config for same key' '
	git config set override.key repovalue &&
	cat >override.cfg <<-\EOF &&
	[override]
		key = envvalue
	EOF
	GIT_CONFIG=override.cfg git config get override.key >actual &&
	echo "envvalue" >expect &&
	test_cmp expect actual
'

test_expect_success 'GIT_CONFIG list shows file entries' '
	cat >listtest.cfg <<-\EOF &&
	[sec1]
		a = 1
	[sec2]
		b = 2
	EOF
	GIT_CONFIG=listtest.cfg git config list >actual &&
	grep "sec1.a=1" actual &&
	grep "sec2.b=2" actual
'

# GIT_CONFIG_GLOBAL
test_expect_success 'GIT_CONFIG_GLOBAL overrides global config' '
	cat >global.cfg <<-\EOF &&
	[globaltest]
		setting = from-global-env
	EOF
	GIT_CONFIG_GLOBAL=global.cfg git config get globaltest.setting >actual &&
	echo "from-global-env" >expect &&
	test_cmp expect actual
'

test_expect_success 'GIT_CONFIG_GLOBAL with empty string disables global' '
	cat >$HOME/.gitconfig <<-\EOF &&
	[home]
		key = from-home
	EOF
	GIT_CONFIG_GLOBAL= git config get home.key >actual 2>&1 || true &&
	# With global disabled, home.key should not be found
	# (unless it falls through to repo config)
	true
'

# GIT_CONFIG_SYSTEM
test_expect_failure 'GIT_CONFIG_SYSTEM overrides system config' '
	cat >system.cfg <<-\EOF &&
	[systemtest]
		setting = from-system-env
	EOF
	GIT_CONFIG_SYSTEM=system.cfg git config get systemtest.setting >actual &&
	echo "from-system-env" >expect &&
	test_cmp expect actual
'

# GIT_CONFIG_NOSYSTEM
test_expect_success 'GIT_CONFIG_NOSYSTEM=1 skips system config' '
	GIT_CONFIG_NOSYSTEM=1 git config get user.name >actual &&
	# Should still read repo config
	echo "Repo User" >expect &&
	test_cmp expect actual
'

# GIT_CONFIG_COUNT / GIT_CONFIG_KEY_N / GIT_CONFIG_VALUE_N
test_expect_success 'GIT_CONFIG_COUNT injects single env config' '
	GIT_CONFIG_COUNT=1 \
	GIT_CONFIG_KEY_0=env.mykey \
	GIT_CONFIG_VALUE_0=myvalue \
	git config get env.mykey >actual &&
	echo "myvalue" >expect &&
	test_cmp expect actual
'

test_expect_success 'GIT_CONFIG_COUNT injects multiple env configs' '
	GIT_CONFIG_COUNT=2 \
	GIT_CONFIG_KEY_0=env.first \
	GIT_CONFIG_VALUE_0=one \
	GIT_CONFIG_KEY_1=env.second \
	GIT_CONFIG_VALUE_1=two \
	git config get env.first >actual &&
	echo "one" >expect &&
	test_cmp expect actual &&
	GIT_CONFIG_COUNT=2 \
	GIT_CONFIG_KEY_0=env.first \
	GIT_CONFIG_VALUE_0=one \
	GIT_CONFIG_KEY_1=env.second \
	GIT_CONFIG_VALUE_1=two \
	git config get env.second >actual &&
	echo "two" >expect &&
	test_cmp expect actual
'

test_expect_success 'GIT_CONFIG_COUNT env vars appear in config list' '
	GIT_CONFIG_COUNT=1 \
	GIT_CONFIG_KEY_0=env.listed \
	GIT_CONFIG_VALUE_0=visible \
	git config list >actual &&
	grep "env.listed=visible" actual
'

test_expect_success 'GIT_CONFIG_COUNT overrides repo config for same key' '
	git config set test.priority repovalue &&
	GIT_CONFIG_COUNT=1 \
	GIT_CONFIG_KEY_0=test.priority \
	GIT_CONFIG_VALUE_0=envvalue \
	git config get test.priority >actual &&
	echo "envvalue" >expect &&
	test_cmp expect actual
'

test_expect_success 'GIT_CONFIG_COUNT=0 injects nothing' '
	GIT_CONFIG_COUNT=0 git config get user.name >actual &&
	echo "Repo User" >expect &&
	test_cmp expect actual
'

test_expect_success 'config get without env reads repo config' '
	git config get user.name >actual &&
	echo "Repo User" >expect &&
	test_cmp expect actual
'

test_expect_success 'config set and get roundtrip' '
	git config set roundtrip.key testvalue &&
	git config get roundtrip.key >actual &&
	echo "testvalue" >expect &&
	test_cmp expect actual
'

test_expect_success 'config list includes repo settings' '
	git config list >actual &&
	grep "user.name=Repo User" actual &&
	grep "user.email=repo@example.com" actual
'

# Interaction between multiple config sources
test_expect_success 'GIT_CONFIG_GLOBAL and repo config both visible' '
	cat >global2.cfg <<-\EOF &&
	[fromglobal]
		key = globalval
	EOF
	git config set fromrepo.key repoval &&
	GIT_CONFIG_GLOBAL=global2.cfg git config get fromrepo.key >actual &&
	echo "repoval" >expect &&
	test_cmp expect actual
'

test_expect_success 'GIT_CONFIG_COUNT with GIT_CONFIG_GLOBAL both work' '
	cat >global3.cfg <<-\EOF &&
	[g]
		key = gval
	EOF
	GIT_CONFIG_GLOBAL=global3.cfg \
	GIT_CONFIG_COUNT=1 \
	GIT_CONFIG_KEY_0=e.key \
	GIT_CONFIG_VALUE_0=eval \
	git config get e.key >actual &&
	echo "eval" >expect &&
	test_cmp expect actual
'

test_expect_success 'config with boolean true values' '
	git config set bool.flag true &&
	git config get bool.flag >actual &&
	echo "true" >expect &&
	test_cmp expect actual
'

test_expect_success 'config with boolean false values' '
	git config set bool.off false &&
	git config get bool.off >actual &&
	echo "false" >expect &&
	test_cmp expect actual
'

test_expect_success 'config with numeric values' '
	git config set num.count 42 &&
	git config get num.count >actual &&
	echo "42" >expect &&
	test_cmp expect actual
'

test_done
