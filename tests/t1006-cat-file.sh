#!/bin/sh
# Ported from git/t/t1006-cat-file.sh (harness-compatible subset).

test_description='gust cat-file'

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
	gust init repo &&
	cd repo &&
	echo_without_newline "$hello_content" >hello &&
	hello_oid=$(gust hash-object -w hello) &&
	echo "$hello_oid" >../hello_oid
'

test_expect_success 'cat-file -e confirms blob exists' '
	cd repo &&
	gust cat-file -e "$(cat ../hello_oid)"
'

test_expect_success 'cat-file -t reports blob type' '
	cd repo &&
	echo blob >expect &&
	gust cat-file -t "$(cat ../hello_oid)" >actual &&
	test_cmp expect actual
'

test_expect_success 'cat-file -s reports blob size' '
	cd repo &&
	echo "$hello_size" >expect &&
	gust cat-file -s "$(cat ../hello_oid)" >actual &&
	test_cmp expect actual
'

test_expect_success 'cat-file default and -p print blob bytes' '
	cd repo &&
	echo_without_newline "$hello_content" >expect &&
	gust cat-file "$(cat ../hello_oid)" >actual &&
	test_cmp expect actual &&
	gust cat-file -p "$(cat ../hello_oid)" >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check default format works for blob' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "$oid blob $hello_size" >expect &&
	echo "$oid" | gust cat-file --batch-check >actual &&
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
	echo "$oid" | gust cat-file --batch >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check custom format supports %(objecttype) %(objectname)' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "blob $oid" >expect &&
	echo "$oid" | gust cat-file --batch-check="%(objecttype) %(objectname)" >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-check custom format supports %(rest)' '
	cd repo &&
	echo "blob trailing words" >expect &&
	printf "%s\n" "$(cat ../hello_oid)   trailing words" |
		gust cat-file --batch-check="%(objecttype) %(rest)" >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command info works with --no-buffer' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "$oid blob $hello_size" >expect &&
	printf "info %s\n" "$oid" | gust cat-file --batch-command --no-buffer >actual &&
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
	printf "contents %s\n" "$oid" | gust cat-file --batch-command --no-buffer >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command --buffer flushes output' '
	cd repo &&
	oid="$(cat ../hello_oid)" &&
	echo "$oid blob $hello_size" >expect &&
	{
		printf "info %s\n" "$oid"
		echo flush
	} | gust cat-file --batch-command --buffer >actual &&
	test_cmp expect actual
'

test_expect_success 'batch-command rejects flush without --buffer' '
	cd repo &&
	test_must_fail sh -c "echo flush | gust cat-file --batch-command --no-buffer"
'

test_expect_success 'batch-check reports missing objects' '
	cd repo &&
	cat >expect <<-\EOF &&
	deadbeef missing
	EOF
	echo deadbeef | gust cat-file --batch-check >actual &&
	test_cmp expect actual
'

test_expect_success 'create tree and commit fixtures' '
	cd repo &&
	gust update-index --add hello &&
	tree_oid=$(gust write-tree) &&
	echo "$tree_oid" >../tree_oid &&
	commit_oid=$(echo "Initial commit" | gust commit-tree "$tree_oid") &&
	echo "$commit_oid" >../commit_oid
'

test_expect_success 'cat-file -p pretty-prints tree entries' '
	cd repo &&
	hello_oid="$(cat ../hello_oid)" &&
	cat >expect <<-EOF &&
	100644 blob $hello_oid	hello
	EOF
	gust cat-file -p "$(cat ../tree_oid)" >actual &&
	test_cmp expect actual
'

test_expect_success 'cat-file -p prints commit payload' '
	cd repo &&
	gust cat-file -p "$(cat ../commit_oid)" >actual &&
	grep "^tree " actual
'

test_done
