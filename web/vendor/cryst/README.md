Cryst is a calm, compact style for apps on desktop and mobile: shadcn/ui conventions and variable names, a Linear-like dark-first look, and Tabler Icons carrying information everywhere. It suits any kind of app, from dashboards and settings to content, commerce and communication. Build with the tokens and the components in `window.Cryst`. (Code written against the old `window.KoguDesignSystem_6c0df3` still works: the bundle keeps that name as an alias.)

There is no logo. Wherever a mark would go, set "Cryst" in plain type (`title-card` or `m-title`, `foreground`); never draw or invent a mark.

## Content fundamentals

- Write calm, precise, engineering-grade copy. Labels are short and verb-first: "Create", "Save changes", "Add contact".
- Use sentence case everywhere. Never Title Case, never exclamation marks.
- Address the user as "you" and speak plainly: "This cannot be undone.", "This email is already in use".
- Never use emoji. Carry meaning with a Tabler icon plus text.
- Set IDs, shortcuts, timestamps and code in mono (`code`, `code-small`): `INV-2041`, `⌘K`, `2026-08-14 09:32`, amounts in tables.
- Japanese copy follows the same rules: 簡潔・断定調、体言止めを許容、敬体は説明文のみ。 Button labels stay short: 「キャンセル」「作成」.
- Empty states are one icon plus one short line in `muted-foreground`. No illustrations.

## Visual foundations

### Color

- One accent: `primary` (Cryst blue). Use it for primary Buttons, the active tab, checked controls (including the selected SegmentedControl and RadioCard outline), links and the BottomSheet ✓ confirm icon. Nothing else is blue except `info`.
- The product is dark-first: the dark theme is its home (near-black `background`, layered `card` and `popover`). The light theme mirrors it.
- Separate surfaces with `border` hairlines, not with shade alone: `background` → `card` → `popover`, each with a 1px `border`.
- Text: `foreground` for primary text, `muted-foreground` for metadata, hints, placeholders and inactive tabs. Both read at 4.5:1 or more on every surface in both themes.
- Hover and selected rows tint to `accent`; recessed fills (kbd, input groups, the pill-tab track) use `muted`, and the selected pill sits on `tab-active`; the secondary Button and quiet chips use `secondary`.
- Status colors: `success` (complete), `warning` (pending, at risk), `destructive` (errors, delete), `info` (notices), `neutral` (no state). Always pair a status color with an icon or a word — Alert, Toast and Badge already do, from one shared table (`STATUS`): `neutral` info-circle, `info` info-circle, `success` circle-check, `warning` alert-triangle, `destructive` circle-x.
- Text on fills uses the matching `*-foreground` token, never a literal white: labels are white on light-theme fills and near-black ink on the brighter dark-theme fills.
- Contrast is part of the palette: every text pair holds at least 4.5:1 in both themes (status colors also on their badge tints), and the focus `ring` holds at least 3:1 on every surface. `input` borders are a quiet hairline by choice (1.2–1.5:1): a field reads by its label and `card` fill, and focus turns its border to `ring`. The light-theme accent and status colors are deeper shades of the original hues for this reason; keep them, and pick a new color only after checking it against the same grounds.
- Keep shadcn/ui variable names so shadcn themes drop in directly.

### Type

- **Two faces, and no third.** Sans is **Noto Sans JP** (400–700) for the whole interface, Japanese and Latin alike; mono is **JetBrains Mono** (400–700) for IDs, shortcuts, timestamps, figures in tables and code. Never introduce another face — not for headings, not for numerals.
- Weights in use: 400 body · 500 controls and labels · 600 headings and titles · 700 bold emphasis and the mobile large title.
- Delivery: both text faces are local variable WOFF2 files in `fonts/`, declared by the app's `src/cryst/fonts.css`. Noto Sans JP (`noto-sans-jp-variable-jis1.woff2`) covers Latin, kana and JIS level-1 kanji; JetBrains Mono (`jetbrains-mono-variable-latin.woff2`) covers Latin. No Google Fonts/CDN request is made. Characters outside the bundled subsets fall back to platform fonts.
- Desktop UI text is 13px (`ui-text`, `ui-label`). Long-form text steps up to 15px (`reading`). Meta is 11px (`meta`).
- Headings are semibold with tight tracking (`tracking-heading`): `title-page` 32px, `title-section` 21px, `title-card` 15px. Overlines use `overline` in uppercase.
- The 13px scale is desktop-only. Mobile surfaces use the `m-` styles: `m-large-title` 34px bold, `m-screen-title` 22px, `m-title` 16px semibold, `m-body` 15px, `m-secondary` 14px, `m-footnote` 12px, `m-tab` 10px. Keep line height at 1.3× or more; 11px is the absolute floor.

