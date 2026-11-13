// The worker that runs the emulator, so the page never freezes.
//
// While the machine runs, the worker runs it in slices of about 20 ms, and
// yields between slices so that it can read the messages of the page.

import init, { Emulator, StopReason } from "./pkg/hartwood_wasm.js";
import type { FromWorker, Machine, ToWorker } from "./protocol";

const RAM_MIB: Record<Machine, number> = { linux: 256, xv6: 128 };
const BOOTARGS = "console=ttyS0 earlycon=sbi";
const SLICE_MS = 20;

let emu: Emulator | null = null;
let running = false;
/// The machine stopped because it waits for input.
let idle = false;
let stepsPerSlice = 200_000;
let breakpoints: bigint[] = [];
let memory = { address: 0x8000_0000n, virtual: false };
let lastSnapshot = 0;

function post(msg: FromWorker, transfer: Transferable[] = []) {
  (self as unknown as Worker).postMessage(msg, transfer);
}

const hex = (v: bigint) => v.toString(16).padStart(16, "0");

/// The URL of the folder of the boot images, from the page.
let imageBase = "";

async function fetchImage(path: string): Promise<Uint8Array> {
  post({ type: "progress", text: `Downloading ${path}` });
  const response = await fetch(new URL(path, imageBase));
  // A development server answers a missing file with its index page, so
  // a type of text/html is a missing image too.
  const type = response.headers.get("content-type") ?? "";
  if (!response.ok || type.startsWith("text/html")) {
    throw new Error(`cannot download images/${path} (HTTP ${response.status})`);
  }
  return new Uint8Array(await response.arrayBuffer());
}

async function boot(machine: Machine, images: string) {
  imageBase = images;
  running = false;
  post({ type: "status", state: "loading", detail: `Loading ${machine}`, mips: 0 });
  await init();
  emu?.free();
  emu = new Emulator(RAM_MIB[machine]);
  if (machine === "linux") {
    const [firmware, kernel, initrd] = await Promise.all([
      fetchImage("linux/fw_jump.bin"),
      fetchImage("linux/Image"),
      fetchImage("linux/rootfs.cpio"),
    ]);
    emu.boot(firmware, kernel, initrd, BOOTARGS);
  } else {
    const [kernel, disk] = await Promise.all([
      fetchImage("xv6/kernel"),
      fetchImage("xv6/fs.img"),
    ]);
    emu.boot(kernel, new Uint8Array(), new Uint8Array(), "");
    emu.set_disk(disk);
  }
  emu.set_breakpoints(BigUint64Array.from(breakpoints));
  sendSnapshot();
  running = true;
  loop();
}

function sendOutput() {
  if (!emu) return;
  const bytes = emu.take_output();
  if (bytes.length > 0) post({ type: "output", bytes }, [bytes.buffer]);
}

function sendSnapshot() {
  if (!emu) return;
  const names = emu.csr_names().split("\n");
  const values = emu.csr_values();
  const disassembly = emu
    .disassemble(emu.pc(), 24)
    .split("\n")
    .map((line) => {
      const [address, bits, text] = line.split("\t");
      return { address, bits, text };
    });
  post({
    type: "snapshot",
    snapshot: {
      pc: hex(emu.pc()),
      mode: emu.mode(),
      instret: emu.instret().toString(),
      x: Array.from(emu.x_registers(), hex),
      f: Array.from(emu.f_registers(), hex),
      csrs: names.map((name, i) => ({ name, value: hex(values[i]) })),
      disassembly,
    },
  });
  sendMemory();
  lastSnapshot = performance.now();
}

function sendMemory() {
  if (!emu) return;
  const bytes = emu.memory(memory.address, 256, memory.virtual);
  const valid = emu.memory_valid(memory.address, 256, memory.virtual);
  post({
    type: "memory",
    view: { address: hex(memory.address), virtual: memory.virtual, bytes, valid },
  });
}

