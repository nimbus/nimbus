import type { Preview } from "@storybook/react";
import { useEffect } from "react";

import "../src/styles/globals.css";

const preview: Preview = {
  parameters: {
    layout: "centered",
    a11y: { test: "error" },
    backgrounds: {
      default: "dark",
      values: [
        { name: "dark", value: "oklch(0.15 0.015 240)" },
        { name: "light", value: "oklch(0.98 0.005 240)" },
      ],
    },
  },
  globalTypes: {
    theme: {
      description: "Color theme",
      defaultValue: "dark",
      toolbar: {
        title: "Theme",
        icon: "circlehollow",
        items: [
          { value: "dark", title: "Dark" },
          { value: "light", title: "Light" },
        ],
        dynamicTitle: true,
      },
    },
  },
  decorators: [
    (Story, context) => {
      const theme = (context.globals.theme as string) ?? "dark";
      useEffect(() => {
        document.documentElement.dataset.theme = theme;
      }, [theme]);
      return <Story />;
    },
  ],
};

export default preview;
