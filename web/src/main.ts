// The page: the xterm.js console, the controls, and the debugger panels.
// The emulator runs in a worker, and each panel draws what the worker sends.

import "@xterm/xterm/css/xterm.css";
import "./style.css";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import type { FromWorker, Machine, MemoryView, Snapshot, State, ToWorker } from "./protocol";

const X_NAMES = [
  "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4", "a5",
  "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4", "t5", "t6",
];
const F_NAMES = [
  "ft0", "ft1", "ft2", "ft3", "ft4", "ft5", "ft6", "ft7", "fs0", "fs1", "fa0", "fa1", "fa2", "fa3",
  "fa4", "fa5", "fa6", "fa7", "fs2", "fs3", "fs4", "fs5", "fs6", "fs7", "fs8", "fs9", "fs10",
  "fs11", "ft8", "ft9", "ft10", "ft11",
];
const MODES: Record<number, string> = { 0: "U", 1: "S", 3: "M" };

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const machineSelect = $<HTMLSelectElement>("machine");
const bootButton = $<HTMLButtonElement>("boot");
const runButton = $<HTMLButtonElement>("run");
const pauseButton = $<HTMLButtonElement>("pause");
const stepButton = $<HTMLButtonElement>("step");
const step100Button = $<HTMLButtonElement>("step100");
const statusText = $("status");
const pcText = $("pc");

// The console.
const terminal = new Terminal({
  convertEol: false,
  cursorBlink: true,
  fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace',
  fontSize: 13,
  theme: { background: "#000000" },
});
const fit = new FitAddon();
terminal.loadAddon(fit);
terminal.open($("terminal"));
fit.fit();
window.addEventListener("resize", () => fit.fit());
terminal.writeln("hartwood: an RV64GC emulator in Rust, running as WebAssembly.");
terminal.writeln("Choose a machine and press Boot.");

// The worker.
const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
const send = (msg: ToWorker) => worker.postMessage(msg);

const encoder = new TextEncoder();
terminal.onData((data) => send({ type: "input", bytes: encoder.encode(data) }));

let state: State | "none" = "none";

function setState(next: State, detail: string, mips: number) {
  state = next;
  const rate = mips > 0 ? `, ${mips.toFixed(1)} MIPS` : "";
  statusText.textContent = `${detail}${rate}`;
  statusText.classList.remove("error");
  const booted = next !== "loading";
  runButton.disabled = !booted || next === "running" || next === "stopped";
  pauseButton.disabled = next !== "running" && next !== "idle";
  stepButton.disabled = next !== "paused";
  step100Button.disabled = next !== "paused";
  bootButton.disabled = next === "loading";
}

// The debugger panels.
let previousX: string[] = [];

function row(name: string, value: string, changed = false) {
  const div = document.createElement("div");
  div.className = "row";
  const n = document.createElement("span");
  n.className = "name";
  n.textContent = name;
  const v = document.createElement("span");
  v.textContent = value;
  if (changed) v.className = "changed";
  div.append(n, v);
  return div;
}

function drawSnapshot(s: Snapshot) {
  pcText.textContent = `pc ${s.pc}  mode ${MODES[s.mode] ?? "?"}  instret ${s.instret}`;

  const xregs = $("xregs");
  xregs.replaceChildren(
    ...s.x.map((v, i) => row(`${X_NAMES[i]} x${i}`, v, previousX.length > 0 && previousX[i] !== v)),
  );
  previousX = s.x;

  $("fregs").replaceChildren(...s.f.map((v, i) => row(F_NAMES[i], v)));
  $("csrs").replaceChildren(...s.csrs.map((c) => row(c.name, c.value)));

  const disasm = $("disasm");
  disasm.replaceChildren(
    ...s.disassembly.map((line, i) => {
      const div = document.createElement("div");
      div.className = i === 0 ? "row current" : "row";
      const addr = document.createElement("span");
      addr.className = "addr";
      addr.textContent = line.address.slice(-9);
      const bits = document.createElement("span");
      bits.className = "bits";
      bits.textContent = line.bits;
      const text = document.createElement("span");
      text.textContent = line.text;
      div.append(addr, bits, text);
      return div;
    }),
  );
}

