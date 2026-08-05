import fs from "node:fs";
import path from "node:path";

import { restoreElements } from "../vendors/excalidraw/packages/excalidraw/data/restore";
import { exportToSvg } from "../vendors/excalidraw/packages/excalidraw/scene/export";

const loadFixture = (name: string) =>
  JSON.parse(fs.readFileSync(path.resolve(name), "utf8"));

describe("svg2excal target profile", () => {
  it.each([
    "fixtures/compat/minimal.excalidraw",
    "target/compat/generated-minimal.excalidraw",
  ])("restores null indices and renders %s", async (fixturePath) => {
    const fixture = loadFixture(fixturePath);
    const restored = restoreElements(fixture.elements as never, null);

    expect(restored).toHaveLength(fixture.elements.length);
    expect(restored.map((element) => element.id)).toEqual(
      fixture.elements.map((element: { id: string }) => element.id),
    );
    expect(restored.map((element) => element.type)).toEqual(
      fixture.elements.map((element: { type: string }) => element.type),
    );
    expect(fixture.elements.every((element: { index: null }) => element.index === null)).toBe(true);
    expect(restored.every((element) => Boolean(element.index))).toBe(true);

    const reloaded = restoreElements(
      JSON.parse(JSON.stringify(restored)) as never,
      null,
    );
    expect(reloaded.map((element) => element.id)).toEqual(
      restored.map((element) => element.id),
    );

    if (fixturePath.includes("generated")) {
      const line = restored.find((element) => element.type === "line");
      expect(line).toMatchObject({
        startBinding: null,
        endBinding: null,
        startArrowhead: null,
        endArrowhead: null,
        polygon: false,
      });
      expect(line?.points.at(0)).toEqual(line?.points.at(-1));
      const arrow = restored.find((element) => element.type === "arrow");
      expect(arrow).toMatchObject({
        startBinding: null,
        endBinding: null,
        startArrowhead: null,
        endArrowhead: "triangle",
        elbowed: false,
      });
      expect(restored.some((element) => element.type === "image")).toBe(true);
      expect(Object.keys(fixture.files)).toHaveLength(1);
      expect(
        Object.values(fixture.files).every(
          (file) =>
            typeof (file as { dataURL?: string }).dataURL === "string",
        ),
      ).toBe(true);
      expect(
        restored.every((element) => Boolean(element.customData?.svg2excal)),
      ).toBe(true);
    }

    const svg = await exportToSvg(
      restored,
      {
        exportBackground: true,
        exportPadding: 0,
        exportScale: 1,
        viewBackgroundColor: fixture.appState.viewBackgroundColor,
      },
      fixture.files ?? {},
      { skipInliningFonts: true },
    );

    expect(svg.tagName.toLowerCase()).toBe("svg");
    expect(svg.querySelectorAll("rect").length).toBeGreaterThan(0);
    if (fixturePath.includes("generated")) {
      expect(svg.querySelector("image")?.getAttribute("href")).toMatch(
        /^data:image\/png;base64,/,
      );
    }
  });
});
