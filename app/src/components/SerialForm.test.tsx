import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import en from "../i18n/en.json";
import { SerialForm } from "./SerialForm";

const last = { port: "COM7", baud: 9600, data_bits: 7, stop_bits: 2, parity: "even", flow: "hardware" };

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    if (command === "serial_ports") return [{ name: "COM7", description: "USB" }];
    if (command === "serial_last") return last;
    return null;
  }),
}));

afterEach(cleanup);

test("starts from the last settings and opens them", async () => {
  const onOpen = vi.fn();
  render(<SerialForm onOpen={onOpen} />);
  await waitFor(() => expect((screen.getByLabelText(en["serial.port"]) as HTMLInputElement).value).toBe("COM7"));
  fireEvent.click(screen.getByRole("button", { name: en["serial.open"] }));
  expect(onOpen).toHaveBeenCalledWith(last);
});

test("an invalid baud rate disables opening", async () => {
  render(<SerialForm onOpen={() => {}} />);
  await waitFor(() => expect((screen.getByLabelText(en["serial.port"]) as HTMLInputElement).value).toBe("COM7"));
  fireEvent.change(screen.getByLabelText(en["serial.baud"]), { target: { value: "12x" } });
  expect((screen.getByRole("button", { name: en["serial.open"] }) as HTMLButtonElement).disabled).toBe(true);
});
