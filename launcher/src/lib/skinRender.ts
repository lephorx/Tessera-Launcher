import { SkinViewer } from "skinview3d";

/**
 * Still pictures of skins for the grids. One hidden 3D canvas renders them one by one;
 * a live canvas per tile would hit the browser's limit on 3D contexts.
 */
let viewer: SkinViewer | null = null;
const cache = new Map<string, Promise<string>>();
let queue: Promise<unknown> = Promise.resolve();

function get(): SkinViewer {
  viewer ??= new SkinViewer({ width: 240, height: 320, pixelRatio: 2, preserveDrawingBuffer: true, renderPaused: true, zoom: 0.9 });
  return viewer;
}

export function renderSkin(texture: string, model: "classic" | "slim", cape: string | null = null): Promise<string> {
  const key = `${model}|${cape ?? ""}|${texture}`;
  let done = cache.get(key);
  if (!done) {
    done = queue.then(async () => {
      const v = get();
      await v.loadSkin(texture, { model: model === "slim" ? "slim" : "default" });
      if (cape) await v.loadCape(cape);
      else v.loadCape(null);
      v.playerWrapper.rotation.y = -0.5;
      // a relaxed pose instead of arms straight down
      v.playerObject.skin.leftArm.rotation.z = 0.12;
      v.playerObject.skin.rightArm.rotation.z = -0.12;
      v.render();
      return v.canvas.toDataURL("image/png");
    });
    queue = done.catch(() => {});
    cache.set(key, done);
  }
  return done;
}
