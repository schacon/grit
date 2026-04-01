#!/bin/sh
# Ported from git/t/t1006-cat-file.sh (harness-compatible subset).

test_description='grit cat-file'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

echo_without_newline() {
	printf '%s' "$*"
}

strlen() {
	echo_without_newline "$1" | wc -c | sed -e 's/^ *//'
}

hello_content="Hello World"
hello_size=$(strlen "$hello_content")

test_expect_success 'setup repository and blob fixture' '
	grit init repo &&
	cd repo &&
	echo_without_newline "$hello_content" >hello &&
	hello_oid=$(grit hash-object -w hello) &&
	echo "$hello_oid" >../hello_oid
'

test_expect_success 'cat-file -e confirms blob exists' '
	cd repo &&
	grit cat-file -e "$(cat ../hello_oid)"
'

test_expect_success 'cat-file -t reports blob type' '
	cd repo &&
	echo blob >expect &&
	grit cat-file -t "$(cat ../hello_oid)" >actual &&
	test_cmp expect actual
'

test_expect_success 'cat-file -s reports blob size' '
	cd repo &&
	echo "$hello_size" >expect &&
	grit cat-file -s "$(cat ../hello_oid)" >actual &&
	test_cmp expect actual
'

test_expect_success 'cat-file default and -p print blob bytes' '
	cd repo &&
	echo_without_newline "$hello_content" >expect &&
	grit cat-file "$(cat ../hello_oid)" >actual &&
	test_cmp expect actual &&
	grit cat-file -p "$(cat ../hello_oid)" >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check default format works for blob' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "$oid blob $hello_size" >expect &&
	echo "$oid" | grit cat-file --batch-check >actual &&
	test_cmp expect actual
'

test_expect_success 'batch default format includes content for blob' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	{
		echo "$oid blob $hello_size" &&
		echo_without_newline "$hello_content"
		echo
	} >expect &&
	echo "$oid" | grit cat-file --batch >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check custom format supports %(objecttype) %(objectname)' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "blob $oid" >expect &&
	echo "$oid" | grit cat-file --batch-check="%(objecttype) %(objectname)" >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check custom format supports %(rest)' '
	cd repo &&
	echo "blob trailing words" >expect &&
	printf "%s\n" "$(cat ../hello_oid)   trailing words" |
		grit cat-file --batch-check="%(objecttype) %(rest)" >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command info works with --no-buffer' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "$oid blob $hello_size" >expect &&
	printf "info %s\n" "$oid" | grit cat-file --batch-command --no-buffer >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command contents works with --no-buffer' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	{
		echo "$oid blob $hello_size" &&
		echo_without_newline "$hello_content"
		echo
	} >expect &&
	printf "contents %s\n" "$oid" | grit cat-file --batch-command --no-buffer >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command --buffer flushes output' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "$oid blob $hello_size" >expect &&
	{
		printf "info %s\n" "$oid"
		echo flush
	} | grit cat-file --batch-command --buffer >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command rejects flush without --buffer' '
	cd repo &&
	test_must_fail sh -c "echo flush | grit cat-file --batch-command --no-buffer"
'

test_expect_success 'batch-check reports missing objects' '
	cd repo &&
	cat >expect <<-\EOF &&
	deadbeef missing
	EOF
	echo deadbeef | grit cat-file --batch-check >actual &&
	test_cmp expect actual
'

test_expect_success 'create tree and commit fixtures' '
	cd repo &&
	grit update-index --add hello &&
	tree_oid=$(grit write-tree) &&
	echo "$tree_oid" >../tree_oid &&
	commit_oid=$(echo "Initial commit" | grit commit-tree "$tree_oid") &&
	echo "$commit_oid" >../commit_oid
'

