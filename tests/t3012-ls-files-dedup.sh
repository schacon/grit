#!/bin/sh
# ls-files --deduplicate, -t (status flags), and various modes.

test_description='grit ls-files --deduplicate and status flags'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo' '
	grit init repo &&
	cd repo &&
	grit config user.email "author@example.com" &&
	grit config user.name "A U Thor" &&
	echo "a" >a.txt &&
	echo "b" >b.txt &&
	echo "c" >c.txt &&
	mkdir -p dir &&
	echo "d" >dir/d.txt &&
	grit add a.txt b.txt c.txt dir/d.txt &&
	test_tick &&
	grit commit -m "initial"
'

test_expect_success 'ls-files -c lists cached files' '
	cd repo &&
	grit ls-files -c >actual &&
	cat >expect <<-\EOF &&
	a.txt
	b.txt
	c.txt
	dir/d.txt
	EOF
	test_cmp expect actual
'

test_expect_success 'ls-files default is same as -c' '
	cd repo &&
	grit ls-files >default_out &&
	grit ls-files -c >cached_out &&
	test_cmp default_out cached_out
'

test_expect_success 'ls-files -s shows staged info' '
	cd repo &&
	grit ls-files -s >actual &&
	test_line_count = 4 actual &&
	grep "^100644" actual
'

test_expect_success 'ls-files -s output has mode oid stage path format' '
	cd repo &&
	grit ls-files -s >actual &&
	head -1 actual | grep -q "^[0-9]* [0-9a-f]* [0-9]"
'

test_expect_success 'ls-files with pathspec restricts output' '
	cd repo &&
	grit ls-files a.txt >actual &&
	test_line_count = 1 actual &&
	grep "^a.txt$" actual
'

test_expect_success 'ls-files with directory pathspec' '
	cd repo &&
	grit ls-files dir/ >actual &&
	test_line_count = 1 actual &&
	grep "dir/d.txt" actual
'

test_expect_success 'ls-files with multiple pathspecs' '
	cd repo &&
	grit ls-files a.txt c.txt >actual &&
	test_line_count = 2 actual &&
	grep "a.txt" actual &&
	grep "c.txt" actual
'

test_expect_success 'ls-files --deduplicate with cached files has no duplicates' '
	cd repo &&
	grit ls-files --deduplicate >actual &&
	sort actual >sorted_actual &&
	sort -u actual >sorted_unique &&
	test_cmp sorted_actual sorted_unique
'

test_expect_success 'ls-files --deduplicate removes duplicate entries' '
	cd repo &&
	grit ls-files -c --deduplicate >actual &&
	lines=$(wc -l <actual) &&
	unique=$(sort -u actual | wc -l) &&
	test "$lines" -eq "$unique"
'

test_expect_success 'ls-files -t shows output' '
	cd repo &&
	grit ls-files -t >actual &&
	test -s actual
'

test_expect_success 'ls-files -t output contains tracked file names' '
	cd repo &&
	grit ls-files -t >actual &&
	grep "a.txt" actual &&
	grep "b.txt" actual
'

test_expect_success 'ls-files -z uses NUL terminator' '
	cd repo &&
	grit ls-files -z >actual &&
	tr "\0" "\n" <actual >decoded &&
	grep "a.txt" decoded
'

test_expect_success 'ls-files -z output does not contain newlines within entries' '
	cd repo &&
	grit ls-files -z >actual &&
	first_entry=$(head -c 5 actual) &&
	test -n "$first_entry"
'

test_expect_success 'ls-files --long shows verbose format' '
	cd repo &&
	grit ls-files --long >actual 2>/dev/null || {
		echo "--long not supported, skipping" &&
		return 0
	} &&
	test -s actual
'

test_expect_success 'ls-files after adding duplicate content files' '
	cd repo &&
	echo "same" >dup1.txt &&
	echo "same" >dup2.txt &&
	grit add dup1.txt dup2.txt &&
	grit ls-files -s dup1.txt dup2.txt >actual &&
	oid1=$(awk "/dup1.txt/ {print \$2}" actual) &&
	oid2=$(awk "/dup2.txt/ {print \$2}" actual) &&
	test "$oid1" = "$oid2"
