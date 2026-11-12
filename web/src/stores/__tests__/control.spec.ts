import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { ControlError, MAX_LOG_LINES, useControlStore } from "../control";

function answer(status: number, body?: unknown) {
  return Promise.resolve(
    new Response(body === undefined ? null : JSON.stringify(body), { status }),
  );
}

const status = { version: "0.2.0", serving: true, port: 4500 };

describe("Control Store", () => {
  const fetchMock = vi.fn();

  beforeEach(() => {
    setActivePinia(createPinia());
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("finds the control API in the Vigil window", async () => {
    fetchMock.mockReturnValueOnce(answer(200, status));
    const control = useControlStore();

    expect(await control.detect()).toBe(true);
    expect(control.status).toEqual(status);
    // Asked once, however often the answer is wanted.
    expect(await control.detect()).toBe(true);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("finds nothing in a browser", async () => {
    fetchMock.mockReturnValueOnce(answer(404));
    const control = useControlStore();

    expect(await control.detect()).toBe(false);
    expect(control.status).toBeNull();
  });

  it("sends only the changed keys and keeps what comes back", async () => {
    const saved = { port_number: 4600 };
    fetchMock.mockReturnValueOnce(answer(200, { settings: saved, status }));
    const control = useControlStore();

    await control.save({ port_number: 4600 });

    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toMatch(/\/control\/settings$/);
    expect(init.method).toBe("PUT");
    expect(JSON.parse(init.body)).toEqual({ port_number: 4600 });
    expect(control.settings).toEqual(saved);
    expect(control.status).toEqual(status);
  });

  it("reports the collector's own reason when a change is refused", async () => {
    fetchMock.mockReturnValueOnce(
      answer(409, { error: "Could not listen on 127.0.0.1:80." }),
    );
    const control = useControlStore();

    await expect(control.save({ port_number: 80 })).rejects.toThrow(
      new ControlError("Could not listen on 127.0.0.1:80."),
    );
    expect(control.settings).toBeNull();
  });

  it("appends new log lines and asks only for later ones", async () => {
    fetchMock
      .mockReturnValueOnce(answer(200, { lines: [{ seq: 1, text: "a" }], newest: 1 }))
      .mockReturnValueOnce(answer(200, { lines: [{ seq: 2, text: "b" }], newest: 2 }));
    const control = useControlStore();

    await control.pollLogs();
    await control.pollLogs();

    expect(fetchMock.mock.calls[1][0]).toMatch(/\/control\/logs\?after=1$/);
    expect(control.logs.map((line) => line.text)).toEqual(["a", "b"]);
  });

  it("keeps no more lines than the collector does", async () => {
    const lines = Array.from({ length: MAX_LOG_LINES + 5 }, (_, i) => ({
      seq: i + 1,
      text: `line ${i + 1}`,
    }));
    fetchMock.mockReturnValueOnce(answer(200, { lines, newest: lines.length }));
    const control = useControlStore();

    await control.pollLogs();

    expect(control.logs).toHaveLength(MAX_LOG_LINES);
    expect(control.logs[0].seq).toBe(6);
  });

  it("starts the log again when the collector has", async () => {
    fetchMock
      .mockReturnValueOnce(answer(200, { lines: [{ seq: 9, text: "old" }], newest: 9 }))
      .mockReturnValueOnce(answer(200, { lines: [{ seq: 1, text: "new" }], newest: 1 }));
    const control = useControlStore();

    await control.pollLogs();
    await control.pollLogs();

    expect(control.logs.map((line) => line.text)).toEqual(["new"]);
  });
});