test_expect_success 'cat-file -p pretty-prints tree entries' '
	cd repo &&
	hello_oid="$(cat ../hello_oid)" &&
	cat >expect <<-EOF &&
	100644 blob $hello_oid	hello
	EOF
	grit cat-file -p "$(cat ../tree_oid)" >actual &&
	test_cmp expect actual
'

test_expect_success 'cat-file -p prints commit payload' '
	cd repo &&
	grit cat-file -p "$(cat ../commit_oid)" >actual &&
	grep "^tree " actual
'

# ---- Tree object extended tests ----

test_expect_success 'cat-file -e confirms tree exists' '
	cd repo &&
	grit cat-file -e "$(cat ../tree_oid)"
'

test_expect_success 'cat-file -t reports tree type' '
	cd repo &&
	echo tree >expect &&
	grit cat-file -t "$(cat ../tree_oid)" >actual &&
	test_cmp expect actual
'

test_expect_success 'cat-file -s reports tree size' '
	cd repo &&
	tree_oid="$(cat ../tree_oid)" &&
	grit cat-file -s "$tree_oid" >actual &&
	test_line_count = 1 actual
'

test_expect_success 'batch-check default format works for tree' '
	cd repo &&
	tree_oid="$(cat ../tree_oid)" &&
	tree_size=$(grit cat-file -s "$tree_oid") &&
	echo "$tree_oid tree $tree_size" >expect &&
	echo "$tree_oid" | grit cat-file --batch-check >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check custom format for tree' '
	cd repo &&
	tree_oid="$(cat ../tree_oid)" &&
	echo "tree $tree_oid" >expect &&
	echo "$tree_oid" | grit cat-file --batch-check="%(objecttype) %(objectname)" >actual &&
	test_cmp expect actual
'

# ---- Commit object extended tests ----

test_expect_success 'cat-file -e confirms commit exists' '
	cd repo &&
	grit cat-file -e "$(cat ../commit_oid)"
'

test_expect_success 'cat-file -t reports commit type' '
	cd repo &&
	echo commit >expect &&
	grit cat-file -t "$(cat ../commit_oid)" >actual &&
	test_cmp expect actual
'

test_expect_success 'cat-file -s reports commit size' '
	cd repo &&
	commit_oid="$(cat ../commit_oid)" &&
	grit cat-file -s "$commit_oid" >actual &&
	test_line_count = 1 actual
'

test_expect_success 'batch-check default format works for commit' '
	cd repo &&
	commit_oid="$(cat ../commit_oid)" &&
	commit_size=$(grit cat-file -s "$commit_oid") &&
	echo "$commit_oid commit $commit_size" >expect &&
	echo "$commit_oid" | grit cat-file --batch-check >actual &&
	test_cmp expect actual
'

test_expect_success 'batch default format includes content for commit' '
	cd repo &&
	commit_oid="$(cat ../commit_oid)" &&
	commit_size=$(grit cat-file -s "$commit_oid") &&
	{
		echo "$commit_oid commit $commit_size" &&
		grit cat-file -p "$commit_oid" &&
		echo
	} >expect &&
	echo "$commit_oid" | grit cat-file --batch >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command info works for commit' '
	cd repo &&
	commit_oid="$(cat ../commit_oid)" &&
	commit_size=$(grit cat-file -s "$commit_oid") &&
	echo "$commit_oid commit $commit_size" >expect &&
	printf "info %s\n" "$commit_oid" | grit cat-file --batch-command --no-buffer >actual &&
	test_cmp expect actual
'

# ---- Tag object fixture and tests ----

test_expect_success 'setup tag fixture' '
	cd repo &&
	hello_oid="$(cat ../hello_oid)" &&
	printf "object %s\ntype blob\ntag testtag\ntagger Test User <test@example.com> 0 +0000\n\nThis is a test tag" \
		"$hello_oid" >../tag_content_file &&
	tag_oid=$(grit hash-object -t tag --stdin -w <../tag_content_file) &&
	echo "$tag_oid" >../tag_oid
'

