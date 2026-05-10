# v0k - Invoke Command Semantically & Safely with AI Assistance

> Don't Repeat Yourself, Let AI Write the Command.

**When you forget the exact syntax of a command just type `v0k` first.**

## Introduction

**v0k** is an AI-powered, "semantic-level" command-line tool built for developers.
By understanding your natural language or fuzzy intents, v0k automatically translates them into precise, high-performance native system commands (like `curl`, `git`, `ffmpeg`, `docker`, etc.).
Say goodbye to memorizing complex flags and sifting through endless man pages!

## Core Features

- **Semantic Translation**: Simply describe what you want to achieve, and v0k will infer and construct the correct command with full parameters.
- **Blazing Fast Native Execution**: The core engine is built in Rust for high performance and low memory footprint. It transparently executes your system's native binaries without reinventing the wheel.
- **Smart Wrappers**: Provides built-in semantic support and parameter fault tolerance for high-frequency, complex tools such as `curl`, `git`, `docker`, `ffmpeg`, `find`, and `tar`.
- **Interactive Safety Confirmation**: Before executing uncertain AI-generated commands, v0k prompts for user confirmation to prevent accidental destructive actions.

## Quick Start

### Installation

```bash
npm install -g @v0k/cli
```

### Initial Setup

Run the setup command to configure your AI provider:

```bash
v0k setup
```

`v0k setup` will also try to append the `v0k` shell integration function to your `~/.bashrc` or `~/.zshrc` automatically.
You can install just the shell integration anytime with:

```bash
v0k setup alias
```

> Fast models are recommended for better performance and cost efficiency.
> You can override the default model in `~/.v0k/config.toml` or via the `V0K_MODEL` environment variable.

## Features

### Fuzzy Parameter Completion

Only remember the command but forgot the exact parameter format? Just throw the messy details at it:

```bash
v0k curl POST localhost:8080/api/users id=1 name=jack json
```

_v0k will use AI reasoning and context to automatically piece together the complete `curl -X POST ...` command and execute it._

### Reviewing Before Execution

If you type something reasonably safe and straightforward like:

```bash
v0k ls -la /var/log
```

v0k will execute it as-is without any AI intervention.

If you type something that looks like a native command but is potentially dangerous, like:

```bash
v0k rm -rf /tmp/test
```

v0k will prompt you for confirmation before executing it.

### Quick Ask for Command Syntax

```bash
v0k ask "How to use ffmpeg to convert a video to mp4 format?"
```

v0k will return the instruction and the complete command.

### Fix the Previous Failed Command

Use `v0k fix` to ask v0k to repair the last failed command and run the suggested fix:

```bash
gti status
v0k fix --command "gti status" --exit-code 127
```

`v0k setup` can install this automatically, but you can also add it manually to `~/.bashrc` or `~/.zshrc` so `v0k fix` can capture the previous command automatically:

```bash
v0k() {
  local exit_code=$?
  if [ "$1" = "fix" ]; then
    local last_cmd
    last_cmd="$(fc -ln -1)"
    command v0k fix --command "$last_cmd" --exit-code "$exit_code" "${@:2}"
  else
    command v0k "$@"
  fi
}
```

Preview a fix without executing it:

```bash
v0k fix --dry-run
```
