#!/bin/sh
# Test reset behavior, including on unborn branch

test_description='grit reset on unborn branch and basic reset modes'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo with commits' '
	grit init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo "a" >a.txt &&
	grit add a.txt &&
	grit commit -m "c1" &&
	echo "b" >b.txt &&
	grit add b.txt &&
	grit commit -m "c2" &&
	echo "c" >c.txt &&
	grit add c.txt &&
	grit commit -m "c3" &&
	grit rev-parse HEAD >head_c3 &&
	grit rev-parse HEAD~1 >head_c2 &&
	grit rev-parse HEAD~2 >head_c1
'

test_expect_success 'reset --soft HEAD~1 keeps changes staged' '
	cd repo &&
	grit reset --soft HEAD~1 &&
	grit rev-parse HEAD >actual &&
	test_cmp head_c2 actual &&
	grit status >output 2>&1 &&
	grep "new file.*c.txt" output
'

test_expect_success 'reset --mixed HEAD unstages changes' '
	cd repo &&
	grit reset --mixed HEAD &&
	grit status >output 2>&1 &&
	grep -i "untracked" output &&
	grep "c.txt" output
'

test_expect_success 'reset (default) unstages added file' '
	cd repo &&
	grit add c.txt &&
	grit ls-files >before &&
	grep "c.txt" before &&
	grit reset &&
	grit ls-files >after &&
	! grep "c.txt" after
'

test_expect_success 'reset --hard HEAD discards working tree changes' '
	cd repo &&
	echo "modified" >a.txt &&
	grit reset --hard HEAD &&
	echo "a" >expect &&
	test_cmp expect a.txt
'

test_expect_success 'reset --hard moves HEAD and cleans tree' '
	cd repo &&
	grit reset --hard $(cat head_c1) &&
	grit rev-parse HEAD >actual &&
	test_cmp head_c1 actual &&
	test_path_is_file a.txt &&
	test_path_is_missing b.txt
'

test_expect_success 'reset HEAD -- file unstages single file' '
	cd repo &&
	echo "new" >new.txt &&
	grit add new.txt &&
	grit ls-files >before &&
	grep "new.txt" before &&
	grit reset HEAD -- new.txt &&
	grit ls-files >after &&
	! grep "new.txt" after
'

test_expect_success 'setup unborn repo' '
	grit init unborn &&
	cd unborn &&
	git config user.name "Test" &&
	git config user.email "test@test.com"
'

test_expect_success 'reset on unborn branch fails gracefully' '
	cd unborn &&
	echo "file" >f.txt &&
	grit add f.txt &&
	test_must_fail grit reset 2>stderr &&
	grep -i "HEAD\|unknown revision\|object not found" stderr
'

test_expect_success 'reset HEAD on unborn branch fails' '
	cd unborn &&
	test_must_fail grit reset HEAD 2>stderr &&
	grep -i "HEAD\|unknown revision\|object not found" stderr
'

test_expect_success 'index still intact after failed reset on unborn' '
	cd unborn &&
	grit ls-files >actual &&
	echo "f.txt" >expect &&
	test_cmp expect actual
'

test_done
