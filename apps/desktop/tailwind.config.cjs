/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    "./index.html",
    "./src/**/*.{ts,tsx}",
    "./node_modules/streamdown/dist/*.js",
  ],
  theme: {
    extend: {
      colors: {
        background: "var(--background)",
        foreground: "var(--foreground)",
        border: "var(--border)",
        primary: "var(--primary)",
        "muted-foreground": "var(--muted-foreground)",
        muted: "var(--secondary)",
        secondary: "var(--secondary)",
        plankton: {
          accent: "#ff3000",
          line: "#000000",
          muted: "#f2f2f2",
        },
      },
    },
  },
  plugins: [],
};
