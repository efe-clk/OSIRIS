/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        osiris: {
          bg: "#0b0f14",
          sink: "#080b0f",
          panel: "#131a22",
          raise: "#18212b",
          line: "#223040",
          ink: "#d6e1ea",
          muted: "#8a9bab",
          faint: "#5d6f80",
          accent: "#22d3ee",
          ok: "#4ade80",
          warn: "#fbbf24",
          bad: "#f87171",
        },
      },
      fontFamily: {
        sans: ['"IBM Plex Sans"', "system-ui", "-apple-system", '"Segoe UI"', "sans-serif"],
        mono: ['"IBM Plex Mono"', "ui-monospace", "SFMono-Regular", "Menlo", "Consolas", "monospace"],
      },
    },
  },
  plugins: [],
};
