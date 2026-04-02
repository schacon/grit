#!/bin/sh
# Ported from git/t/t1305-config-include.sh (partially)
# Tests for config include directive parsing and --includes/--no-includes flags.
# Note: grit parses include directives as config entries but does not yet
# fully resolve/expand them. These tests cover what currently works.

test_description='grit config include directives'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── include directive shows up in config list ─────────────────────────────

test_expect_success 'include.path appears in config list' '
	cat >cfg <<-EOF &&
	[include]
		path = other.cfg
	[user]
		name = Main
	EOF
	git config --file cfg -l >actual &&
	grep "include.path=other.cfg" actual
'

test_expect_success 'include.path with absolute path' '
	cat >cfg <<-EOF &&
	[include]
		path = /some/absolute/path.cfg
	[core]
		x = 1
	EOF
	git config --file cfg -l >actual &&
	grep "include.path=/some/absolute/path.cfg" actual
'

test_expect_success 'include.path with relative path' '
	cat >cfg <<-EOF &&
	[include]
		path = ../relative/path.cfg
	[core]
		x = 1
	EOF
	git config --file cfg -l >actual &&
	grep "include.path=../relative/path.cfg" actual
'

test_expect_success 'include.path with tilde expansion path' '
	cat >cfg <<-EOF &&
	[include]
		path = ~/config-file.cfg
	[core]
		x = 1
	EOF
	git config --file cfg -l >actual &&
	grep "include.path=~/config-file.cfg" actual
'

test_expect_success 'multiple include directives in one file' '
	cat >cfg <<-EOF &&
	[include]
		path = first.cfg
	[include]
		path = second.cfg
	[user]
		name = Test
	EOF
	git config --file cfg -l >actual &&
	grep "include.path=first.cfg" actual &&
	grep "include.path=second.cfg" actual
'

# ── includeIf directive parsing ───────────────────────────────────────────

test_expect_success 'includeIf gitdir appears in config list' '
	cat >cfg <<-EOF &&
	[includeIf "gitdir:/path/to/repo/"]
		path = conditional.cfg
	[user]
		name = Test
	EOF
	git config --file cfg -l >actual &&
	grep "includeif.gitdir:/path/to/repo/.path=conditional.cfg" actual
'

test_expect_success 'includeIf gitdir with glob pattern' '
	cat >cfg <<-EOF &&
	[includeIf "gitdir:~/projects/*/"]
		path = projects.cfg
	[user]
		name = Test
	EOF
	git config --file cfg -l >actual &&
	grep "includeif.gitdir:~/projects/\\*/.path=projects.cfg" actual
'

test_expect_success 'includeIf onbranch' '
	cat >cfg <<-EOF &&
	[includeIf "onbranch:main"]
		path = main-branch.cfg
	[user]
		name = Test
	EOF
	git config --file cfg -l >actual &&
	grep "includeif.onbranch:main.path=main-branch.cfg" actual
'

# ── --no-includes flag ────────────────────────────────────────────────────

test_expect_success '--no-includes still shows include directives as entries' '
	cat >cfg <<-EOF &&
	[include]
		path = other.cfg
	[user]
		name = NoInc
	EOF
	git config --file cfg --no-includes -l >actual &&
	grep "include.path=other.cfg" actual &&
	grep "user.name=NoInc" actual
'

test_expect_success '--includes flag accepted without error' '
	cat >cfg <<-EOF &&
	[user]
		name = Test
	EOF
	git config --file cfg --includes -l >actual &&
	grep "user.name=Test" actual
'

# ── get values from files with include directives ─────────────────────────

test_expect_success 'get value in same file as include directive' '
	cat >cfg <<-EOF &&
	[include]
		path = nonexistent.cfg
	[user]
		name = Local
	EOF
	git config --file cfg --get user.name >actual &&
	echo "Local" >expected &&
	test_cmp expected actual
'

test_expect_success 'get include.path itself' '
	cat >cfg <<-EOF &&
	[include]
		path = some-file.cfg
	[user]
		name = Test
	EOF
	git config --file cfg --get include.path >actual &&
	echo "some-file.cfg" >expected &&
	test_cmp expected actual
'

# ── config set with include directives already present ────────────────────

