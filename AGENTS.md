# AGENTS.md — Seelen UI Launcher for Android

Android launcher written 100% in Rust (`android-activity` + `ndk`, no Kotlin/Java) with a [Slint](https://slint.dev) UI
(`ui/*.slint`). See `README.md` for build/run instructions.

## Design (mandatory)

This project follows the **Seelen UI design manifesto**: `../Seelen-UI/documentation/design-system.md`. Read it before
touching any UI (`ui/**/*.slint`) or anything that decides how the UI looks. It is a contract, not a suggestion: if a
change conflicts with the manifesto, the change is wrong. Changing the design language requires an explicit decision
from the maintainers.

The manifesto is written for the desktop (CSS/webviews/Windows). Its principles apply here unchanged; only the
mechanics are translated to Slint/Android:

| Manifesto (desktop)                                            | This project (Slint / Android)                                                                                                                       |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| CSS variables (`--slu-std-*`, `--color-*`, `--spacing-*`, ...) | Properties of Slint `global` singletons defined once in a shared tokens file under `ui/`, using the **same names and values** (e.g. `Tokens.spacing-xs`). |
| `--system-accent-*` = Windows accent                           | The user's **Android system accent** (dynamic color when available), exposed from Rust into the tokens global. Never a fixed brand color.             |
| Light/dark via `color-scheme` / `light-dark()`                 | Follow the system scheme (`Palette.color-scheme`); tokens resolve per scheme inside the global. Components never branch on the scheme themselves.    |
| `data-skin` standard components                                | Shared components (button, input, switch, list item, popover...) defined once under `ui/` and reused everywhere. No one-off variants per page.       |
| Default theme holds widget visuals                             | Visual values come from the tokens global and the shared components, never literals inside page components.                                         |
| `rem` font sizes, system font                                  | `rem` units for font sizes, never `px`; never set `font-family` (system font, no monospace).                                                          |
| i18n                                                           | All visible text through `I18n.t("key")` (see [i18n](#i18n)).                                                                                        |

Rules (summary of the manifesto — the manifesto wins on any doubt):

- **UX comes first**: rules may only be broken for a functional need (legibility, state feedback, affordance), never
  for looks. The accepted exceptions are the manifesto's table; a new one (e.g. translucent launcher surfaces over the
  wallpaper) needs maintainer approval.
- **Flat design**: solid opaque surfaces, `shadow-s/m/l` tokens only, soft rounded corners. No gradients, glows,
  glass/acrylic, borders or eye candy. Depth = surface tone (`bg` / `bg-light` / `bg-dark` elevation) + shadow.
- **Colors are never hardcoded** (no `#fff`, `black`, `white`, `rgb()` literals) and never derived with ad-hoc alphas;
  hover/pressed/selected use the `ui-*-overlay` tokens. Text on accent uses the `ui-fg` token. De-emphasis uses the
  `fg-secondary/muted/disabled` tokens, never `opacity` (only disabled = 50% opacity).
- **Accent ALWAYS = the user's system accent.** Hue scales (red, yellow, green, blue...) only convey meaning:
  error = red, warning = yellow, success = green, info = blue. Never accent or decoration.
- **Spacing, radius, shadows always from tokens** (`2xs` 4 · `xs` 8 · `s` 12 · `m` 16 · `l` 20 · `xl` 24 · `2xl` 32;
  radius `xs` 4 · `s` 6 · `m` 10 · `l` 16 · `full`). Element width/height are not tokenized.
- **Concentric corners**: inner radius = `max(0, outer radius - padding)`.
- **No framing borders**: separate with spacing and surface tone; dividers only when needed, 1px in `fg-muted`. Outlines
  only for focus and state indicators (selection, drop target, field error).
- **Standard components, defined once**; one primary (solid/accent) action per view.
- **Motion**: avoid animations; only for show/hide, control state changes and ongoing-state indicators. The UI must
  work with all animations disabled.
- **Consistency over novelty**: never redesign, add decorative effects or invent one-off control styles unless the
  maintainers explicitly ask.
- Check every UI change in **light and dark** mode and with a **non-default accent**. Use the manifesto's checklist
  (§9) before finishing.

Where things live:

- Tokens: `ui/tokens.slint` (`Tokens` global: spacing, radius, gray scales, `slu-std-*`, `system-accent-*`, shadows).
  The accent is read from Android (`system-modules/theme.rs`, Material You tones on 12+) and set from Rust.
- Standard components: `ui/components/` (`button`, `text-field`, `select`, `menu` = popover surface + items,
  `wallpaper-label`). App entries share their touch behaviour through `app-item-base.slint`.

Launcher-specific exception (maintainer-approved): Home and the Apps Menu draw directly over the **system wallpaper**
(transparent window, wallpaper theme in the manifest). Text there uses `Tokens.wallpaper-fg-color`, picked from the
wallpaper's own brightness hint (content-derived color), plus the manifesto's "text-shadow over dynamic images"
exception (`WallpaperLabel`). Popups, inputs and drop targets on top of it stay opaque standard surfaces.

Slint gotchas (1.17):

- `ScrollView`/`ListView` ignore touch drags unless `mouse-drag-pan-enabled: true`. Always set it.
- Inside a `PopupWindow`, run the action **before** `popup.close()`: after closing, the popup's bindings (e.g.
  `root.app`) read default values.
- Gesture parents (`SwipeGestureHandler`, Flickable) steal moves from children; an entry that recognized a long press
  sets `Interaction.item-lifted`, and gesture handlers are disabled while it's set.

## i18n

- Translations live in `i18n/<lang>.yml`. **Only edit `i18n/en.yml`**; every other language file (and `hash.yml`) is
  generated by `npm run translate` (`scripts/translate/mod.ts`, run directly by Node, no Deno/tsx). Never hand-edit the
  generated files.
- In Slint, every visible string uses the `I18n` global (`ui/i18n.slint`): `text: I18n.t("home.open_settings");`. Never
  write literal user-facing strings in `.slint` files.
- In Rust, translations are embedded at compile time by `rust_i18n::i18n!("i18n", fallback = "en")` (`src/lib.rs`), and
  the device locale is read from `java.util.Locale` (`src/system-modules/locale.rs`). Every new window must call
  `setup_i18n(window.global::<I18n>(), &locale)` before running. Rust-side strings use `rust_i18n::t!("key")`.
- Keys are grouped by page/feature in snake_case (`home.*`, `settings.*`).