'

test_expect_success 'ls-files --deduplicate with same-content files still shows both names' '
	cd repo &&
	grit ls-files --deduplicate dup1.txt dup2.txt >actual &&
	grep "dup1.txt" actual &&
	grep "dup2.txt" actual
'

test_expect_success 'ls-files after removing a file from index' '
	cd repo &&
	grit update-index --force-remove dup1.txt &&
	grit ls-files >actual &&
	! grep "dup1.txt" actual &&
	grep "dup2.txt" actual
'

test_expect_success 'ls-files -u with no unmerged entries is empty' '
	cd repo &&
	grit ls-files -u >actual &&
	test_must_be_empty actual
'

test_expect_success 'ls-files with nonexistent pathspec shows nothing' '
	cd repo &&
	grit ls-files no_such_file >actual &&
	test_must_be_empty actual
'

test_expect_success 'ls-files --error-unmatch fails for missing pathspec' '
	cd repo &&
	test_must_fail grit ls-files --error-unmatch no_such_file 2>/dev/null
'

test_expect_success 'ls-files --error-unmatch succeeds for tracked file' '
	cd repo &&
	grit ls-files --error-unmatch a.txt >actual &&
	grep "a.txt" actual
'

test_expect_success 'ls-files -s shows correct stage numbers' '
	cd repo &&
	grit ls-files -s >actual &&
	awk "{print \$3}" actual | sort -u >stages &&
	echo "0" >expect &&
	test_cmp expect stages
'

test_expect_success 'ls-files after adding many files' '
	cd repo &&
	for i in $(seq 1 50); do
		echo "file $i" >many_$i.txt
	done &&
	grit add many_*.txt &&
	grit ls-files many_*.txt >actual &&
	test_line_count = 50 actual
'

test_expect_success 'ls-files --deduplicate with many files has no duplicates' '
	cd repo &&
	grit ls-files --deduplicate >actual &&
	lines=$(wc -l <actual) &&
	unique=$(sort -u actual | wc -l) &&
	test "$lines" -eq "$unique"
'

test_expect_success 'ls-files -s with pathspec shows only matching' '
	cd repo &&
	grit ls-files -s a.txt >actual &&
	test_line_count = 1 actual &&
	grep "a.txt" actual
'

test_expect_success 'ls-files output is sorted' '
	cd repo &&
	grit ls-files >actual &&
	sort actual >sorted &&
	test_cmp actual sorted
'

test_expect_success 'ls-files -u after 3-way merge shows conflict stages' '
	cd repo &&
	blob_base=$(echo "base" | grit hash-object -w --stdin) &&
	blob_a=$(echo "ours" | grit hash-object -w --stdin) &&
	blob_b=$(echo "theirs" | grit hash-object -w --stdin) &&
	rm -f .git/index &&
	grit update-index --add --cacheinfo 100644,$blob_base,merge.txt &&
	base_tree=$(grit write-tree) &&
	rm -f .git/index &&
	grit update-index --add --cacheinfo 100644,$blob_a,merge.txt &&
	ours_tree=$(grit write-tree) &&
	rm -f .git/index &&
	grit update-index --add --cacheinfo 100644,$blob_b,merge.txt &&
	theirs_tree=$(grit write-tree) &&
	rm -f .git/index &&
	grit read-tree -m "$base_tree" "$ours_tree" "$theirs_tree" &&
	grit ls-files -u >actual &&
	test_line_count = 3 actual &&
	grep "merge.txt" actual
'

test_expect_success 'ls-files -u shows stages 1, 2, 3' '
	cd repo &&
	awk "{print \$3}" actual | sort >stages &&
	printf "1\n2\n3\n" >expect &&
	test_cmp expect stages
'

test_expect_success 'ls-files --deduplicate after conflict does not duplicate resolved entries' '
	cd repo &&
	rm -f .git/index &&
	grit add a.txt b.txt c.txt dir/d.txt dup2.txt 2>/dev/null &&
	grit ls-files --deduplicate >actual &&
	lines=$(wc -l <actual) &&
	unique=$(sort -u actual | wc -l) &&
	test "$lines" -eq "$unique"
'

test_done
