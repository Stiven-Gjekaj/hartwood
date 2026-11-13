// The messages between the page and the worker that runs the emulator.
// Each 64-bit value is a hex string, so it crosses with no loss.

export type Machine = "linux" | "xv6";

export type ToWorker =
  /// `images` is the URL of the folder of the boot images.
  | { type: "boot"; machine: Machine; images: string }
  | { type: "run" }
  | { type: "pause" }
  | { type: "step"; count: number }
  | { type: "input"; bytes: Uint8Array }
  | { type: "breakpoints"; addresses: string[] }
  | { type: "memory"; address: string; virtual: boolean };

export type State = "loading" | "running" | "paused" | "idle" | "stopped";

export interface Snapshot {
  pc: string;
  mode: number;
  instret: string;
  x: string[];
  f: string[];
  csrs: { name: string; value: string }[];
  disassembly: { address: string; bits: string; text: string }[];
}

export interface MemoryView {
  address: string;
  virtual: boolean;
  bytes: Uint8Array;
  valid: Uint8Array;
}

export type FromWorker =
  | { type: "output"; bytes: Uint8Array }
  | { type: "status"; state: State; detail: string; mips: number }
  | { type: "snapshot"; snapshot: Snapshot }
  | { type: "memory"; view: MemoryView }
  | { type: "progress"; text: string }
  | { type: "error"; message: string };
