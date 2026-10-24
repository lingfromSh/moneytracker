import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ShortcutInput, keyToAccelerator } from "./shortcut-input";
import { formatAccelerator } from "../lib/shortcut";

function chord(init: KeyboardEventInit & { code: string; key: string }): KeyboardEvent {
  return new KeyboardEvent("keydown", { ...init });
}

describe("shortcut-input", () => {
  it("records a chord with the platform primary modifier", () => {
    const mac = { primaryIsMeta: true };
    expect(
      keyToAccelerator(chord({ code: "Space", key: " ", metaKey: true, shiftKey: true }), mac),
    ).toBe("CmdOrCtrl+Shift+Space");
    expect(
      keyToAccelerator(chord({ code: "KeyK", key: "k", ctrlKey: true, altKey: true }), mac),
    ).toBe("Ctrl+Alt+K");
    // ctrl on macOS is the secondary modifier, not the primary one.
    expect(
      keyToAccelerator(chord({ code: "KeyK", key: "k", metaKey: true, ctrlKey: true }), mac),
    ).toBe("CmdOrCtrl+Ctrl+K");
    expect(
      keyToAccelerator(chord({ code: "Digit1", key: "1", ctrlKey: true }), { primaryIsMeta: false }),
    ).toBe("CmdOrCtrl+1");
  });

  it("ignores modifier-only presses and bare keys", () => {
    expect(keyToAccelerator(chord({ code: "ShiftLeft", key: "Shift", shiftKey: true }))).toBeNull();
    expect(keyToAccelerator(chord({ code: "KeyK", key: "k" }))).toBeNull();
  });

  it("formats an accelerator for display", () => {
    expect(formatAccelerator("CmdOrCtrl+Shift+Space")).toBe("⌘ ⇧ Space");
    expect(formatAccelerator("")).toBe("—");
  });

  it("emits the recorded accelerator and warns on a bare key", async () => {
    const onChange = vi.fn();
    render(<ShortcutInput value="CmdOrCtrl+Shift+Space" onChange={onChange} />);

    await userEvent.click(screen.getByLabelText("录制全局快捷键"));
    await userEvent.type(screen.getByLabelText("录制全局快捷键"), "{k}");
    expect(screen.getByText(/请同时按下修饰键/)).toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();

    await userEvent.click(screen.getByLabelText("录制全局快捷键"));
    await userEvent.type(
      screen.getByLabelText("录制全局快捷键"),
      "{Control>}{Alt>}{Shift>}k{/Shift}{/Alt}{/Control}",
    );
    expect(onChange).toHaveBeenCalledWith("CmdOrCtrl+Alt+Shift+K");
  });

  it("restores the default with the reset button", async () => {
    const onChange = vi.fn();
    render(<ShortcutInput value="Ctrl+Alt+K" onChange={onChange} />);
    await userEvent.click(screen.getByText("重置"));
    expect(onChange).toHaveBeenCalledWith("CmdOrCtrl+Shift+Space");
  });
});