test_expect_success 'cat-file -e confirms tag exists' '
	cd repo &&
	grit cat-file -e "$(cat ../tag_oid)"
'

test_expect_success 'cat-file -t reports tag type' '
	cd repo &&
	echo tag >expect &&
	grit cat-file -t "$(cat ../tag_oid)" >actual &&
	test_cmp expect actual
'

test_expect_success 'cat-file -s reports tag size' '
	cd repo &&
	tag_oid="$(cat ../tag_oid)" &&
	tag_size=$(grit cat-file -s "$tag_oid") &&
	strlen "$(cat ../tag_content_file)" >expect &&
	echo "$tag_size" >actual &&
	test_cmp expect actual
'

test_expect_success 'cat-file -p prints tag content' '
	cd repo &&
	cp ../tag_content_file expect &&
	grit cat-file -p "$(cat ../tag_oid)" >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check default format works for tag' '
	cd repo &&
	tag_oid="$(cat ../tag_oid)" &&
	tag_size=$(grit cat-file -s "$tag_oid") &&
	echo "$tag_oid tag $tag_size" >expect &&
	echo "$tag_oid" | grit cat-file --batch-check >actual &&
	test_cmp expect actual
'

test_expect_success 'batch default format includes content for tag' '
	cd repo &&
	tag_oid="$(cat ../tag_oid)" &&
	tag_size=$(grit cat-file -s "$tag_oid") &&
	{
		echo "$tag_oid tag $tag_size" &&
		grit cat-file -p "$tag_oid" &&
		echo
	} >expect &&
	echo "$tag_oid" | grit cat-file --batch >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command info works for tag' '
	cd repo &&
	tag_oid="$(cat ../tag_oid)" &&
	tag_size=$(grit cat-file -s "$tag_oid") &&
	echo "$tag_oid tag $tag_size" >expect &&
	printf "info %s\n" "$tag_oid" | grit cat-file --batch-command --no-buffer >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command contents works for tag' '
	cd repo &&
	tag_oid="$(cat ../tag_oid)" &&
	tag_size=$(grit cat-file -s "$tag_oid") &&
	{
		echo "$tag_oid tag $tag_size" &&
		grit cat-file -p "$tag_oid" &&
		echo
	} >expect &&
	printf "contents %s\n" "$tag_oid" | grit cat-file --batch-command --no-buffer >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check custom format for tag' '
	cd repo &&
	tag_oid="$(cat ../tag_oid)" &&
	echo "tag $tag_oid" >expect &&
	echo "$tag_oid" | grit cat-file --batch-check="%(objecttype) %(objectname)" >actual &&
	test_cmp expect actual
'

# ---- Missing / non-existent object tests ----

test_expect_success 'batch-check for multiple non-existent named objects' '
	cd repo &&
	cat >expect <<-\EOF &&
	foobar42 missing
	foobar84 missing
	EOF
	printf "foobar42\nfoobar84" >in &&
	grit cat-file --batch-check <in >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check for multiple non-existent OIDs' '
	cd repo &&
	cat >expect <<-\EOF &&
	0000000000000000000000000000000000000042 missing
	0000000000000000000000000000000000000084 missing
	EOF
	printf "0000000000000000000000000000000000000042\n0000000000000000000000000000000000000084" >in &&
	grit cat-file --batch-check <in >actual &&
	test_cmp expect actual
'

test_expect_success 'batch for existent blob and non-existent OID' '
	cd repo &&
	blob_oid="$(cat ../hello_oid)" &&
	{
		echo "$blob_oid blob $hello_size" &&
		echo_without_newline "$hello_content" &&
		echo &&
		echo "0000000000000000000000000000000000000000 missing"
	} >expect &&
	printf "%s\n%s" "$blob_oid" "0000000000000000000000000000000000000000" >in &&
	grit cat-file --batch <in >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check empty line gives missing' '
	cd repo &&
	printf " missing\n" >expect &&
	echo >in &&
	grit cat-file --batch-check <in >actual &&
	test_cmp expect actual
