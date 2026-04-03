#!/bin/sh
#
# Tests for config reading from blob objects (--blob option)

test_description='config reading from blob objects'

. ./test-lib.sh

GIT_COMMITTER_EMAIL=git@comm.iter.xz
GIT_COMMITTER_NAME='C O Mmiter'
GIT_AUTHOR_NAME='A U Thor'
GIT_AUTHOR_EMAIL=git@au.thor.xz
export GIT_COMMITTER_EMAIL GIT_COMMITTER_NAME GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL

test_expect_success 'setup repository with config files committed' '
	git init -b main . &&
	cat >.my-config <<-\EOF &&
	[section]
		key = value
		number = 42
		flag = true
	[other]
		name = test-name
	EOF
	git add .my-config &&
	test_tick &&
	git commit -m "add config file" &&
	git tag with-config
'

test_expect_success 'config --blob reads from HEAD:path' '
	git config --blob HEAD:.my-config section.key >actual &&
	echo "value" >expect &&
	test_cmp expect actual
'

test_expect_success 'config --blob reads number value' '
	git config --blob HEAD:.my-config section.number >actual &&
	echo "42" >expect &&
	test_cmp expect actual
'

test_expect_success 'config --blob reads boolean value' '
	git config --blob HEAD:.my-config section.flag >actual &&
	echo "true" >expect &&
	test_cmp expect actual
'

test_expect_success 'config --blob reads from other section' '
	git config --blob HEAD:.my-config other.name >actual &&
	echo "test-name" >expect &&
	test_cmp expect actual
'

test_expect_success 'config --blob with tag ref' '
	git config --blob with-config:.my-config section.key >actual &&
	echo "value" >expect &&
	test_cmp expect actual
'

test_expect_success 'config --blob with raw SHA' '
	BLOB=$(git rev-parse HEAD:.my-config) &&
	git config --blob $BLOB section.key >actual &&
	echo "value" >expect &&
	test_cmp expect actual
'

test_expect_success 'config --blob fails for missing key' '
	test_must_fail git config --blob HEAD:.my-config section.nonexistent
'

test_expect_success 'config --blob fails for missing blob' '
	test_must_fail git config --blob HEAD:nonexistent section.key
'

# Update the config file and commit again
test_expect_success 'setup second version of config' '
	cat >.my-config <<-\EOF &&
	[section]
		key = updated-value
		number = 99
		flag = false
		newkey = added
	[other]
		name = new-name
	EOF
	git add .my-config &&
	test_tick &&
	git commit -m "update config file" &&
	git tag updated-config
'

test_expect_success 'config --blob reads updated value from HEAD' '
	git config --blob HEAD:.my-config section.key >actual &&
	echo "updated-value" >expect &&
	test_cmp expect actual
'

test_expect_success 'config --blob reads old value from old commit' '
	git config --blob with-config:.my-config section.key >actual &&
	echo "value" >expect &&
	test_cmp expect actual
'

test_expect_success 'config --blob reads new key from HEAD' '
	git config --blob HEAD:.my-config section.newkey >actual &&
	echo "added" >expect &&
	test_cmp expect actual
'

test_expect_success 'config --blob with --list shows all entries' '
	git config --blob HEAD:.my-config --list >actual &&
	grep "section.key=updated-value" actual &&
	grep "section.number=99" actual &&
	grep "other.name=new-name" actual
'

# Test config reading from file (--file) which grit does support via GIT_CONFIG
test_expect_success 'config get reads from repo config' '
	git config set test.localkey localvalue &&
	git config get test.localkey >actual &&
	echo "localvalue" >expect &&
	test_cmp expect actual
'

test_expect_success 'config get reads committed value via GIT_CONFIG' '
	GIT_CONFIG=.my-config git config get section.key >actual &&
	echo "updated-value" >expect &&
	test_cmp expect actual
'

test_expect_success 'GIT_CONFIG points to specific file' '
	cat >custom.cfg <<-\EOF &&
	[custom]
		setting = hello
	EOF
	GIT_CONFIG=custom.cfg git config get custom.setting >actual &&
	echo "hello" >expect &&
	test_cmp expect actual
'

test_expect_success 'GIT_CONFIG overrides repo config' '
	git config set override.key repovalue &&
	cat >override.cfg <<-\EOF &&
	[override]
		key = filevalue
	EOF
	GIT_CONFIG=override.cfg git config get override.key >actual &&
	echo "filevalue" >expect &&
	test_cmp expect actual
'

test_expect_success 'config list shows all config entries' '
	git config list >actual &&
	grep "test.localkey=localvalue" actual
'

test_expect_success 'config list with GIT_CONFIG shows file entries' '
	GIT_CONFIG=custom.cfg git config list >actual &&
	grep "custom.setting=hello" actual
'

test_expect_success 'config --blob with --get-regexp' '
	git config --blob HEAD:.my-config --get-regexp "section\." >actual &&
	test $(wc -l <actual) -ge 3
'

test_expect_success 'config --blob nonexistent ref fails' '
	test_must_fail git config --blob nonexistent-ref:.my-config section.key
'

test_expect_success 'config --blob with tree (not blob) fails' '
	test_must_fail git config --blob HEAD: section.key
'

test_done
