import { useId, type JSX, type ReactNode } from "react";

import type { LucideIcon } from "lucide-react";

import "./ChoiceGroup.css";

type ChoiceGroupProps = {
  label: ReactNode;
  value: string;
  options: readonly { value: string; label: ReactNode; icon?: LucideIcon }[];
  onChange: (value: string) => void;
  disabled?: boolean;
  initialFocus?: boolean;
  className?: string;
  id?: string;
  "aria-label"?: string;
  "data-testid"?: string;
};

/** A native radio group for a small set of named choices. */
export function ChoiceGroup(props: ChoiceGroupProps): JSX.Element {
  const name = useId();
  return (
    <fieldset
      aria-label={props["aria-label"]}
      className={`choice-group ${props.className ?? ""}`}
      data-testid={props["data-testid"]}
      disabled={props.disabled}
      id={props.id}
    >
      <legend>{props.label}</legend>
      <div className="choice-group-options">
        {props.options.map((option) => {
          const Icon = option.icon;
          return (
            <label
              className="choice-group-option"
              data-selected={option.value === props.value}
              key={option.value}
            >
              <input
                checked={option.value === props.value}
                data-dialog-initial-focus={
                  props.initialFocus && option.value === props.value
                    ? "true"
                    : undefined
                }
                disabled={props.disabled}
                name={name}
                onChange={() => props.onChange(option.value)}
                type="radio"
                value={option.value}
              />
              {Icon ? (
                <Icon
                  aria-hidden="true"
                  focusable="false"
                  size={16}
                  strokeWidth={1.75}
                />
              ) : null}
              <span>{option.label}</span>
            </label>
          );
        })}
      </div>
    </fieldset>
  );
}