'

# ---- Multi-object batch tests ----

test_expect_success 'batch-check with multiple objects of different types' '
	cd repo &&
	blob_oid="$(cat ../hello_oid)" &&
	tree_oid="$(cat ../tree_oid)" &&
	commit_oid="$(cat ../commit_oid)" &&
	tag_oid="$(cat ../tag_oid)" &&
	tree_size=$(grit cat-file -s "$tree_oid") &&
	commit_size=$(grit cat-file -s "$commit_oid") &&
	tag_size=$(grit cat-file -s "$tag_oid") &&
	{
		echo "$blob_oid blob $hello_size"
		echo "$tree_oid tree $tree_size"
		echo "$commit_oid commit $commit_size"
		echo "$tag_oid tag $tag_size"
		echo "deadbeef missing"
	} >expect &&
	printf "%s\n%s\n%s\n%s\ndeadbeef" \
		"$blob_oid" "$tree_oid" "$commit_oid" "$tag_oid" >in &&
	grit cat-file --batch-check <in >actual &&
	test_cmp expect actual
'

test_expect_success 'batch with multiple objects of different types' '
	cd repo &&
	blob_oid="$(cat ../hello_oid)" &&
	tag_oid="$(cat ../tag_oid)" &&
	tag_size=$(grit cat-file -s "$tag_oid") &&
	{
		echo "$blob_oid blob $hello_size" &&
		echo_without_newline "$hello_content" &&
		echo &&
		echo "$tag_oid tag $tag_size" &&
		grit cat-file -p "$tag_oid" &&
		echo
	} >expect &&
	printf "%s\n%s" "$blob_oid" "$tag_oid" >in &&
	grit cat-file --batch <in >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command multiple info calls' '
	cd repo &&
	blob_oid="$(cat ../hello_oid)" &&
	tree_oid="$(cat ../tree_oid)" &&
	commit_oid="$(cat ../commit_oid)" &&
	tree_size=$(grit cat-file -s "$tree_oid") &&
	commit_size=$(grit cat-file -s "$commit_oid") &&
	{
		echo "$blob_oid blob $hello_size"
		echo "$tree_oid tree $tree_size"
		echo "$commit_oid commit $commit_size"
		echo "deadbeef missing"
	} >expect &&
	printf "info %s\ninfo %s\ninfo %s\ninfo deadbeef\n" \
		"$blob_oid" "$tree_oid" "$commit_oid" |
		grit cat-file --batch-command --no-buffer >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command multiple contents calls' '
	cd repo &&
	blob_oid="$(cat ../hello_oid)" &&
	tag_oid="$(cat ../tag_oid)" &&
	tag_size=$(grit cat-file -s "$tag_oid") &&
	{
		echo "$blob_oid blob $hello_size" &&
		echo_without_newline "$hello_content" &&
		echo &&
		echo "$tag_oid tag $tag_size" &&
		grit cat-file -p "$tag_oid" &&
		echo &&
		echo "deadbeef missing"
	} >expect &&
	printf "contents %s\ncontents %s\ncontents deadbeef\n" \
		"$blob_oid" "$tag_oid" |
		grit cat-file --batch-command --no-buffer >actual &&
	test_cmp expect actual
'

# ---- Additional custom batch-check format tests ----

test_expect_success 'batch-check %(objectsize) format for blob' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "$hello_size" >expect &&
	echo "$oid" | grit cat-file --batch-check="%(objectsize)" >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check %(objecttype) %(objectsize) format for commit' '
	cd repo &&
	commit_oid="$(cat ../commit_oid)" &&
	commit_size=$(grit cat-file -s "$commit_oid") &&
	echo "commit $commit_size" >expect &&
	echo "$commit_oid" | grit cat-file --batch-check="%(objecttype) %(objectsize)" >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check %(objectname) %(objecttype) format for tree' '
	cd repo &&
	tree_oid="$(cat ../tree_oid)" &&
	echo "$tree_oid tree" >expect &&
	echo "$tree_oid" | grit cat-file --batch-check="%(objectname) %(objecttype)" >actual &&
	test_cmp expect actual
