import { afterEach, describe, expect, it } from "vitest";
import { isCaptureWindow } from "./shell";

function withLabel(label: string) {
  (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label } },
  };
}

afterEach(() => {
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  window.location.hash = "";
});

describe("isCaptureWindow", () => {
  it("is true for the palette window even without a hash", () => {
    // Tauri's asset URLs can percent-encode `#/capture`, so the label is the
    // only reliable signal — this case used to paint an opaque white body.
    withLabel("capture");
    expect(isCaptureWindow()).toBe(true);
  });

  it("is false for the main window", () => {
    withLabel("main");
    expect(isCaptureWindow()).toBe(false);
  });

  it("falls back to the hash in a plain browser", () => {
    window.location.hash = "#/capture";
    expect(isCaptureWindow()).toBe(true);
    window.location.hash = "#/dashboard";
    expect(isCaptureWindow()).toBe(false);
  });
});
