---
name: Bug report
about: Report something that does not work as expected
title: ""
labels: bug
assignees: ""
---

## What happened

A clear description of the bug.

## The command you ran

```
hartwood run ...
```

If you used the web page, say which machine and which button.

## What you expected

What you expected to happen instead.

## What actually happened

The output or the error, copied exactly. For a guest that stops or
crashes, the last lines of the console help most.

## The guest

- The image: a riscv-tests program, xv6, Linux, or your own program.
- If it is your own program, how you built it, and the file if you can
  share it.
- Does the same image work on QEMU (`qemu-system-riscv64 -machine virt`)?

## The host

- hartwood version (from `hartwood --version`) or the commit:
- Operating system, or the browser and its version:
