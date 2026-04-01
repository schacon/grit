# Gust - Git in Rust

Gust is a **from-scratch reimplementation of Git** in idiomatic Rust. The goal is to match Git’s behavior closely enough that the upstream test suite (under `git/t/`) can be ported and run against this tool.

The catch is that this entire implementation is written by Cursor. I hand wrote the AGENT.md instructions and copied a snapshot of the most recent Git source code HEAD into the `git/` directory and then asked Cursor to generate a plan file from a set of commands I wanted it to start with. Then I gave somewhat minimal further instructions to Cursor (via the Glass desktop) to act like an orchestrator, spawning subagents for tasks it thought could be run in parallel and continue until all the relevant original Git tests passed with the Gust binary subcommand equivalents. 


