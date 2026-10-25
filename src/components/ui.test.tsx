import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Modal } from "./ui";

describe("Modal", () => {
  it("renders nothing when closed", () => {
    render(
      <Modal open={false} onClose={() => {}} title="编辑交易">
        内容
      </Modal>,
    );
    expect(screen.queryByText("编辑交易")).toBeNull();
  });

  it("is a labelled dialog with a close button", async () => {
    const onClose = vi.fn();
    render(
      <Modal open onClose={onClose} title="编辑交易">
        内容
      </Modal>,
    );
    const dialog = screen.getByRole("dialog", { name: "编辑交易" });
    expect(dialog).toBeTruthy();
    // 手机上它是从底部升起的整宽面板：贴底、上方圆角、有限高。
    expect(dialog.className).toContain("max-h-[92dvh]");
    expect(dialog.className).toContain("rounded-t-2xl");
    expect(dialog.className).toContain("sm:rounded-[var(--radius-card)]");

    await userEvent.click(screen.getByLabelText("关闭"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape", async () => {
    const onClose = vi.fn();
    render(
      <Modal open onClose={onClose} title="编辑交易">
        内容
      </Modal>,
    );
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