function drawMemory(view: MemoryView) {
  const base = BigInt(`0x${view.address}`);
  const lines: string[] = [];
  for (let off = 0; off < view.bytes.length; off += 16) {
    const hex: string[] = [];
    let ascii = "";
    for (let i = off; i < off + 16; i++) {
      if (view.valid[i]) {
        const b = view.bytes[i];
        hex.push(b.toString(16).padStart(2, "0"));
        ascii += b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : ".";
      } else {
        hex.push("--");
        ascii += " ";
      }
    }
    const addr = (base + BigInt(off)).toString(16).padStart(16, "0");
    lines.push(`${addr}  ${hex.slice(0, 8).join(" ")}  ${hex.slice(8).join(" ")}  ${ascii}`);
  }
  $("memory").textContent = lines.join("\n");
}

worker.onmessage = (event: MessageEvent<FromWorker>) => {
  const msg = event.data;
  switch (msg.type) {
    case "output":
      terminal.write(msg.bytes);
      break;
    case "status":
      setState(msg.state, msg.detail, msg.mips);
      break;
    case "snapshot":
      drawSnapshot(msg.snapshot);
      break;
    case "memory":
      drawMemory(msg.view);
      break;
    case "progress":
      statusText.textContent = msg.text;
      break;
    case "error":
      statusText.textContent = msg.message;
      statusText.classList.add("error");
      bootButton.disabled = false;
      break;
  }
};

// The controls.
bootButton.addEventListener("click", () => {
  terminal.reset();
  previousX = [];
  const images = new URL("images/", document.baseURI).href;
  send({ type: "boot", machine: machineSelect.value as Machine, images });
  terminal.focus();
});
runButton.addEventListener("click", () => {
  send({ type: "run" });
  terminal.focus();
});
pauseButton.addEventListener("click", () => send({ type: "pause" }));
stepButton.addEventListener("click", () => send({ type: "step", count: 1 }));
step100Button.addEventListener("click", () => send({ type: "step", count: 100 }));

// Breakpoints: a list of hex addresses.
const breakpoints = new Set<string>();
const bpInput = $<HTMLInputElement>("bp-input");

function drawBreakpoints() {
  const list = $("bp-list");
  list.replaceChildren(
    ...[...breakpoints].map((a) => {
      const li = document.createElement("li");
      li.textContent = a;
      const remove = document.createElement("button");
      remove.textContent = "x";
      remove.title = "Remove this breakpoint";
      remove.addEventListener("click", () => {
        breakpoints.delete(a);
        drawBreakpoints();
      });
      li.append(remove);
      return li;
    }),
  );
  send({ type: "breakpoints", addresses: [...breakpoints] });
}

function addBreakpoint() {
  const text = bpInput.value.trim().toLowerCase().replace(/^0x/, "");
  if (!/^[0-9a-f]{1,16}$/.test(text)) {
    statusText.textContent = "A breakpoint is a hex address, such as 80200000.";
    statusText.classList.add("error");
    return;
  }
  breakpoints.add(text.padStart(16, "0"));
  bpInput.value = "";
  drawBreakpoints();
}

$("bp-add").addEventListener("click", addBreakpoint);
bpInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") addBreakpoint();
});

// The memory view.
function showMemory() {
  const text = $<HTMLInputElement>("mem-addr").value.trim().toLowerCase().replace(/^0x/, "");
  if (!/^[0-9a-f]{1,16}$/.test(text)) return;
  send({ type: "memory", address: text, virtual: $<HTMLInputElement>("mem-virtual").checked });
}

$("mem-go").addEventListener("click", showMemory);
$("mem-addr").addEventListener("keydown", (e) => {
  if (e.key === "Enter") showMemory();
});

// A link such as #xv6 boots that machine at once.
const fromHash = location.hash.slice(1);
if (fromHash === "linux" || fromHash === "xv6") {
  machineSelect.value = fromHash;
  bootButton.click();
}
void state;
