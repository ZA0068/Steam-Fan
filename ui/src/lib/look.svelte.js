// What the current skin puts in the window: icons, the library layout, the cover size.
import { ICON_SETS } from "./skins.js";

export const look = $state({
  skin: "default",
  icons: { ...ICON_SETS.glyph },
  layout: "grid",
  cover: 0,
});
