Short hint on hover or focus; add `shortcut` to teach keyboard commands.

```jsx
<Tooltip content="Search" shortcut="⌘K"><IconButton icon="search" label="Search" /></Tooltip>
```

- Wrap a focusable element; the tooltip is linked with `aria-describedby` and hides on Esc.
- `delayDuration` (300ms), `open` and `onOpenChange` match shadcn.
- Never put essential information only in a tooltip; touch screens can't hover. More than a line, or anything interactive, belongs in a Popover.