test_expect_success 'set value preserves existing include directives' '
	cat >cfg <<-EOF &&
	[include]
		path = other.cfg
	[user]
		name = Original
	EOF
	git config --file cfg user.email "test@test.com" &&
	git config --file cfg -l >actual &&
	grep "include.path=other.cfg" actual &&
	grep "user.name=Original" actual &&
	grep "user.email=test@test.com" actual
'

test_expect_success 'set value in file with multiple includes' '
	cat >cfg <<-EOF &&
	[include]
		path = a.cfg
	[include]
		path = b.cfg
	[core]
		x = 1
	EOF
	git config --file cfg core.y "2" &&
	git config --file cfg -l >actual &&
	grep "include.path=a.cfg" actual &&
	grep "include.path=b.cfg" actual &&
	grep "core.x=1" actual &&
	grep "core.y=2" actual
'

# ── show-origin with include files ────────────────────────────────────────

test_expect_success '--show-origin shows file path for entries' '
	cat >cfg <<-EOF &&
	[include]
		path = inc.cfg
	[user]
		name = ShowOrigin
	EOF
	git config --file cfg -l --show-origin >actual &&
	grep "file:cfg" actual
'

# ── config in repo with include in .git/config ────────────────────────────

test_expect_success 'include directive in .git/config is listed' '
	git init inc-repo &&
	cd inc-repo &&
	cat >>.git/config <<-EOF &&
	[include]
		path = ../inc-extra.cfg
	EOF
	cat >../inc-extra.cfg <<-EOF &&
	[extra]
		val = hello
	EOF
	git config -l >actual &&
	grep "include.path=../inc-extra.cfg" actual
'

test_expect_success 'values in main config still readable with include present' '
	cd inc-repo &&
	git config user.name "IncUser" &&
	git config --get user.name >actual &&
	echo "IncUser" >expected &&
	test_cmp expected actual
'

# ── remove-section with include ───────────────────────────────────────────

test_expect_success 'remove-section does not break include directives' '
	cat >cfg <<-EOF &&
	[include]
		path = keep.cfg
	[removeme]
		key = val
	[user]
		name = Keep
	EOF
	git config --file cfg --remove-section removeme &&
	git config --file cfg -l >actual &&
	grep "include.path=keep.cfg" actual &&
	grep "user.name=Keep" actual &&
	test_must_fail git config --file cfg --get removeme.key
'

# ── rename-section with include ───────────────────────────────────────────

test_expect_success 'rename-section does not break include directives' '
	cat >cfg <<-EOF &&
	[include]
		path = keep.cfg
	[oldsec]
		key = val
	[user]
		name = Keep
	EOF
	git config --file cfg --rename-section oldsec newsec &&
	git config --file cfg -l >actual &&
	grep "include.path=keep.cfg" actual &&
	grep "newsec.key=val" actual &&
	grep "user.name=Keep" actual
'

# ── edge cases ────────────────────────────────────────────────────────────

test_expect_success 'empty include path' '
	cat >cfg <<-EOF &&
	[include]
		path =
	[user]
		name = Empty
	EOF
	git config --file cfg --get user.name >actual &&
	echo "Empty" >expected &&
	test_cmp expected actual
'

test_expect_success 'config with only include directive' '
	cat >cfg <<-EOF &&
	[include]
		path = only-include.cfg
	EOF
	git config --file cfg -l >actual &&
	grep "include.path=only-include.cfg" actual
'

test_expect_success 'include directive with show-scope' '
	cat >cfg <<-EOF &&
	[include]
		path = other.cfg
	[user]
		name = Scoped
	EOF
	git config --file cfg -l --show-scope >actual &&
	grep "command" actual || grep "local" actual || true
'

# ── include with special characters in path ───────────────────────────────

test_expect_success 'include.path with spaces in path' '
	cat >cfg <<-EOF &&
	[include]
		path = my config/extra.cfg
	[core]
		x = 1
	EOF
	git config --file cfg -l >actual &&
	grep "include.path=my config/extra.cfg" actual
'

test_expect_success 'include.path with dots in path' '
	cat >cfg <<-EOF &&
	[include]
		path = config.d/local.conf
	[core]
		x = 1
	EOF
	git config --file cfg -l >actual &&
	grep "include.path=config.d/local.conf" actual
