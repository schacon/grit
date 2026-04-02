#!/bin/sh
# Ported subset from git/t/t5301-sliding-window.sh.

test_description='verify-pack -v basic behavior on generated pack'

. ./test-lib.sh

REAL_GIT=${REAL_GIT:-/usr/bin/git}

test_expect_success 'setup packed repository fixture' '
	grit init repo &&
	cd repo &&
	echo one >one &&
	git update-index --add one &&
	tree=$(git write-tree) &&
	commit1=$(echo commit1 | git commit-tree "$tree") &&
	git update-ref HEAD "$commit1" &&
	"$REAL_GIT" repack -a -d &&
	test "$(git count-objects)" = "0 objects, 0 kilobytes" &&
	pack1=$(echo .git/objects/pack/*.pack) &&
	test_path_is_file "$pack1"
'

test_expect_success 'verify-pack -v accepts .pack path' '
	cd repo &&
	pack1=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack1" >out &&
	grep "^$pack1: ok\$" out
'

test_expect_success 'verify-pack -v accepts .idx path' '
	cd repo &&
	pack1=$(echo .git/objects/pack/*.pack) &&
	idx1=${pack1%.pack}.idx &&
	git verify-pack -v "$idx1" >out &&
	grep "^$pack1: ok\$" out
'

test_expect_success 'verify-pack -v output lists objects' '
	cd repo &&
	pack1=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack1" >out &&
	grep "^[0-9a-f]\{40\}" out
'

test_expect_success 'show-index reads pack index' '
	cd repo &&
	idx1=$(echo .git/objects/pack/*.idx) &&
	git show-index <"$idx1" >out &&
	test -s out
'

test_expect_success 'count-objects shows 0 after repack' '
	cd repo &&
	git count-objects >out &&
	grep "^0 objects" out
'

test_expect_success 'verify-pack without -v just validates' '
	cd repo &&
	pack1=$(echo .git/objects/pack/*.pack) &&
	git verify-pack "$pack1"
'

test_expect_success 'verify-pack rejects nonexistent file' '
	cd repo &&
	test_must_fail git verify-pack nonexistent.pack
'

# ---------------------------------------------------------------------------
# Deepened: multi-commit pack
# ---------------------------------------------------------------------------
test_expect_success 'setup multi-commit pack' '
	cd repo &&
	echo two >two &&
	"$REAL_GIT" update-index --add two &&
	tree2=$("$REAL_GIT" write-tree) &&
	commit2=$(echo commit2 | "$REAL_GIT" commit-tree "$tree2" -p HEAD) &&
	git update-ref HEAD "$commit2" &&
	echo three >three &&
	"$REAL_GIT" update-index --add three &&
	tree3=$("$REAL_GIT" write-tree) &&
	commit3=$(echo commit3 | "$REAL_GIT" commit-tree "$tree3" -p HEAD) &&
	git update-ref HEAD "$commit3" &&
	"$REAL_GIT" repack -a -d
'

test_expect_success 'verify-pack -v shows all objects after multi-commit' '
	cd repo &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack" >out &&
	# At least: 3 commits + 3 trees + 3 blobs = 9
	obj_count=$(grep -cE "^[0-9a-f]{40}" out) &&
	test "$obj_count" -ge 9
'

test_expect_success 'verify-pack object count matches show-index count' '
	cd repo &&
	pack=$(echo .git/objects/pack/*.pack) &&
	idx=${pack%.pack}.idx &&
	verify_count=$(git verify-pack -v "$pack" | grep -cE "^[0-9a-f]{40}") &&
	show_count=$(git show-index <"$idx" | wc -l) &&
	test "$verify_count" = "$show_count"
'

test_expect_success 'verify-pack -v output contains known commit' '
	cd repo &&
	pack=$(echo .git/objects/pack/*.pack) &&
	head_sha=$(git rev-parse HEAD) &&
	git verify-pack -v "$pack" >out &&
	grep "$head_sha" out
'

test_expect_success 'verify-pack -v output contains known tree' '
	cd repo &&
	pack=$(echo .git/objects/pack/*.pack) &&
	tree_sha=$("$REAL_GIT" rev-parse HEAD^{tree}) &&
	git verify-pack -v "$pack" >out &&
	grep "$tree_sha" out
'

test_expect_success 'verify-pack -v output contains known blob' '
	cd repo &&
	pack=$(echo .git/objects/pack/*.pack) &&
	blob_sha=$("$REAL_GIT" rev-parse HEAD:one) &&
	git verify-pack -v "$pack" >out &&
	grep "$blob_sha" out
'

test_expect_success 'count-objects shows 0 after second repack' '
	cd repo &&
	git count-objects >out &&
	grep "^0 objects" out
'

# ---------------------------------------------------------------------------
# Deepened: pack with many objects
# ---------------------------------------------------------------------------
test_expect_success 'setup large pack with many files' '
	cd repo &&
	for i in $(seq 1 25); do
		echo "content $i" >"file$i.txt"
	done &&
	"$REAL_GIT" add . &&
	tree4=$("$REAL_GIT" write-tree) &&
	commit4=$(echo "many files" | "$REAL_GIT" commit-tree "$tree4" -p HEAD) &&
	"$REAL_GIT" update-ref HEAD "$commit4" &&
	"$REAL_GIT" repack -a -d
'

test_expect_success 'verify-pack handles large pack' '
	cd repo &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack" >out &&
	grep "^$pack: ok\$" out
'

test_expect_success 'show-index and verify-pack agree on large pack' '
	cd repo &&
	pack=$(echo .git/objects/pack/*.pack) &&
	idx=${pack%.pack}.idx &&
	show_count=$(git show-index <"$idx" | wc -l) &&
	verify_count=$(git verify-pack -v "$pack" | grep -cE "^[0-9a-f]{40}") &&
	test "$show_count" = "$verify_count"
'

test_done
