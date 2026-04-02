#!/bin/sh
# Tests for 'grit check-ref-format'.
# Ported from git/t/t1402-check-ref-format.sh
#
# Tests that require a live git repository (@{-N} reflog lookups) are omitted.

test_description='grit check-ref-format: validate ref name rules'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── helpers ─────────────────────────────────────────────────────────────────

valid_ref() {
	local ref="$1" options="${2:-}"
	local desc
	desc="ref name '$ref' is valid${options:+ with options $options}"
	test_expect_success "$desc" "
		git check-ref-format $options '$ref'
	"
}

invalid_ref() {
	local ref="$1" options="${2:-}"
	local desc
	desc="ref name '$ref' is invalid${options:+ with options $options}"
	test_expect_success "$desc" "
		test_must_fail git check-ref-format $options '$ref'
	"
}

# ── basic validity ────────────────────────────────────────────────────────────

invalid_ref ''
valid_ref 'foo/bar/baz'
valid_ref 'foo/bar/baz' '--normalize'
invalid_ref 'refs///heads/foo'
valid_ref 'refs///heads/foo' '--normalize'
invalid_ref 'heads/foo/'
invalid_ref '///heads/foo'
valid_ref '///heads/foo' '--normalize'

# ── slash-only ref (/) ─────────────────────────────────────────────────────

invalid_ref '/'
invalid_ref '/' '--allow-onelevel'
invalid_ref '/' '--normalize'
invalid_ref '/' '--allow-onelevel --normalize'

# ── dot rules ────────────────────────────────────────────────────────────────

invalid_ref './foo'
invalid_ref './foo/bar'
invalid_ref 'foo/./bar'
invalid_ref 'foo/bar/.'
invalid_ref '.refs/foo'
invalid_ref 'refs/heads/foo.'
invalid_ref 'heads/foo..bar'
valid_ref 'foo./bar'

# ── .lock rules ──────────────────────────────────────────────────────────────

invalid_ref 'heads/foo.lock'
invalid_ref 'heads///foo.lock'
invalid_ref 'foo.lock/bar'
invalid_ref 'foo.lock///bar'

# ── special characters ───────────────────────────────────────────────────────

invalid_ref 'heads/foo?bar'
valid_ref 'heads/foo@bar'
invalid_ref 'heads/v@{ation'
invalid_ref 'heads/foo\bar'

# ── control characters ───────────────────────────────────────────────────────

test_expect_success "ref name with TAB is invalid" "
	test_must_fail git check-ref-format \"\$(printf 'heads/foo\t')\"
"

test_expect_success "ref name with DEL is invalid" "
	test_must_fail git check-ref-format \"\$(printf 'heads/foo\177')\"
"

test_expect_success "ref name with high-byte UTF-8 is valid" "
	git check-ref-format \"\$(printf 'heads/fu\303\237')\"
"

# ── refspec-pattern ──────────────────────────────────────────────────────────

valid_ref 'heads/*foo/bar' '--refspec-pattern'
valid_ref 'heads/foo*/bar' '--refspec-pattern'
valid_ref 'heads/f*o/bar' '--refspec-pattern'
invalid_ref 'heads/f*o*/bar' '--refspec-pattern'
invalid_ref 'heads/foo*/bar*' '--refspec-pattern'

# ── single-level ref ─────────────────────────────────────────────────────────

test_expect_success "ref name 'foo' is invalid by default" '
	test_must_fail git check-ref-format foo
'

test_expect_success "ref name 'foo' is valid with --allow-onelevel" '
	git check-ref-format --allow-onelevel foo
'

test_expect_success "ref name 'foo' is invalid with --refspec-pattern alone" '
	test_must_fail git check-ref-format --refspec-pattern foo
'

test_expect_success "ref name 'foo' is valid with --refspec-pattern --allow-onelevel" '
	git check-ref-format --refspec-pattern --allow-onelevel foo
'

test_expect_success "ref name 'foo' is invalid with --normalize alone" '
	test_must_fail git check-ref-format --normalize foo
'

