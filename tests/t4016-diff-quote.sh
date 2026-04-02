#!/bin/sh
# Test diff output with special characters in filenames:
# spaces, tabs, quotes, backslashes, unicode, etc.

test_description='diff with quoted/special filenames'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo' '
	grit init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test"
'

# ── spaces ──

test_expect_success 'diff with spaces in filename' '
	cd repo &&
	echo "content" >"file with spaces.txt" &&
	grit add "file with spaces.txt" &&
	grit commit -m "add spaced file" &&
	echo "changed" >"file with spaces.txt" &&
	grit diff >actual &&
	grep "file with spaces.txt" actual
'

test_expect_success 'diff --name-only with spaces in filename' '
	cd repo &&
	grit diff --name-only >actual &&
	grep "file with spaces.txt" actual
'

test_expect_success 'diff --name-status with spaces in filename' '
	cd repo &&
	grit diff --name-status >actual &&
	grep "M" actual &&
	grep "file with spaces.txt" actual
'

test_expect_success 'diff --stat with spaces in filename' '
	cd repo &&
	grit diff --stat >actual &&
	grep "file with spaces" actual
'

test_expect_success 'diff --numstat with spaces in filename' '
	cd repo &&
	grit diff --numstat >actual &&
	grep "file with spaces.txt" actual
'

test_expect_success 'commit spaced file changes' '
	cd repo &&
	grit add "file with spaces.txt" &&
	grit commit -m "update spaced file"
'

# ── multiple spaces ──

test_expect_success 'diff with multiple consecutive spaces' '
	cd repo &&
	echo "data" >"a    b.txt" &&
	grit add "a    b.txt" &&
	grit commit -m "multi-space" &&
	echo "new" >"a    b.txt" &&
	grit diff >actual &&
	grep "a    b.txt" actual
'

test_expect_success 'commit multi-space file' '
	cd repo &&
	grit add "a    b.txt" &&
	grit commit -m "update multi-space"
'

# ── leading/trailing spaces ──

test_expect_success 'diff with leading space in filename' '
	cd repo &&
	echo "data" >" leading.txt" &&
	grit add " leading.txt" &&
	grit commit -m "leading space" &&
	echo "new" >" leading.txt" &&
	grit diff --name-only >actual &&
	grep "leading.txt" actual
'

test_expect_success 'commit leading space file' '
	cd repo &&
	grit add " leading.txt" &&
	grit commit -m "update leading"
'

test_expect_success 'diff with trailing space in filename' '
	cd repo &&
	echo "data" >"trailing .txt" &&
	grit add "trailing .txt" &&
	grit commit -m "trailing space" &&
	echo "new" >"trailing .txt" &&
	grit diff --name-only >actual &&
	grep "trailing " actual
'

test_expect_success 'commit trailing space file' '
	cd repo &&
	grit add "trailing .txt" &&
	grit commit -m "update trailing"
'

# ── special chars ──

test_expect_success 'diff with parentheses in filename' '
	cd repo &&
	echo "data" >"file(1).txt" &&
	grit add "file(1).txt" &&
	grit commit -m "parens" &&
	echo "new" >"file(1).txt" &&
	grit diff --name-only >actual &&
	grep "file(1).txt" actual
'

test_expect_success 'commit parens file' '
	cd repo &&
	grit add "file(1).txt" &&
	grit commit -m "update parens"
'

test_expect_success 'diff with brackets in filename' '
	cd repo &&
	echo "data" >"file[1].txt" &&
	grit add "file[1].txt" &&
	grit commit -m "brackets" &&
	echo "new" >"file[1].txt" &&
	grit diff --name-only >actual &&
	grep "file\[1\].txt" actual
'

test_expect_success 'commit brackets file' '
	cd repo &&
	grit add "file[1].txt" &&
	grit commit -m "update brackets"
'

test_expect_success 'diff with exclamation mark in filename' '
	cd repo &&
	echo "data" >"important!.txt" &&
	grit add "important!.txt" &&
	grit commit -m "bang" &&
	echo "new" >"important!.txt" &&
	grit diff --name-only >actual &&
	grep "important!.txt" actual
'

test_expect_success 'commit bang file' '
	cd repo &&
	grit add "important!.txt" &&
	grit commit -m "update bang"
'

