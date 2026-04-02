#!/bin/sh
# Tests for ls-files output formatting: --stage, --long, -z, pathspecs, combos.

test_description='grit ls-files output formatting'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ===========================================================================
# Setup
# ===========================================================================

test_expect_success 'setup: init repo with multiple files' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo "aaa" >afile.txt &&
	echo "bbb" >bfile.txt &&
	mkdir -p dir &&
	echo "ccc" >dir/cfile.txt &&
	echo "#!/bin/sh" >exec.sh &&
	chmod +x exec.sh &&
	git add afile.txt bfile.txt dir/cfile.txt exec.sh &&
	git commit -m "initial commit"
'

# ===========================================================================
# Basic ls-files (cached, default)
# ===========================================================================

test_expect_success 'ls-files lists all tracked files' '
	cd repo &&
	git ls-files >actual &&
	cat >expect <<-\EOF &&
	afile.txt
	bfile.txt
	dir/cfile.txt
	exec.sh
	EOF
	test_cmp expect actual
'

test_expect_success 'ls-files --cached is same as default' '
	cd repo &&
	git ls-files >default_out &&
	git ls-files --cached >cached_out &&
	test_cmp default_out cached_out
'

test_expect_success 'ls-files -c is same as --cached' '
	cd repo &&
	git ls-files -c >short_out &&
	git ls-files --cached >long_out &&
	test_cmp short_out long_out
'

test_expect_success 'ls-files output is sorted alphabetically' '
	cd repo &&
	git ls-files >actual &&
	sort actual >sorted &&
	test_cmp sorted actual
'

test_expect_success 'ls-files includes files in subdirectories' '
	cd repo &&
	git ls-files >actual &&
	grep "dir/cfile.txt" actual
'

# ===========================================================================
# --stage output format
# ===========================================================================

test_expect_success 'ls-files --stage shows mode, oid, stage, filename' '
	cd repo &&
	git ls-files --stage >actual &&
	grep "^100644 [0-9a-f]\{40\} 0	afile.txt$" actual &&
	grep "^100644 [0-9a-f]\{40\} 0	bfile.txt$" actual &&
	grep "^100644 [0-9a-f]\{40\} 0	dir/cfile.txt$" actual &&
	grep "^100755 [0-9a-f]\{40\} 0	exec.sh$" actual
'

test_expect_success 'ls-files --stage shows correct OID for known content' '
	cd repo &&
	expected_oid=$(git hash-object afile.txt) &&
	git ls-files --stage afile.txt >actual &&
	grep "$expected_oid" actual
'

test_expect_success 'ls-files -s is equivalent to --stage' '
	cd repo &&
	git ls-files -s >short_out &&
	git ls-files --stage >long_out &&
	test_cmp short_out long_out
'

test_expect_success 'ls-files --stage shows stage 0 for non-conflicted files' '
	cd repo &&
	git ls-files --stage >actual &&
	! grep -v " 0	" actual
'

test_expect_success 'ls-files --stage output has tab before filename' '
	cd repo &&
	git ls-files --stage >actual &&
	while IFS= read -r line; do
		echo "$line" | grep "	" || return 1
	done <actual
'

test_expect_success 'ls-files --stage shows 100755 for executable' '
	cd repo &&
	git ls-files --stage exec.sh >actual &&
	grep "^100755" actual
'

test_expect_success 'ls-files --stage shows 100644 for regular file' '
	cd repo &&
	git ls-files --stage afile.txt >actual &&
	grep "^100644" actual
'

# ===========================================================================
# --long format
# ===========================================================================

test_expect_success 'ls-files --long produces output' '
	cd repo &&
	git ls-files --long >actual &&
	test -s actual
'

test_expect_success 'ls-files --long includes all tracked files' '
	cd repo &&
	git ls-files --long >actual &&
	grep "afile.txt" actual &&
	grep "exec.sh" actual
'

# ===========================================================================
# -z null termination
# ===========================================================================

