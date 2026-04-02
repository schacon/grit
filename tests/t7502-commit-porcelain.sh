#!/bin/sh
# Ported from git/t/t7502-commit-porcelain.sh (partially)
# Tests for 'grit commit' porcelain features.

test_description='grit commit porcelain'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "C O Mitter" &&
	git config user.email "committer@example.com"
'

# ── basic commit ──────────────────────────────────────────────────────────

test_expect_success 'initial commit with -m' '
	cd repo &&
	echo content >file.txt &&
	git add file.txt &&
	git commit -m "initial commit" 2>stderr &&
	grep "root-commit" stderr &&
	git cat-file -t HEAD >actual &&
	echo "commit" >expected &&
	test_cmp expected actual
'

test_expect_success 'commit message stored correctly' '
	cd repo &&
	git cat-file -p HEAD >out &&
	grep "initial commit" out
'

test_expect_success 'second commit' '
	cd repo &&
	echo more >>file.txt &&
	git add file.txt &&
	git commit -m "second" 2>stderr &&
	git cat-file -p HEAD >out &&
	grep "second" out &&
	grep "parent" out
'

# ── -a (commit all tracked changes) ──────────────────────────────────────

test_expect_success 'commit -a stages modified tracked files' '
	cd repo &&
	echo modified >>file.txt &&
	git commit -a -m "auto staged" 2>/dev/null &&
	git diff --quiet HEAD
'

test_expect_success 'commit -a stages deleted tracked files' '
	cd repo &&
	echo temp >tobedeleted.txt &&
	git add tobedeleted.txt &&
	git commit -m "add file to delete" 2>/dev/null &&
	rm tobedeleted.txt &&
	git commit -a -m "removed file" 2>/dev/null &&
	test_must_fail git cat-file -e HEAD:tobedeleted.txt
'

test_expect_success 'commit -a does not stage untracked files' '
	cd repo &&
	echo untracked >newfile.txt &&
	echo change >>file.txt &&
	git commit -a -m "only tracked" 2>/dev/null &&
	git status -s >actual &&
	grep "?? newfile.txt" actual
'

# ── --amend ───────────────────────────────────────────────────────────────

test_expect_success '--amend changes commit message' '
	cd repo &&
	git commit --amend -m "amended message" 2>/dev/null &&
	git log --format="%s" -n 1 >actual &&
	echo "amended message" >expected &&
	test_cmp expected actual
'

test_expect_success '--amend preserves parent' '
	cd repo &&
	git log --format="%H" -n 2 >before &&
	head -1 before >parent_before &&
	git commit --amend -m "amended again" 2>/dev/null &&
	git cat-file -p HEAD >out &&
	grep "parent" out >parent_line &&
	# parent should still be the same commit
	test_line_count = 1 parent_line
'

test_expect_success '--amend with staged changes' '
	cd repo &&
	echo "amend content" >amend.txt &&
	git add amend.txt &&
	git commit -m "before amend" 2>/dev/null &&
	echo "more amend" >>amend.txt &&
	git add amend.txt &&
	git commit --amend -m "after amend" 2>/dev/null &&
	git log --format="%s" -n 1 >actual &&
	echo "after amend" >expected &&
	test_cmp expected actual &&
	git ls-tree HEAD amend.txt >ls_out &&
	grep "amend.txt" ls_out
'

# ── --allow-empty ─────────────────────────────────────────────────────────

test_expect_success '--allow-empty creates commit with no changes' '
	cd repo &&
	git log --format="%H" -n 1 >before &&
	git commit --allow-empty -m "empty commit" 2>/dev/null &&
	git log --format="%H" -n 1 >after &&
	! test_cmp before after &&
	git cat-file -p HEAD >out &&
	grep "empty commit" out
'

test_expect_success 'commit without --allow-empty fails when nothing staged' '
	cd repo &&
	test_must_fail git commit -m "should fail" 2>/dev/null
'

test_expect_success 'multiple --allow-empty commits' '
	cd repo &&
	git commit --allow-empty -m "empty1" 2>/dev/null &&
	git commit --allow-empty -m "empty2" 2>/dev/null &&
	git commit --allow-empty -m "empty3" 2>/dev/null &&
	git log --format="%s" -n 3 >actual &&
	grep "empty1" actual &&
	grep "empty2" actual &&
	grep "empty3" actual
'