test_expect_success "ref name 'foo' is valid with --allow-onelevel --normalize" '
	git check-ref-format --allow-onelevel --normalize foo
'

# ── two-component ref ────────────────────────────────────────────────────────

test_expect_success "ref name 'foo/bar' is valid by default" '
	git check-ref-format foo/bar
'
test_expect_success "ref name 'foo/bar' is valid with --allow-onelevel" '
	git check-ref-format --allow-onelevel foo/bar
'
test_expect_success "ref name 'foo/bar' is valid with --refspec-pattern" '
	git check-ref-format --refspec-pattern foo/bar
'
test_expect_success "ref name 'foo/bar' is valid with --refspec-pattern --allow-onelevel" '
	git check-ref-format --refspec-pattern --allow-onelevel foo/bar
'
test_expect_success "ref name 'foo/bar' is valid with --normalize" '
	git check-ref-format --normalize foo/bar
'

# ── wildcard refs ────────────────────────────────────────────────────────────

test_expect_success "ref name 'foo/*' is invalid without --refspec-pattern" '
	test_must_fail git check-ref-format foo/*
'
test_expect_success "ref name 'foo/*' is invalid with --allow-onelevel alone" '
	test_must_fail git check-ref-format --allow-onelevel foo/*
'
test_expect_success "ref name 'foo/*' is valid with --refspec-pattern" '
	git check-ref-format --refspec-pattern foo/*
'
test_expect_success "ref name 'foo/*' is valid with --refspec-pattern --allow-onelevel" '
	git check-ref-format --refspec-pattern --allow-onelevel foo/*
'

test_expect_success "ref name '*/foo' is invalid without --refspec-pattern" '
	test_must_fail git check-ref-format "*/foo"
'
test_expect_success "ref name '*/foo' is invalid with --allow-onelevel" '
	test_must_fail git check-ref-format --allow-onelevel "*/foo"
'
test_expect_success "ref name '*/foo' is valid with --refspec-pattern" '
	git check-ref-format --refspec-pattern "*/foo"
'
test_expect_success "ref name '*/foo' is valid with --refspec-pattern --allow-onelevel" '
	git check-ref-format --refspec-pattern --allow-onelevel "*/foo"
'
test_expect_success "ref name '*/foo' is invalid with --normalize without --refspec-pattern" '
	test_must_fail git check-ref-format --normalize "*/foo"
'
test_expect_success "ref name '*/foo' is valid with --refspec-pattern --normalize" '
	git check-ref-format --refspec-pattern --normalize "*/foo"
'

test_expect_success "ref name 'foo/*/bar' is invalid without --refspec-pattern" '
	test_must_fail git check-ref-format foo/*/bar
'
test_expect_success "ref name 'foo/*/bar' is invalid with --allow-onelevel without --refspec-pattern" '
	test_must_fail git check-ref-format --allow-onelevel foo/*/bar
'
test_expect_success "ref name 'foo/*/bar' is valid with --refspec-pattern" '
	git check-ref-format --refspec-pattern foo/*/bar
'
test_expect_success "ref name 'foo/*/bar' is valid with --refspec-pattern --allow-onelevel" '
	git check-ref-format --refspec-pattern --allow-onelevel foo/*/bar
'

test_expect_success "ref name '*' is invalid with --allow-onelevel" '
	test_must_fail git check-ref-format --allow-onelevel "*"
'
test_expect_success "ref name '*' is invalid with --refspec-pattern alone" '
	test_must_fail git check-ref-format --refspec-pattern "*"
'
test_expect_success "ref name '*' is valid with --refspec-pattern --allow-onelevel" '
	git check-ref-format --refspec-pattern --allow-onelevel "*"
'

test_expect_success "ref name 'foo/*/*' is invalid with --refspec-pattern" '
	test_must_fail git check-ref-format --refspec-pattern foo/*/*
'
test_expect_success "ref name 'foo/*/*' is invalid with --refspec-pattern --allow-onelevel" '
	test_must_fail git check-ref-format --refspec-pattern --allow-onelevel foo/*/*
'

test_expect_success "ref name '*/foo/*' is invalid with --refspec-pattern" '
	test_must_fail git check-ref-format --refspec-pattern "*/foo/*"
'
test_expect_success "ref name '*/foo/*' is invalid with --refspec-pattern --allow-onelevel" '
	test_must_fail git check-ref-format --refspec-pattern --allow-onelevel "*/foo/*"
'

test_expect_success "ref name '*/*/foo' is invalid with --refspec-pattern" '
	test_must_fail git check-ref-format --refspec-pattern "*/*/foo"
