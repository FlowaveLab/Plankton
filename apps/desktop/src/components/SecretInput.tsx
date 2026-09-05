import {
  useEffect,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type JSX,
} from "react";
import { Eye, EyeOff } from "lucide-react";
import "./SecretInput.css";

type SecretInputProps = Omit<ComponentPropsWithoutRef<"input">, "type"> & {
  locale?: string;
  fieldName?: string;
  autoReveal?: boolean;
  resetKey?: number;
  onReveal?: () => Promise<boolean>;
  onConceal?: () => void;
};

/** Each field owns its visibility; async reveals cannot reopen an unmounted field. */
export function SecretInput({
  locale,
  fieldName,
  autoReveal = false,
  resetKey,
  onReveal,
  onConceal,
  ...input
}: SecretInputProps): JSX.Element {
  const [visible, setVisible] = useState(false);
  const [loading, setLoading] = useState(false);
  const generation = useRef(0);
  const manuallyConcealed = useRef(false);
  const revealRef = useRef(onReveal);
  revealRef.current = onReveal;
  const show = async (): Promise<void> => {
    const token = ++generation.current;
    setLoading(true);
    try {
      const loaded = await (revealRef.current?.() ?? Promise.resolve(true));
      if (generation.current === token && loaded) setVisible(true);
    } finally {
      if (generation.current === token) setLoading(false);
    }
  };
  useEffect(() => {
    setVisible(false);
    setLoading(false);
    if (autoReveal && !manuallyConcealed.current) void show();
    return () => {
      generation.current += 1;
    };
  }, [autoReveal, resetKey]);
  const name =
    fieldName ??
    input["aria-label"] ??
    (locale === "zh-CN" ? "密码" : "password");
  const action =
    locale === "zh-CN"
      ? `${visible ? "隐藏" : "显示"}${name}`
      : `${visible ? "Hide" : "Show"} ${name}`;
  return (
    <span className="secret-input-control" aria-busy={loading}>
      <input {...input} type={visible ? "text" : "password"} />
      <button
        className="secret-input-toggle"
        type="button"
        aria-label={action}
        title={action}
        aria-pressed={visible}
        disabled={input.disabled || loading}
        onClick={() => {
          if (visible) {
            manuallyConcealed.current = true;
            setVisible(false);
            onConceal?.();
          } else {
            manuallyConcealed.current = false;
            void show();
          }
        }}
      >
        {visible ? (
          <EyeOff size={17} aria-hidden="true" />
        ) : (
          <Eye size={17} aria-hidden="true" />
        )}
      </button>
    </span>
  );
}