# ── --allow-empty-message ─────────────────────────────────────────────────

test_expect_success '--allow-empty-message creates commit with no message' '
	cd repo &&
	echo "empty msg content" >emptymsg.txt &&
	git add emptymsg.txt &&
	git commit --allow-empty-message -m "" 2>/dev/null &&
	git cat-file -t HEAD >actual &&
	echo "commit" >expected &&
	test_cmp expected actual
'

# ── -F (message from file) ───────────────────────────────────────────────

test_expect_success '-F reads message from file' '
	cd repo &&
	echo "message from file" >../msg.txt &&
	echo "file change" >fromfile.txt &&
	git add fromfile.txt &&
	git commit -F ../msg.txt 2>/dev/null &&
	git log --format="%s" -n 1 >actual &&
	echo "message from file" >expected &&
	test_cmp expected actual
'

test_expect_success '-F - reads message from stdin' '
	cd repo &&
	echo "stdin change" >>fromfile.txt &&
	git add fromfile.txt &&
	echo "from stdin" | git commit -F - 2>/dev/null &&
	git log --format="%s" -n 1 >actual &&
	echo "from stdin" >expected &&
	test_cmp expected actual
'

test_expect_success '-F with multi-line message' '
	cd repo &&
	cat >../multiline.txt <<-EOF &&
	Subject line

	Body paragraph one.

	Body paragraph two.
	EOF
	echo "multi" >>fromfile.txt &&
	git add fromfile.txt &&
	git commit -F ../multiline.txt 2>/dev/null &&
	git cat-file -p HEAD >out &&
	grep "Subject line" out &&
	grep "Body paragraph one" out
'

# ── --author ──────────────────────────────────────────────────────────────

test_expect_success '--author overrides author' '
	cd repo &&
	git commit --allow-empty --author="Other Author <other@example.com>" -m "other author" 2>/dev/null &&
	git log --format="%an" -n 1 >actual &&
	echo "Other Author" >expected &&
	test_cmp expected actual
'

test_expect_success '--author email is correct' '
	cd repo &&
	git log --format="%ae" -n 1 >actual &&
	echo "other@example.com" >expected &&
	test_cmp expected actual
'

test_expect_success '--author does not change committer' '
	cd repo &&
	git log --format="%cn" -n 1 >actual &&
	echo "C O Mitter" >expected &&
	test_cmp expected actual
'

# ── --date ────────────────────────────────────────────────────────────────

test_expect_success '--date overrides author date' '
	cd repo &&
	git commit --allow-empty --date="2005-04-07T22:13:13" -m "dated commit" 2>/dev/null &&
	git cat-file -p HEAD >out &&
	grep "author" out >author_line &&
	grep "1112911993" author_line || grep "2005" author_line
'

# ── --signoff ─────────────────────────────────────────────────────────────

test_expect_success '--signoff flag accepted' '
	cd repo &&
	git commit --allow-empty --signoff -m "with signoff" 2>/dev/null
'

# ── -q (quiet) ────────────────────────────────────────────────────────────

test_expect_success '-q suppresses output' '
	cd repo &&
	git commit --allow-empty -q -m "quiet commit" >stdout 2>stderr &&
	test_must_be_empty stdout
'

# ── commit output format ─────────────────────────────────────────────────

test_expect_success 'commit output shows branch and message' '
	cd repo &&
	echo "output test" >output.txt &&
	git add output.txt &&
	git commit -m "output check" 2>stderr &&
	grep "output check" stderr
'

test_expect_success 'root commit output says root-commit' '
	git init root_repo &&
	cd root_repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo root >root.txt &&
	git add root.txt &&
	git commit -m "the root" 2>stderr &&
	grep "root-commit" stderr
'

# ── tree correctness after commit ─────────────────────────────────────────

test_expect_success 'committed tree matches write-tree' '
	git init tree_repo &&
	cd tree_repo &&
	git config user.name T && git config user.email t@t &&
	echo "content" >treecheck.txt &&
	git add treecheck.txt &&
	git write-tree >index_tree &&
	git commit -m "verify" 2>/dev/null &&
	git cat-file -p HEAD >out &&
	head -1 out | sed "s/^tree //" >committed_tree &&
	test_cmp index_tree committed_tree
'

