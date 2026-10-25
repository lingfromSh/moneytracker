import { describe, expect, it, vi, beforeEach } from "vitest";

const showCapture = vi.fn(async () => undefined);
vi.mock("./ipc", () => ({ api: { showCapture: () => showCapture() } }));

const state = { mobile: false };
vi.mock("./platform", () => ({
  get isMobile() {
    return state.mobile;
  },
}));

import { startCapture } from "./capture";

describe("startCapture", () => {
  beforeEach(() => {
    showCapture.mockClear();
    window.location.hash = "";
    state.mobile = false;
  });

  it("asks the shell for the palette window on the desktop", () => {
    startCapture();
    expect(showCapture).toHaveBeenCalledTimes(1);
    expect(window.location.hash).toBe("");
  });

  it("navigates to the capture page on a phone, where there is no second window", () => {
    state.mobile = true;
    startCapture();
    expect(showCapture).not.toHaveBeenCalled();
    expect(window.location.hash).toBe("#/capture");
  });
});
