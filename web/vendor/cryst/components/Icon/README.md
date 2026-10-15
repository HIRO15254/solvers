Renders a Tabler icon by name; use it wherever a glyph reinforces information (labels, list rows, empty states, buttons).

```jsx
<Icon name="settings" />
<Icon name="alert-triangle" size={20} color="var(--warning)" />
<Icon name="heart" filled />
```

- Defaults to 16px in `currentColor`. Use `filled` only for emphasis (favorited, active).
- The webfont is self-hosted with the system; load the tokens and bundle stylesheets and icons work offline.
- Replaces lucide-react in shadcn code: swap `<Settings />` for `<Icon name="settings" />`.
- Never use emoji, Unicode symbols or hand-drawn SVG as icons.