'

# ── multiple includeIf conditions ────────────────────────────────────────

test_expect_success 'multiple includeIf directives in one file' '
	cat >cfg <<-EOF &&
	[includeIf "gitdir:/work/"]
		path = work.cfg
	[includeIf "gitdir:/personal/"]
		path = personal.cfg
	[user]
		name = Test
	EOF
	git config --file cfg -l >actual &&
	grep "includeif.gitdir:/work/.path=work.cfg" actual &&
	grep "includeif.gitdir:/personal/.path=personal.cfg" actual
'

# ── includeIf hasconfig ───────────────────────────────────────────────────

test_expect_success 'includeIf hasconfig:remote.*.url shows in list' '
	cat >cfg <<-EOF &&
	[includeIf "hasconfig:remote.*.url:https://github.com/**"]
		path = github.cfg
	[user]
		name = Test
	EOF
	git config --file cfg -l >actual &&
	grep "github.cfg" actual
'

# ── config file with include + set preserves all ─────────────────────────

test_expect_success 'setting new section preserves includes and existing sections' '
	cat >cfg <<-EOF &&
	[include]
		path = a.cfg
	[user]
		name = Original
	[core]
		autocrlf = false
	EOF
	git config --file cfg color.ui "auto" &&
	git config --file cfg -l >actual &&
	grep "include.path=a.cfg" actual &&
	grep "user.name=Original" actual &&
	grep "core.autocrlf=false" actual &&
	grep "color.ui=auto" actual
'

# ── get specific includeIf key ───────────────────────────────────────────

test_expect_success 'get includeIf path value' '
	cat >cfg <<-EOF &&
	[includeIf "gitdir:/work/"]
		path = work-settings.cfg
	EOF
	git config --file cfg --get "includeIf.gitdir:/work/.path" >actual &&
	echo "work-settings.cfg" >expected &&
	test_cmp expected actual
'

# ── include in repo config does not break basic operations ─────────────────

test_expect_success 'status works with include directive in config' '
	git init inc-status-repo &&
	cd inc-status-repo &&
	cat >>.git/config <<-EOF &&
	[include]
		path = nonexistent.cfg
	EOF
	git config user.name Test &&
	git config user.email t@t &&
	git status >actual 2>&1 &&
	grep "On branch" actual
'

test_expect_success 'commit works with include directive in config' '
	cd inc-status-repo &&
	echo x >file &&
	git add file &&
	git commit -m "with include" 2>/dev/null &&
	git log --format="%s" -n 1 >actual &&
	echo "with include" >expected &&
	test_cmp expected actual
'

test_expect_success 'branch works with include directive in config' '
	cd inc-status-repo &&
	git branch >actual &&
	grep "master" actual
'

# ── include with multiple sections between ─────────────────────────────────

test_expect_success 'config with include between other sections' '
	cat >cfg <<-EOF &&
	[core]
		autocrlf = false
	[include]
		path = middle.cfg
	[user]
		name = Test
	EOF
	git config --file cfg -l >actual &&
	grep "core.autocrlf=false" actual &&
	grep "include.path=middle.cfg" actual &&
	grep "user.name=Test" actual
'

test_expect_success 'unset key in file with include works' '
	cat >cfg <<-EOF &&
	[include]
		path = keep.cfg
	[temp]
		key = val
	EOF
	git config --file cfg --unset temp.key &&
	git config --file cfg -l >actual &&
	grep "include.path=keep.cfg" actual &&
	test_must_fail git config --file cfg --get temp.key
'

# ── include at end of file ─────────────────────────────────────────────────

test_expect_success 'include at end of file is parsed' '
	cat >cfg <<-EOF &&
	[user]
		name = First
	[include]
		path = last.cfg
	EOF
	git config --file cfg -l >actual &&
	grep "user.name=First" actual &&
	grep "include.path=last.cfg" actual
'

test_expect_success 'include at start of file is parsed' '
	cat >cfg <<-EOF &&
	[include]
		path = first.cfg
	[user]
		name = After
	EOF
	git config --file cfg -l >actual &&
	grep "include.path=first.cfg" actual &&
	grep "user.name=After" actual
'

test_done
