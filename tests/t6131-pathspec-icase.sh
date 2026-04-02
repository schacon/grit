#!/bin/sh
# Tests for case-sensitivity in pathspec matching, filenames, config keys,
# branch/tag names, and related operations.

test_description='case-sensitive pathspec and name matching'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Setup ────────────────────────────────────────────────────────────────────

test_expect_success 'setup repository with mixed-case filenames' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	mkdir -p Dir DIR dir sub/Lower sub/UPPER &&
	echo a >Dir/a.txt &&
	echo b >DIR/b.txt &&
	echo c >dir/c.txt &&
	echo d >File.TXT &&
	echo e >file.txt &&
	echo f >FILE.TXT &&
	echo g >MixedCase.Md &&
	echo h >sub/Lower/x.txt &&
	echo i >sub/UPPER/y.txt &&
	git add . &&
	git commit -m "initial with mixed case"
'

# ── ls-files case-sensitive matching ────────────────────────────────────────

test_expect_success 'ls-files is case-sensitive for filenames' '
	cd repo &&
	git ls-files File.TXT >actual &&
	echo "File.TXT" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files does not match wrong case of file' '
	cd repo &&
	git ls-files fILE.txt >actual &&
	test_must_be_empty actual
'

test_expect_success 'ls-files file.txt matches only lowercase variant' '
	cd repo &&
	git ls-files file.txt >actual &&
	echo "file.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files FILE.TXT matches only uppercase variant' '
	cd repo &&
	git ls-files FILE.TXT >actual &&
	echo "FILE.TXT" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files MixedCase.Md matches exactly' '
	cd repo &&
	git ls-files MixedCase.Md >actual &&
	echo "MixedCase.Md" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files mixedcase.md does not match MixedCase.Md' '
	cd repo &&
	git ls-files mixedcase.md >actual &&
	test_must_be_empty actual
'

test_expect_success 'ls-files MIXEDCASE.MD does not match MixedCase.Md' '
	cd repo &&
	git ls-files MIXEDCASE.MD >actual &&
	test_must_be_empty actual
'

# ── Directory case sensitivity ──────────────────────────────────────────────

test_expect_success 'ls-files dir/ matches only lowercase dir' '
	cd repo &&
	git ls-files dir/ >actual &&
	echo "dir/c.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files Dir/ matches only title-case Dir' '
	cd repo &&
	git ls-files Dir/ >actual &&
	echo "Dir/a.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files DIR/ matches only uppercase DIR' '
	cd repo &&
	git ls-files DIR/ >actual &&
	echo "DIR/b.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files sub/Lower/ matches only sub/Lower' '
	cd repo &&
	git ls-files sub/Lower/ >actual &&
	echo "sub/Lower/x.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files sub/UPPER/ matches only sub/UPPER' '
	cd repo &&
	git ls-files sub/UPPER/ >actual &&
	echo "sub/UPPER/y.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files sub/lower/ does not match sub/Lower' '
	cd repo &&
	git ls-files sub/lower/ >actual &&
	test_must_be_empty actual
'

# ── Config key case insensitivity ───────────────────────────────────────────

test_expect_success 'config section names are case-insensitive' '
	cd repo &&
	git config core.testKey "hello" &&
	val=$(git config --get CORE.testKey) &&
	test "$val" = "hello"
'

test_expect_success 'config variable names are case-insensitive' '
	cd repo &&
	git config core.myVar "world" &&
	val=$(git config --get core.MYVAR) &&
	test "$val" = "world"
'

test_expect_success 'config mixed case section+variable lookup' '
	cd repo &&
	git config User.Name "Some Name" &&
	val=$(git config --get user.name) &&
	# user.name was set earlier; User.Name should be same section
	test -n "$val"
'

test_expect_success 'config subsection names are case-sensitive' '
	cd repo &&
	git config "branch.Main.remote" "origin" &&
	val=$(git config --get "branch.Main.remote") &&
	test "$val" = "origin" &&
	! git config --get "branch.main.remote" 2>/dev/null
'

# ── Branch name case sensitivity ────────────────────────────────────────────

test_expect_success 'branches with different cases coexist' '
	cd repo &&
	git branch Feature &&
	git branch feature &&
	git branch FEATURE &&
	git branch -l >actual &&
	grep "Feature" actual &&
	grep "feature" actual &&
	grep "FEATURE" actual
'

test_expect_success 'show-ref distinguishes branch case' '
	cd repo &&
	git show-ref refs/heads/Feature >out_title 2>&1 &&
	git show-ref refs/heads/feature >out_lower 2>&1 &&
	git show-ref refs/heads/FEATURE >out_upper 2>&1 &&
	test_line_count = 1 out_title &&
	test_line_count = 1 out_lower &&
	test_line_count = 1 out_upper
'

# ── Tag name case sensitivity ───────────────────────────────────────────────

test_expect_success 'tags with different cases coexist' '
	cd repo &&
	git tag Release-1.0 &&
	git tag release-1.0 &&
	git tag RELEASE-1.0 &&
	git tag -l >actual &&
	grep "Release-1.0" actual &&
	grep "release-1.0" actual &&
	grep "RELEASE-1.0" actual
'

test_expect_success 'tag -l pattern is case-sensitive' '
	cd repo &&
	git tag -l "Release*" >actual &&
	echo "Release-1.0" >expect &&
	test_cmp expect actual
'

test_expect_success 'tag -l lowercase pattern matches only lowercase' '
	cd repo &&
	git tag -l "release*" >actual &&
	echo "release-1.0" >expect &&
	test_cmp expect actual
'

test_expect_success 'tag -l uppercase pattern matches only uppercase' '
	cd repo &&
	git tag -l "RELEASE*" >actual &&
	echo "RELEASE-1.0" >expect &&
	test_cmp expect actual
'

# ── add with case-exact paths ───────────────────────────────────────────────

test_expect_success 'add is case-sensitive for pathspec' '
	cd repo &&
	echo "new" >NewFile.txt &&
	git add NewFile.txt &&
	git ls-files NewFile.txt >actual &&
	echo "NewFile.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'add wrong-case filename fails to match' '
	cd repo &&
	echo "another" >AnotherFile.txt &&
	test_must_fail git add anotherfile.txt 2>err &&
	# Should not have added the wrong-case name
	git ls-files anotherfile.txt >actual &&
	test_must_be_empty actual
'

# ── commit message preserves case ───────────────────────────────────────────

test_expect_success 'commit message preserves exact case' '
	cd repo &&
	git add . &&
	git commit --allow-empty -m "CamelCase Message Here" &&
	git log -n1 --format="%s" >actual &&
	echo "CamelCase Message Here" >expect &&
	test_cmp expect actual
'

# ── rev-parse with case-sensitive refs ──────────────────────────────────────

test_expect_success 'rev-parse resolves case-sensitive branch names' '
	cd repo &&
	oid_upper=$(git rev-parse FEATURE) &&
	oid_lower=$(git rev-parse feature) &&
	oid_title=$(git rev-parse Feature) &&
	test "$oid_upper" = "$oid_lower" &&
	test "$oid_lower" = "$oid_title"
'

test_expect_success 'rev-parse resolves case-sensitive tag names' '
	cd repo &&
	oid_upper=$(git rev-parse RELEASE-1.0) &&
	oid_lower=$(git rev-parse release-1.0) &&
	test "$oid_upper" = "$oid_lower"
'

test_done
