import * as RTooltip from "@radix-ui/react-tooltip";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { formatDuration, parseDuration, parseScore } from "../lib/typed";
import { useApp } from "../store";
import { TuningPanel } from "../views/TuningPanel";
import { linearScale, RangeSlider, stopScale } from "./TypedSlider";
import { Segmented } from "./ui";

// Radix sliders measure themselves; jsdom has no layout.
class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as unknown as { ResizeObserver: typeof RO }).ResizeObserver = RO;

function LengthHarness({ onChange }: { onChange: (v: [number | null, number | null]) => void }) {
  const [v, setV] = useState<[number | null, number | null]>([2, null]);
  const scale = stopScale<number | null>([0.5, 1, 2, 5, 10, 30, null], formatDuration, parseDuration);
  return (
    <RangeSlider
      label="Segment length"
      scale={scale}
      value={v}
      onChange={(x) => {
        setV(x);
        onChange(x);
      }}
    />
  );
}

describe("TypedSlider", () => {
  it("accepts a typed value on double-click", () => {
    const onChange = vi.fn();
    render(<LengthHarness onChange={onChange} />);
    fireEvent.doubleClick(screen.getByText("2s"));
    const input = screen.getByLabelText("Segment length minimum value");
    fireEvent.change(input, { target: { value: "1:30" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onChange).toHaveBeenLastCalledWith([90, null]);
    expect(screen.getByText("1:30")).toBeInTheDocument();
  });

  it("rejects an invalid value and keeps editing", () => {
    const onChange = vi.fn();
    render(<LengthHarness onChange={onChange} />);
    fireEvent.doubleClick(screen.getByText("none"));
    const input = screen.getByLabelText("Segment length maximum value");
    fireEvent.change(input, { target: { value: "soon" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onChange).not.toHaveBeenCalled();
    expect(input).toHaveAttribute("title");
  });

  it("formats scores 0–100", () => {
    const s = linearScale(0, 1, 100, (v) => `${Math.round(v * 100)}`, parseScore);
    expect(s.format(0.62)).toBe("62");
    expect(s.toPos(0.62)).toBe(62);
  });
});

describe("Segmented", () => {
  it("is a radio group", () => {
    const onChange = vi.fn();
    render(
      <Segmented
        label="Tier"
        value="b"
        onChange={onChange}
        options={[
          { value: "a", label: "A" },
          { value: "b", label: "B" },
        ]}
      />,
    );
    expect(screen.getByRole("radio", { name: "B" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(screen.getByRole("radio", { name: "A" }));
    expect(onChange).toHaveBeenCalledWith("a");
  });
});

describe("TuningPanel with the mock backend", () => {
  it("loads presets and changes a flag tier", async () => {
    await act(async () => {
      await useApp.getState().init();
    });
    render(
      <RTooltip.Provider>
        <TuningPanel />
      </RTooltip.Provider>,
    );
    expect(screen.getByText("All-Rounder")).toBeInTheDocument();
    const shaky = screen.getByRole("radiogroup", { name: "Shaky tier" });
    fireEvent.click(shaky.querySelector('[title="Always drop samples with this flag"]')!);
    await waitFor(() => expect(useApp.getState().settings?.tiers[5]).toBe("force_exclude"));
    expect(useApp.getState().modified).toBe(true);
    await waitFor(() => expect(useApp.getState().scored?.selects.length).toBeGreaterThan(0));
  });
});
