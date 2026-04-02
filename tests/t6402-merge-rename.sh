#!/bin/sh
# Test grit's ability to inspect repositories after merges with renames.
# Uses /usr/bin/git for merge operations, grit for verification.

test_description='grit verification of merge-with-rename scenarios'

. ./test-lib.sh

# We need real git for merge operations
REAL_GIT=/usr/bin/git

test_expect_success 'setup basic rename-merge repo' '
	$REAL_GIT init repo &&
	cd repo &&
	$REAL_GIT config user.name "Test User" &&
	$REAL_GIT config user.email "test@test.com" &&
	echo "original content" >file.txt &&
	$REAL_GIT add file.txt &&
	$REAL_GIT commit -m "initial" &&
	$REAL_GIT checkout -b rename-branch &&
	mv file.txt renamed.txt &&
	$REAL_GIT add -A &&
	$REAL_GIT commit -m "rename file" &&
	$REAL_GIT checkout master &&
	echo "modified" >>file.txt &&
	$REAL_GIT add -A &&
	$REAL_GIT commit -m "modify file" &&
	$REAL_GIT merge rename-branch
'

test_expect_success 'grit ls-files shows renamed file after merge' '
	cd repo &&
	grit ls-files >actual &&
	echo "renamed.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'grit log shows merge commit' '
	cd repo &&
	grit log --oneline >actual &&
	test_line_count = 4 actual &&
	head -1 actual | grep -i "merge"
'

test_expect_success 'grit diff between merge parents shows rename' '
	cd repo &&
	grit diff HEAD~1 HEAD^2 >actual &&
	grep "renamed.txt" actual
'

test_expect_success 'grit cat-file on merged file has correct content' '
	cd repo &&
	blob=$(grit ls-files -s renamed.txt | awk "{print \$2}") &&
	grit cat-file -p "$blob" >actual &&
	printf "original content\nmodified\n" >expect &&
	test_cmp expect actual
'

test_expect_success 'setup rename in both branches (no conflict)' '
	$REAL_GIT init repo2 &&
	cd repo2 &&
	$REAL_GIT config user.name "Test User" &&
	$REAL_GIT config user.email "test@test.com" &&
	echo "aaa" >a.txt &&
	echo "bbb" >b.txt &&
	$REAL_GIT add -A &&
	$REAL_GIT commit -m "initial" &&
	$REAL_GIT checkout -b branch1 &&
	mv a.txt a-renamed.txt &&
	$REAL_GIT add -A &&
	$REAL_GIT commit -m "rename a" &&
	$REAL_GIT checkout master &&
	mv b.txt b-renamed.txt &&
	$REAL_GIT add -A &&
	$REAL_GIT commit -m "rename b" &&
	$REAL_GIT merge branch1
'

test_expect_success 'grit ls-files shows both renames after merge' '
	cd repo2 &&
	grit ls-files | sort >actual &&
	cat >expect <<-\EOF &&
	a-renamed.txt
	b-renamed.txt
	EOF
	test_cmp expect actual
'

test_expect_success 'grit log shows merge with 4 commits' '
	cd repo2 &&
	grit log --oneline >actual &&
	test_line_count = 4 actual
'

test_expect_success 'setup rename + content change merge' '
	$REAL_GIT init repo3 &&
	cd repo3 &&
	$REAL_GIT config user.name "Test User" &&
	$REAL_GIT config user.email "test@test.com" &&
	cat >code.c <<-\EOF &&
	int main() {
	    printf("hello\n");
	    return 0;
	}
	EOF
	$REAL_GIT add code.c &&
	$REAL_GIT commit -m "initial code" &&
	$REAL_GIT checkout -b refactor &&
	mv code.c main.c &&
	$REAL_GIT add -A &&
	$REAL_GIT commit -m "rename code.c to main.c" &&
	$REAL_GIT checkout master &&
	cat >code.c <<-\EOF &&
	int main() {
	    printf("hello world\n");
	    return 0;
	}
	EOF
	$REAL_GIT add code.c &&
	$REAL_GIT commit -m "update greeting" &&
	$REAL_GIT merge refactor
'

test_expect_success 'merged file has new name and updated content' '
	cd repo3 &&
	grit ls-files >actual &&
	echo "main.c" >expect &&
	test_cmp expect actual &&
	grit cat-file -p $(grit ls-files -s main.c | awk "{print \$2}") >actual_content &&
	grep "hello world" actual_content
