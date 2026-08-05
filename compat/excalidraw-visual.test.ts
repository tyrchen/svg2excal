import fs from "node:fs";
import path from "node:path";

import { restoreElements } from "../vendors/excalidraw/packages/excalidraw/data/restore";
import { exportToSvg } from "../vendors/excalidraw/packages/excalidraw/scene/export";

describe("svg2excal visual export", () => {
  it("restores and exports the canonical architecture scene", async () => {
    const fixturePath = path.resolve("target/visual/arch.excalidraw");
    const fixture = JSON.parse(fs.readFileSync(fixturePath, "utf8"));
    const restored = restoreElements(fixture.elements as never, null);

    expect(restored).toHaveLength(fixture.elements.length);
    const svg = await exportToSvg(
      restored,
      {
        exportBackground: true,
        exportPadding: 0,
        exportScale: 1,
        viewBackgroundColor: fixture.appState.viewBackgroundColor,
      },
      fixture.files,
      { skipInliningFonts: true },
    );

    expect(svg.getAttribute("width")).toBe("2180");
    expect(svg.getAttribute("height")).toBe("1420");
    fs.writeFileSync(
      path.resolve("target/visual/arch-excalidraw.svg"),
      new XMLSerializer().serializeToString(svg),
    );
  });
});
