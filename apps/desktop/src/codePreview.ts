import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import dos from "highlight.js/lib/languages/dos";
import javascript from "highlight.js/lib/languages/javascript";
import powershell from "highlight.js/lib/languages/powershell";
import python from "highlight.js/lib/languages/python";

import { escapeHtml } from "./dashboardModel";

hljs.registerLanguage("bash", bash);
hljs.registerLanguage("dos", dos);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("powershell", powershell);
hljs.registerLanguage("python", python);

const EXTENSION_LANGUAGE_MAP = {
  bash: {
    language: "bash",
    label: "bash",
  },
  bat: {
    language: "dos",
    label: "bat",
  },
  cmd: {
    language: "dos",
    label: "cmd",
  },
  fish: {
    language: "bash",
    label: "fish",
  },
  cjs: {
    language: "javascript",
    label: "cjs",
  },
  js: {
    language: "javascript",
    label: "js",
  },
  jsx: {
    language: "javascript",
    label: "jsx",
  },
  mjs: {
    language: "javascript",
    label: "mjs",
  },
  ps1: {
    language: "powershell",
    label: "ps1",
  },
  py: {
    language: "python",
    label: "py",
  },
  sh: {
    language: "bash",
    label: "sh",
  },
  zsh: {
    language: "bash",
    label: "zsh",
  },
} as const;

type SupportedExtension = keyof typeof EXTENSION_LANGUAGE_MAP;

type HighlightMapping = {
  language: string;
  label: string;
};

export type PreviewHighlightResult = {
  highlighted: boolean;
  html: string;
  label: string;
};

export function getSupportedPreviewExtensions(): string[] {
  return Object.keys(EXTENSION_LANGUAGE_MAP).sort();
}

function getInterpreterToken(candidate: string): string | null {
  const trimmed = candidate.trim().toLowerCase();
  if (!trimmed) {
    return null;
  }

  const basename = trimmed.split(/[\\/]/).at(-1) ?? trimmed;
  return basename || null;
}

function getPathExtension(path: string | null): string | null {
  if (!path) {
    return null;
  }

  const normalizedPath = path.trim().toLowerCase();
  if (!normalizedPath) {
    return null;
  }

  const basename = normalizedPath.split(/[\\/]/).at(-1) ?? normalizedPath;
  const extension = basename.split(".").at(-1);

  if (!extension || extension === basename) {
    return null;
  }

  return extension;
}

function getMappingFromExtension(path: string | null): HighlightMapping | null {
  const extension = getPathExtension(path);
  if (!extension || !(extension in EXTENSION_LANGUAGE_MAP)) {
    return null;
  }

  return EXTENSION_LANGUAGE_MAP[extension as SupportedExtension];
}

function getMappingFromShebang(previewText: string): HighlightMapping | null {
  const firstLine = previewText.split(/\r?\n/, 1)[0]?.trim() ?? "";
  if (!firstLine.startsWith("#!")) {
    return null;
  }

  const tokens = firstLine
    .slice(2)
    .trim()
    .split(/\s+/)
    .map(getInterpreterToken)
    .filter((value): value is string => Boolean(value));

  if (tokens.length === 0) {
    return null;
  }

  const envIndex = tokens.findIndex((token) => token === "env");
  const interpreter =
    envIndex >= 0
      ? tokens.slice(envIndex + 1).find((token) => token !== "-s")
      : tokens[0];

  switch (interpreter) {
    case "bash":
    case "sh":
    case "zsh":
    case "fish":
      return {
        language: "bash",
        label: interpreter,
      };
    case "python":
    case "python3":
    case "pythonw":
      return {
        language: "python",
        label: "python",
      };
    case "bun":
    case "deno":
    case "node":
    case "nodejs":
      return {
        language: "javascript",
        label: "javascript",
      };
    case "pwsh":
    case "powershell":
    case "powershell.exe":
    case "pwsh.exe":
      return {
        language: "powershell",
        label: "powershell",
      };
    case "cmd":
    case "cmd.exe":
      return {
        language: "dos",
        label: "cmd",
      };
    default:
      return null;
  }
}

