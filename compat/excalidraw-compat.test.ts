import fs from "node:fs";
import path from "node:path";

import { restoreElements } from "../vendors/excalidraw/packages/excalidraw/data/restore";
import { exportToSvg } from "../vendors/excalidraw/packages/excalidraw/scene/export";

const fixture = JSON.parse(
  fs.readFileSync(
    path.resolve("fixtures/compat/minimal.excalidraw"),
    "utf8",
  ),
);

describe("svg2excal target profile", () => {
  it("restores null indices and renders the minimal v2 document", async () => {
    const restored = restoreElements(fixture.elements as never, null);

    expect(restored).toHaveLength(1);
    expect(restored[0]?.id).toBe("phase0-minimal-rect");
    expect(restored[0]?.index).toBeTruthy();

    const svg = await exportToSvg(
      restored,
      {
        exportBackground: true,
        exportPadding: 0,
        exportScale: 1,
        viewBackgroundColor: fixture.appState.viewBackgroundColor,
      },
      {},
      { skipInliningFonts: true },
    );

    expect(svg.tagName.toLowerCase()).toBe("svg");
    expect(svg.querySelectorAll("rect").length).toBeGreaterThan(0);
  });
});
