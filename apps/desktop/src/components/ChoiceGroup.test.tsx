// @vitest-environment jsdom

import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";

import { ChoiceGroup } from "./ChoiceGroup";

it("keeps radio groups independent and respects disabled choices", () => {
  const changed = vi.fn();
  function Harness() {
    const [value, setValue] = useState("protected");
    const options = [
      { value: "protected", label: "Protected" },
      { value: "direct", label: "Direct" },
    ];
    return (
      <>
        <ChoiceGroup
          label="First"
          value={value}
          options={options}
          onChange={setValue}
        />
        <ChoiceGroup
          label="Second"
          value="protected"
          options={options}
          onChange={changed}
          disabled
        />
      </>
    );
  }
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  try {
    act(() => root.render(<Harness />));
    const groups = container.querySelectorAll("fieldset");
    const first = groups[0].querySelector<HTMLInputElement>(
      'input[value="direct"]',
    )!;
    const second = groups[1].querySelector<HTMLInputElement>(
      'input[value="protected"]',
    )!;
    expect(first.name).not.toBe(second.name);
    act(() => first.click());
    expect(first.checked).toBe(true);
    expect(second.checked).toBe(true);
    act(() =>
      groups[1]
        .querySelector<HTMLInputElement>('input[value="direct"]')
        ?.click(),
    );
    expect(changed).not.toHaveBeenCalled();
    expect(second.checked).toBe(true);
  } finally {
    act(() => root.unmount());
    container.remove();
  }
});
