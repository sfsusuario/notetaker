/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./popup.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      // Tokens de tema (ver src/index.css): rgb(var(--x) / alpha) permite
      // usar bg-surface/50, text-fg-muted, ring-line/10, etc. en ambos temas.
      colors: {
        bg: "rgb(var(--bg) / <alpha-value>)",
        surface: "rgb(var(--surface) / <alpha-value>)",
        "surface-2": "rgb(var(--surface-2) / <alpha-value>)",
        fg: "rgb(var(--fg) / <alpha-value>)",
        "fg-muted": "rgb(var(--fg-muted) / <alpha-value>)",
        line: "rgb(var(--line) / <alpha-value>)",
      },
    },
  },
  plugins: [],
};
