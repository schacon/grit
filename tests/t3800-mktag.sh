#!/bin/sh
# Tests for 'grit mktag'.
# Ported from git/t/t3800-mktag.sh
#
# Note: tests that require `test-tool ref-store`, `git for-each-ref
# --format="%(*objectname)"`, or `git fast-export` are skipped — those
# depend on Git internals not yet ported.

test_description='grit mktag: tag object validate and create'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

###########################################################
# Initialize repository at script level so $head is available
# for top-level tag.sig heredocs.

git init .
git config user.name "Test User"
git config user.email "test@example.com"
test_commit A
test_commit B
head=$(git rev-parse --verify HEAD)
head_parent=$(git rev-parse --verify HEAD~)
tree=$(git rev-parse "HEAD^{tree}")
blob=$(git rev-parse --verify HEAD:B.t)
export head head_parent tree blob

###########################################################
# check_verify_failure SUBJECT [--no-strict] [--fsck-obj-ok]
#
# Expects 'tag.sig' in TRASH_DIRECTORY to be a file that mktag rejects.
# --no-strict: strict mode fails, non-strict mode succeeds.
# (no --no-strict): both strict and non-strict fail.
# --fsck-obj-ok: ignored (test-tool not yet ported).

check_verify_failure() {
	subject=$1
	shift

	no_strict=
	while test $# != 0; do
		case "$1" in
		--no-strict)   no_strict=yes ;;
		--fsck-obj-ok) ;; # ignored
		esac
		shift
	done

	test_expect_success "fail in strict mode: $subject" '
		test_must_fail git mktag <tag.sig
	'

	if test -n "$no_strict"; then
		test_expect_success "succeed with --no-strict: $subject" '
			git mktag --no-strict <tag.sig
		'
	else
		test_expect_success "fail with --no-strict too: $subject" '
			test_must_fail git mktag --no-strict <tag.sig
		'
	fi
}

test_expect_mktag_success() {
	test_expect_success "$1" '
		git hash-object -t tag -w --stdin <tag.sig >expected &&
		git mktag <tag.sig >hash &&
		test_cmp expected hash
	'
}

###########################################################
# basic usage

test_expect_success 'basic usage' '
	cat >tag.sig <<-EOF &&
	object $head
	type commit
	tag mytag
	tagger T A Gger <tagger@example.com> 1206478233 -0500
	EOF
	git mktag <tag.sig &&
	git mktag --no-strict <tag.sig
'

test_expect_success 'unknown option is rejected' '
	test_must_fail git mktag --unknown-option 2>/dev/null
'

###########################################################
#  1. length check

cat >tag.sig <<EOF
too short for a tag
EOF

check_verify_failure 'Tag object length check'

###########################################################
#  2. object line label check

cat >tag.sig <<EOF
xxxxxx $head
type tag
tag mytag
tagger . <> 0 +0000

EOF

check_verify_failure '"object" line label check'

###########################################################
#  3. object line hash check

cat >tag.sig <<EOF
object $(echo "$head" | tr 0-9a-f z)
type tag
tag mytag
tagger . <> 0 +0000

EOF

check_verify_failure '"object" line hash check'

###########################################################
#  4. type line label check

cat >tag.sig <<EOF
object $head
xxxx tag
tag mytag
tagger . <> 0 +0000

EOF

check_verify_failure '"type" line label check'

###########################################################
#  5. type line eol check (no trailing newline on type line)

printf 'object %s\ntype tagsssssssssssssssssssssssssssssss' "$head" >tag.sig

check_verify_failure '"type" line eol check'

###########################################################
#  6. tag line label check

cat >tag.sig <<EOF
object $head
type tag
xxx mytag
tagger . <> 0 +0000

EOF

check_verify_failure '"tag" line label check'

###########################################################
#  7. type line type-name invalid

cat >tag.sig <<EOF
object $head
type taggggggggggggggggggggggggggggggg
tag mytag
EOF

check_verify_failure '"type" line type-name length check'

###########################################################
#  9. verify object (hash/type) checks

cat >tag.sig <<EOF
object 0000000000000000000000000000000000000001
type tag
tag mytag
tagger . <> 0 +0000

EOF

check_verify_failure 'verify object -- correct type, nonexisting object' \
	--fsck-obj-ok

cat >tag.sig <<EOF
object $head
type tagggg
tag mytag
tagger . <> 0 +0000

