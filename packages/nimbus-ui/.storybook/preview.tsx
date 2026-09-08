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
        { name: "dark", value: "#0a0b0c" },
        { name: "light", value: "#ffffff" },
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