test_expect_success 'ls-files -z uses null byte as separator' '
	cd repo &&
	git ls-files -z >actual &&
	count=$(tr "\0" "\n" <actual | grep -c ".") &&
	test "$count" -eq 4
'

test_expect_success 'ls-files --stage -z uses null terminator with stage info' '
	cd repo &&
	git ls-files --stage -z >actual &&
	tr "\0" "\n" <actual >converted &&
	grep "^100644" converted
'

test_expect_success 'ls-files -z does not contain newlines in entries' '
	cd repo &&
	git ls-files -z >actual &&
	lines=$(wc -l <actual) &&
	test "$lines" -eq 0 || test "$lines" -eq 1
'

# ===========================================================================
# Pathspec filtering
# ===========================================================================

test_expect_success 'ls-files with exact filename pathspec' '
	cd repo &&
	git ls-files bfile.txt >actual &&
	echo bfile.txt >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with directory pathspec' '
	cd repo &&
	git ls-files dir/ >actual &&
	echo dir/cfile.txt >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with nonexistent pathspec shows nothing' '
	cd repo &&
	git ls-files nonexistent >actual &&
	test_must_be_empty actual
'

test_expect_success 'ls-files with multiple pathspecs' '
	cd repo &&
	git ls-files afile.txt exec.sh >actual &&
	cat >expect <<-\EOF &&
	afile.txt
	exec.sh
	EOF
	test_cmp expect actual
'

test_expect_success 'ls-files --stage with pathspec limits output' '
	cd repo &&
	git ls-files --stage afile.txt >actual &&
	test_line_count = 1 actual
'

test_expect_success 'ls-files --stage with dir pathspec shows files in dir' '
	cd repo &&
	git ls-files --stage dir/ >actual &&
	test_line_count = 1 actual &&
	grep "dir/cfile.txt" actual
'

# ===========================================================================
# Content-OID relationship
# ===========================================================================

test_expect_success 'files with same content have same OID in stage output' '
	cd repo &&
	echo "same" >same1.txt &&
	echo "same" >same2.txt &&
	git add same1.txt same2.txt &&
	git ls-files --stage same1.txt >oid1 &&
	git ls-files --stage same2.txt >oid2 &&
	oid_1=$(awk "{print \$2}" oid1) &&
	oid_2=$(awk "{print \$2}" oid2) &&
	test "$oid_1" = "$oid_2"
'

test_expect_success 'files with different content have different OIDs' '
	cd repo &&
	git ls-files --stage afile.txt >oid_a &&
	git ls-files --stage bfile.txt >oid_b &&
	oid_a=$(awk "{print \$2}" oid_a) &&
	oid_b=$(awk "{print \$2}" oid_b) &&
	test "$oid_a" != "$oid_b"
'

test_expect_success 'OID in ls-files matches hash-object output' '
	cd repo &&
	hash_oid=$(git hash-object bfile.txt) &&
	git ls-files --stage bfile.txt >ls_out &&
	ls_oid=$(awk "{print \$2}" ls_out) &&
	test "$hash_oid" = "$ls_oid"
'

# ===========================================================================
# After modifications to index
# ===========================================================================

test_expect_success 'ls-files reflects newly added files' '
	cd repo &&
	echo "new" >new.txt &&
	git add new.txt &&
	git ls-files >actual &&
	grep "new.txt" actual
'

test_expect_success 'ls-files --stage reflects newly added file OID' '
	cd repo &&
	expected_oid=$(git hash-object new.txt) &&
	git ls-files --stage new.txt >actual &&
	grep "$expected_oid" actual
'

test_expect_success 'ls-files does not show removed file after git rm' '
	cd repo &&
	git rm -f new.txt &&
	git ls-files >actual &&
	! grep "new.txt" actual
'

test_expect_success 'ls-files count matches expected after add and rm' '
	cd repo &&
	git ls-files >actual &&
	test_line_count = 6 actual
'

test_done