'

# ---- Error handling tests ----

test_expect_success 'cat-file -t fails for missing full OID' '
	cd repo &&
	test_must_fail grit cat-file -t 0000000000000000000000000000000000000001
'

test_expect_success 'cat-file -p fails for missing full OID' '
	cd repo &&
	test_must_fail grit cat-file -p 0000000000000000000000000000000000000001
'

test_expect_success 'cat-file -s fails for missing full OID' '
	cd repo &&
	test_must_fail grit cat-file -s 0000000000000000000000000000000000000001
'

test_expect_success 'cat-file -e fails for missing full OID' '
	cd repo &&
	test_must_fail grit cat-file -e 0000000000000000000000000000000000000001
'

# ---- Incompatible flag combination tests ----

test_expect_success 'usage: cmdmode -e -p' '
	test_must_fail grit cat-file -e -p
'

test_expect_success 'usage: cmdmode -p -t' '
	test_must_fail grit cat-file -p -t
'

test_expect_success 'usage: cmdmode -t -s' '
	test_must_fail grit cat-file -t -s
'

test_expect_success 'usage: cmdmode -s -e' '
	test_must_fail grit cat-file -s -e
'

# ---- Batch flags incompatible with single-object flags ----

test_expect_success 'passing -t with --batch fails' '
	cd repo &&
	test_must_fail grit cat-file --batch -t "$(cat ../hello_oid)"
'

test_expect_success 'passing --batch with -t fails' '
	cd repo &&
	test_must_fail grit cat-file -t --batch "$(cat ../hello_oid)"
'

test_expect_success 'passing -s with --batch fails' '
	cd repo &&
	test_must_fail grit cat-file --batch -s "$(cat ../hello_oid)"
'

test_expect_success 'passing --batch with -s fails' '
	cd repo &&
	test_must_fail grit cat-file -s --batch "$(cat ../hello_oid)"
'

test_expect_success 'passing -e with --batch fails' '
	cd repo &&
	test_must_fail grit cat-file --batch -e "$(cat ../hello_oid)"
'

test_expect_success 'passing --batch with -e fails' '
	cd repo &&
	test_must_fail grit cat-file -e --batch "$(cat ../hello_oid)"
'

test_expect_success 'passing -p with --batch fails' '
	cd repo &&
	test_must_fail grit cat-file --batch -p "$(cat ../hello_oid)"
'

test_expect_success 'passing --batch with -p fails' '
	cd repo &&
	test_must_fail grit cat-file -p --batch "$(cat ../hello_oid)"
'

test_expect_success 'passing -t with --batch-check fails' '
	cd repo &&
	test_must_fail grit cat-file --batch-check -t "$(cat ../hello_oid)"
'

test_expect_success 'passing --batch-check with -t fails' '
	cd repo &&
	test_must_fail grit cat-file -t --batch-check "$(cat ../hello_oid)"
'

test_expect_success 'passing -s with --batch-check fails' '
	cd repo &&
	test_must_fail grit cat-file --batch-check -s "$(cat ../hello_oid)"
'

test_expect_success 'passing --batch-check with -s fails' '
	cd repo &&
	test_must_fail grit cat-file -s --batch-check "$(cat ../hello_oid)"
'

test_expect_success 'passing -e with --batch-check fails' '
	cd repo &&
	test_must_fail grit cat-file --batch-check -e "$(cat ../hello_oid)"
'

test_expect_success 'passing --batch-check with -e fails' '
	cd repo &&
	test_must_fail grit cat-file -e --batch-check "$(cat ../hello_oid)"