function mappingForLanguageHint(hint: string): HighlightMapping | null {
  const normalized = hint.trim().toLowerCase();
  if (/^(py|python|python3)$/.test(normalized)) {
    return { language: "python", label: "python heredoc" };
  }
  if (/^(js|javascript|node|nodejs|bun|deno)$/.test(normalized)) {
    return { language: "javascript", label: "javascript heredoc" };
  }
  if (/^(sh|shell|bash|zsh|fish)$/.test(normalized)) {
    return { language: "bash", label: "bash heredoc" };
  }
  return null;
}

function getMappingFromHeredoc(previewText: string): HighlightMapping | null {
  const opening = previewText.match(
    /(?:^|\r?\n)([^\r\n]*?)<<-?\s*(['"]?)([A-Za-z_][A-Za-z0-9_]*)\2[ \t]*(?:\r?\n)/,
  );
  if (!opening) {
    return null;
  }

  const commandPrefix = opening[1] ?? "";
  const delimiter = opening[3] ?? "";
  const commandTokens = commandPrefix
    .trim()
    .split(/\s+/)
    .map(getInterpreterToken)
    .filter((value): value is string => Boolean(value));
  const commandHint = [...commandTokens]
    .reverse()
    .find((token) =>
      /^(python\d*|node(?:js)?|bun|deno|bash|zsh|fish|sh)$/.test(token),
    );

  return (
    mappingForLanguageHint(commandHint?.replace(/\d+$/, "") ?? "") ??
    mappingForLanguageHint(delimiter)
  );
}

function getMappingFromContent(previewText: string): HighlightMapping | null {
  const scores = {
    bash: 0,
    javascript: 0,
    python: 0,
  };

  if (/^\s*(?:def|class|from\s+\S+\s+import|import)\b/m.test(previewText)) {
    scores.python += 3;
  }
  if (/\b(?:print|range|len)\s*\(/.test(previewText)) scores.python += 1;
  if (/\bif\s+__name__\s*==/.test(previewText)) scores.python += 3;

  if (/^\s*(?:const|let|var|function|import|export)\b/m.test(previewText)) {
    scores.javascript += 3;
  }
  if (/(?:=>|console\.(?:log|error)|require\s*\()/.test(previewText)) {
    scores.javascript += 2;
  }

  if (/^\s*(?:export|source|set\s+-[a-z]|function)\b/m.test(previewText)) {
    scores.bash += 3;
  }
  if (/(?:\$\{|\$\(|\b(?:then|fi|done)\s*$)/m.test(previewText)) {
    scores.bash += 2;
  }

  const ranked = Object.entries(scores).sort(
    (left, right) => right[1] - left[1],
  );
  const [language, score] = ranked[0] ?? ["", 0];
  if (score < 2) {
    return null;
  }
  return {
    language,
    label: language === "javascript" ? "javascript" : language,
  };
}

export function getPreviewHighlightResult(
  path: string | null,
  previewText: string,
): PreviewHighlightResult {
  const mapping =
    getMappingFromHeredoc(previewText) ??
    getMappingFromShebang(previewText) ??
    getMappingFromExtension(path) ??
    getMappingFromContent(previewText);

  if (!mapping) {
    return {
      highlighted: false,
      html: escapeHtml(previewText),
      label: "plain text",
    };
  }

  try {
    return {
      highlighted: true,
      html: hljs.highlight(previewText, {
        language: mapping.language,
        ignoreIllegals: true,
      }).value,
      label: mapping.label,
    };
  } catch (error) {
    console.warn(
      "syntax highlighting failed; rendering escaped plain text",
      error,
    );
    return {
      highlighted: false,
      html: escapeHtml(previewText),
      label: "plain text",
    };
  }
}