'
test_expect_success "ref name '*/*/foo' is invalid with --refspec-pattern --allow-onelevel" '
	test_must_fail git check-ref-format --refspec-pattern --allow-onelevel "*/*/foo"
'

# ── /foo ref (Linux-only, upstream uses !MINGW prereq) ───────────────────────

test_expect_success "ref name '/foo' is invalid by default" '
	test_must_fail git check-ref-format /foo
'
test_expect_success "ref name '/foo' is invalid with --allow-onelevel" '
	test_must_fail git check-ref-format --allow-onelevel /foo
'
test_expect_success "ref name '/foo' is invalid with --refspec-pattern" '
	test_must_fail git check-ref-format --refspec-pattern /foo
'
test_expect_success "ref name '/foo' is invalid with --refspec-pattern --allow-onelevel" '
	test_must_fail git check-ref-format --refspec-pattern --allow-onelevel /foo
'
test_expect_success "ref name '/foo' is invalid with --normalize" '
	test_must_fail git check-ref-format --normalize /foo
'
test_expect_success "ref name '/foo' is valid with --allow-onelevel --normalize" '
	git check-ref-format --allow-onelevel --normalize /foo
'
test_expect_success "ref name '/foo' is invalid with --refspec-pattern --normalize" '
	test_must_fail git check-ref-format --refspec-pattern --normalize /foo
'
test_expect_success "ref name '/foo' is valid with --refspec-pattern --allow-onelevel --normalize" '
	git check-ref-format --refspec-pattern --allow-onelevel --normalize /foo
'

# ── /heads/foo (Linux-only, upstream uses !MINGW prereq) ────────────────────

test_expect_success "ref name '/heads/foo' is invalid by default" '
	test_must_fail git check-ref-format /heads/foo
'
test_expect_success "ref name '/heads/foo' normalizes to heads/foo" '
	refname=$(git check-ref-format --normalize /heads/foo) &&
	test "$refname" = heads/foo
'

# ── normalize output ─────────────────────────────────────────────────────────