/// Runs slices until a pause, a breakpoint, an exit or an idle wait.
async function loop() {
  let windowStart = performance.now();
  let windowSteps = 0;
  while (running && emu) {
    const before = emu.cycle();
    const t0 = performance.now();
    const reason = emu.run(stepsPerSlice);
    const elapsed = performance.now() - t0;
    const done = Number(emu.cycle() - before);
    windowSteps += done;
    // Keep each slice near SLICE_MS, so the worker answers quickly.
    if (reason === StopReason.StepLimit && elapsed > 0) {
      const ideal = (stepsPerSlice * SLICE_MS) / elapsed;
      stepsPerSlice = Math.max(10_000, Math.min(5_000_000, Math.round(ideal)));
    }
    sendOutput();

    const now = performance.now();
    if (now - windowStart > 500) {
      const mips = windowSteps / ((now - windowStart) * 1000);
      post({ type: "status", state: "running", detail: "Running", mips });
      windowStart = now;
      windowSteps = 0;
    }
    if (now - lastSnapshot > 250) sendSnapshot();

    if (reason === StopReason.Breakpoint) {
      running = false;
      post({ type: "status", state: "paused", detail: `Breakpoint at ${hex(emu.pc())}`, mips: 0 });
      sendSnapshot();
      return;
    }
    if (reason === StopReason.Exit || reason === StopReason.Reset) {
      running = false;
      const code = emu.exit_code();
      const detail = reason === StopReason.Reset ? "The machine asked for a reset" : `Powered off (code ${code ?? 0})`;
      post({ type: "status", state: "stopped", detail, mips: 0 });
      sendSnapshot();
      return;
    }
    if (reason === StopReason.Idle) {
      // The hart sleeps until input comes. The input message runs it again.
      running = false;
      idle = true;
      post({ type: "status", state: "idle", detail: "Waiting for input", mips: 0 });
      sendSnapshot();
      return;
    }
    // Yield, so the messages of the page can come in.
    await yieldToEvents();
  }
}

/// Lets the worker read its queued messages, then continues. A message on a
/// channel is a task, as each message of the page is, so the page's
/// messages that came first run first. `setTimeout` would also work, but a
/// browser can slow its timers to one call a second in a hidden tab.
function yieldToEvents(): Promise<void> {
  return new Promise((resolve) => {
    const channel = new MessageChannel();
    channel.port1.onmessage = () => resolve();
    channel.port2.postMessage(null);
  });
}

function start() {
  if (!emu || running) return;
  running = true;
  idle = false;
  post({ type: "status", state: "running", detail: "Running", mips: 0 });
  loop();
}

self.onmessage = async (event: MessageEvent<ToWorker>) => {
  const msg = event.data;
  try {
    switch (msg.type) {
      case "boot":
        await boot(msg.machine, msg.images);
        break;
      case "run":
        start();
        break;
      case "pause":
        running = false;
        idle = false;
        post({ type: "status", state: "paused", detail: "Paused", mips: 0 });
        sendSnapshot();
        break;
      case "step":
        if (!emu || running) break;
        for (let i = 0; i < msg.count; i++) emu.step();
        sendOutput();
        post({ type: "status", state: "paused", detail: `Stepped ${msg.count}`, mips: 0 });
        sendSnapshot();
        break;
      case "input":
        if (!emu) break;
        emu.push_input(msg.bytes);
        // A key wakes a machine that waits for input, but not one that a
        // person paused.
        if (idle) start();
        break;
      case "breakpoints":
        breakpoints = msg.addresses.map((a) => BigInt(`0x${a}`));
        emu?.set_breakpoints(BigUint64Array.from(breakpoints));
        break;
      case "memory":
        memory = { address: BigInt(`0x${msg.address}`), virtual: msg.virtual };
        sendMemory();
        break;
    }
  } catch (e) {
    running = false;
    post({ type: "error", message: e instanceof Error ? e.message : String(e) });
  }
};