'

test_expect_success 'grit diff shows no diff between HEAD tree and index' '
	cd repo3 &&
	grit diff-index HEAD >actual &&
	test_must_be_empty actual
'

test_expect_success 'setup rename to subdirectory merge' '
	$REAL_GIT init repo4 &&
	cd repo4 &&
	$REAL_GIT config user.name "Test User" &&
	$REAL_GIT config user.email "test@test.com" &&
	echo "data" >info.txt &&
	$REAL_GIT add info.txt &&
	$REAL_GIT commit -m "initial" &&
	$REAL_GIT checkout -b move-branch &&
	mkdir -p archive &&
	mv info.txt archive/info.txt &&
	$REAL_GIT add -A &&
	$REAL_GIT commit -m "move to archive" &&
	$REAL_GIT checkout master &&
	echo "extra data" >>info.txt &&
	$REAL_GIT add -A &&
	$REAL_GIT commit -m "update info" &&
	$REAL_GIT merge move-branch
'

test_expect_success 'grit sees file in subdirectory after merge' '
	cd repo4 &&
	grit ls-files >actual &&
	echo "archive/info.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'grit ls-tree shows archive directory' '
	cd repo4 &&
	grit ls-tree HEAD >actual &&
	grep "archive" actual
'

test_expect_success 'grit cat-file on moved file has merged content' '
	cd repo4 &&
	blob=$(grit ls-files -s archive/info.txt | awk "{print \$2}") &&
	grit cat-file -p "$blob" >actual &&
	grep "data" actual &&
	grep "extra data" actual
'

test_expect_success 'setup multiple renames in single merge' '
	$REAL_GIT init repo5 &&
	cd repo5 &&
	$REAL_GIT config user.name "Test User" &&
	$REAL_GIT config user.email "test@test.com" &&
	echo "one" >1.txt &&
	echo "two" >2.txt &&
	echo "three" >3.txt &&
	$REAL_GIT add -A &&
	$REAL_GIT commit -m "initial" &&
	$REAL_GIT checkout -b renames &&
	mv 1.txt one.txt &&
	mv 2.txt two.txt &&
	mv 3.txt three.txt &&
	$REAL_GIT add -A &&
	$REAL_GIT commit -m "rename all" &&
	$REAL_GIT checkout master &&
	echo "one modified" >1.txt &&
	echo "two modified" >2.txt &&
	echo "three modified" >3.txt &&
	$REAL_GIT add -A &&
	$REAL_GIT commit -m "modify all" &&
	$REAL_GIT merge renames
'

test_expect_success 'grit ls-files shows all three renamed files' '
	cd repo5 &&
	grit ls-files | sort >actual &&
	cat >expect <<-\EOF &&
	one.txt
	three.txt
	two.txt
	EOF
	test_cmp expect actual
'

test_expect_success 'grit verifies content of all renamed files' '
	cd repo5 &&
	blob1=$(grit ls-files -s one.txt | awk "{print \$2}") &&
	blob2=$(grit ls-files -s two.txt | awk "{print \$2}") &&
	blob3=$(grit ls-files -s three.txt | awk "{print \$2}") &&
	grit cat-file -p "$blob1" >actual1 &&
	grit cat-file -p "$blob2" >actual2 &&
	grit cat-file -p "$blob3" >actual3 &&
	grep "one modified" actual1 &&
	grep "two modified" actual2 &&
	grep "three modified" actual3
'

test_expect_success 'grit diff between original and merged shows renames' '
	cd repo5 &&
	base=$(grit log --oneline | tail -1 | awk "{print \$1}") &&
	grit diff "$base" HEAD >actual &&
	grep "one.txt" actual &&
	grep "two.txt" actual &&
	grep "three.txt" actual
'

test_expect_success 'grit log --first-parent shows merge history' '
	cd repo5 &&
	grit log --oneline --first-parent >actual &&
	head -1 actual | grep -i "merge" &&
	test_line_count = 3 actual
'

test_expect_success 'grit show-ref lists all branches' '
	cd repo5 &&
	grit show-ref >actual &&
	grep "refs/heads/master" actual &&
	grep "refs/heads/renames" actual
'

test_expect_success 'grit diff HEAD^1 HEAD^2 across merge parents' '
	cd repo5 &&
	grit diff HEAD^1 HEAD^2 >actual &&
	test -s actual
'

test_done