'

test_expect_success 'passing -p with --batch-check fails' '
	cd repo &&
	test_must_fail grit cat-file --batch-check -p "$(cat ../hello_oid)"
'

test_expect_success 'passing --batch-check with -p fails' '
	cd repo &&
	test_must_fail grit cat-file -p --batch-check "$(cat ../hello_oid)"
'

test_expect_success 'passing -t with --batch-command fails' '
	cd repo &&
	test_must_fail grit cat-file --batch-command -t "$(cat ../hello_oid)"
'

test_expect_success 'passing --batch-command with -t fails' '
	cd repo &&
	test_must_fail grit cat-file -t --batch-command "$(cat ../hello_oid)"
'

test_expect_success 'passing -s with --batch-command fails' '
	cd repo &&
	test_must_fail grit cat-file --batch-command -s "$(cat ../hello_oid)"
'

test_expect_success 'passing --batch-command with -s fails' '
	cd repo &&
	test_must_fail grit cat-file -s --batch-command "$(cat ../hello_oid)"
'

test_expect_success 'passing -e with --batch-command fails' '
	cd repo &&
	test_must_fail grit cat-file --batch-command -e "$(cat ../hello_oid)"
'

test_expect_success 'passing --batch-command with -e fails' '
	cd repo &&
	test_must_fail grit cat-file -e --batch-command "$(cat ../hello_oid)"
'

test_expect_success 'passing -p with --batch-command fails' '
	cd repo &&
	test_must_fail grit cat-file --batch-command -p "$(cat ../hello_oid)"
'

test_expect_success 'passing --batch-command with -p fails' '
	cd repo &&
	test_must_fail grit cat-file -p --batch-command "$(cat ../hello_oid)"
'

test_expect_success 'passing --batch with --batch-check fails' '
	cd repo &&
	test_must_fail grit cat-file --batch --batch-check
'

test_expect_success 'passing --batch-check with --batch fails' '
	cd repo &&
	test_must_fail grit cat-file --batch-check --batch
'

# ---- cat-file <type> <oid> positional form ----

test_expect_success 'cat-file blob <oid> prints blob contents' '
	cd repo &&
	echo_without_newline "$hello_content" >expect &&
	grit cat-file blob "$(cat ../hello_oid)" >actual &&
	test_cmp expect actual
'

test_expect_success 'cat-file <type> with wrong type fails' '
	cd repo &&
	test_must_fail grit cat-file commit "$(cat ../hello_oid)"
'

test_expect_success 'cat-file tree with wrong type fails' '
	cd repo &&
	test_must_fail grit cat-file tree "$(cat ../hello_oid)"
'

test_expect_success 'cat-file tag with wrong type fails' '
	cd repo &&
	test_must_fail grit cat-file tag "$(cat ../hello_oid)"
'

test_expect_success 'cat-file commit <oid> prints commit contents' '
	cd repo &&
	commit_oid="$(cat ../commit_oid)" &&
	grit cat-file commit "$commit_oid" >actual &&
	grep "^tree " actual
'

# ---- Empty --batch-check format ----

test_expect_success 'empty --batch-check notices missing object' '
	cd repo &&
	echo "0000000000000000000000000000000000000000 missing" >expect &&
	echo "0000000000000000000000000000000000000000" | grit cat-file --batch-check="" >actual &&
	test_cmp expect actual
'

test_expect_success 'empty --batch-check shows default for existing object' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "$oid blob $hello_size" >expect &&
	echo "$oid" | grit cat-file --batch-check="" >actual &&
	test_cmp expect actual
'

# ---- batch-check with %(rest) and %(objectsize) combined ----

test_expect_success 'batch-check custom format %(objecttype) %(objectsize) %(rest)' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "blob $hello_size extra words" >expect &&
	printf "%s   extra words\n" "$oid" |
		grit cat-file --batch-check="%(objecttype) %(objectsize) %(rest)" >actual &&
	test_cmp expect actual