test_expect_success 'diff with at-sign in filename' '
	cd repo &&
	echo "data" >"user@host.txt" &&
	grit add "user@host.txt" &&
	grit commit -m "at sign" &&
	echo "new" >"user@host.txt" &&
	grit diff --name-only >actual &&
	grep "user@host.txt" actual
'

test_expect_success 'commit at-sign file' '
	cd repo &&
	grit add "user@host.txt" &&
	grit commit -m "update at"
'

test_expect_success 'diff with hash in filename' '
	cd repo &&
	echo "data" >"issue#42.txt" &&
	grit add "issue#42.txt" &&
	grit commit -m "hash" &&
	echo "new" >"issue#42.txt" &&
	grit diff --name-only >actual &&
	grep "issue#42.txt" actual
'

test_expect_success 'commit hash file' '
	cd repo &&
	grit add "issue#42.txt" &&
	grit commit -m "update hash"
'

test_expect_success 'diff with plus and equals in filename' '
	cd repo &&
	echo "data" >"a+b=c.txt" &&
	grit add "a+b=c.txt" &&
	grit commit -m "plus-equals" &&
	echo "new" >"a+b=c.txt" &&
	grit diff --name-only >actual &&
	grep "a+b=c.txt" actual
'

test_expect_success 'commit plus-equals file' '
	cd repo &&
	grit add "a+b=c.txt" &&
	grit commit -m "update plus-equals"
'

# ── deeply nested path with spaces ──

test_expect_success 'diff with spaces in directory names' '
	cd repo &&
	mkdir -p "dir with spaces/sub dir" &&
	echo "data" >"dir with spaces/sub dir/file.txt" &&
	grit add "dir with spaces/sub dir/file.txt" &&
	grit commit -m "nested spaces" &&
	echo "new" >"dir with spaces/sub dir/file.txt" &&
	grit diff --name-only >actual &&
	grep "dir with spaces/sub dir/file.txt" actual
'

test_expect_success 'commit nested spaces' '
	cd repo &&
	grit add "dir with spaces/sub dir/file.txt" &&
	grit commit -m "update nested"
'

# ── diff between commits with special filenames ──

test_expect_success 'diff between commits shows special filenames' '
	cd repo &&
	grit diff --name-only HEAD~1 HEAD >actual &&
	grep "dir with spaces" actual
'

test_expect_success 'diff --stat between commits with special filenames' '
	cd repo &&
	grit diff --stat HEAD~1 HEAD >actual &&
	grep "dir with spaces" actual
'

test_expect_success 'diff --numstat between commits with special filenames' '
	cd repo &&
	grit diff --numstat HEAD~1 HEAD >actual &&
	grep "dir with spaces" actual
'

# ── unicode filenames ──

test_expect_success 'diff with unicode filename' '
	cd repo &&
	echo "data" >"café.txt" &&
	grit add "café.txt" &&
	grit commit -m "unicode" &&
	echo "new" >"café.txt" &&
	grit diff >actual &&
	cat actual | grep -c "diff --git" >count &&
	test "$(cat count)" = "1"
'

test_expect_success 'commit unicode file' '
	cd repo &&
	grit add "café.txt" &&
	grit commit -m "update unicode"
'

test_expect_success 'diff with CJK filename' '
	cd repo &&
	echo "data" >"文件.txt" &&
	grit add "文件.txt" &&
	grit commit -m "cjk" &&
	echo "new" >"文件.txt" &&
	grit diff >actual &&
	test -s actual
'

test_expect_success 'commit CJK file' '
	cd repo &&
	grit add "文件.txt" &&
	grit commit -m "update cjk"
'

test_expect_success 'diff with emoji filename' '
	cd repo &&
	echo "data" >"🎉.txt" &&
	grit add "🎉.txt" &&
	grit commit -m "emoji" &&
	echo "new" >"🎉.txt" &&
	grit diff >actual &&
	test -s actual
'

test_expect_success 'commit emoji file' '
	cd repo &&
	grit add "🎉.txt" &&
	grit commit -m "update emoji"
'

# ── cached diff with special names ──

test_expect_success 'diff --cached with special filenames' '
	cd repo &&
	echo "staged" >"file with spaces.txt" &&
	grit add "file with spaces.txt" &&
	grit diff --cached --name-only >actual &&
	grep "file with spaces.txt" actual
'

test_expect_success 'cleanup staged' '
	cd repo &&
	grit commit -m "staged cleanup"
'

test_done