EOF

check_verify_failure 'verify object -- made-up type, valid object'

cat >tag.sig <<EOF
object $head
type tree
tag mytag
tagger . <> 0 +0000

EOF

check_verify_failure 'verify object -- mismatched type, valid object' \
	--fsck-obj-ok

###########################################################
# 10. verify tag-name check (tab in name)

printf 'object %s\ntype commit\ntag my\ttag\ntagger . <> 0 +0000\n\n' \
	"$head" >tag.sig

check_verify_failure 'verify tag-name check (tab in name)' \
	--no-strict \
	--fsck-obj-ok

###########################################################
# 11. tagger line missing

cat >tag.sig <<EOF
object $head
type commit
tag mytag

This is filler
EOF

check_verify_failure '"tagger" line label check (missing)' \
	--no-strict \
	--fsck-obj-ok

###########################################################
# 12. tagger line with no value

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger

This is filler
EOF

check_verify_failure '"tagger" line label check (empty value)' \
	--no-strict \
	--fsck-obj-ok

###########################################################
# 13. allow missing tag author name

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger  <> 0 +0000

This is filler
EOF

test_expect_mktag_success 'allow missing tag author name'

###########################################################
# 14. disallow malformed tagger (unclosed email angle bracket)

printf 'object %s\ntype commit\ntag mytag\ntagger T A Gger <\n> 0 +0000\n\n' \
	"$head" >tag.sig

check_verify_failure 'disallow malformed tagger' \
	--no-strict \
	--fsck-obj-ok

###########################################################
# 15. allow empty tag email

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger T A Gger <> 0 +0000

EOF

test_expect_mktag_success 'allow empty tag email'

###########################################################
# 16. allow spaces in tag email

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger T A Gger <tag ger@example.com> 0 +0000

EOF

test_expect_mktag_success 'allow spaces in tag email like fsck'

###########################################################
# 17. disallow missing tag timestamp

printf 'object %s\ntype commit\ntag mytag\ntagger T A Gger <tagger@example.com>  \n\n' \
	"$head" >tag.sig

check_verify_failure 'disallow missing tag timestamp'

###########################################################
# 18. detect invalid tag timestamp (human-readable date)

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger T A Gger <tagger@example.com> Tue Mar 25 15:47:44 2008

EOF

check_verify_failure 'detect invalid tag timestamp1'

###########################################################
# 19. detect invalid tag timestamp (ISO 8601)

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger T A Gger <tagger@example.com> 2008-03-31T12:20:15-0500

EOF

check_verify_failure 'detect invalid tag timestamp2'

###########################################################
# 20. detect invalid tag timezone (word)

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger T A Gger <tagger@example.com> 1206478233 GMT

EOF

check_verify_failure 'detect invalid tag timezone1'

###########################################################
# 21. detect invalid tag timezone (spaces inside)

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger T A Gger <tagger@example.com> 1206478233 +  30

EOF

check_verify_failure 'detect invalid tag timezone2'

###########################################################
# 22. allow out-of-range timezone (no range enforcement)

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger T A Gger <tagger@example.com> 1206478233 -1430

EOF

test_expect_mktag_success 'allow out-of-range timezone'

###########################################################
# 23. detect extra header entry

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger T A Gger <tagger@example.com> 1206478233 -0500
this line should not be here

EOF

check_verify_failure 'detect invalid header entry' \
	--no-strict \
	--fsck-obj-ok

test_expect_success 'extraHeaderEntry: strict fails, no-strict succeeds' '
	test_must_fail git mktag <tag.sig &&
	git mktag --no-strict <tag.sig
'

###########################################################
# Extra newlines / body format

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger T A Gger <tagger@example.com> 1206478233 -0500


this line comes after an extra newline
EOF

test_expect_mktag_success 'allow extra newlines at start of body'

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger T A Gger <tagger@example.com> 1206478233 -0500

EOF

test_expect_mktag_success 'allow a blank line before an empty body'

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger T A Gger <tagger@example.com> 1206478233 -0500
EOF

test_expect_mktag_success 'allow no blank line before an empty body'

###########################################################
# 24. create valid tag

cat >tag.sig <<EOF
object $head
type commit
tag mytag
tagger T A Gger <tagger@example.com> 1206478233 -0500
EOF

test_expect_mktag_success 'create valid tag object'

test_done
