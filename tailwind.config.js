/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        nps: {
          primary: "var(--bg-primary)",
          secondary: "var(--bg-secondary)",
          tertiary: "var(--bg-tertiary)",
          hover: "var(--bg-hover)",
          card: "var(--bg-card)",
          accent: "var(--accent)",
          "accent-hover": "var(--accent-hover)",
          success: "var(--success)",
          danger: "var(--danger)",
          text: "var(--text-primary)",
          "text-dim": "var(--text-secondary)",
          border: "var(--border)",
        },
      },
    },
  },
  plugins: [],
};