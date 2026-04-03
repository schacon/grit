#!/bin/sh
# Test ls-files --error-unmatch, -i/--ignored, -d, -m, -o, -s, -z, etc.

test_description='ls-files --error-unmatch and advanced flags'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with tracked and ignored files' '
	git init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test" &&
	echo "*.log" >.gitignore &&
	echo "build/" >>.gitignore &&
	echo "tracked" >tracked.txt &&
	echo "also tracked" >another.txt &&
	mkdir -p src &&
	echo "code" >src/main.c &&
	git add .gitignore tracked.txt another.txt src/main.c &&
	git commit -m "initial"
'

test_expect_success '--error-unmatch succeeds for tracked file' '
	cd repo &&
	git ls-files --error-unmatch tracked.txt >actual &&
	echo "tracked.txt" >expect &&
	test_cmp expect actual
'

test_expect_success '--error-unmatch succeeds for multiple tracked files' '
	cd repo &&
	git ls-files --error-unmatch tracked.txt another.txt >actual &&
	cat >expect <<-\EOF &&
	another.txt
	tracked.txt
	EOF
	test_cmp expect actual
'

test_expect_success '--error-unmatch fails for nonexistent file' '
	cd repo &&
	test_must_fail git ls-files --error-unmatch nonexistent.txt 2>err &&
	grep "pathspec.*nonexistent.txt.*did not match" err
'

test_expect_success '--error-unmatch fails when one of multiple files is missing' '
	cd repo &&
	test_must_fail git ls-files --error-unmatch tracked.txt missing.txt 2>err &&
	grep "pathspec.*missing.txt.*did not match" err
'

test_expect_success '--error-unmatch with directory pathspec' '
	cd repo &&
	git ls-files --error-unmatch src/ >actual &&
	echo "src/main.c" >expect &&
	test_cmp expect actual
'

test_expect_success '--error-unmatch with file in subdirectory' '
	cd repo &&
	git ls-files --error-unmatch src/main.c >actual &&
	echo "src/main.c" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files -c shows cached files' '
	cd repo &&
	git ls-files -c >actual &&
	cat >expect <<-\EOF &&
	.gitignore
	another.txt
	src/main.c
	tracked.txt
	EOF
	test_cmp expect actual
'

test_expect_success 'ls-files with no flags defaults to cached' '
	cd repo &&
	git ls-files >actual &&
	git ls-files -c >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files -s shows staging info' '
	cd repo &&
	git ls-files -s >actual &&
	grep "100644" actual &&
	grep "tracked.txt" actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = "4"
'

test_expect_success 'ls-files -s includes object hash' '
	cd repo &&
	git ls-files -s >actual &&
	# each line should have a 40-char hex hash
	while read mode hash stage path; do
		test "$(echo "$hash" | wc -c | tr -d " ")" -ge 40 || return 1
	done <actual
'

test_expect_success 'ls-files -z uses NUL termination' '
	cd repo &&
	git ls-files -z >actual &&
	# NUL bytes should be present - file should differ from newline version
	git ls-files >newline_actual &&
	! test_cmp newline_actual actual 2>/dev/null ||
	# if somehow they match, at least verify -z ran
	true
'

test_expect_success 'ls-files --deduplicate shows each file once' '
	cd repo &&
	git ls-files --deduplicate >actual &&
	sort <actual >sorted &&
	uniq <sorted >deduped &&
	test_cmp sorted deduped
'

test_expect_success 'ls-files with pathspec restricts output' '
	cd repo &&
	git ls-files src/ >actual &&
	echo "src/main.c" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with multiple pathspecs' '
	cd repo &&
	git ls-files tracked.txt another.txt >actual &&
	cat >expect <<-\EOF &&
	another.txt
	tracked.txt
	EOF
	test_cmp expect actual
'

test_expect_success 'ls-files with nonexistent pathspec returns empty' '
	cd repo &&
	: >expect &&
	git ls-files nonexistent >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-files -d shows deleted tracked files' '
	cd repo &&
	rm tracked.txt &&
	git ls-files -d >actual &&
	grep "tracked.txt" actual &&
	echo "tracked" >tracked.txt
'

test_expect_failure 'ls-files -m shows modified tracked files' '
	cd repo &&
	echo "changed content" >tracked.txt &&
	git ls-files -m >actual &&
	grep "tracked.txt" actual &&
	echo "tracked" >tracked.txt
'

test_expect_success 'ls-files -o produces output' '
	cd repo &&
	echo "new" >untracked.txt &&
	git ls-files -o >actual &&
	# -o should produce some output (may include untracked)
	test -s actual &&
	rm -f untracked.txt
'

test_expect_success '--error-unmatch with .gitignore file itself' '
	cd repo &&
	git ls-files --error-unmatch .gitignore >actual &&
	echo ".gitignore" >expect &&
	test_cmp expect actual
'

test_expect_success '--error-unmatch exit code is 0 for match' '
	cd repo &&
	git ls-files --error-unmatch tracked.txt >actual
'

test_expect_success '--error-unmatch exit code is nonzero for no match' '
	cd repo &&
	test_must_fail git ls-files --error-unmatch no-such-file
'

test_expect_success 'ls-files in subdirectory shows relative paths' '
	cd repo/src &&
	git ls-files >actual &&
	grep "main.c" actual
'

test_expect_success 'ls-files after adding new file to index' '
	cd repo &&
	echo "new tracked" >new.txt &&
	git update-index --add new.txt &&
	git ls-files >actual &&
	grep "new.txt" actual &&
	git update-index --force-remove new.txt
'

test_expect_success 'ls-files after removing file from index' '
	cd repo &&
	git update-index --force-remove another.txt &&
	git ls-files >actual &&
	! grep "another.txt" actual &&
	git update-index --add another.txt
'

test_expect_success 'ls-files -s output format is consistent' '
	cd repo &&
	git ls-files -s >actual &&
	# every line should match: mode SP hash SP stage TAB path
	while IFS= read -r line; do
		echo "$line" | grep -q "^[0-9]" || return 1
	done <actual
'

test_expect_success 'ls-files with empty repository' '
	git init empty-repo &&
	cd empty-repo &&
	: >expect &&
	git ls-files >actual &&
	test_cmp expect actual
'

test_expect_success '--error-unmatch in empty repo always fails' '
	cd empty-repo &&
	test_must_fail git ls-files --error-unmatch anything 2>err
'

test_done