'

# ---- Batch-check with no trailing newline on stdin ----

test_expect_success 'batch-check reads input without trailing newline' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "$oid blob $hello_size" >expect &&
	printf "%s" "$oid" | grit cat-file --batch-check >actual &&
	test_cmp expect actual
'

# ---- Batch with duplicate OIDs ----

test_expect_success 'batch-check handles duplicate OIDs' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	{
		echo "$oid blob $hello_size" &&
		echo "$oid blob $hello_size"
	} >expect &&
	printf "%s\n%s\n" "$oid" "$oid" | grit cat-file --batch-check >actual &&
	test_cmp expect actual
'

test_expect_success 'batch handles duplicate OIDs' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	{
		echo "$oid blob $hello_size" &&
		echo_without_newline "$hello_content" &&
		echo &&
		echo "$oid blob $hello_size" &&
		echo_without_newline "$hello_content" &&
		echo
	} >expect &&
	printf "%s\n%s\n" "$oid" "$oid" | grit cat-file --batch >actual &&
	test_cmp expect actual
'

# ---- Batch-command --buffer with interleaved info and contents ----

test_expect_success 'batch-command --buffer interleaves info and contents' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	{
		echo "$oid blob $hello_size" &&
		echo "$oid blob $hello_size" &&
		echo_without_newline "$hello_content" &&
		echo
	} >expect &&
	printf "info %s\ncontents %s\nflush\n" "$oid" "$oid" |
		grit cat-file --batch-command --buffer >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command --buffer multiple flush cycles' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	{
		echo "$oid blob $hello_size" &&
		echo "deadbeef missing"
	} >expect &&
	printf "info %s\nflush\ninfo deadbeef\nflush\n" "$oid" |
		grit cat-file --batch-command --buffer >actual &&
	test_cmp expect actual
'

# ---- Batch-command error cases ----

test_expect_success 'batch-command rejects empty command' '
	cd repo &&
	test_must_fail sh -c "echo \"\" | grit cat-file --batch-command"
'

test_expect_success 'batch-command rejects unknown command' '
	cd repo &&
	test_must_fail sh -c "echo unknown_command | grit cat-file --batch-command"
'

test_expect_success 'batch-command flush without --buffer fails' '
	cd repo &&
	test_must_fail sh -c "echo flush | grit cat-file --batch-command"
'

test_expect_success 'batch-command flush without --buffer (--no-buffer) fails' '
	cd repo &&
	test_must_fail sh -c "echo flush | grit cat-file --batch-command --no-buffer"
'

# ---- Batch-check and batch with ref names ----

test_expect_success 'setup branch for ref resolution tests' '
	cd repo &&
	commit_oid="$(cat ../commit_oid)" &&
	grit update-ref refs/heads/master "$commit_oid"
'

test_expect_success 'batch-check resolves HEAD' '
	cd repo &&
	commit_oid="$(cat ../commit_oid)" &&
	commit_size=$(grit cat-file -s "$commit_oid") &&
	echo "$commit_oid commit $commit_size" >expect &&
	echo "HEAD" | grit cat-file --batch-check >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check resolves refs/heads/master' '
	cd repo &&
	commit_oid="$(cat ../commit_oid)" &&
	commit_size=$(grit cat-file -s "$commit_oid") &&
	echo "$commit_oid commit $commit_size" >expect &&
	echo "refs/heads/master" | grit cat-file --batch-check >actual &&
	test_cmp expect actual
'

test_expect_success 'batch resolves HEAD and prints content' '
	cd repo &&
	commit_oid="$(cat ../commit_oid)" &&
	commit_size=$(grit cat-file -s "$commit_oid") &&
	{
		echo "$commit_oid commit $commit_size" &&
		grit cat-file -p "$commit_oid" &&
		echo
	} >expect &&
	echo "HEAD" | grit cat-file --batch >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check reports non-existent ref as missing' '
	cd repo &&
	echo "refs/heads/nonexistent missing" >expect &&
	echo "refs/heads/nonexistent" | grit cat-file --batch-check >actual &&
	test_cmp expect actual
