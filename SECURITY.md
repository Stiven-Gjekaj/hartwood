<div align="center">
  <a href="README.md"><b>hartwood</b></a>
</div>

# Security Policy

## Supported versions

Fixes go to the latest version on the default branch.
Older versions are not maintained.

## Reporting a vulnerability

Report security problems privately, and not through a public issue.

- Preferred: open a private security advisory with the "Report a vulnerability"
  button on the Security tab of this repository.
- Alternative: email the maintainer at stivenagostingjekaj@gmail.com.

Include the steps to reproduce, the guest image, and the impact as you
understand it.
You can expect a first answer within a few days.

## The threat model

**The guest is untrusted.**
hartwood runs a program that somebody else built. The program controls each
register, each page table and each request to a device. Nothing that a guest
does may harm the host.

**These are in scope:**

- A guest that makes the emulator read or write host memory outside the
  RAM of the guest.
- A guest that makes the emulator panic, hang, or use memory without bound.
  A malformed ELF file, device tree, descriptor chain or page table is a
  guest input too.
- A guest that reads a file of the host, or reaches the network.
- In the browser: a guest or an image that runs script in the page.

**These are out of scope:**

- A guest that crashes itself. A program can break its own kernel.
- A guest that runs for a long time. A program can loop.
- Bugs in the guest images that this repository builds: report those to
  Linux, OpenSBI, BusyBox or xv6.
