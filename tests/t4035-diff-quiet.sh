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

# ---------------------------------------------------------------------------
# Additional tests from git/t/t4035-diff-quiet.sh
# ---------------------------------------------------------------------------

test_expect_success 'diff --quiet between commits detects change' '
    cd repo &&
    test_expect_code 1 git diff --quiet HEAD^ HEAD
'

test_expect_success 'diff --quiet between identical commits' '
    cd repo &&
    test_expect_code 0 git diff --quiet HEAD HEAD
'

test_expect_success 'diff --quiet HEAD^ HEAD -- a returns 0 (a unchanged)' '
    cd repo &&
    test_expect_code 0 git diff --quiet HEAD^ HEAD -- a
'

test_expect_success 'diff --quiet HEAD^ HEAD -- b returns 1 (b was added)' '
    cd repo &&
    test_expect_code 1 git diff --quiet HEAD^ HEAD -- b
'

test_expect_success 'diff-files --quiet returns 0 when clean' '
    cd repo &&
    git checkout -- . 2>/dev/null || git update-index a b &&
    git diff-files --quiet
'

test_expect_success 'diff-files --quiet returns 1 when dirty' '
    cd repo &&
    echo dirty >>b &&
    test_expect_code 1 git diff-files --quiet
'

test_expect_success 'diff-index --quiet --cached HEAD returns 0 when clean' '
    cd repo &&
    git update-index b &&
    c=$(git rev-parse HEAD) &&
    test_must_fail git diff-index --quiet --cached "$c"
'

# ---------------------------------------------------------------------------
# Additional quiet tests with a second repository
# ---------------------------------------------------------------------------

test_expect_success 'setup repo2 with two commits for additional quiet tests' '
    git init repo2 &&
    cd repo2 &&
    git config user.name "Test User" &&
    git config user.email "test@test.com" &&
    echo a1 >a &&
    echo b1 >b &&
    git add a b &&
    git commit -m first &&
    echo b2 >b &&
    git add b &&
    git commit -m second
'

test_expect_success 'diff --quiet HEAD^ HEAD detects change in second commit' '
    cd repo2 &&
    test_expect_code 1 git diff --quiet HEAD^ HEAD
'

test_expect_success 'diff --quiet HEAD^ HEAD -- a returns 0 (a not changed)' '
    cd repo2 &&
    test_expect_code 0 git diff --quiet HEAD^ HEAD -- a
'

test_expect_success 'diff --quiet HEAD^ HEAD -- b returns 1 (b changed)' '
    cd repo2 &&
    test_expect_code 1 git diff --quiet HEAD^ HEAD -- b
'

test_expect_success 'diff --exit-code HEAD HEAD returns 0' '
    cd repo2 &&
    test_expect_code 0 git diff --exit-code HEAD HEAD
'

# ---------------------------------------------------------------------------
# Additional quiet/exit-code tests
# ---------------------------------------------------------------------------

test_expect_success 'diff --quiet with pathspec on unchanged file returns 0' '
    cd repo2 &&
    test_expect_code 0 git diff --quiet HEAD^ HEAD -- a
'

test_expect_success 'diff --quiet with pathspec on changed file returns 1' '
    cd repo2 &&
    test_expect_code 1 git diff --quiet HEAD^ HEAD -- b
'

test_expect_success 'diff --exit-code with pathspec on unchanged file returns 0' '
    cd repo2 &&
    test_expect_code 0 git diff --exit-code HEAD^ HEAD -- a
'

test_expect_success 'diff --exit-code with pathspec on changed file returns 1' '
    cd repo2 &&
    test_expect_code 1 git diff --exit-code HEAD^ HEAD -- b
'

test_expect_success 'diff --quiet suppresses output between commits' '
    cd repo2 &&
    git diff --quiet HEAD^ HEAD >out 2>&1 || true &&
    test_must_be_empty out
'

test_expect_success 'setup repo3 for staged quiet tests' '
    git init repo3 &&
    cd repo3 &&
    git config user.name "Test User" &&
    git config user.email "test@test.com" &&
    echo x >x &&
    git add x &&
    git commit -m base &&
    echo y >y &&
    git add y &&
    git commit -m add-y
'

test_expect_success 'diff --cached --quiet returns 0 when index matches HEAD' '
    cd repo3 &&
    test_expect_code 0 git diff --cached --quiet
'

test_expect_success 'diff --cached --quiet returns 1 when index differs from HEAD' '
    cd repo3 &&
    echo modified >x &&
    git add x &&
    test_expect_code 1 git diff --cached --quiet
'

test_expect_success 'diff --cached --exit-code returns 1 when staged changes exist' '
    cd repo3 &&
    test_expect_code 1 git diff --cached --exit-code
'

test_expect_success 'diff --cached --quiet suppresses output' '
    cd repo3 &&
    git diff --cached --quiet >out 2>&1 || true &&
    test_must_be_empty out
'

# ---------------------------------------------------------------------------
# More quiet/exit-code edge cases
# ---------------------------------------------------------------------------

test_expect_success 'setup repo4 with multiple files' '
    git init repo4 &&
    cd repo4 &&
    git config user.name "Test User" &&
    git config user.email "test@test.com" &&
    echo one >f1 &&
    echo two >f2 &&
    echo three >f3 &&
    git add f1 f2 f3 &&
    git commit -m "three files" &&
    echo modified >f2 &&
    git add f2 &&
    git commit -m "modify f2"
'

test_expect_success 'diff --quiet HEAD^ HEAD -- f1 returns 0 (unchanged)' '
    cd repo4 &&
    test_expect_code 0 git diff --quiet HEAD^ HEAD -- f1
'

test_expect_success 'diff --quiet HEAD^ HEAD -- f2 returns 1 (changed)' '
    cd repo4 &&
    test_expect_code 1 git diff --quiet HEAD^ HEAD -- f2
'

test_expect_success 'diff --quiet HEAD^ HEAD -- f3 returns 0 (unchanged)' '
    cd repo4 &&
    test_expect_code 0 git diff --quiet HEAD^ HEAD -- f3
'

test_expect_success 'diff --exit-code HEAD^ HEAD -- f1 returns 0' '
    cd repo4 &&
    test_expect_code 0 git diff --exit-code HEAD^ HEAD -- f1
'

test_expect_success 'diff --exit-code HEAD^ HEAD -- f2 returns 1' '
    cd repo4 &&
    test_expect_code 1 git diff --exit-code HEAD^ HEAD -- f2
'

test_expect_success 'diff --quiet with unstaged change in f1' '
    cd repo4 &&
    echo dirty >f1 &&
    test_expect_code 1 git diff --quiet &&
    git checkout -- f1
'

test_expect_success 'diff --quiet returns 0 after checkout restore' '
    cd repo4 &&
    test_expect_code 0 git diff --quiet
'

test_expect_success 'diff --quiet --cached with new file staged' '
    cd repo4 &&
    echo new >f4 &&
    git add f4 &&
    test_expect_code 1 git diff --quiet --cached &&
    git reset HEAD -- f4 &&
    rm f4
'

test_expect_success 'diff --quiet --cached with deleted file staged' '
    cd repo4 &&
    git rm f3 &&
    test_expect_code 1 git diff --quiet --cached &&
    git checkout HEAD -- f3
'

test_expect_success 'diff --quiet --cached returns 0 after restoring' '
    cd repo4 &&
    test_expect_code 0 git diff --quiet --cached
'

test_expect_success 'diff --exit-code --cached with no changes returns 0' '
    cd repo4 &&
    test_expect_code 0 git diff --exit-code --cached
'

test_done
