import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { installTauriMock } from "../lib/tauri-mock";
import { makeT } from "../i18n";
import HistoryList from "./HistoryList";

describe("HistoryList", () => {
  it("loads, filters, and paginates local history", async () => {
    installTauriMock();
    const user = userEvent.setup();
    render(<HistoryList locale="en" t={makeT("en")} />);

    expect(await screen.findByText("55 dictations stored locally")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Load more" })).toBeInTheDocument();

    await user.type(screen.getByRole("textbox", { name: "Search history…" }), "standby");
    expect(await screen.findByText("14 dictations stored locally")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
  });

  it("explains why an interrupted session failed and removes it on discard", async () => {
    installTauriMock();
    const user = userEvent.setup();
    render(<HistoryList locale="en" t={makeT("en")} />);

    expect(await screen.findByText("Interrupted by restart")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Discard" })).toHaveLength(2);

    await user.click(screen.getAllByRole("button", { name: "Discard" })[0]);
    const dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Discard" }));
    await waitFor(() => expect(screen.queryByText("Interrupted by restart")).not.toBeInTheDocument());
    expect(screen.getAllByRole("button", { name: "Discard" })).toHaveLength(1);
  });
});
