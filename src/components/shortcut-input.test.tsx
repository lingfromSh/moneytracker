import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import {
  ShortcutInput,
  chordProblem,
  keyToAccelerator,
} from "./shortcut-input";
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

  it("explains why a chord is unusable", () => {
    const mac = true;
    expect(chordProblem(chord({ code: "KeyK", key: "k" }), mac)).toMatch(/按住修饰键/);
    expect(chordProblem(chord({ code: "KeyA", key: "A", shiftKey: true }), mac)).toMatch(
      /只有 ⇧ 不够/,
    );
    // ⌘A is owned by the macOS app menu and never reaches the webview.
    expect(chordProblem(chord({ code: "KeyA", key: "a", metaKey: true }), mac)).toMatch(
      /macOS 菜单占用/,
    );
    expect(chordProblem(chord({ code: "KeyA", key: "A", metaKey: true, shiftKey: true }), mac)).toBeNull();
    // A held modifier on its own is not a problem, just an incomplete chord.
    expect(chordProblem(chord({ code: "MetaLeft", key: "Meta", metaKey: true }), mac)).toBeNull();
  });

  it("formats an accelerator for display", () => {
    expect(formatAccelerator("CmdOrCtrl+Shift+Space")).toBe("⌘ ⇧ Space");
    expect(formatAccelerator("")).toBe("—");
  });

  it("records from a window level listener and releases the hotkey meanwhile", async () => {
    const onChange = vi.fn();
    const onRecordingChange = vi.fn();
    render(
      <ShortcutInput
        value="CmdOrCtrl+Shift+Space"
        onChange={onChange}
        onRecordingChange={onRecordingChange}
      />,
    );

    await userEvent.click(screen.getByLabelText("录制全局快捷键"));
    expect(onRecordingChange).toHaveBeenCalledWith(true);
    expect(screen.getByText(/请按下新的快捷键/)).toBeInTheDocument();

    // Modifiers alone update the preview without committing.
    fireEvent.keyDown(window, { code: "ControlLeft", key: "Control", ctrlKey: true });
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByText(/新快捷键：⌘/)).toBeInTheDocument();

    fireEvent.keyDown(window, { code: "KeyK", key: "k", ctrlKey: true, shiftKey: true });
    expect(onChange).toHaveBeenCalledWith("CmdOrCtrl+Shift+K");
    expect(onRecordingChange).toHaveBeenCalledWith(false);
  });

  it("warns instead of committing a shift-only chord", async () => {
    const onChange = vi.fn();
    render(<ShortcutInput value="CmdOrCtrl+Shift+Space" onChange={onChange} />);

    await userEvent.click(screen.getByLabelText("录制全局快捷键"));
    fireEvent.keyDown(window, { code: "KeyA", key: "A", shiftKey: true });

    expect(screen.getByText(/只有 ⇧ 不够/)).toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("cancels with Escape and restores the hotkey", async () => {
    const onChange = vi.fn();
    const onRecordingChange = vi.fn();
    render(
      <ShortcutInput
        value="CmdOrCtrl+Shift+Space"
        onChange={onChange}
        onRecordingChange={onRecordingChange}
      />,
    );

    await userEvent.click(screen.getByLabelText("录制全局快捷键"));
    fireEvent.keyDown(window, { code: "Escape", key: "Escape" });

    expect(onChange).not.toHaveBeenCalled();
    expect(onRecordingChange).toHaveBeenLastCalledWith(false);
    expect(screen.getByText(/当前：⌘ ⇧ Space/)).toBeInTheDocument();
  });

  it("stops recording and restores the hotkey when the user clicks away", async () => {
    const onChange = vi.fn();
    const onRecordingChange = vi.fn();
    render(
      <ShortcutInput
        value="CmdOrCtrl+Shift+Space"
        onChange={onChange}
        onRecordingChange={onRecordingChange}
      />,
    );

    await userEvent.click(screen.getByLabelText("录制全局快捷键"));
    fireEvent.pointerDown(document.body);

    expect(onChange).not.toHaveBeenCalled();
    expect(onRecordingChange).toHaveBeenLastCalledWith(false);
    expect(screen.getByText(/当前：⌘ ⇧ Space/)).toBeInTheDocument();
  });

  it("restores the default with the reset button", async () => {
    const onChange = vi.fn();
    render(<ShortcutInput value="Ctrl+Alt+K" onChange={onChange} />);
    await userEvent.click(screen.getByText("重置"));
    expect(onChange).toHaveBeenCalledWith("CmdOrCtrl+Shift+Space");
  });
});
