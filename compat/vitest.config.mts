import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  defineConfig,
  mergeConfig,
} from "../vendors/excalidraw/node_modules/vitest/dist/config.js";

import upstream from "../vendors/excalidraw/vitest.config.mts";

const directory = path.dirname(fileURLToPath(import.meta.url));
const repository = path.resolve(directory, "..");

const merged = mergeConfig(
  upstream,
  defineConfig({
    root: repository,
    test: {
      setupFiles: [
        path.join(repository, "vendors/excalidraw/setupTests.ts"),
      ],
    },
  }),
);

export default {
  ...merged,
  test: {
    ...merged.test,
    setupFiles: [path.join(repository, "vendors/excalidraw/setupTests.ts")],
  },
};
