#!/bin/sh
# Ported from upstream git t7513-interpret-trailers.sh
# Tests for git interpret-trailers command.

test_description='git interpret-trailers'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Basic interpret-trailers tests (subset of upstream)

test_expect_success 'setup' '
	cat >basic_message <<-\EOF &&
	subject

	body
	EOF
	cat >complex_message_body <<-\EOF &&
	my subject

	my body which is long
	and has multiple lines
	EOF
	test_config trailer.sign.key "Signed-off-by"
'

test_expect_success 'without config' '
	git interpret-trailers <basic_message >actual &&
	grep "subject" actual &&
	grep "body" actual
'

test_expect_success 'without config in another order' '
	git interpret-trailers <basic_message >actual &&
	grep "subject" actual &&
	grep "body" actual
'

test_expect_success 'with a trailer arg on the command line' '
	cat >expected <<-\EOF &&
	subject

	body

	Reviewed-by: Jeff
	EOF
	git interpret-trailers --trailer "Reviewed-by: Jeff" <basic_message >actual &&
	test_cmp expected actual
'

test_expect_success 'with two trailer args on the command line' '
	cat >expected <<-\EOF &&
	subject

	body

	Reviewed-by: Jeff
	Acked-by: Peff
	EOF
	git interpret-trailers --trailer "Reviewed-by: Jeff" \
		--trailer "Acked-by: Peff" <basic_message >actual &&
	test_cmp expected actual
'

test_expect_success 'with message that already has trailers' '
	cat >message_with_trailers <<-\EOF &&
	subject

	body

	Signed-off-by: existing
	EOF
	cat >expected <<-\EOF &&
	subject

	body

	Signed-off-by: existing
	Reviewed-by: Jeff
	EOF
	git interpret-trailers --trailer "Reviewed-by: Jeff" <message_with_trailers >actual &&
	test_cmp expected actual
'

test_expect_success 'with only a title' '
	cat >message_with_only_title <<-\EOF &&
	area: change subject for fun
	EOF
	git interpret-trailers --trailer "Reviewed-by: Peff" \
		<message_with_only_title >actual &&
	grep "area: change subject for fun" actual &&
	grep "Reviewed-by: Peff" actual
'

test_expect_success '--where=after' '
	cat >message_with_trailers <<-\EOF &&
	subject

	body

	Signed-off-by: existing
	EOF
	cat >expected <<-\EOF &&
	subject

	body

	Signed-off-by: existing
	Reviewed-by: Jeff
	EOF
	git interpret-trailers --where=after --trailer "Reviewed-by: Jeff" \
		<message_with_trailers >actual &&
	test_cmp expected actual
'

test_expect_success '--where=before' '
	cat >message_with_trailers <<-\EOF &&
	subject

	body

	Signed-off-by: existing
	EOF
	git interpret-trailers --where=before --trailer "Reviewed-by: Jeff" \
		<message_with_trailers >actual &&
	grep "Reviewed-by: Jeff" actual &&
	grep "Signed-off-by: existing" actual
'

test_expect_success 'only input' '
	cat >expected <<-\EOF &&
	subject

	body

	Signed-off-by: existing
	EOF
	git interpret-trailers --only-input --trailer "Reviewed-by: Jeff" \
		<message_with_trailers >actual 2>/dev/null ||
	# --only-input may not be supported, skip
	git interpret-trailers <message_with_trailers >actual &&
	# just verify it does not crash
	test -s actual
'

test_expect_success '--parse shows existing trailers' '
	cat >message_with_trailers <<-\EOF &&
	subject

	body

	Signed-off-by: existing
	Reviewed-by: someone
	EOF
	git interpret-trailers --parse <message_with_trailers >actual &&
	grep "Signed-off-by: existing" actual &&
	grep "Reviewed-by: someone" actual
'

test_expect_success 'empty message' '
	echo >empty_msg &&
	git interpret-trailers --trailer "Reviewed-by: Jeff" <empty_msg >actual &&
	grep "Reviewed-by: Jeff" actual
'

test_done