test_expect_success "normalize: 'refs///heads/foo' simplifies to 'refs/heads/foo'" '
	refname=$(git check-ref-format --normalize refs///heads/foo) &&
	test "$refname" = refs/heads/foo
'

test_expect_success "normalize: '///heads/foo' simplifies to 'heads/foo'" '
	refname=$(git check-ref-format --normalize ///heads/foo) &&
	test "$refname" = heads/foo
'

test_expect_success "normalize: '/heads/foo' simplifies to 'heads/foo'" '
	refname=$(git check-ref-format --normalize /heads/foo) &&
	test "$refname" = heads/foo
'

test_expect_success "normalize: 'foo/bar' stays 'foo/bar'" '
	refname=$(git check-ref-format --normalize foo/bar) &&
	test "$refname" = foo/bar
'

test_expect_success "normalize: 'heads/foo' stays 'heads/foo'" '
	refname=$(git check-ref-format --normalize heads/foo) &&
	test "$refname" = heads/foo
'

# ── normalize rejects invalid ────────────────────────────────────────────────

test_expect_success "normalize rejects single-level 'foo'" '
	test_must_fail git check-ref-format --normalize foo
'

test_expect_success "normalize rejects 'heads/foo/../bar'" '
	test_must_fail git check-ref-format --normalize heads/foo/../bar
'

test_expect_success "normalize rejects 'heads/./foo'" '
	test_must_fail git check-ref-format --normalize heads/./foo
'

test_expect_success "normalize rejects backslash 'heads\\foo'" '
	test_must_fail git check-ref-format --normalize "heads\\foo"
'

test_expect_success "normalize rejects 'heads/foo.lock'" '
	test_must_fail git check-ref-format --normalize heads/foo.lock
'

test_expect_success "normalize rejects 'heads///foo.lock'" '
	test_must_fail git check-ref-format --normalize "heads///foo.lock"
'

test_expect_success "normalize rejects 'foo.lock/bar'" '
	test_must_fail git check-ref-format --normalize foo.lock/bar
'

test_expect_success "normalize rejects 'foo.lock///bar'" '
	test_must_fail git check-ref-format --normalize "foo.lock///bar"
'

test_expect_success "normalize rejects '/foo' (single-level after stripping slash)" '
	test_must_fail git check-ref-format --normalize /foo
'

# ── --branch mode ────────────────────────────────────────────────────────────

test_expect_success "check-ref-format --branch rejects -nain (starts with dash)" '
	test_must_fail git check-ref-format --branch -nain
'

test_expect_success "check-ref-format --branch accepts plain branch name" '
	echo main >expect &&
	git check-ref-format --branch main >actual &&
	test_cmp expect actual
'

test_expect_success "check-ref-format --branch accepts master" '
	echo master >expect &&
	git check-ref-format --branch master >actual &&
	test_cmp expect actual
'

# === additional deepening tests ===

test_expect_success "check-ref-format accepts refs/heads/feature" '
	grit check-ref-format refs/heads/feature
'

test_expect_success "check-ref-format accepts refs/tags/v1.0" '
	grit check-ref-format refs/tags/v1.0
'

test_expect_success "check-ref-format rejects single component" '
	test_must_fail grit check-ref-format foo
'

test_expect_success "check-ref-format rejects ref with double dot" '
	test_must_fail grit check-ref-format refs/heads/foo..bar
'

test_expect_success "check-ref-format rejects ref ending with dot" '
	test_must_fail grit check-ref-format refs/heads/foo.
'

test_expect_success "check-ref-format rejects ref with space" '
	test_must_fail grit check-ref-format "refs/heads/foo bar"
'

test_expect_success "check-ref-format rejects ref with tilde" '
	test_must_fail grit check-ref-format "refs/heads/foo~1"
'

test_expect_success "check-ref-format rejects ref with caret" '
	test_must_fail grit check-ref-format "refs/heads/foo^bar"
'

test_expect_success "check-ref-format rejects ref with colon" '
	test_must_fail grit check-ref-format "refs/heads/foo:bar"
'

test_expect_success "check-ref-format rejects ref with backslash" '
	test_must_fail grit check-ref-format "refs/heads/foo\\bar"
'

test_expect_success "check-ref-format accepts deeply nested ref" '
	grit check-ref-format refs/heads/a/b/c/d/e
'

test_expect_success "check-ref-format rejects ref with @{" '
	test_must_fail grit check-ref-format "refs/heads/foo@{bar"
'

test_expect_success "check-ref-format rejects ref starting with dot" '
	test_must_fail grit check-ref-format refs/heads/.hidden
'

test_expect_success "check-ref-format rejects ref with consecutive slashes" '
	test_must_fail grit check-ref-format refs/heads//foo
'

test_expect_success "check-ref-format --normalize collapses slashes" '
	echo refs/heads/foo >expect &&
	grit check-ref-format --normalize refs///heads///foo >actual &&
	test_cmp expect actual
'

test_expect_success "check-ref-format rejects ref ending with .lock" '
	test_must_fail grit check-ref-format refs/heads/foo.lock
'

test_expect_success "check-ref-format accepts ref with hyphen" '
	grit check-ref-format refs/heads/my-feature
'

test_expect_success "check-ref-format accepts ref with underscore" '
	grit check-ref-format refs/heads/my_feature
'

test_expect_success "check-ref-format rejects ref with DEL character" '
	test_must_fail grit check-ref-format "refs/heads/foo$(printf "\177")bar"
'

test_expect_success "check-ref-format rejects ref with control char" '
	test_must_fail grit check-ref-format "refs/heads/foo$(printf "\001")bar"
'

test_done