#### Bundled font provenance and licenses

The font licenses remain separate from the repository's source-code license.
Original copyright notices and full license texts are stored beside the fonts:

| Asset | Upstream source/version | License text | Bytes |
|---|---|---|---:|
| `fonts/noto-sans-jp-variable-jis1.woff2` | [Google Fonts Noto Sans JP 2.004](https://github.com/google/fonts/tree/66a36c8c94b1a5d992ee4e7f392fccfe4945767c/ofl/notosansjp), subset for this app | `fonts/OFL-NotoSansJP.txt` (SIL OFL 1.1) | 1,052,740 |
| `fonts/jetbrains-mono-variable-latin.woff2` | [JetBrains Mono 2.211](https://github.com/JetBrains/JetBrainsMono); Latin subset supplied by the canonical Cryst artifact | `fonts/OFL-JetBrainsMono.txt` (SIL OFL 1.1) | 40,404 |
| `fonts/tabler-icons.woff2` | [Tabler Icons 3.34.1](https://github.com/tabler/tabler-icons/tree/v3.34.1), supplied by the canonical Cryst artifact | `fonts/LICENSE-TablerIcons.txt` (MIT) | 829,244 |

Noto was generated with FontTools 4.61.1 and Brotli 1.2.0 from the pinned
upstream `NotoSansJP[wght].ttf` (9,589,900 bytes; SHA-256
`c2f3b4d463500a2ddcd3849cded1fceeb9fd6d1c32e6cbecd568453ba50fc68f`).
The upstream repository provides the complete variable font as TTF, so it was
converted to WOFF2 during subsetting rather than assembled from the browser's
many Google Fonts WOFF2 shards.

The requested Unicode set is U+0020–024F (Latin), U+2000–206F (punctuation),
U+20A0–20CF (currency), U+3000–30FF (Japanese punctuation, hiragana and katakana),
U+31F0–31FF (katakana extensions), U+FF00–FFEF (half/full-width forms), plus all
2,965 JIS X 0208 level-1 kanji. The latter are obtained by decoding the assigned
cells of JIS rows 16–47 with Python's `euc_jp` codec (bytes `row + 0xA0`,
`cell + 0xA0`, cells 1–94). Unassigned cells are omitted. Only codepoints present
in the upstream font are retained: 3,739 codepoints and 4,894 glyphs, including
all 2,965 level-1 kanji. JIS level-2 kanji and other characters outside this set
use platform fallback; arbitrary Japanese names are not guaranteed by this subset.

FontTools `subset.Options` retains all layout features (`layout_features=["*"]`)
and names (`name_IDs=["*"]`, `name_legacy=True`, `name_languages=["*"]`), with
other options at their defaults. After subsetting, `instantiateVariableFont`
restricts `wght` to `(400, 400, 700)` (minimum, default, maximum); the result is
saved with `font.flavor = "woff2"`. This preserves the actual UI weights
400/500/600/700 and interpolation between them, rather than shipping four static
faces or the unused 100–900 range.
The output SHA-256 is
`59d7c076eb42e316e223119a2977275c7181dc824f7d4fbbb5444001f2d72dc6`.


### Spacing and layout

- Space in 4px steps (`space-1` … `space-16`), with 2px and 6px half-steps (`space-05`, `space-15`) for dense rows.
- Desktop control heights: `control-sm` 28px, `control-md` 32px (default), `control-lg` 36px.
- Every interactive component has a mobile size, `size="xl"` = `m-control` 44px: Button, IconButton, Input, Select, Textarea (15px text), Checkbox and Radio (20px glyph in a 44px row), Switch (40×24 track in a 44px row), SegmentedControl (44px pills, 14px labels), RadioCard (44px-minimum cards), and Tabs (a 44px pill track or 44px underline tabs, with `fullWidth` to split the screen width).
- Mobile targets are 44px throughout, the iOS HIG and WCAG 2.5.5 minimum: rows and controls 44px (`m-list-row`, `m-control`), nav bar and tab bar 44px (`m-navbar-height`, `m-tabbar-height`), screen inset 16px (`m-inset`), sheet corners 12px (`m-sheet-radius`). Keep the density in the type and the spacing, not in the size of things people tap.
- Desktop layout: `sidebar-width` 244px sidebar, reading and settings content up to `content-max` 1024px.

### Surfaces, borders, shadows, radii

- Surfaces are flat and opaque. No gradients, textures or background images.
- The `border` hairline does the structural work. Shadows are barely there and only on floating layers: `shadow-sm` on secondary Buttons and Cards, `shadow-md` on dropdowns and Toasts, `shadow-lg` on Dialog and BottomSheet, `shadow-popover` (ring plus depth) on popovers.
- Radii: `radius-sm` 4px inner controls · `radius-md` 6px controls · `radius-lg` 8px cards · `radius-xl` 12px dialogs and sheets · `radius-full` pills and switches.
- The only transparency is the overlay scrim: 50% black with a 4px blur, shared by Dialog and BottomSheet.

### States and motion

- Hover: tint the background to `accent`; ghost labels move from `muted-foreground` to `foreground`. Primary fills brighten slightly.
- Press: a slight brightness drop and a 0.5px downward nudge.
- Focus: a 2px `ring` offset by 2px of `background` on buttons; a 3px `selection` halo on fields.
- Disabled: 50% opacity, not-allowed cursor.
- Motion is fast and quiet: `duration-fast` 100ms for hover and press, `duration-base` 160ms for toggles and tabs, `duration-slow` 250ms for overlays, always `ease-out`. Fades and small translates only; never bounces, springs or scale-ups.

### Density

- The default density is compact (13px desktop text, 32px controls), which keeps data-heavy screens calm. For reading-first or consumer screens, step body text up to `reading` and use `lg` controls; on mobile always use the `m-` scale and `xl` controls.

### Desktop vs. mobile containers

- On desktop, confirmations and short forms open in a centered Dialog.
- On mobile, every form and picker opens in a BottomSheet: a full-width panel anchored to the bottom, 12px top corners, the same scrim, a centered title. It comes in two kinds, and the kind decides the top of the sheet:
  - **Dismissible** — safe to close at any time (pickers, menus, read-only panels, anything that saves as it goes). It has a grab, and dragging the grab down, a scrim tap or Esc closes it. No header icons.
  - **Explicit** — closing needs an explicit decision (a form saved as a whole). No grab. An × icon cancels top-left, a ✓ icon (`primary`) confirms top-right; nothing else closes it.
- A grab always means "safe to close": never put one on an explicit sheet. Header controls are icons, never text. No footer button row. Never use a centered Dialog on mobile, except for a destructive confirmation.
- BottomSheet is not part of shadcn/ui; treat it as a shadcn custom component built on these tokens.

### Imagery

- The interface itself carries no decorative imagery. Content images (photos, products, avatars) sit in `radius-lg` frames on `muted`; illustrations, if any, stay neutral, cool and unadorned.

## Iconography

- Use Tabler Icons only. The webfont (`@tabler/icons-webfont@3.34.1`, outline and filled glyphs) is self-hosted: `fonts/tabler-icons.woff2`, with the `.ti` classes in the bundle stylesheet, so icons render without reaching an external CDN. Load the tokens stylesheet and the bundle stylesheet together wherever you use icons.
- In React, render icons with `<Icon name="settings" />`; in plain HTML, `<i class="ti ti-settings"></i>`. Icons default to 16px in `currentColor`; use the filled variant only for emphasis (starred, active).
- Icons lead information. Status: `circle-check`, `alert-triangle`, `circle-x`, `info-circle`, `loader-2`. Metadata: `calendar`, `clock`, `user`, `folder`, `map-pin`, `tag`. Navigation: `home`, `search`, `bell`, `settings`. Metadata is always icon plus text.
- Coming from shadcn/ui code? Replace lucide-react icons with `<Icon name="…" />` using the Tabler name.
- Never use emoji, Unicode symbols as icons, or hand-drawn SVG.

## Components

36 components, grouped as the author grouped them. Every one has a live preview, usage guidelines and a props file beside it; `components/index.d.ts` lists the whole surface.

- Core: **Icon**, the required wrapper that enforces the Tabler rule.
- Actions: **Button** (`default`, `secondary`, `outline`, `ghost`, `destructive`, `link`), **IconButton** (label required).
- Forms: **Input** and **Textarea** (with **Label**), **Select**, **Checkbox**, **Radio** (**RadioGroup**), **SegmentedControl**, **RadioCard** (**RadioCardGroup**), **Switch**.
- Display: **Card**, **Badge**, **Avatar**, **Tag**, **Kbd**, **Alert**, **Table**, **Separator**, **Skeleton**, **Progress**, **EmptyState**, **SelectionBar**, **FilterBar**.
- Navigation: **Tabs**, **Pagination**, **Sidebar**, **List**.
- Overlay: **Dialog** (desktop only), **BottomSheet** (mobile forms), **DropdownMenu**, **Popover**, **CommandPalette**, **Toast** (with **Toaster** and `toast()`), **Tooltip**, **ShortcutsDialog**.

Intentional additions beyond shadcn/ui's set: **Icon** (enforces the icon rule), **IconButton** (an icon button with a required label), **Tag** (removable label chips), **BottomSheet** (the default mobile form container, in place of shadcn's Drawer), **SegmentedControl** (a short pick-one inside a form, distinct from Tabs), **RadioCard** (a pick-one of rich options), and the tool set — **Sidebar**, **List**, **CommandPalette**, **ShortcutsDialog**, **Kbd**, **SelectionBar**, **FilterBar**, **EmptyState** — which shadcn leaves to the app.

## Conventions

These hold across every component, so one component teaches the next:

- **Sizes.** `sm` · `default` (32px) · `lg` on desktop controls, `xl` (44px) for touch. Every component that takes a size accepts `default` as the name of its own default.
- **Status.** One vocabulary — `neutral`, `info`, `success`, `warning`, `destructive` — with one icon and one accent each, shared by Alert, Toast and Badge. `warning` and `destructive` are announced as alerts to screen readers.
- **State callbacks.** `onValueChange` (Select, Tabs, RadioGroup, SegmentedControl, RadioCardGroup, Pagination's `onPageChange`), `onCheckedChange` (Checkbox, Switch), `onOpenChange` (Dialog, BottomSheet, Popover, DropdownMenu, Tooltip). Native `onChange` still fires on form controls, and native props pass through everywhere.
- **Labels.** A visible label is the `label` prop on form controls; on IconButton and Progress, `label` is the accessible name instead. Anything icon-only needs one.
- **Composition.** Where shadcn composes, the parts are exported (`CardHeader`, `TabsTrigger`, `TableRow`, `DropdownMenuItem`, …). Roots carry shadcn's `data-slot`; internal class names are prefixed `cr-` and are an implementation detail — style through tokens, not through them.
- **The consumer owns the data.** Components render and call back; sorting, paging, fetching and open/closed state live in the app.

## Keyboard

A tool is judged on how fast it is without the mouse. These bindings are the system's, and an app that has the action should use the key:

- `⌘K` / `Ctrl+K` opens the **CommandPalette** — every action and destination, searchable. `/` focuses the screen's search field. `?` opens the **ShortcutsDialog**; both ignore the key while a field has focus.
- In a **List**: `↑` `↓` move, `J` `K` move too when the app opts in, `Home` / `End` jump, `↵` opens the focused row, `X` selects it.
- `Esc` always steps back: close the palette, the Dialog, a dismissible BottomSheet, the Popover, the menu; then clear the search field. An explicit BottomSheet ignores it — only × and ✓ close that one.
- `⌘↵` submits a form from inside a field; `⌘S` saves where saving is explicit.
- Sequences read as "G then O" and are written as two caps; combinations sit next to each other.
- Show the shortcut where the action is — Tooltip's `shortcut`, DropdownMenuItem's `shortcut`, the palette's `shortcut` — and list every one of them in the ShortcutsDialog. Never bind a key that appears nowhere.

## Tool patterns

- **Screen frame.** Sidebar on the left, a 44–48px header with the title and the primary action, then the work area. Below 720px the Sidebar hides and the mobile tab bar takes over.
- **A list screen, in order:** toolbar (search, `Filter`, `Columns`, primary action) → FilterBar chips → SelectionBar while rows are picked → Table or List → Pagination. Skeleton rows on first load, EmptyState when nothing matches.
- **Selection.** A Checkbox in each row's `leading`, a header Checkbox that goes `indeterminate` on partial selection, and SelectionBar for what to do with them. Destructive bulk actions confirm in a Dialog.
- **Columns.** A DropdownMenu of `DropdownMenuCheckboxItem`s; the menu stays open while toggling.
- **Detail.** Open the row in a detail panel beside the list on desktop (list stays visible), full-screen on mobile.
- **Feedback.** Toast for what just happened, Alert for a condition that stays, Progress for work with a length, and `aria-live` is already handled by those components.

## Choosing between components

- **Alert or Toast?** Alert is a state of the page and stays until it changes; Toast is the result of an action and disappears. One invalid field is neither: that is Input's `error`.
- **Badge or Tag?** Badge is a state the system computes; Tag is a label a person attached, and can remove.
- **Select or DropdownMenu?** Select picks a value that stays in the field; DropdownMenu runs an action.
- **Dialog, BottomSheet, Popover, DropdownMenu or Tooltip?** Dialog blocks the task until answered (BottomSheet is its form on touch screens); Popover is a panel that can be ignored; DropdownMenu is a list of actions; Tooltip is one line of hint.
- **Skeleton or Progress?** Skeleton for the first load of an empty screen; Progress for work with a length, or a refresh of content already on screen.
- **IconButton or Button size="icon"?** IconButton, which requires the label. `size="icon"` is there so shadcn snippets drop in.
- **Tabs or Pagination?** Tabs switch between views of the same thing; Pagination moves through one long list.
- **Tabs or SegmentedControl?** Tabs swap the view below them; a SegmentedControl sets a value that gets saved. Pill Tabs are a track with a raised pill, a SegmentedControl is separate outlined pills, so the two never read as each other.
- **RadioGroup, SegmentedControl or RadioCard?** Plain labels in a list → RadioGroup. Two to four one-word choices side by side → SegmentedControl. Options that need a description or a value to compare → RadioCard. More than five → Select.
- **Dismissible or explicit BottomSheet?** If closing it by accident loses nothing, it is dismissible (grab, no icons). If it holds edits that are saved together, it is explicit (× and ✓, no grab).
- **List or Table?** A List when a record reads as one line; a Table when columns need comparing. On mobile, a List — always.
- **CommandPalette or a search field?** The palette finds actions and places; the search field filters what is on screen. A tool wants both, and `/` belongs to the field.

## Using it like shadcn/ui

- Props follow shadcn/ui: `variant` and `size` on Button and Badge, `asChild` on Button and Badge, `open`/`onOpenChange` on Dialog, BottomSheet and Tooltip, `checked`/`onCheckedChange` on Checkbox and Switch, `value`/`defaultValue`/`onValueChange` on RadioGroup, SegmentedControl, RadioCardGroup, Select and Tabs, and native props pass through. Form controls forward refs.
- Every component takes shadcn's props first; the extras (`icon`, `label`, `hint`, `error`, `size="xl"`) are additions, never replacements.
- Composition parts are exported where shadcn composes: `CardHeader`, `CardTitle`, `CardDescription`, `CardAction`, `CardContent`, `CardFooter`; `TabsList`, `TabsTrigger`, `TabsContent`; `AvatarImage`, `AvatarFallback`; `RadioGroup`, `RadioGroupItem`; `RadioCardGroup`, `RadioCard`; `TableHeader`, `TableBody`, `TableFooter`, `TableRow`, `TableHead`, `TableCell`, `TableCaption`; `AlertTitle`, `AlertDescription`; `PopoverTrigger`, `PopoverContent`; `DropdownMenuTrigger`, `DropdownMenuContent`, `DropdownMenuItem`, `DropdownMenuLabel`, `DropdownMenuSeparator`; `Toaster` and `toast()` in Sonner's shape; `buttonVariants()` for button styling on other elements. Roots carry shadcn's `data-slot` names, and `components/index.d.ts` lists every export in one place.
- Every shadcn color variable exists, including `chart-1` … `chart-5` and the `sidebar-*` set. Dark mode answers to `data-theme="dark"` and to the shadcn `class="dark"`.
- Known differences: Select is a styled native select (no SelectTrigger/SelectContent); Dialog takes title, description and footer as props instead of DialogHeader/DialogFooter parts; Tooltip wraps its trigger instead of using TooltipTrigger/TooltipContent; Badge `destructive` is a tint; icons are Tabler, not lucide.

## Exceptions

This system is the whole vocabulary. A product screen that needs something it does not describe builds it as an **exception**: local to that screen, recorded in the product with its reason, and never precedent for another screen. If the same exception is needed a second time, propose it here as a component instead of copying it.

Exceptions currently recorded by Sapphire 2's live session screen (its `docs/design/cryst-exceptions.md` holds the reasons). None of them is a Cryst pattern:

- The bottom ActionBar: 48px tiles with the icon above an 11px label.
- Inline edit fields that show no border until focused (player name, scanned names), committed on blur.
- Leave and Delete as an outline Button with `destructive` text.
- The full-height Timeline sheet, and the event-type colors on its markers.
- The hero seat and hero row highlighted in `primary`, and seat markers and avatars tinted by player kind.
- The Blinds tab rows: the running level tinted with `primary`, breaks labeled in `warning`, and a pill-shaped Games button tinted with `info` once a level has its own games.
- The Blinds tab level rows below the 44px target and on one line per level: 32px cells for the blinds, the ante and the minutes, a 24px icon-only Games button and a 22px remove button; a level with games takes one more line per game group, each stake a 32px field with its caption stacked above the value, so a whole structure fits on screen.
- The paused-session veil: `background` at 72% with a 2px blur over the cockpit (the only transparency besides the scrim).
- The compact master-drift notice (tighter padding than Alert): the banner on the session sheet's Basics and Blinds tabs, and the row inside the Overview master card.
- The unlinked-master notices tinted with `warning`: the session card's warning border and fill, and the borderless notice at the top of the master link sheet.
- The seat map: seat names at 8px, below the 11px floor, and the 34px square scan and clear-seats buttons on the table, below the 44px target.
- The sans face: the app still ships Noto Sans (Latin) instead of Noto Sans JP until it adopts the Japanese face.

## Layout patterns

- Desktop app: a `sidebar-width` (244px) navigation column on `background`, split by a `border` hairline; a 44–48px header with the page title (`title-card`) and actions on the right (primary action last); content up to `content-max` with `space-4`–`space-6` padding.
- Mobile app: a `m-navbar-height` nav bar with the screen title (`m-title`), content inset by `m-inset`, `m-list-row` rows separated by hairlines, a `m-tabbar-height` bottom tab bar with `m-tab` labels under icons, and forms in a BottomSheet.
- Lists: icon, title, then metadata as icon plus text in `muted-foreground`; row actions as ghost IconButtons revealed on hover (always visible on touch), or a DropdownMenu at the end of the row.
- Tables and long lists: a Table on desktop, list rows on mobile; Pagination underneath, Skeleton rows for the first load, and an Alert above the list when something is wrong with the whole set.
- Empty states: one icon and one short line; offer the primary action as a Button beneath.

## Migrated from a legacy design system

This system was carried over from the standalone version on 2026-09-17. The part of this README the author wrote predates the move, so any file names in it are the old ones. Where things are now:

- `styles.css`, `tokens/fonts.css`, `tokens/icons.css`, `tokens/colors.css`, `tokens/typography.css`, `tokens/spacing.css`, … and 3 more (the global stylesheets) → `project/components/bundle.css`, with the token declarations moved to `project/tokens.json` (`project/tokens.css` is generated from them)
- `_ds_bundle.js` → `project/components/bundle.js` (its duplicate copy under `docs/` was removed)
- showcase pages (`components/*/*.card.html`), foundation cards (`guidelines/*.html`), the sample issue-tracker kit (`ui_kits/cryst-app/`) and the starter templates (`templates/`): removed; each component has its own preview and the guide above covers the foundations and layouts
- `tokens/icons.css` (Tabler from jsDelivr) → self-hosted `project/fonts/tabler-icons.woff2` plus the icon classes in `project/components/bundle.css`
- the migration report, which lists what did not come across: `project/assets/notes/MIGRATION-REPORT.md`
