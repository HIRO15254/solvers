Icon-only square button for toolbars, row actions and window chrome; `label` is required and becomes the accessible name.

```jsx
<IconButton icon="dots" label="More options" />
<IconButton icon="bell" label="Notifications" variant="outline" />
<IconButton icon="star" label="Favorite" active />
```

- Ghost by default; `outline` for standalone actions; `secondary` for a quiet fill.
- `active` marks a toggled state and sets `aria-pressed`.
- Equivalent to shadcn `<Button size="icon">`, with the label enforced — prefer this one in Cryst code, and keep `Button size="icon"` for shadcn snippets you are porting.
