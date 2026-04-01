#!/bin/sh
test_description='Return value of diffs (grit diff --quiet / --exit-code)'

. ./test-lib.sh

test_expect_success 'setup' '
    git init repo &&
    cd repo &&
    git config user.name "Test User" &&
    git config user.email "test@test.com" &&
    echo 1 >a &&
    git add a &&
    git commit -m first &&
    echo 2 >b &&
    git add b &&
    git commit -m second
'

test_expect_success 'git diff --quiet is clean after second commit' '
    cd repo &&
    test_expect_code 0 git diff --quiet
'

test_expect_success 'git diff --exit-code is clean' '
    cd repo &&
    test_expect_code 0 git diff --exit-code
'

test_expect_success 'git diff --cached --quiet is clean' '
    cd repo &&
    test_expect_code 0 git diff --cached --quiet
'

test_expect_success 'git diff --staged --quiet is clean' '
    cd repo &&
    test_expect_code 0 git diff --staged --quiet
'

test_expect_success 'modify file and check diff --quiet detects change' '
    cd repo &&
    echo modified >a &&
    test_expect_code 1 git diff --quiet
'

test_expect_success 'git diff --exit-code detects unstaged change' '
    cd repo &&
    echo modified >a &&
    test_expect_code 1 git diff --exit-code
'

test_expect_success 'unstaged change does not affect diff --cached' '
    cd repo &&
    echo modified >a &&
    test_expect_code 0 git diff --cached --quiet
'

test_expect_success 'stage the change and check diff --cached detects it' '
    cd repo &&
    echo modified >a &&
    git add a &&
    test_expect_code 1 git diff --cached --quiet
'

test_expect_success 'diff --quiet suppresses output' '
    cd repo &&
    echo modified2 >a &&
    git diff --quiet >out 2>&1 || true &&
    test_must_be_empty out
'

test_expect_success 'diff --cached --quiet suppresses output' '
    cd repo &&
    echo modified2 >a &&
    git add a &&
    git diff --cached --quiet >out 2>&1 || true &&
    test_must_be_empty out
'

test_done
