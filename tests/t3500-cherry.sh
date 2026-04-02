#!/bin/sh
#
# Ported from git/t/t3500-cherry.sh
# Tests for 'grit cherry' — patch-id-based commit equivalence detection.

test_description='grit cherry should detect patches integrated upstream

This test cherry-picks one local change of two into main branch, and
checks that grit cherry only returns the second patch in the local branch
'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

GIT_AUTHOR_EMAIL=bogus_email_address
export GIT_AUTHOR_EMAIL

test_expect_success 'setup repository' '
    git init -b main . &&
    git config user.name "Test User" &&
    git config user.email "bogus@example.com"
'

test_expect_success 'prepare repository with topic branch, and check cherry finds the 2 patches from there' '
    echo First > A &&
    git update-index --add A &&
    test_tick &&
    git commit -m "Add A." &&

    git checkout -b my-topic-branch &&

    echo Second > B &&
    git update-index --add B &&
    test_tick &&
    git commit -m "Add B." &&

    echo AnotherSecond > C &&
    git update-index --add C &&
    test_tick &&
    git commit -m "Add C." &&

    git checkout -f main &&
    rm -f B C &&

    echo Third >> A &&
    git update-index A &&
    test_tick &&
    git commit -m "Modify A." &&

    expr "$(echo $(git cherry main my-topic-branch) )" : "+ [^ ]* + .*"
'

test_expect_success 'check that cherry with limit returns only the top patch' '
    expr "$(echo $(git cherry main my-topic-branch my-topic-branch^1) )" : "+ [^ ]*"
'

test_expect_success 'cherry-pick one of the 2 patches, and check cherry recognized one and only one as new' '
    git cherry-pick my-topic-branch^0 &&
    expr "$(echo $(git cherry main my-topic-branch) )" : "+ [^ ]* - .*"
'

test_expect_success 'cherry ignores whitespace' '
    mkdir whitespace-test &&
    cd whitespace-test &&
    git init . &&
    git config user.name "Test User" &&
    git config user.email "bogus@example.com" &&

    git switch --orphan upstream-with-space &&

    echo base > base.txt &&
    git add base.txt &&
    test_tick &&
    git commit -m "initial" &&

    git switch --create feature-without-space &&

    printf "space" > file &&
    git add file &&
    test_tick &&
    git commit -m "file without space" &&
    feat_spaceless=$(git log --format="%h" -n 1) &&

    echo more > more.txt &&
    git add more.txt &&
    test_tick &&
    git commit -m "change" &&
    feat_change=$(git log --format="%h" -n 1) &&

    git switch upstream-with-space &&
    printf "s p a c e" > file &&
    git add file &&
    test_tick &&
    git commit -m "file with space" &&

    printf -- "- %s\n+ %s\n" "$feat_spaceless" "$feat_change" > expect &&
    git cherry upstream-with-space feature-without-space > actual &&
    test_cmp expect actual
'

test_expect_success 'cherry with no upstream argument fails' '
    test_must_fail git cherry 2>err
'

test_expect_success 'cherry with identical branches shows all minus' '
    cd whitespace-test &&
    git switch upstream-with-space &&
    git cherry upstream-with-space upstream-with-space >actual &&
    test_must_be_empty actual
'

test_expect_success 'cherry shows + for commits not in upstream' '
    cd whitespace-test &&
    git cherry upstream-with-space feature-without-space >actual &&
    grep "^+" actual
'

test_expect_success 'cherry shows - for cherry-picked equivalent commits' '
    cd whitespace-test &&
    git cherry upstream-with-space feature-without-space >actual &&
    grep "^-" actual
'

test_expect_success 'cherry output has correct number of lines' '
    cd whitespace-test &&
    git cherry upstream-with-space feature-without-space >actual &&
    test_line_count = 2 actual
'

test_expect_success 'cherry output lines start with + or -' '
    cd whitespace-test &&
    git cherry upstream-with-space feature-without-space >actual &&
    ! grep -v "^[+-]" actual
'

test_expect_success 'cherry with limit restricts output' '
    git cherry main my-topic-branch my-topic-branch~1 >actual &&
    test_line_count = 1 actual
'

test_expect_success 'cherry with nonexistent branch fails' '
    test_must_fail git cherry main nonexistent-branch 2>err
'

test_expect_success 'cherry between same commit is empty' '
    git cherry main main >actual &&
    test_must_be_empty actual
'

test_expect_success 'cherry each output line contains a commit hash' '
    git cherry main my-topic-branch >actual &&
    while read sign hash; do
        echo "$hash" | grep "^[0-9a-f]\{7,\}$" || return 1
    done <actual
'

test_done