'

# ---- Batch-command with ref names ----

test_expect_success 'batch-command info resolves HEAD' '
	cd repo &&
	commit_oid="$(cat ../commit_oid)" &&
	commit_size=$(grit cat-file -s "$commit_oid") &&
	echo "$commit_oid commit $commit_size" >expect &&
	printf "info HEAD\n" | grit cat-file --batch-command --no-buffer >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command contents resolves HEAD' '
	cd repo &&
	commit_oid="$(cat ../commit_oid)" &&
	commit_size=$(grit cat-file -s "$commit_oid") &&
	{
		echo "$commit_oid commit $commit_size" &&
		grit cat-file -p "$commit_oid" &&
		echo
	} >expect &&
	printf "contents HEAD\n" | grit cat-file --batch-command --no-buffer >actual &&
	test_cmp expect actual
'

# ---- Additional batch format tokens ----

test_expect_success 'batch-check %(objectsize) alone' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "$hello_size" >expect &&
	echo "$oid" | grit cat-file --batch-check="%(objectsize)" >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check %(objectname) alone' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "$oid" >expect &&
	echo "$oid" | grit cat-file --batch-check="%(objectname)" >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check %(objecttype) alone' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "blob" >expect &&
	echo "$oid" | grit cat-file --batch-check="%(objecttype)" >actual &&
	test_cmp expect actual
'

# ---- Multi-object batch with all four types and interleaved missing ----

test_expect_success 'batch with all four types plus missing' '
	cd repo &&
	blob_oid="$(cat ../hello_oid)" &&
	tree_oid="$(cat ../tree_oid)" &&
	commit_oid="$(cat ../commit_oid)" &&
	tag_oid="$(cat ../tag_oid)" &&
	{
		echo "$blob_oid blob $hello_size" &&
		echo_without_newline "$hello_content" &&
		echo &&
		echo "deadbeef missing" &&
		echo "$tag_oid tag $(grit cat-file -s $tag_oid)" &&
		grit cat-file -p "$tag_oid" &&
		echo
	} >expect &&
	printf "%s\ndeadbeef\n%s\n" "$blob_oid" "$tag_oid" |
		grit cat-file --batch >actual &&
	test_cmp expect actual
'

# ---- Batch-command contents for missing returns missing ----

test_expect_success 'batch-command contents for missing returns missing' '
	cd repo &&
	echo "deadbeef missing" >expect &&
	printf "contents deadbeef\n" | grit cat-file --batch-command --no-buffer >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command info for missing returns missing' '
	cd repo &&
	echo "deadbeef missing" >expect &&
	printf "info deadbeef\n" | grit cat-file --batch-command --no-buffer >actual &&
	test_cmp expect actual
'

# ---- Batch with empty input (just newline) ----

test_expect_success 'batch reports missing for empty line' '
	cd repo &&
	printf " missing\n" >expect &&
	echo | grit cat-file --batch >actual &&
	test_cmp expect actual
'

# ---- cat-file without any flag or type requires an argument ----

test_expect_success 'cat-file with no arguments fails' '
	test_must_fail grit cat-file
'

# ---- Too many positional arguments ----

test_expect_success 'too many arguments to cat-file -t' '
	test_must_fail grit cat-file -t one two three
'

test_expect_success 'too many arguments to cat-file -s' '
	test_must_fail grit cat-file -s one two three
'

test_expect_success 'too many arguments to cat-file -p' '
	test_must_fail grit cat-file -p one two three
'

test_expect_success 'too many arguments to cat-file -e' '
	test_must_fail grit cat-file -e one two three
'

test_done
