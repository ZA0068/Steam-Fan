# Skins

A skin changes how the whole of Steam-Fan looks. It can set:

- the colors and sizes,
- the library layout,
- the icons,
- any CSS it likes.

To pick one, go to **Settings → Appearance → Skin**. Five skins are built in:

| Skin | Library layout | In the style of |
|---|---|---|
| Steam-Fan | grid of covers | the standard look |
| Cards | big rounded cards with titles | self-hosted libraries like Gameyfin |
| Details | a list with columns (group, kind, collections, size, status) | Playnite's details view |
| Media center | rows of big covers under a spotlight; ←→↑↓ and Enter work like a remote | Kodi and other TV media centers |
| Steam | grid | Steam's colors |

The three buttons at the bottom of the library switch layouts at any time. The slider sets the cover size. Both are remembered until you pick another skin.

## Making your own

The easiest way is to set up a look you like, then use **Save my look as a skin…**. Your colors, sizes, layout, icon set and custom CSS go into a new folder in the skins folder. **Open skins folder** shows it, so you can edit it from there. You can also start from `skins/Neon/` in this repository.

A skin is a folder:

```
skins/<name>/
  skin.json      what it is and what it sets
  style.css      optional: any CSS, applied after Steam-Fan's own
  icons/ …       optional: pictures and fonts that skin.json and style.css point to
```

`skin.json`:

```json
{
  "name": "Neon",
  "author": "you",
  "description": "Shown under its name in Settings",
  "theme": {
    "colors": { "bg": "#0a0612", "panel": "#160d26", "text": "#f6ecff", "accent": "#ff2bd6",
                "good": "#3dffb0", "warn": "#f3b94c", "bad": "#ff6f6f" },
    "preset": "default",
    "radius": 6, "font_size": 14, "font": "", "density": "compact | normal | roomy",
    "sidebar": "left | right | icons", "glow": true
  },
  "layout": "grid | list | shelves",
  "cover": 170,
  "icon_set": "glyph | line",
  "icons": { "play": "icons/play.svg", "fan": "✦" }
}
```

Every field is optional.

**theme** is where the look starts when someone picks the skin. They can still change the colors and sizes on top of it.

**icons** replaces single icons from the icon set. Each value is either a picture file in the skin's folder (`.svg`, `.png`, `.webp`, `.jpg`, `.gif`) or up to 8 characters. Markup isn't accepted, because it could run code. The icon names are:

`library pc tags fan nonsteam rom steam settings add play search refresh artwork update download remove edit grid list shelves`

**style.css** is ordinary CSS. `url(...)` in it is relative to the skin's folder, so `url(bg.jpg)` and `url(fonts/MyFont.woff2)` work. These variables hold the current colors and sizes:

`--bg --bg-2 --panel --panel-2 --line --line-2 --text --muted --faint --accent --accent-2 --good --warn --bad --sidebar --radius --r --font --font-size`

For see-through colors there are `--accent-rgb --bg-rgb --panel-rgb --good-rgb --warn-rgb --bad-rgb`, for example `rgba(var(--accent-rgb), .3)`.

Some useful selectors:
- `.card`, `.card .art`, `.card .title`: grid covers
- `.row`: list rows
- `.scard`, `.spotlight`: shelves
- `aside`: the sidebar
- `.hero`, `.btn.play`, `.vstrip`: a game's page
- `html.side-right`, `html.side-icons`, `html.compact`, `html.roomy`: layout choices

## Sharing

Zip the skin's folder and send it. The other person uses **Install skin (zip)…** or **Install skin (folder)…**.

## Safety

The stylesheet of a skin from the skins folder isn't applied until you press **Save look** and then **Keep** within 15 seconds. If you don't, it's taken back by itself. The bar with the Keep button stays visible even if the CSS hides the whole window.

If a skin still leaves Steam-Fan unusable:
- **Ctrl+Shift+0** (⌘⇧0 on a Mac) goes back to the default look;
- `steam-fan-app --default-theme` starts once without your look;
- deleting `~/.config/steam-fan/theme.json` starts over (`%APPDATA%\steam-fan\theme.json` on Windows, `~/Library/Application Support/steam-fan/theme.json` on a Mac).