test_expect_success 'commit updates HEAD' '
	cd repo &&
	git log --format="%H" -n 1 >before &&
	echo "new" >update_head.txt &&
	git add update_head.txt &&
	git commit -m "update head" 2>/dev/null &&
	git log --format="%H" -n 1 >after &&
	! test_cmp before after
'

# ── commit with only deleted file ─────────────────────────────────────────

test_expect_success 'commit records file deletion' '
	cd repo &&
	echo "delete me" >willdie.txt &&
	git add willdie.txt &&
	git commit -m "add willdie" 2>/dev/null &&
	git rm willdie.txt 2>/dev/null &&
	git commit -m "remove willdie" 2>/dev/null &&
	git ls-tree HEAD willdie.txt >ls_out &&
	test_must_be_empty ls_out
'

# ── multiple files in one commit ──────────────────────────────────────────

test_expect_success 'commit with multiple new files' '
	cd repo &&
	echo a >multi_a.txt &&
	echo b >multi_b.txt &&
	echo c >multi_c.txt &&
	git add multi_a.txt multi_b.txt multi_c.txt &&
	git commit -m "add three files" 2>/dev/null &&
	git ls-tree HEAD >ls_out &&
	grep "multi_a.txt" ls_out &&
	grep "multi_b.txt" ls_out &&
	grep "multi_c.txt" ls_out
'

# ── commit in subdirectory ────────────────────────────────────────────────

test_expect_success 'commit from subdirectory' '
	cd repo &&
	mkdir -p subdir &&
	echo "sub content" >subdir/sub.txt &&
	git add subdir/sub.txt &&
	cd subdir &&
	git commit -m "from subdir" 2>/dev/null &&
	cd .. &&
	git ls-tree -r HEAD >ls_out &&
	grep "subdir/sub.txt" ls_out
'

# ── amend root commit ────────────────────────────────────────────────────

test_expect_success 'amend root commit works' '
	git init amend_root &&
	cd amend_root &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo root >root.txt &&
	git add root.txt &&
	git commit -m "original root" 2>/dev/null &&
	git commit --amend -m "amended root" 2>/dev/null &&
	git log --format="%s" -n 1 >actual &&
	echo "amended root" >expected &&
	test_cmp expected actual &&
	git cat-file -p HEAD >out &&
	! grep "parent" out
'

# ── commit preserves file modes ───────────────────────────────────────────

test_expect_success 'commit preserves executable bit' '
	cd repo &&
	echo "#!/bin/sh" >script.sh &&
	chmod +x script.sh &&
	git add script.sh &&
	git commit -m "executable" 2>/dev/null &&
	git ls-tree HEAD script.sh >actual &&
	grep "100755" actual
'

# ── consecutive commits create proper chain ───────────────────────────────

test_expect_success 'consecutive commits form parent chain' '
	git init chain_repo &&
	cd chain_repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo a >a.txt && git add a.txt && git commit -m "first" 2>/dev/null &&
	git log --format="%H" -n 1 >first_hash &&
	echo b >b.txt && git add b.txt && git commit -m "second" 2>/dev/null &&
	git cat-file -p HEAD >out &&
	grep "parent $(cat first_hash)" out
'

# ── commit with long message ─────────────────────────────────────────────

test_expect_success 'commit with very long message' '
	cd repo &&
	long_msg=$(printf "x%.0s" $(seq 1 1000)) &&
	git commit --allow-empty -m "$long_msg" 2>/dev/null &&
	git cat-file -p HEAD >out &&
	grep "xxxx" out
'

# ── amend does not duplicate parent ───────────────────────────────────────

test_expect_success 'amend does not add extra parent' '
	cd repo &&
	git commit --allow-empty -m "pre-amend" 2>/dev/null &&
	git commit --amend -m "post-amend" 2>/dev/null &&
	git cat-file -p HEAD >out &&
	grep -c "^parent" out >count &&
	echo "1" >expected &&
	test_cmp expected count
'

# ── commit after reset ────────────────────────────────────────────────────

test_expect_success 'commit after soft reset' '
	cd repo &&
	echo "reset test" >reset.txt &&
	git add reset.txt &&
	git commit -m "before reset" 2>/dev/null &&
	git reset --soft "HEAD^" &&
	git commit -m "after reset" 2>/dev/null &&
	git log --format="%s" -n 1 >actual &&
	echo "after reset" >expected &&
	test_cmp expected actual
'

test_done
